use crate::error::R;
use std::env;
use std::path::PathBuf;

/// Filesystem layout for everything CodexDesk touches.
#[derive(Clone, Debug)]
pub struct Paths {
    /// `$CODEX_HOME` or `~/.codex` — where the official CLI keeps its state.
    pub codex_home: PathBuf,
    /// The single credential file the official CLI reads.
    pub auth_json: PathBuf,
    /// `~/.local/share/codexdesk` — app data.
    pub app_dir: PathBuf,
    pub vault_file: PathBuf,
    pub settings_file: PathBuf,
    pub audit_file: PathBuf,
    pub activity_file: PathBuf,
    /// Backup of the auth.json that existed before a CodexDesk switch.
    pub backup_file: PathBuf,
}

pub fn codex_home() -> PathBuf {
    if let Ok(h) = env::var("CODEX_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

pub fn resolve() -> R<Paths> {
    let codex_home = codex_home();
    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".local/share"))
        .join("codexdesk");

    let paths = Paths {
        auth_json: codex_home.join("auth.json"),
        backup_file: codex_home.join("auth.json.codexdesk.bak"),
        vault_file: app_dir.join("vault.json"),
        settings_file: app_dir.join("settings.json"),
        audit_file: app_dir.join("audit.jsonl"),
        activity_file: app_dir.join("activity.jsonl"),
        codex_home,
        app_dir,
    };

    std::fs::create_dir_all(&paths.app_dir)?;
    Ok(paths)
}
