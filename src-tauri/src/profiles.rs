use crate::error::{AppError, R};
use crate::models::ProfileInfo;
use crate::paths::Paths;
use std::fs;

/// Per-account Codex profiles live as `$CODEX_HOME/<name>.config.toml`.
pub fn list(paths: &Paths) -> Vec<ProfileInfo> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&paths.codex_home) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".config.toml") {
            continue;
        }
        let stem = name.trim_end_matches(".config.toml");
        out.push(ProfileInfo {
            name: stem.to_string(),
            is_base: stem == "config",
            content: None,
        });
    }
    out.sort_by(|a, b| {
        a.is_base
            .cmp(&b.is_base)
            .reverse()
            .then(a.name.cmp(&b.name))
    });
    out
}

pub fn read(paths: &Paths, name: &str) -> R<ProfileInfo> {
    let path = profile_path(paths, name)?;
    Ok(ProfileInfo {
        name: name.to_string(),
        is_base: name == "config",
        content: Some(fs::read_to_string(&path)?),
    })
}

pub fn write(paths: &Paths, name: &str, content: &str) -> R<()> {
    // Validate before touching disk so a typo can't break the user's CLI.
    if !content.trim().is_empty() {
        toml::from_str::<toml::Value>(content).map_err(|e| {
            AppError::Msg(format!("invalid TOML: {e}"))
        })?;
    }
    let path = profile_path(paths, name)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn create(paths: &Paths, name: &str) -> R<ProfileInfo> {
    let path = profile_path(paths, name)?;
    if path.exists() {
        return Err(AppError::Msg(format!("profile {name} already exists")));
    }
    let content = format!(
        "# CodexDesk profile \"{name}\"\n# Layered over config.toml when Codex runs with --profile {name}\n"
    );
    fs::write(&path, &content)?;
    Ok(ProfileInfo {
        name: name.to_string(),
        is_base: false,
        content: Some(content),
    })
}

fn profile_path(paths: &Paths, name: &str) -> R<std::path::PathBuf> {
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if safe.is_empty() || safe != name {
        return Err(AppError::Msg(
            "profile names may only contain letters, digits, - and _".into(),
        ));
    }
    Ok(paths.codex_home.join(format!("{name}.config.toml")))
}
