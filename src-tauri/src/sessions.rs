use crate::error::{AppError, R};
use crate::models::{AuditEntry, SessionInfo};
use crate::paths::Paths;
use crate::vault::append_log;
use chrono::Utc;
use serde_json::Value;
use std::fs;

/// Walk `$CODEX_HOME/sessions` and summarize every rolled-up session.
pub fn list(paths: &Paths) -> Vec<SessionInfo> {
    let mut out = Vec::new();
    let root = paths.codex_home.join("sessions");
    let Ok(entries) = fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        collect_jsonl(&entry.path(), &mut out);
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out.truncate(400);
    out
}

fn collect_jsonl(path: &std::path::Path, out: &mut Vec<SessionInfo>) {
    let Ok(meta) = fs::metadata(path) else { return };
    if meta.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for e in entries.flatten() {
                collect_jsonl(&e.path(), out);
            }
        }
        return;
    }
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return;
    }
    let Ok(raw) = fs::read_to_string(path) else { return };
    let mut id = None;
    let mut cwd = String::new();
    let mut created = 0i64;
    let mut title = String::new();
    let mut count = 0usize;

    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if id.is_none() {
                    id = v.get("id").and_then(Value::as_str).map(|s| s.to_string());
                    cwd = v
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    created = v.get("timestamp").and_then(Value::as_i64).unwrap_or(0);
                }
                count += 1;
            }
            Some("user_message") => {
                count += 1;
                if title.is_empty() {
                    title = v
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(extract_text)
                        .unwrap_or_default();
                }
            }
            Some(_) => count += 1,
            None => {}
        }
    }

    if let Some(id) = id {
        let title = clean_title(&title);
        out.push(SessionInfo {
            id,
            cwd,
            created_at: created,
            title,
            message_count: count,
        });
    }
}

fn extract_text(content: &Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    let arr = content.as_array()?;
    let mut text = String::new();
    for part in arr {
        let t = part
            .get("text")
            .or_else(|| part.get("content"))
            .and_then(Value::as_str);
        if let Some(t) = t {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(t);
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn clean_title(t: &str) -> String {
    let mut t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > 80 {
        t = t.chars().take(80).collect::<String>() + "…";
    }
    if t.is_empty() {
        "(untitled)".into()
    } else {
        t
    }
}

/// resume/fork open the session in the CLI; archive/delete run directly.
pub fn act(paths: &Paths, id: &str, action: &str) -> R<()> {
    let (ok, output) = match action {
        "archive" => crate::desktop::run_codex(&["archive", id])?,
        "delete" => crate::desktop::run_codex(&["delete", id])?,
        other => {
            return Err(AppError::Msg(format!(
                "unknown session action {other} (use resume or fork from the launcher)"
            )));
        }
    };
    append_log(
        &paths.audit_file,
        &serde_json::to_value(AuditEntry {
            ts: Utc::now().timestamp(),
            kind: format!("session-{action}"),
            account_id: None,
            detail: if ok {
                format!("session {id}")
            } else {
                format!("session {id}: {output}")
            },
        })?,
        500,
    )?;
    Ok(())
}
