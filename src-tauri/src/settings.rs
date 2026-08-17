use crate::error::R;
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    pub tray_mode: String,
    pub hotkey: Option<String>,
    pub launch_at_login: bool,
    pub notifications: bool,
    pub compact: bool,
    pub refresh_interval_secs: u64,
    pub warmup_tick_secs: u64,
    pub terminal_preset: String,
    pub custom_terminal: Option<String>,
    pub vault_encrypted: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            tray_mode: "icon".into(),
            hotkey: Some("ctrl+alt+d".into()),
            launch_at_login: false,
            notifications: true,
            compact: false,
            refresh_interval_secs: 300,
            warmup_tick_secs: 20,
            terminal_preset: "auto".into(),
            custom_terminal: None,
            vault_encrypted: false,
        }
    }
}

pub fn load(paths: &Paths) -> Settings {
    fs::read_to_string(&paths.settings_file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(paths: &Paths, settings: &Settings) -> R<()> {
    fs::write(
        &paths.settings_file,
        serde_json::to_string_pretty(settings)?,
    )?;
    Ok(())
}
