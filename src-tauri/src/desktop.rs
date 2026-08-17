use crate::error::{AppError, R};
use crate::models::ProcessInfo;
use crate::settings::Settings;
use std::process::Command;

/// Find every running process belonging to the official Codex CLI.
pub fn find_codex_processes() -> Vec<ProcessInfo> {
    #[cfg(target_os = "linux")]
    {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(pid) = name.to_str().and_then(|n| n.parse::<i32>().ok()) else {
                    continue;
                };
                let base = entry.path();
                let comm = std::fs::read_to_string(base.join("comm")).unwrap_or_default();
                if comm.trim() != "codex" {
                    continue;
                }
                let cmdline = std::fs::read_to_string(base.join("cmdline"))
                    .unwrap_or_default()
                    .replace('\0', " ")
                    .trim()
                    .to_string();
                out.push(ProcessInfo {
                    pid,
                    name: "codex".into(),
                    cmdline,
                });
            }
        }
        out
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps")
            .args(["-axo", "pid=,comm=,args="])
            .output()
            .unwrap_or_default();
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut parts = line.trim().splitn(3, char::is_whitespace);
                let pid = parts.next()?.parse().ok()?;
                let name = parts.next()?.trim().to_string();
                if name != "codex" {
                    return None;
                }
                let cmdline = parts.next().unwrap_or("").trim().to_string();
                Some(ProcessInfo {
                    pid,
                    name,
                    cmdline,
                })
            })
            .collect()
    }
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FO", "CSV", "/NH", "/FI", "IMAGENAME eq codex.exe"])
            .output()
            .unwrap_or_default();
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split(',').map(|f| f.trim_matches('"')).collect();
                let name = fields.first()?.to_string();
                let pid = fields.get(1)?.parse().ok()?;
                Some(ProcessInfo {
                    pid,
                    name,
                    cmdline: "codex.exe".into(),
                })
            })
            .collect()
    }
}

/// Kill a codex process. Codex sessions are interactive, so this is the
/// force-close path offered by the UI after a blocked switch.
pub fn kill_process(pid: i32) -> R<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .status()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("kill").args(["-9", &pid.to_string()]).status()?;
    }
    Ok(())
}

/// Open a URL in the user's default browser (used once, for OAuth login).
pub fn open_url(url: &str) -> R<()> {
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
    }
    Ok(())
}

struct TerminalPreset {
    id: &'static str,
    name: &'static str,
    /// Args with {title} and {command} placeholders.
    args: &'static [&'static str],
}

const PRESETS: &[TerminalPreset] = &[
    TerminalPreset {
        id: "gnome-terminal",
        name: "GNOME Terminal",
        args: &["--title", "{title}", "--", "bash", "-lc", "{command}"],
    },
    TerminalPreset {
        id: "konsole",
        name: "Konsole",
        args: &["-e", "bash", "-lc", "{command}"],
    },
    TerminalPreset {
        id: "xterm",
        name: "xterm",
        args: &["-T", "{title}", "-e", "bash", "-lc", "{command}"],
    },
    TerminalPreset {
        id: "kitty",
        name: "kitty",
        args: &["bash", "-lc", "{command}"],
    },
    TerminalPreset {
        id: "alacritty",
        name: "Alacritty",
        args: &["-T", "{title}", "-e", "bash", "-lc", "{command}"],
    },
    TerminalPreset {
        id: "wezterm",
        name: "WezTerm",
        args: &["start", "--", "bash", "-lc", "{command}"],
    },
    TerminalPreset {
        id: "foot",
        name: "foot",
        args: &["bash", "-lc", "{command}"],
    },
    TerminalPreset {
        id: "tilix",
        name: "Tilix",
        args: &["-e", "bash", "-lc", "{command}"],
    },
    TerminalPreset {
        id: "x-terminal-emulator",
        name: "System terminal",
        args: &["-e", "bash", "-lc", "{command}"],
    },
];

fn which(bin: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(bin.to_string());
        }
    }
    None
}

pub fn terminal_presets() -> Vec<(String, String, bool)> {
    PRESETS
        .iter()
        .map(|p| (p.id.to_string(), p.name.to_string(), which(p.id).is_some()))
        .collect()
}

/// Launch a shell command in a terminal window, detached from the app.
pub fn launch_terminal(settings: &Settings, command: &str, title: &str) -> R<()> {
    let cmd = format!("{command}; exec bash");
    let run = |bin: &str, args: &[&str]| -> R<()> {
        let resolved: Vec<String> = args
            .iter()
            .map(|a| {
                a.replace("{command}", &cmd)
                    .replace("{title}", &title.replace('\'', ""))
            })
            .collect();
        Command::new(bin).args(&resolved).spawn()?;
        Ok(())
    };

    if let Some(custom) = &settings.custom_terminal {
        if !custom.trim().is_empty() {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            return Ok(Command::new(shell)
                .arg("-c")
                .arg(custom)
                .env("CODEXDESK_TITLE", title)
                .env("CODEXDESK_COMMAND", &cmd)
                .spawn()
                .map(|_| ())?);
        }
    }

    if settings.terminal_preset != "auto" {
        if let Some(p) = PRESETS.iter().find(|p| p.id == settings.terminal_preset) {
            if which(p.id).is_some() {
                return run(p.id, p.args);
            }
        }
    }

    for p in PRESETS {
        if which(p.id).is_some() {
            return run(p.id, p.args);
        }
    }

    Err(AppError::Msg(
        "no supported terminal found — set a custom terminal command in Settings".into(),
    ))
}

/// The codex command line used when launching the CLI for an account.
pub fn codex_command(profile: Option<&str>, extra: Option<&str>) -> String {
    match (profile, extra) {
        (Some(p), Some(e)) => format!("codex --profile '{p}' {e}"),
        (Some(p), None) => format!("codex --profile '{p}'"),
        (None, Some(e)) => format!("codex {e}"),
        (None, None) => "codex".into(),
    }
}

/// Best-effort detection of the installed codex binary version.
pub fn codex_version() -> Option<String> {
    let bin = which("codex")?;
    let output = Command::new(bin).arg("--version").output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a plain codex subcommand, capturing its output.
pub fn run_codex(args: &[&str]) -> R<(bool, String)> {
    let bin = which("codex").ok_or_else(|| AppError::Msg("codex CLI not found on PATH".into()))?;
    let output = Command::new(bin).args(args).output()?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}
