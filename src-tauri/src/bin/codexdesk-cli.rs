//! Terminal companion for CodexDesk: switch/status/list without the GUI.
//! Reads the same encrypted vault the desktop app uses.

use codexdesk_lib::{paths, switch, vault::Vault};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    let result = match cmd {
        "list" => cmd_list(),
        "status" => cmd_status(),
        "switch" => cmd_switch(args.get(1)),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => {
            eprintln!("unknown command: {cmd}\n");
            print_help();
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("codexdesk-cli: {e}");
        std::process::exit(1);
    }
}

fn load() -> Result<(Vault, paths::Paths), String> {
    let paths = paths::resolve().map_err(|e| e.to_string())?;
    let vault = Vault::load(paths.clone()).map_err(|e| e.to_string())?;
    Ok((vault, paths))
}

fn cmd_list() -> Result<(), String> {
    let (vault, _) = load()?;
    let active = vault.active_id();
    for account in vault.accounts() {
        let marker = if active.as_deref() == Some(account.id.as_str()) {
            "*"
        } else {
            " "
        };
        let kind = match account.kind {
            codexdesk_lib::AuthKind::Chatgpt => "chatgpt",
            codexdesk_lib::AuthKind::Apikey => "apikey",
        };
        println!(
            "{marker} {:<24} {:<8} {}",
            account.name,
            kind,
            account.email.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

fn cmd_status() -> Result<(), String> {
    let (vault, paths) = load()?;
    let Some(active) = vault.active_id().and_then(|id| vault.account(&id)) else {
        println!("no active account");
        return Ok(());
    };
    println!("active: {}", active.name);
    if let Some(snap) = vault.snapshot(&active.id) {
        if let Some(sess) = &snap.session {
            let pct = if sess.maximum_tokens > 0.0 {
                sess.remaining_tokens / sess.maximum_tokens * 100.0
            } else {
                0.0
            };
            let reset = chrono::DateTime::from_timestamp(sess.reset_at, 0)
                .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();
            println!("5h window: {pct:.0}% remaining · resets {reset}");
        }
        if let Some(weekly) = &snap.weekly {
            let pct = if weekly.maximum_tokens > 0.0 {
                weekly.remaining_tokens / weekly.maximum_tokens * 100.0
            } else {
                0.0
            };
            println!("weekly: {pct:.0}% remaining");
        }
        if let Some(rc) = &snap.reset_credits {
            println!("reset credits: {}", rc.available_count);
        }
    }
    if paths.auth_json.exists() {
        println!("auth.json: {}", paths.auth_json.display());
    }
    Ok(())
}

fn cmd_switch(target: Option<&String>) -> Result<(), String> {
    let Some(target) = target else {
        eprintln!("usage: codexdesk-cli switch <name>");
        std::process::exit(2);
    };
    let (mut vault, paths) = load()?;
    let id = vault
        .find_by_name(target)
        .map(|a| a.id)
        .ok_or_else(|| format!("no account named {target:?} (use `codexdesk-cli list`)"))?;
    let result = switch::switch_account(&mut vault, &paths, &id, true)
        .map_err(|e| e.to_string())?;
    if result.switched {
        println!("switched to {target}");
        Ok(())
    } else {
        Err("switch blocked by running codex processes".into())
    }
}

fn print_help() {
    println!(
        "codexdesk-cli — CodexDesk terminal companion\n\nUSAGE:\n  codexdesk-cli list\n  codexdesk-cli status\n  codexdesk-cli switch <name>\n"
    );
}
