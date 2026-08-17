use crate::desktop::find_codex_processes;
use crate::error::{AppError, R};
use crate::models::{Account, AuditEntry, AuthKind, SwitchResult};
use crate::paths::Paths;
use crate::vault::{append_log, Vault};
use chrono::Utc;
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// Serialize an account into the exact shape the official Codex CLI reads.
pub fn auth_json_for(account: &Account) -> serde_json::Value {
    let now = Utc::now().to_rfc3339();
    match account.kind {
        AuthKind::Apikey => serde_json::json!({
            "OPENAI_API_KEY": account.auth.openai_api_key.clone().unwrap_or_default(),
            "auth_mode": "apikey",
            "last_refresh": now,
        }),
        AuthKind::Chatgpt => {
            let t = account.auth.tokens.clone().unwrap_or_default();
            serde_json::json!({
                "auth_mode": "chatgpt",
                "last_refresh": now,
                "tokens": {
                    "access_token": t.access_token,
                    "account_id": t.account_id,
                    "id_token": t.id_token,
                    "refresh_token": t.refresh_token,
                },
            })
        }
    }
}

pub fn write_auth_json(paths: &Paths, account: &Account) -> R<()> {
    let text = serde_json::to_string_pretty(&auth_json_for(account))?;
    fs::write(&paths.auth_json, text)?;
    set_private(&paths.auth_json);
    Ok(())
}

/// Snapshot the current auth.json (whatever wrote it) so it can be restored.
pub fn backup_current(paths: &Paths) -> R<bool> {
    if paths.auth_json.exists() {
        fs::copy(&paths.auth_json, &paths.backup_file)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn restore_backup(paths: &Paths) -> R<bool> {
    if paths.backup_file.exists() {
        fs::rename(&paths.backup_file, &paths.auth_json)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Switch the CLI to `account`. Refuses (without touching anything) while
/// Codex processes run, unless `force` is set. Returns what happened.
pub fn switch_account(vault: &mut Vault, paths: &Paths, id: &str, force: bool) -> R<SwitchResult> {
    let account = vault
        .account(id)
        .ok_or_else(|| AppError::Msg(format!("account not found: {id}")))?;

    let running = find_codex_processes();
    if !running.is_empty() && !force {
        return Ok(SwitchResult {
            switched: false,
            blocked: running,
        });
    }

    backup_current(paths)?;
    write_auth_json(paths, &account)?;

    let mut updated = account;
    updated.last_used_at = Some(Utc::now().timestamp());
    vault.upsert(updated)?;
    vault.set_active(Some(id))?;

    append_log(
        &paths.audit_file,
        &serde_json::to_value(AuditEntry {
            ts: Utc::now().timestamp(),
            kind: "switch".into(),
            account_id: Some(id.to_string()),
            detail: format!(
                "active account switched{}",
                if force { " (forced)" } else { "" }
            ),
        })?,
        500,
    )?;

    Ok(SwitchResult {
        switched: true,
        blocked: running,
    })
}

fn set_private(path: &std::path::Path) {
    #[cfg(unix)]
    {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}
