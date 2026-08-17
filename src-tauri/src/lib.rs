pub mod authz;
pub mod desktop;
pub mod error;
pub mod models;
pub mod notify;
pub mod paths;
pub mod profiles;
pub mod quota;
pub mod sessions;
pub mod settings;
pub mod switch;
pub mod trayui;
pub mod vault;
pub mod warmup;

pub use models::*;

use error::{AppError, R};
use paths::Paths;
use rand::RngCore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    pub paths: Paths,
    pub vault: Arc<Mutex<vault::Vault>>,
    pub settings: Arc<Mutex<settings::Settings>>,
    pub http: quota::QuotaClient,
    pub warmup: warmup::Warmup,
    pub notify: notify::Notify,
    pub quitting: Arc<AtomicBool>,
}

// ---------- frontend-facing bundles ----------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusBundle {
    accounts: Vec<AccountPublic>,
    active_account_id: Option<String>,
    snapshots: SnapshotMap,
    settings: settings::Settings,
    codex_version: Option<String>,
    processes: Vec<ProcessInfo>,
    vault_encrypted: bool,
    terminal_presets: Vec<(String, String, bool)>,
}

fn bundle(state: &State<'_, AppState>) -> StatusBundle {
    let (accounts, active_id, snapshots) = {
        let vault = state.vault.lock().unwrap();
        (vault.accounts(), vault.active_id(), vault.snapshots())
    };
    let public = accounts
        .iter()
        .map(|a| {
            let snap = snapshots.get(&a.id);
            a.public(
                active_id.as_deref() == Some(a.id.as_str()),
                snap.and_then(|s| s.plan.clone()),
                snap.and_then(|s| s.subscription_expires_at),
            )
        })
        .collect();
    let settings = state.settings.lock().unwrap().clone();
    StatusBundle {
        accounts: public,
        active_account_id: active_id,
        snapshots,
        vault_encrypted: state.vault.lock().unwrap().encrypted(),
        codex_version: desktop::codex_version(),
        processes: desktop::find_codex_processes(),
        settings,
        terminal_presets: desktop::terminal_presets(),
    }
}

// ---------- refresh core (shared by commands, scheduler, menu) ----------

async fn refresh_account(
    app: &AppHandle<tauri::Wry>,
    id: &str,
    with_stats: bool,
) -> R<UsageSnapshot> {
    let state = app.state::<AppState>();
    let account = {
        let vault = state.vault.lock().unwrap();
        vault.account(id).ok_or_else(|| AppError::Msg("account not found".into()))?
    };
    let tokens = match account.kind {
        AuthKind::Chatgpt => account
            .auth
            .tokens
            .clone()
            .ok_or_else(|| AppError::Msg("account has no ChatGPT tokens".into()))?,
        AuthKind::Apikey => {
            return Err(AppError::Msg(
                "API-key accounts do not expose ChatGPT usage stats".into(),
            ));
        }
    };

    let result = state.http.fetch_snapshot(&tokens, with_stats).await;
    let snap = match result {
        Ok(s) => s,
        Err(quota::QuotaError::Unauthorized) => {
            // One silent token refresh, then one retry.
            let fresh = state.http.refresh_tokens(&tokens).await?;
            let mut updated = account.clone();
            updated.auth.tokens = Some(fresh.clone());
            {
                let mut vault = state.vault.lock().unwrap();
                vault.upsert(updated.clone())?;
                if vault.active_id().as_deref() == Some(id) {
                    switch::write_auth_json(&state.paths, &updated)?;
                }
            }
            let _ = app.emit("vault://changed", ());
            state
                .http
                .fetch_snapshot(&fresh, with_stats)
                .await
                .map_err(AppError::from)?
        }
        Err(e) => return Err(e.into()),
    };

    {
        let mut vault = state.vault.lock().unwrap();
        vault.set_snapshot(id, snap.clone())?;
    }
    let _ = app.emit("usage://updated", &snap);

    let settings = state.settings.lock().unwrap().clone();
    state.notify.evaluate(app, &settings, &account, &snap);
    trayui::rebuild_menu(app);
    Ok(snap)
}

async fn refresh_active(app: &AppHandle<tauri::Wry>, with_stats: bool) {
    let active = {
        let state = app.state::<AppState>();
        let vault = state.vault.lock().unwrap();
        vault.active_id()
    };
    let Some(id) = active else {
        return;
    };
    let _ = refresh_account(app, &id, with_stats).await;
}

async fn refresh_all(app: &AppHandle<tauri::Wry>) {
    let ids: Vec<String> = {
        let state = app.state::<AppState>();
        let vault = state.vault.lock().unwrap();
        vault.accounts().into_iter().map(|a| a.id).collect()
    };
    for id in ids {
        let _ = refresh_account(app, &id, false).await;
    }
}

// ---------- commands ----------

#[tauri::command]
fn desk_status(state: State<'_, AppState>) -> StatusBundle {
    bundle(&state)
}

#[tauri::command]
async fn desk_oauth_start(app: AppHandle, state: State<'_, AppState>) -> R<()> {
    let vault = state.vault.clone();
    let paths = state.paths.clone();
    let http = state.http.clone();
    tauri::async_runtime::spawn(async move {
        authz::start_oauth(app, vault, paths, http).await;
    });
    Ok(())
}

#[tauri::command]
async fn desk_import_auth(
    app: AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> R<AccountPublic> {
    let path = match path {
        Some(p) => p,
        None => {
            use tauri_plugin_dialog::DialogExt;
            let picked = app
                .dialog()
                .file()
                .add_filter("auth.json", &["json"])
                .blocking_pick_file();
            match picked {
                Some(f) => f
                    .into_path()
                    .map(|p| p.to_string_lossy().to_string())
                    .map_err(|e| AppError::Msg(e.to_string()))?,
                None => return Err(AppError::Msg("import cancelled".into())),
            }
        }
    };
    let raw = std::fs::read_to_string(&path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|_| AppError::Msg("that file is not valid JSON".into()))?;

    let account = import_from_value(&value, &state)?;

    // Duplicate handling: refresh the existing entry instead of stacking.
    let existing_id = {
        let vault = state.vault.lock().unwrap();
        vault
            .accounts()
            .into_iter()
            .find(|a| same_identity(a, &account))
            .map(|a| a.id)
    };

    let account = if let Some(existing) = existing_id {
        let mut vault = state.vault.lock().unwrap();
        let mut acc = account;
        acc.id = existing;
        vault.upsert(acc.clone())?;
        acc
    } else {
        let mut vault = state.vault.lock().unwrap();
        let was_empty = vault.accounts().is_empty();
        vault.upsert(account.clone())?;
        if was_empty {
            vault.set_active(Some(&account.id))?;
            switch::backup_current(&state.paths)?;
            switch::write_auth_json(&state.paths, &account)?;
        }
        account
    };

    let active = {
        let vault = state.vault.lock().unwrap();
        vault.active_id().as_deref() == Some(account.id.as_str())
    };
    let _ = app.emit("vault://changed", ());
    trayui::rebuild_menu(&app);
    Ok(account.public(active, None, None))
}

fn same_identity(a: &Account, b: &Account) -> bool {
    let aid = a.auth.tokens.as_ref().and_then(|t| t.account_id.clone());
    let bid = b.auth.tokens.as_ref().and_then(|t| t.account_id.clone());
    match (aid, bid) {
        (Some(x), Some(y)) => x == y,
        _ => a
            .auth
            .openai_api_key
            .as_deref()
            .zip(b.auth.openai_api_key.as_deref())
            .map(|(x, y)| x == y)
            .unwrap_or(false),
    }
}

fn import_from_value(value: &serde_json::Value, state: &State<'_, AppState>) -> R<Account> {
    let now = chrono::Utc::now().timestamp();
    if let Some(key) = value.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
        if key.is_empty() {
            return Err(AppError::Msg("OPENAI_API_KEY is empty".into()));
        }
        let name = format!("API key …{}", &key[key.len().saturating_sub(4)..]);
        return Ok(Account {
            id: random_id(),
            name,
            email: None,
            kind: AuthKind::Apikey,
            profile: None,
            added_at: now,
            last_used_at: None,
            warmup: WarmupConfig::default(),
            auth: AuthBundle {
                openai_api_key: Some(key.to_string()),
                tokens: None,
            },
        });
    }
    if let Some(tokens) = value.get("tokens") {
        let set = TokenSet {
            access_token: tokens
                .get("access_token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            id_token: tokens.get("id_token").and_then(|v| v.as_str()).map(String::from),
            refresh_token: tokens
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(String::from),
            account_id: tokens
                .get("account_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            expires_at: None,
        };
        if set.access_token.is_empty() {
            return Err(AppError::Msg("auth.json has no usable credentials".into()));
        }
        let email = set
            .id_token
            .as_deref()
            .and_then(|t| quota::jwt_claim(t, "email"));
        let name = email
            .as_deref()
            .and_then(|e| e.split('@').next())
            .unwrap_or("Imported account")
            .to_string();
        let _ = state;
        return Ok(Account {
            id: random_id(),
            name,
            email,
            kind: AuthKind::Chatgpt,
            profile: None,
            added_at: now,
            last_used_at: None,
            warmup: WarmupConfig::default(),
            auth: AuthBundle {
                openai_api_key: None,
                tokens: Some(set),
            },
        });
    }
    Err(AppError::Msg(
        "no OPENAI_API_KEY and no tokens block in that file".into(),
    ))
}

fn random_id() -> String {
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[tauri::command]
async fn desk_import_apikey(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    name: Option<String>,
) -> R<AccountPublic> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Msg("API key is empty".into()));
    }
    let account = Account {
        id: random_id(),
        name: name
            .unwrap_or_else(|| format!("API key …{}", &key[key.len().saturating_sub(4)..])),
        email: None,
        kind: AuthKind::Apikey,
        profile: None,
        added_at: chrono::Utc::now().timestamp(),
        last_used_at: None,
        warmup: WarmupConfig::default(),
        auth: AuthBundle {
            openai_api_key: Some(key.to_string()),
            tokens: None,
        },
    };
    let was_empty = {
        let mut vault = state.vault.lock().unwrap();
        let empty = vault.accounts().is_empty();
        vault.upsert(account.clone())?;
        if empty {
            vault.set_active(Some(&account.id))?;
            switch::backup_current(&state.paths)?;
            switch::write_auth_json(&state.paths, &account)?;
        }
        empty
    };
    let _ = app.emit("vault://changed", ());
    trayui::rebuild_menu(&app);
    Ok(account.public(was_empty, None, None))
}

#[tauri::command]
async fn desk_remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> R<()> {
    {
        let mut vault = state.vault.lock().unwrap();
        vault.remove(&id)?;
    }
    let _ = app.emit("vault://changed", ());
    trayui::rebuild_menu(&app);
    Ok(())
}

#[tauri::command]
async fn desk_rename_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> R<()> {
    {
        let mut vault = state.vault.lock().unwrap();
        let mut account = vault
            .account(&id)
            .ok_or_else(|| AppError::Msg("account not found".into()))?;
        account.name = name.trim().to_string();
        vault.upsert(account)?;
    }
    let _ = app.emit("vault://changed", ());
    trayui::rebuild_menu(&app);
    Ok(())
}

#[tauri::command]
async fn desk_export_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: Option<String>,
) -> R<String> {
    let account = {
        let vault = state.vault.lock().unwrap();
        vault
            .account(&id)
            .ok_or_else(|| AppError::Msg("account not found".into()))?
    };
    let value = switch::auth_json_for(&account);
    let text = serde_json::to_string_pretty(&value)?;
    let path = match path {
        Some(p) => p,
        None => {
            use tauri_plugin_dialog::DialogExt;
            let picked = app
                .dialog()
                .file()
                .set_file_name("auth.json")
                .add_filter("auth.json", &["json"])
                .blocking_save_file();
            match picked {
                Some(f) => f
                    .into_path()
                    .map(|p| p.to_string_lossy().to_string())
                    .map_err(|e| AppError::Msg(e.to_string()))?,
                None => return Err(AppError::Msg("export cancelled".into())),
            }
        }
    };
    std::fs::write(&path, text)?;
    Ok(path)
}

#[tauri::command]
async fn desk_set_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    profile: Option<String>,
) -> R<()> {
    {
        let mut vault = state.vault.lock().unwrap();
        let mut account = vault
            .account(&id)
            .ok_or_else(|| AppError::Msg("account not found".into()))?;
        account.profile = profile.filter(|p| !p.trim().is_empty());
        vault.upsert(account)?;
    }
    let _ = app.emit("vault://changed", ());
    Ok(())
}

#[tauri::command]
async fn desk_set_warmup(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    warmup_cfg: WarmupConfig,
) -> R<()> {
    {
        let mut vault = state.vault.lock().unwrap();
        let mut account = vault
            .account(&id)
            .ok_or_else(|| AppError::Msg("account not found".into()))?;
        account.warmup = warmup_cfg;
        vault.upsert(account)?;
    }
    let _ = app.emit("vault://changed", ());
    Ok(())
}

#[tauri::command]
async fn desk_switch(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    force: bool,
) -> R<SwitchResult> {
    let result = {
        let mut vault = state.vault.lock().unwrap();
        switch::switch_account(&mut vault, &state.paths, &id, force)?
    };
    let _ = app.emit("vault://changed", ());
    trayui::rebuild_menu(&app);
    if result.switched {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = refresh_account(&app2, &id, false).await;
        });
    }
    Ok(result)
}

#[tauri::command]
async fn desk_restore_backup(app: AppHandle, state: State<'_, AppState>) -> R<bool> {
    let done = switch::restore_backup(&state.paths)?;
    {
        let mut vault = state.vault.lock().unwrap();
        vault.set_active(None)?;
    }
    let _ = app.emit("vault://changed", ());
    trayui::rebuild_menu(&app);
    Ok(done)
}

#[tauri::command]
async fn desk_refresh_usage(app: AppHandle, id: String) -> R<UsageSnapshot> {
    refresh_account(&app, &id, true).await
}

#[tauri::command]
async fn desk_refresh_all(app: AppHandle) -> R<()> {
    refresh_all(&app).await;
    Ok(())
}

#[tauri::command]
async fn desk_warmup(app: AppHandle, state: State<'_, AppState>, ids: Option<Vec<String>>) -> R<()> {
    let vault = state.vault.clone();
    let paths = state.paths.clone();
    let http = state.http.clone();
    warmup::warm_now(&app, &vault, &paths, &http, ids).await;
    Ok(())
}

#[tauri::command]
fn desk_activity_log(state: State<'_, AppState>, limit: Option<usize>) -> Vec<ActivityEntry> {
    vault::read_log(&state.paths.activity_file, limit.unwrap_or(50))
}

#[tauri::command]
fn desk_audit_log(state: State<'_, AppState>, limit: Option<usize>) -> Vec<AuditEntry> {
    vault::read_log(&state.paths.audit_file, limit.unwrap_or(50))
}

#[tauri::command]
fn desk_sessions(state: State<'_, AppState>) -> Vec<SessionInfo> {
    sessions::list(&state.paths)
}

#[tauri::command]
fn desk_session_action(state: State<'_, AppState>, id: String, action: String) -> R<()> {
    sessions::act(&state.paths, &id, &action)
}

#[tauri::command]
async fn desk_session_launch(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    mode: String,
) -> R<()> {
    let mode = match mode.as_str() {
        "resume" => "resume",
        "fork" => "fork",
        _ => return Err(AppError::Msg("mode must be resume or fork".into())),
    };
    let settings = state.settings.lock().unwrap().clone();
    let cmd = format!("codex {mode} {id}");
    desktop::launch_terminal(&settings, &cmd, "Codex session")?;
    let _ = app.emit("toast://show", format!("{mode} opened in a terminal"));
    Ok(())
}

#[tauri::command]
fn desk_profiles(state: State<'_, AppState>) -> Vec<ProfileInfo> {
    profiles::list(&state.paths)
}

#[tauri::command]
fn desk_profile_read(state: State<'_, AppState>, name: String) -> R<ProfileInfo> {
    profiles::read(&state.paths, &name)
}

#[tauri::command]
fn desk_profile_write(state: State<'_, AppState>, name: String, content: String) -> R<()> {
    profiles::write(&state.paths, &name, &content)
}

#[tauri::command]
fn desk_profile_create(state: State<'_, AppState>, name: String) -> R<ProfileInfo> {
    profiles::create(&state.paths, &name)
}

#[tauri::command]
fn desk_find_codex_processes() -> Vec<ProcessInfo> {
    desktop::find_codex_processes()
}

#[tauri::command]
fn desk_kill_codex(pid: i32) -> R<()> {
    desktop::kill_process(pid)
}

#[tauri::command]
async fn desk_open_codex(app: AppHandle, state: State<'_, AppState>, account_id: Option<String>) -> R<()> {
    let (profile, settings) = {
        let vault = state.vault.lock().unwrap();
        let settings = state.settings.lock().unwrap().clone();
        let profile = account_id
            .as_deref()
            .and_then(|id| vault.account(id))
            .and_then(|a| a.profile.clone());
        (profile, settings)
    };
    let cmd = desktop::codex_command(profile.as_deref(), None);
    desktop::launch_terminal(&settings, &cmd, "Codex")?;
    let _ = app.emit("toast://show", format!("launching codex in a terminal"));
    Ok(())
}

#[tauri::command]
fn desk_settings(state: State<'_, AppState>) -> settings::Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
async fn desk_save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    new_settings: settings::Settings,
) -> R<settings::Settings> {
    let mut settings = new_settings;
    settings.vault_encrypted = state.vault.lock().unwrap().encrypted();
    settings::save(&state.paths, &settings)?;
    *state.settings.lock().unwrap() = settings.clone();

    // Launch at login.
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    if settings.launch_at_login {
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
    }

    // Global hotkey re-registration.
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if let Some(hotkey) = &settings.hotkey {
        if let Ok(shortcut) = hotkey.parse::<tauri_plugin_global_shortcut::Shortcut>() {
            let _ = gs.register(shortcut);
        }
    }

    let _ = app.emit("settings://changed", &settings);
    trayui::rebuild_menu(&app);
    Ok(settings)
}

#[tauri::command]
fn desk_reveal_vault(state: State<'_, AppState>) -> R<()> {
    let dir = state.paths.app_dir.to_string_lossy().to_string();
    desktop::open_url(&format!("file://{dir}"))?;
    Ok(())
}

#[tauri::command]
fn desk_check_codex() -> Option<String> {
    desktop::codex_version()
}

#[tauri::command]
async fn desk_show_main(app: AppHandle) -> R<()> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
    Ok(())
}

#[tauri::command]
async fn desk_hide_popup(app: AppHandle) -> R<()> {
    if let Some(win) = app.get_webview_window("tray-popup") {
        let _ = win.hide();
    }
    Ok(())
}

#[tauri::command]
async fn desk_quit(app: AppHandle, state: State<'_, AppState>) -> R<()> {
    state.quitting.store(true, Ordering::Relaxed);
    app.exit(0);
    Ok(())
}

// ---------- menu events ----------

fn on_menu_event(app: &AppHandle<tauri::Wry>, id: &str) {
    let id = id.to_string();
    let app = app.clone();
    if id == "quit" {
        app.state::<AppState>().quitting.store(true, Ordering::Relaxed);
        app.exit(0);
        return;
    }
    if id == "open" {
        let _ = app.get_webview_window("main").map(|w| {
            let _ = w.show();
            let _ = w.set_focus();
        });
        return;
    }
    if id == "panel" {
        trayui::toggle_popup(&app);
        return;
    }
    if id == "refresh" {
        tauri::async_runtime::spawn(async move {
            refresh_active(&app, false).await;
        });
        return;
    }
    if id == "warmall" {
        let state = app.state::<AppState>();
        let vault = state.vault.clone();
        let paths = state.paths.clone();
        let http = state.http.clone();
        tauri::async_runtime::spawn(async move {
            warmup::warm_now(&app, &vault, &paths, &http, None).await;
        });
        return;
    }
    if let Some(account_id) = id.strip_prefix("acct:") {
        let account_id = account_id.to_string();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let result = {
                let mut vault = state.vault.lock().unwrap();
                switch::switch_account(&mut vault, &state.paths, &account_id, false)
            };
            match result {
                Ok(r) => {
                    if r.switched {
                        let _ = app.emit("vault://changed", ());
                        let _ = app.emit("toast://show", format!("switched to another account"));
                        trayui::rebuild_menu(&app);
                        let app2 = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = refresh_account(&app2, &account_id, false).await;
                        });
                    } else {
                        let _ = app.emit("switch://blocked", &r.blocked);
                        let _ = app.emit("toast://show", "codex is still running — close it first");
                    }
                }
                Err(e) => {
                    let _ = app.emit("toast://show", e.to_string());
                }
            }
        });
    }
}

// ---------- lifecycle ----------

fn register_shortcut(app: &AppHandle<tauri::Wry>, hotkey: Option<&str>) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let Some(hotkey) = hotkey else { return };
    let Ok(shortcut) = hotkey.parse::<tauri_plugin_global_shortcut::Shortcut>() else { return };
    let _ = gs.register(shortcut);
}

pub fn run() {
    use tauri_plugin_global_shortcut::ShortcutState;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        trayui::toggle_popup(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            desk_status,
            desk_oauth_start,
            desk_import_auth,
            desk_import_apikey,
            desk_remove_account,
            desk_rename_account,
            desk_export_account,
            desk_set_profile,
            desk_set_warmup,
            desk_switch,
            desk_restore_backup,
            desk_refresh_usage,
            desk_refresh_all,
            desk_warmup,
            desk_activity_log,
            desk_audit_log,
            desk_sessions,
            desk_session_action,
            desk_session_launch,
            desk_profiles,
            desk_profile_read,
            desk_profile_write,
            desk_profile_create,
            desk_find_codex_processes,
            desk_kill_codex,
            desk_open_codex,
            desk_settings,
            desk_save_settings,
            desk_reveal_vault,
            desk_check_codex,
            desk_show_main,
            desk_hide_popup,
            desk_quit,
        ])
        .setup(|app| {
            let paths = paths::resolve()?;
            let vault = vault::Vault::load(paths.clone())?;
            let mut settings = settings::load(&paths);
            settings.vault_encrypted = vault.encrypted();
            let warmup_engine = warmup::Warmup::new();
            let notify_engine = notify::Notify::new();

            let state = AppState {
                paths: paths.clone(),
                vault: Arc::new(Mutex::new(vault)),
                settings: Arc::new(Mutex::new(settings.clone())),
                http: quota::QuotaClient::new(),
                warmup: warmup_engine,
                notify: notify_engine,
                quitting: Arc::new(AtomicBool::new(false)),
            };

            // Bring launch-at-login in line with saved settings.
            use tauri_plugin_autostart::ManagerExt;
            if settings.launch_at_login {
                let _ = app.autolaunch().enable();
            }

            trayui::build(app)?;
            trayui::rebuild_menu(app.handle());
            register_shortcut(app.handle(), settings.hotkey.as_deref());

            app.manage(state);
            let state = app.state::<AppState>();

            state.warmup.start(
                app.handle().clone(),
                state.vault.clone(),
                state.paths.clone(),
                state.http.clone(),
            );

            // Refresh the active account's stats in the background at startup.
            {
                let app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    refresh_active(&app, true).await;
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let quitting = app.state::<AppState>().quitting.load(Ordering::Relaxed);
                if window.label() == "main" && !quitting {
                    api.prevent_close();
                    let _ = window.hide();
                } else if window.label() == "tray-popup" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            if let tauri::WindowEvent::Focused(false) = event {
                if window.label() == "tray-popup" {
                    let _ = window.hide();
                }
            }
        })
        .on_menu_event(|app, event| {
            on_menu_event(app, event.id().as_ref());
        })
        .run(tauri::generate_context!())
        .expect("error while running CodexDesk");
}

// Silence dead-code lint for the module tree in the CLI build.
#[allow(dead_code)]
fn _module_marker() {
    let _ = paths::resolve();
    let _ = error::R::<()>::Ok(());
}
