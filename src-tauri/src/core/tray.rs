//! System tray icon + menu. Created at app startup.
//!
//! Menu:
//!   Show / Hide window     toggle
//!   ----
//!   Start core             when stopped (greyed when no config)
//!   Stop core              when running
//!   ----
//!   Quit
//!
//! Window close button intercepts via lib::run setup, hiding the window
//! instead of exiting (subject to settings.minimize_to_tray, defaults on).

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

pub fn build(app: &AppHandle) -> AppResult<()> {
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let start = MenuItem::with_id(app, "core_start", "Start core", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "core_stop", "Stop core", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&show, &hide, &sep1, &start, &stop, &sep2, &quit],
    )?;

    let _tray = TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().ok_or_else(|| {
            AppError::Other("no default window icon for tray".into())
        })?)
        .tooltip("Inkwing")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => focus_main(app),
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            "core_start" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<crate::state::AppState>();
                    let _ = crate::commands::core_cmd::core_start(app.clone(), state).await;
                });
            }
            "core_stop" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<crate::state::AppState>();
                    let _ = crate::commands::core_cmd::core_stop(app.clone(), state).await;
                });
            }
            "quit" => {
                // Best-effort: ask the running core to stop, then exit.
                let app_for_stop = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_for_stop.state::<crate::state::AppState>();
                    let _ = crate::commands::core_cmd::core_stop(app_for_stop.clone(), state).await;
                    app_for_stop.exit(0);
                });
            }
            _ => {}
        })
        // Left-click the tray icon = toggle window visibility (matches the
        // desktop convention most users expect).
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(w) = tray.app_handle().get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
