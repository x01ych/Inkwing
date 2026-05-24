//! Persistent app settings, backed by tauri-plugin-store.
//!
//! Anything user-tunable that needs to outlive a process restart goes here.
//! Currently:
//!   - minimize_to_tray   (close button hides instead of quits — this is
//!                         currently the always-on backend behaviour;
//!                         the flag is exposed so we can wire an opt-out
//!                         later without UI churn)
//!   - autostart          (launch GUI on OS login — drives
//!                         tauri-plugin-autostart)
//!   - latency_test_url   (used by Proxies page Test buttons)
//!   - language           ("en" | "zh") — frontend reads, currently
//!                         informational
//!   - theme              ("dark" | "light")

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Wry};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_store::StoreExt;

use crate::error::{AppError, AppResult};

pub(crate) const STORE_FILE: &str = "settings.json";
pub(crate) const SETTINGS_KEY: &str = "settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub minimize_to_tray: bool,
    pub autostart: bool,
    pub latency_test_url: String,
    pub language: String,
    pub theme: String,
    /// shadcn theme palette name. One of: zinc, slate, blue, green,
    /// rose. Frontend applies via class on <html>; nothing here on
    /// the backend cares — it's just persisted.
    #[serde(default = "default_theme_color")]
    pub theme_color: String,
    /// TUN runtime overlay: when true, ensure the active config has a TUN
    /// inbound when sing-box starts; when false, strip TUN inbounds from
    /// the runtime copy. The user's source file is never modified either
    /// way. Defaults to false (off) for safety on first run.
    #[serde(default)]
    pub tun_enabled: bool,
    /// Routing mode: "rule" (use user's route.rules), "global" (everything
    /// goes through the GLOBAL selector), "direct" (everything bypasses
    /// proxies). Applied at sing-box start time as a runtime overlay on
    /// the merged config — user's source file is never modified. sing-box
    /// 1.13's clash_api PATCH /configs doesn't actually swap mode, so we
    /// implement this as a restart-driven config rewrite (same model as
    /// the TUN toggle).
    #[serde(default = "default_proxy_mode")]
    pub proxy_mode: String,
    /// Local proxy port runtime overlays. None = leave the user's
    /// inbound for that protocol untouched (or absent if user didn't
    /// configure one). Some(p) = inject/replace an inbound listening
    /// 127.0.0.1:p for that protocol. Same zero-loss principle as TUN:
    /// only the runtime config is changed, never the user's file.
    #[serde(default)]
    pub mixed_port: Option<u16>,
    #[serde(default)]
    pub socks_port: Option<u16>,
    #[serde(default)]
    pub http_port: Option<u16>,
    /// Identifier of the currently-selected sing-box binary. None →
    /// use the bundled sidecar; Some("v1.10.7") → look up under
    /// `data_dir/binaries/v1.10.7/sing-box[.exe]`. The Dashboard
    /// version-picker writes this; `core_start` reads it.
    #[serde(default)]
    pub selected_singbox_version: Option<String>,
}

fn default_proxy_mode() -> String {
    "rule".into()
}

fn default_theme_color() -> String {
    "zinc".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            minimize_to_tray: true,
            autostart: false,
            latency_test_url: "https://www.gstatic.com/generate_204".into(),
            language: "en".into(),
            theme: "dark".into(),
            theme_color: default_theme_color(),
            tun_enabled: false,
            proxy_mode: default_proxy_mode(),
            mixed_port: Some(7890),
            socks_port: None,
            http_port: None,
            selected_singbox_version: None,
        }
    }
}

fn load_or_default(app: &AppHandle<Wry>) -> Settings {
    match app.store(STORE_FILE) {
        Ok(store) => store
            .get(SETTINGS_KEY)
            .and_then(|v| serde_json::from_value::<Settings>(v).ok())
            .unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

fn persist(app: &AppHandle<Wry>, s: &Settings) -> AppResult<()> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| AppError::Other(format!("open store: {e}")))?;
    let v = serde_json::to_value(s)?;
    store.set(SETTINGS_KEY, v);
    store
        .save()
        .map_err(|e| AppError::Other(format!("save store: {e}")))?;
    Ok(())
}

/// Reflect the persisted autostart flag into the OS via the autostart
/// plugin. Best-effort; logs but doesn't fail the whole call.
fn sync_autostart(app: &AppHandle<Wry>, want_enabled: bool) {
    let manager = app.autolaunch();
    let currently = manager.is_enabled().unwrap_or(false);
    if currently == want_enabled {
        return;
    }
    let res = if want_enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(e) = res {
        tracing::warn!(?e, "autostart {} failed", if want_enabled { "enable" } else { "disable" });
    }
}

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> AppResult<Settings> {
    Ok(load_or_default(&app))
}

#[tauri::command]
pub async fn settings_set(
    patch: serde_json::Value,
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
) -> AppResult<Settings> {
    // Serialise the whole patch+restart window. Without this, a rapid
    // TUN→mode→TUN sequence from the sidebar fires three concurrent
    // settings_set calls; each reads `current` from the store before any
    // of them have written, so later calls overwrite earlier ones and
    // the user-visible toggle stops responding. Holding this lock until
    // core_restart finishes also keeps the running-state coherent for
    // the Proxies refresh that runs right after.
    let _guard = state.settings_op.lock().await;
    let mut current = load_or_default(&app);
    let prev = current.clone();
    // Shallow merge: each present key in `patch` overrides current. Lets
    // the frontend send partial updates.
    if let Some(obj) = patch.as_object() {
        if let Some(v) = obj.get("minimize_to_tray").and_then(|v| v.as_bool()) {
            current.minimize_to_tray = v;
        }
        if let Some(v) = obj.get("autostart").and_then(|v| v.as_bool()) {
            current.autostart = v;
        }
        if let Some(v) = obj.get("latency_test_url").and_then(|v| v.as_str()) {
            current.latency_test_url = v.to_string();
        }
        if let Some(v) = obj.get("language").and_then(|v| v.as_str()) {
            current.language = v.to_string();
        }
        if let Some(v) = obj.get("theme").and_then(|v| v.as_str()) {
            current.theme = v.to_string();
        }
        if let Some(v) = obj.get("theme_color").and_then(|v| v.as_str()) {
            current.theme_color = v.to_string();
        }
        if let Some(v) = obj.get("tun_enabled").and_then(|v| v.as_bool()) {
            current.tun_enabled = v;
        }
        if let Some(v) = obj.get("proxy_mode").and_then(|v| v.as_str()) {
            current.proxy_mode = v.to_string();
        }
        // Ports accept null (clear) or an integer (set). The frontend sends
        // the literal `null` JSON value when the user toggles a port off.
        if obj.contains_key("mixed_port") {
            current.mixed_port = obj.get("mixed_port").and_then(|v| v.as_u64()).and_then(|n| u16::try_from(n).ok());
        }
        if obj.contains_key("socks_port") {
            current.socks_port = obj.get("socks_port").and_then(|v| v.as_u64()).and_then(|n| u16::try_from(n).ok());
        }
        if obj.contains_key("http_port") {
            current.http_port = obj.get("http_port").and_then(|v| v.as_u64()).and_then(|n| u16::try_from(n).ok());
        }
        if obj.contains_key("selected_singbox_version") {
            current.selected_singbox_version = obj
                .get("selected_singbox_version")
                .and_then(|v| v.as_str())
                .map(String::from);
        }
    }
    // Backend guard: tun_enabled flipping ON without privileges should
    // surface a structured PrivilegeRequired error so the frontend opens
    // the privilege dialog instead of starting sing-box, hitting the
    // 30s wait_ready timeout, and crashing it. macOS is excepted because
    // its osascript admin shell handles the prompt at spawn time.
    if !prev.tun_enabled && current.tun_enabled && !cfg!(target_os = "macos") {
        let report = crate::commands::core_cmd::core_check_privilege(app.clone()).await?;
        if !report.tun_capable {
            // Roll back the tun_enabled change so the persisted file
            // doesn't claim TUN is on while it actually can't run.
            current.tun_enabled = false;
            persist(&app, &current)?;
            return Err(crate::error::AppError::PrivilegeRequired {
                platform: detect_platform_label(),
                hint: report.hint,
            });
        }
    }

    persist(&app, &current)?;
    sync_autostart(&app, current.autostart);

    // If a runtime-overlay-relevant field changed AND sing-box is currently
    // running, restart so the overlay takes effect immediately. Pure UI
    // fields (theme/language) don't trigger this.
    let overlay_changed = prev.tun_enabled != current.tun_enabled
        || prev.proxy_mode != current.proxy_mode
        || prev.mixed_port != current.mixed_port
        || prev.socks_port != current.socks_port
        || prev.http_port != current.http_port;
    if overlay_changed && state.core.lock().running {
        let _ = crate::commands::core_cmd::core_restart(app.clone(), state.clone()).await;
    }

    Ok(current)
}

fn detect_platform_label() -> String {
    if cfg!(target_os = "linux") {
        "linux".into()
    } else if cfg!(target_os = "windows") {
        "windows".into()
    } else if cfg!(target_os = "macos") {
        "macos".into()
    } else {
        "other".into()
    }
}

/// Convenience: read the persisted tun_enabled flag.
pub fn current_tun_enabled(app: &AppHandle<Wry>) -> bool {
    load_or_default(app).tun_enabled
}

pub fn current_settings(app: &AppHandle<Wry>) -> Settings {
    load_or_default(app)
}

/// Used by the Windows relaunch-as-admin flow: persist `tun_enabled`
/// without touching any other field, bypassing the merge / restart
/// logic in `settings_set` (the current process is about to die).
pub fn force_set_tun_enabled(app: &AppHandle<Wry>, on: bool) -> AppResult<()> {
    let mut s = load_or_default(app);
    s.tun_enabled = on;
    persist(app, &s)
}

/// Called once during app startup so the OS state matches the persisted
/// preference even if the user toggled it from another session.
pub fn sync_on_startup(app: &AppHandle<Wry>) {
    let s = load_or_default(app);
    sync_autostart(app, s.autostart);
}
