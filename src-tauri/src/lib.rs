pub mod commands;
pub mod core;
pub mod error;
pub mod paths;
pub mod state;
pub mod util;

use tauri::{AppHandle, Manager};

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,inkwing_lib=debug")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            app.manage(state::AppState::new());
            // System tray. Failure to build the tray (e.g. missing
            // libayatana on Linux) is non-fatal — log and continue.
            if let Err(e) = core::tray::build(app.handle()) {
                tracing::warn!(?e, "failed to build system tray");
            }
            // Reflect persisted autostart preference into OS state.
            commands::settings_cmd::sync_on_startup(app.handle());
            // Pull the persisted config library into AppState + load the
            // active config into the cache the legacy commands read.
            commands::config_cmd::hydrate_on_startup(app.handle());
            Ok(())
        })
        // Window-close interception: hide to tray when the user has
        // minimize_to_tray on (default), otherwise let the close
        // proceed — which triggers RunEvent::ExitRequested below and
        // the shutdown hook ultimately kills sing-box. The Quit menu
        // item in the tray is the explicit way out either way.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let app = window.app_handle();
                    let to_tray = commands::settings_cmd::current_settings(app).minimize_to_tray;
                    if to_tray {
                        let _ = window.hide();
                        api.prevent_close();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::core_cmd::core_version,
            commands::core_cmd::core_status,
            commands::core_cmd::core_start,
            commands::core_cmd::core_stop,
            commands::core_cmd::core_check_privilege,
            commands::core_cmd::core_restart,
            commands::core_cmd::logs_recent,
            #[cfg(target_os = "linux")]
            commands::core_cmd::core_grant_tun_capability_linux,
            #[cfg(target_os = "windows")]
            commands::core_cmd::core_relaunch_as_admin_windows,
            #[cfg(target_os = "macos")]
            commands::core_cmd::core_test_macos_admin,
            commands::config_cmd::config_open_dialog,
            commands::config_cmd::config_load,
            commands::config_cmd::config_validate,
            commands::config_cmd::config_get_raw,
            commands::config_cmd::config_save,
            commands::config_cmd::config_reveal,
            commands::config_cmd::config_current_path,
            commands::config_cmd::config_library_list,
            commands::config_cmd::config_library_add_local,
            commands::config_cmd::config_library_add_from_text,
            commands::config_cmd::config_library_remove,
            commands::config_cmd::config_library_rename,
            commands::config_cmd::config_library_select,
            commands::config_cmd::config_library_view,
            commands::config_cmd::config_library_reveal,
            commands::config_cmd::config_library_refresh_from_subscription,
            commands::config_cmd::config_active_summary,
            commands::proxies_cmd::proxies_list,
            commands::proxies_cmd::proxies_select,
            commands::proxies_cmd::proxies_test,
            commands::proxies_cmd::proxies_test_many,
            commands::proxies_cmd::proxies_speedtest,
            commands::connections_cmd::connections_close,
            commands::connections_cmd::connections_close_all,
            commands::rules_cmd::rules_list,
            commands::rules_cmd::rules_add,
            commands::rules_cmd::rules_update,
            commands::rules_cmd::rules_delete,
            commands::rules_cmd::rules_reorder,
            commands::rules_cmd::rules_mask,
            commands::rules_cmd::rules_unmask,
            commands::rules_cmd::rules_revert,
            commands::rules_cmd::rules_commit,
            commands::rules_cmd::rule_sets_list,
            commands::rules_cmd::rule_sets_add,
            commands::rules_cmd::rule_sets_update,
            commands::rules_cmd::rule_sets_delete,
            commands::rules_cmd::rule_sets_mask,
            commands::rules_cmd::rule_sets_unmask,
            commands::rules_cmd::rule_sets_revert,
            commands::rules_cmd::rule_sets_commit,
            commands::dns_cmd::dns_servers_list,
            commands::dns_cmd::dns_servers_add,
            commands::dns_cmd::dns_servers_update,
            commands::dns_cmd::dns_servers_delete,
            commands::dns_cmd::dns_servers_mask,
            commands::dns_cmd::dns_servers_unmask,
            commands::dns_cmd::dns_servers_revert,
            commands::dns_cmd::dns_rules_list,
            commands::dns_cmd::dns_rules_add,
            commands::dns_cmd::dns_rules_update,
            commands::dns_cmd::dns_rules_delete,
            commands::dns_cmd::dns_rules_mask,
            commands::dns_cmd::dns_rules_unmask,
            commands::dns_cmd::dns_rules_revert,
            commands::dns_cmd::dns_commit,
            commands::settings_cmd::settings_get,
            commands::settings_cmd::settings_set,
            commands::subscriptions_cmd::subs_list,
            commands::subscriptions_cmd::subs_add,
            commands::subscriptions_cmd::subs_update,
            commands::subscriptions_cmd::subs_remove,
            commands::subscriptions_cmd::subs_refresh,
            commands::subscriptions_cmd::subs_apply,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // Fired when the OS (or our own app.exit / window close
            // without prevent_close) is about to terminate the process.
            // This is our last chance to kill the sing-box sidecar
            // synchronously — without this, the child becomes an orphan
            // on Windows (job-control isn't set up by default).
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
                shutdown_singbox_sync(app);
            }
            _ => {}
        });
}

/// Synchronous best-effort: take the sidecar handle out of AppState and
/// .kill() it. We can't await stop_sidecar's grace timeout from inside
/// a non-async callback, so we just SIGKILL/TerminateProcess and move
/// on. sing-box's own TUN cleanup is its responsibility on signal.
fn shutdown_singbox_sync(app: &AppHandle) {
    let state = app.state::<state::AppState>();
    let (handle, traffic_t, log_t, conn_t) = {
        let mut g = state.core.lock();
        (
            g.handle.take(),
            g.traffic_task.take(),
            g.log_task.take(),
            g.conn_task.take(),
        )
    };
    if let Some(t) = traffic_t {
        t.abort();
    }
    if let Some(t) = log_t {
        t.abort();
    }
    if let Some(t) = conn_t {
        t.abort();
    }
    if let Some(h) = handle {
        let _ = h.child.kill();
    }
}
