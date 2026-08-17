use crate::models::UsageSnapshot;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition};
use tauri::{Emitter, Runtime};

const TRAY_ID: &str = "codexdesk-tray";

pub fn build<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let menu = build_menu(app.handle())?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().expect("app icon").clone())
        .tooltip("CodexDesk")
        .menu(&menu)
        .show_menu_on_left_click(true);

    // On platforms where left-click events fire (macOS/Windows/X11), a left
    // click toggles the quick panel. On Linux AppIndicator the menu shows
    // instead, which is why the menu also has a "Quick panel" item.
    builder = builder.on_tray_icon_event(|tray, event| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            #[cfg(not(target_os = "linux"))]
            {
                let app = tray.app_handle();
                crate::trayui::toggle_popup(&app);
            }
            #[cfg(target_os = "linux")]
            let _ = &tray;
        }
    });

    builder.build(app)?;
    Ok(())
}

/// Rebuild the tray menu to reflect accounts, the active one, and its quota.
pub fn rebuild_menu<R: Runtime>(app: &AppHandle<R>) {
    let Ok(menu) = build_menu(app) else { return };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let state = app.state::<crate::AppState>();
    let (accounts, active_id, snapshots) = {
        let vault = state.vault.lock().unwrap();
        (
            vault.accounts(),
            vault.active_id(),
            vault.snapshots(),
        )
    };

    let menu = Menu::new(app)?;

    let active_label = match active_id.as_deref().and_then(|id| {
        accounts.iter().find(|a| a.id == id).map(|a| {
            let snap = snapshots.get(id);
            format!(
                "Active: {} {}",
                a.name,
                quota_suffix(snap).unwrap_or_default()
            )
        })
    }) {
        Some(l) => l,
        None => "No active account".to_string(),
    };
    menu.append(&MenuItem::with_id(app, "status-label", active_label, false, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    if accounts.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "none-label",
            "Add an account from the dashboard",
            false,
            None::<&str>,
        )?)?;
    }
    for account in &accounts {
        let item = CheckMenuItem::with_id(
            app,
            format!("acct:{}", account.id),
            account.name.clone(),
            true,
            active_id.as_deref() == Some(account.id.as_str()),
            None::<&str>,
        )?;
        menu.append(&item)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "refresh", "Refresh usage", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "warmall", "Warm up all", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "panel", "Quick panel", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "open", "Open CodexDesk", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit CodexDesk", true, None::<&str>)?)?;
    Ok(menu)
}

fn quota_suffix(snap: Option<&UsageSnapshot>) -> Option<String> {
    let snap = snap?;
    let bucket = snap.session.as_ref().or(snap.weekly.as_ref())?;
    let pct = if bucket.maximum_tokens > 0.0 {
        bucket.remaining_tokens / bucket.maximum_tokens * 100.0
    } else {
        0.0
    };
    let is_weekly = snap.session.is_none();
    Some(format!(
        "{pct:.0}% {}",
        if is_weekly { "wk" } else { "5h" }
    ))
}

/// Show or hide the small always-on-top panel next to the cursor.
pub fn toggle_popup<R: Runtime>(app: &AppHandle<R>) {
    let Some(win) = app.get_webview_window("tray-popup") else {
        return;
    };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    let size = win.inner_size().unwrap_or(tauri::PhysicalSize {
        width: 380,
        height: 560,
    });
    let cursor = app
        .cursor_position()
        .unwrap_or(PhysicalPosition { x: 0.0, y: 0.0 });
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(m) = monitor {
        let mp = m.position();
        let ms = m.size();
        let min_x = (mp.x + 8) as f64;
        let max_x = ((mp.x + ms.width as i32 - 8) as f64 - size.width as f64).max(min_x);
        let x = (cursor.x - size.width as f64 / 2.0).clamp(min_x, max_x);
        let mut y = cursor.y + 12.0;
        let bottom = (mp.y + ms.height as i32 - 8) as f64;
        if y + size.height as f64 > bottom {
            y = (cursor.y - size.height as f64 - 12.0).max((mp.y + 8) as f64);
        }
        let _ = win.set_position(PhysicalPosition::new(x as i32, y as i32));
    }
    let _ = win.show();
    let _ = win.set_focus();
    let _ = app.emit("popup://opened", ());
}
