use crate::models::{Account, UsageSnapshot};
use crate::settings::Settings;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Rate-limit desktop notifications, deduplicated per window so the user is
/// alerted once per window instead of every refresh.
pub struct Notify {
    state: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    /// "account|kind" -> bucket identity ("name@reset_at") already notified
    low: HashMap<String, String>,
    /// "account|kind" -> reset_at already announced
    reset: HashMap<String, i64>,
}

impl Notify {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Inner::default())),
        }
    }

    pub fn evaluate(&self, app: &AppHandle, settings: &Settings, account: &Account, snap: &UsageSnapshot) {
        if !settings.notifications {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        for (kind, bucket) in [("session", &snap.session), ("weekly", &snap.weekly)] {
            let Some(b) = bucket else { continue };
            let key = format!("{}|{}", account.id, kind);
            let id = format!("{}@{}", b.name, b.reset_at);

            // Window just reset -> one friendly announcement.
            if now >= b.reset_at && now - b.reset_at < 120 {
                let mut guard = self.state.lock().unwrap();
                if guard.reset.get(&key).copied().unwrap_or(0) != b.reset_at {
                    guard.reset.insert(key.clone(), b.reset_at);
                    drop(guard);
                    show(
                        app,
                        "CodexDesk — fresh window",
                        &format!("{}: the {kind} window has reset", account.name),
                    );
                }
                continue;
            }

            let pct = if b.maximum_tokens > 0.0 {
                b.remaining_tokens / b.maximum_tokens * 100.0
            } else {
                100.0
            };
            let mut guard = self.state.lock().unwrap();
            if pct < 10.0 {
                if guard.low.get(&key).map(|s| s != &id).unwrap_or(true) {
                    guard.low.insert(key.clone(), id.clone());
                    drop(guard);
                    let body = format!(
                        "{} — {} window at {:.0}% ({} left)",
                        account.name,
                        kind,
                        pct,
                        fmt_tokens(b.remaining_tokens)
                    );
                    show(app, "CodexDesk — limit getting low", &body);
                }
            }
        }
    }
}

fn show(app: &AppHandle, title: &str, body: &str) {
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show();
}

fn fmt_tokens(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}k", n / 1_000.0)
    } else {
        format!("{n:.0}")
    }
}
