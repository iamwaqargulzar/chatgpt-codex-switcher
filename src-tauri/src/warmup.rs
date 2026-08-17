use crate::error::R;
use crate::models::{Account, ActivityEntry, AuthKind};
use crate::paths::Paths;
use crate::quota::QuotaClient;
use crate::vault::{append_log, Vault};
use chrono::Utc;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

/// Background engine that fires warm-ups. Every request it makes is written
/// to the activity log, which the UI renders — no hidden traffic.
pub struct Warmup {
    state: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    /// account id -> reset_at of the last auto warm-up
    last_auto: HashMap<String, i64>,
    /// "account|HH:MM" -> "YYYY-MM-DD" of the last timed run
    last_timed: HashMap<String, String>,
}

impl Warmup {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Inner::default())),
        }
    }

    /// Spawn the scheduler loop. Tick interval comes from settings.
    pub fn start(&self, app: AppHandle, vault: Arc<Mutex<Vault>>, paths: Paths, http: QuotaClient) {
        let inner = self.state.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let settings = {
                    let state = app.state::<crate::AppState>();
                    let s = state.settings.lock().unwrap();
                    s.clone()
                };
                let tick = std::time::Duration::from_secs(settings.warmup_tick_secs.max(10));
                evaluate(&app, &vault, &paths, &http, &inner, &settings).await;
                tokio::time::sleep(tick).await;
            }
        });
    }

    /// Record bookkeeping for a warm-up without performing one.
    pub fn note_auto(&self, account_id: &str, reset_at: i64) {
        self.state
            .lock()
            .unwrap()
            .last_auto
            .insert(account_id.to_string(), reset_at);
    }
}

async fn evaluate(
    app: &AppHandle,
    vault: &Arc<Mutex<Vault>>,
    paths: &Paths,
    http: &QuotaClient,
    inner: &Arc<Mutex<Inner>>,
    _settings: &crate::settings::Settings,
) {
    let (accounts, snapshots) = {
        let v = vault.lock().unwrap();
        (v.accounts(), v.snapshots())
    };

    let now = Utc::now();
    let today = now.format("%Y-%m-%d").to_string();
    let hm = now.format("%H:%M").to_string();

    for account in accounts {
        if !account.warmup.enabled {
            continue;
        }
        let snap = snapshots.get(&account.id);

        let mut do_warm = false;
        let mut reason = String::new();

        if account.warmup.auto_after_reset {
            // Prefer the session window; fall back to the weekly window on
            // accounts that only have one (ChatGPT dropped the 5h window).
            let window = snap
                .and_then(|s| s.session.as_ref())
                .or_else(|| snap.and_then(|s| s.weekly.as_ref()));
            if let Some(w) = window {
                if w.reset_at <= now.timestamp() {
                    let last = inner
                        .lock()
                        .unwrap()
                        .last_auto
                        .get(&account.id)
                        .copied()
                        .unwrap_or(0);
                    if last < w.reset_at {
                        // When warming the session, skip if the weekly window
                        // is truly empty. Warming the weekly itself is fine
                        // right after its own reset.
                        let weekly_empty = snap
                            .and_then(|s| s.weekly.as_ref())
                            .map(|x| x.remaining_tokens <= 0.0)
                            .unwrap_or(false);
                        let warming_weekly = w.name == "weekly";
                        if !weekly_empty || warming_weekly {
                            do_warm = true;
                            reason = format!(
                                "{} window reset at {}",
                                if warming_weekly { "weekly" } else { "session" },
                                fmt_ts(w.reset_at)
                            );
                        }
                    }
                }
            }
        }

        for t in &account.warmup.timed_at {
            if *t == hm {
                let key = format!("{}|{}", account.id, t);
                let mut guard = inner.lock().unwrap();
                if guard.last_timed.get(&key).map(|d| d != &today).unwrap_or(true) {
                    guard.last_timed.insert(key.clone(), today.clone());
                    drop(guard);
                    do_warm = true;
                    reason = format!("scheduled time {t}");
                }
            }
        }

        if do_warm {
            perform_warm(app, vault, paths, http, &account, &reason).await;
        }
    }
}

/// Warm one account: refresh the OAuth tokens when they are old, then make a
/// single minimal request so the usage window has activity.
pub async fn perform_warm(
    app: &AppHandle,
    vault: &Arc<Mutex<Vault>>,
    paths: &Paths,
    http: &QuotaClient,
    account: &Account,
    reason: &str,
) {
    let mut account = account.clone();
    let mut detail = reason.to_string();

    if account.kind == AuthKind::Chatgpt {
        let Some(mut tokens) = account.auth.tokens.clone() else {
            log_activity(app, paths, account, "warm-up", false, "no tokens stored");
            return;
        };
        let stale = tokens
            .expires_at
            .map(|e| e - Utc::now().timestamp() < 300)
            .unwrap_or(true);
        if stale {
            match http.refresh_tokens(&tokens).await {
                Ok(fresh) => {
                    tokens = fresh;
                    detail.push_str(" · tokens refreshed");
                }
                Err(e) => {
                    log_activity(app, paths, account, "warm-up", false, &e.to_string());
                    return;
                }
            }
        }
        account.auth.tokens = Some(tokens.clone());
        let ok = http.health_check(&tokens).await.is_ok();
        if ok {
            let id = account.id.clone();
            {
                let mut v = vault.lock().unwrap();
                let _ = v.upsert(account.clone());
                if v.active_id().as_deref() == Some(id.as_str()) {
                    let _ = crate::switch::write_auth_json(paths, &account);
                }
            }
            let _ = app.emit("vault://changed", json!({}));
        }
        log_activity(
            app,
            paths,
            account,
            "warm-up",
            ok,
            if ok { &detail } else { "health check failed" },
        );
        return;
    }

    // API-key accounts get a model-list probe.
    match &account.auth.openai_api_key {
        Some(_) => {
            let ok = probe_api_key(http, &account).await.is_ok();
            log_activity(
                app,
                paths,
                account,
                "warm-up",
                ok,
                if ok { &detail } else { "api probe failed" },
            );
        }
        None => {
            log_activity(app, paths, account, "warm-up", false, "no credentials to warm with");
        }
    }
}

async fn probe_api_key(http: &QuotaClient, account: &Account) -> R<()> {
    http.probe_api_key(account.auth.openai_api_key.as_deref().unwrap_or(""))
        .await
}

fn log_activity(
    app: &AppHandle,
    paths: &Paths,
    account: Account,
    action: &str,
    ok: bool,
    detail: &str,
) {
    let entry = ActivityEntry {
        ts: Utc::now().timestamp(),
        account_id: account.id.clone(),
        account_name: account.name.clone(),
        action: action.to_string(),
        ok,
        detail: detail.to_string(),
    };
    let _ = append_log(&paths.activity_file, &serde_json::to_value(&entry).unwrap_or(json!({})), 200);
    let _ = app.emit("activity://new", serde_json::to_value(&entry).unwrap_or(json!({})));
}

/// Manual warm-up entry point (button / tray menu).
pub async fn warm_now(
    app: &AppHandle,
    vault: &Arc<Mutex<Vault>>,
    paths: &Paths,
    http: &QuotaClient,
    ids: Option<Vec<String>>,
) {
    let accounts: Vec<Account> = {
        let v = vault.lock().unwrap();
        match ids {
            Some(ids) => v
                .accounts()
                .into_iter()
                .filter(|a| ids.contains(&a.id))
                .collect(),
            None => v
                .accounts()
                .into_iter()
                .filter(|a| a.warmup.enabled || a.kind == AuthKind::Chatgpt)
                .collect(),
        }
    };
    for account in accounts {
        perform_warm(app, vault, paths, http, &account, "manual").await;
    }
}

fn fmt_ts(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.format("%H:%M").to_string())
        .unwrap_or_default()
}
