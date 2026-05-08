use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::core::clash_api::ClashClient;
use crate::core::clash_inject::{
    apply_cache_file_overlay, apply_local_ports_overlay, apply_mode_overlay, apply_tun_overlay,
    inject_clash_api,
};
use crate::core::conn_pump::spawn_conn_pump;
use crate::core::log_pump::{spawn_log_pump, LogEntry};
use crate::core::process::{run_sidecar_oneshot, spawn_run, stop_sidecar, ElevationMode};
use crate::core::traffic_pump::spawn_traffic_pump;
use crate::error::{AppError, AppResult};
use crate::paths::{
    cache_file_path, global_overrides_path, per_config_overrides_path, runtime_config_path,
};
use crate::state::AppState;
use crate::util::atomic_write::atomic_write;

/// Spawn the bundled sing-box once with `version` and return its stdout.
#[tauri::command]
pub async fn core_version(app: AppHandle) -> AppResult<String> {
    run_sidecar_oneshot(&app, &["version"]).await
}

#[derive(Debug, Serialize, Clone)]
pub struct CoreStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub version: Option<String>,
    pub started_at_ms: Option<u64>,
    pub clash_api_addr: Option<String>,
    /// Current session epoch (0 before the first start). Pump events are
    /// stamped with this; the frontend uses it to ignore stragglers from
    /// a just-stopped session.
    pub epoch: u64,
    /// Last few stderr lines — handy for diagnosing crashes from the UI.
    pub recent_stderr: Vec<String>,
}

#[tauri::command]
pub async fn core_status(state: State<'_, AppState>) -> AppResult<CoreStatus> {
    let epoch = state.session_epoch.load(Ordering::SeqCst);
    let g = state.core.lock();
    let recent_stderr = g
        .handle
        .as_ref()
        .map(|h| h.snapshot_stderr())
        .unwrap_or_default();
    Ok(CoreStatus {
        running: g.running,
        pid: g.pid,
        version: g.version.clone(),
        started_at_ms: g.started_at_ms,
        clash_api_addr: g.clash_api_addr.clone(),
        epoch,
        recent_stderr,
    })
}

#[tauri::command]
pub async fn core_start(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CoreStatus> {
    // Idempotent: if already running, return the current status without
    // spawning a second sing-box. (Without this, a double-click on Start
    // — or the React 18 StrictMode dev double-effect — silently leaks the
    // first sidecar handle and starts a second process competing for
    // wintun / clash_api ports.)
    {
        let g = state.core.lock();
        if g.running {
            return Ok(CoreStatus {
                running: true,
                pid: g.pid,
                version: g.version.clone(),
                started_at_ms: g.started_at_ms,
                clash_api_addr: g.clash_api_addr.clone(),
                epoch: state.session_epoch.load(Ordering::SeqCst),
                recent_stderr: g
                    .handle
                    .as_ref()
                    .map(|h| h.snapshot_stderr())
                    .unwrap_or_default(),
            });
        }
    }

    // 1. Snapshot the parsed user config (we don't write to it).
    let user_value = {
        let cfg = state.config.lock();
        cfg.parsed
            .clone()
            .ok_or_else(|| AppError::Config("no config loaded — open one first".into()))?
    };

    // 2. Inject experimental.clash_api with a port + secret we control,
    //    then apply the runtime TUN + local-ports overlays (from settings).
    let mut injected = inject_clash_api(&user_value)?;
    let s = crate::commands::settings_cmd::current_settings(&app);
    // Order matters: mode overlay rewrites route.rules / route.final, so it
    // must run before any later step that might inspect routes. TUN +
    // local ports only touch inbounds, so they're independent of mode.
    apply_mode_overlay(&mut injected.merged, &s.proxy_mode)?;
    apply_tun_overlay(&mut injected.merged, Some(s.tun_enabled))?;
    apply_local_ports_overlay(
        &mut injected.merged,
        s.mixed_port,
        s.socks_port,
        s.http_port,
    )?;
    // Force cache_file.path to an absolute path under our data dir so an
    // orphan sing-box (likely on macOS / Linux where we have no Job Object
    // equivalent) holding a stale ./cache.db lock can't deadlock us with
    // "initialize cache-file: timeout".
    let cache_path = cache_file_path()?;
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    apply_cache_file_overlay(&mut injected.merged, &cache_path)?;

    // Merge user-managed overrides (per-config + global) *after* every
    // other overlay. The user's source config file is never modified;
    // their GUI-level rule edits live in
    // <data_dir>/overrides/{<entry_id>.json,global.json} and only
    // appear here in the runtime/config.json that sing-box reads.
    {
        let active_id = state.config.lock().active_id.clone();
        let per = match active_id {
            Some(ref id) => crate::core::overrides::load_per_config(
                &per_config_overrides_path(id)?,
            ),
            None => crate::core::overrides::LocalOverrides::default(),
        };
        let global =
            crate::core::overrides::load_global(&global_overrides_path()?);
        crate::core::overrides::apply_overrides_overlay(&mut injected.merged, &per, &global)?;
    }

    // 3. Write merged config to <data_dir>/runtime/config.json (atomic).
    let runtime_path = runtime_config_path()?;
    let bytes = serde_json::to_vec_pretty(&injected.merged)?;
    atomic_write(&runtime_path, &bytes)?;

    // 4. Spawn sing-box run -c <runtime_path>.
    //    macOS without an entitlement cannot manage TUN devices unless
    //    sing-box itself runs as root — wrap via osascript admin shell
    //    when TUN is active. Other platforms ignore this flag.
    #[cfg(target_os = "macos")]
    let elevation = if s.tun_enabled {
        ElevationMode::MacosAdmin
    } else {
        ElevationMode::None
    };
    #[cfg(not(target_os = "macos"))]
    let elevation = ElevationMode::None;
    let handle = spawn_run(&app, &runtime_path, elevation)?;
    let pid = handle.pid;

    // 5. Wait for /version. Rule-set heavy configs need >10s on first
    //    start (no cache yet) — give 30s before declaring failure.
    let client = ClashClient::new(&injected.addr, &injected.secret);
    let version_info = match client.wait_ready(Duration::from_secs(30)).await {
        Ok(v) => v,
        Err(e) => {
            let stderr_tail = handle.snapshot_stderr().join("\n");
            let _ = stop_sidecar(handle, Duration::from_secs(2)).await;
            return Err(AppError::SingBoxFailed(format!(
                "{e}\nrecent stderr:\n{stderr_tail}"
            )));
        }
    };

    // 6. Reset logs ring (new core session = new log stream), bump the
    //    session epoch, and start the three pumps stamped with it.
    state.logs.lock().clear();
    let epoch = state.session_epoch.fetch_add(1, Ordering::SeqCst) + 1;

    let traffic_task = spawn_traffic_pump(
        app.clone(),
        injected.addr.clone(),
        injected.secret.clone(),
        epoch,
    );
    let log_task = spawn_log_pump(
        app.clone(),
        injected.addr.clone(),
        injected.secret.clone(),
        state.logs.clone(),
        epoch,
    );
    let conn_task = spawn_conn_pump(
        app.clone(),
        injected.addr.clone(),
        injected.secret.clone(),
        epoch,
    );

    // 7. Commit to AppState.
    let started_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let exit_rx_for_watcher = handle.exit_rx.clone();
    {
        let mut g = state.core.lock();
        g.running = true;
        g.pid = Some(pid);
        g.clash_api_addr = Some(injected.addr.clone());
        g.clash_api_secret = Some(injected.secret.clone());
        g.version = Some(version_info.version.clone());
        g.started_at_ms = Some(started_at_ms);
        g.handle = Some(handle);
        g.traffic_task = Some(traffic_task);
        g.log_task = Some(log_task);
        g.conn_task = Some(conn_task);
    }

    // Watch for unexpected sing-box exit (TUN bring-up race / OS sleep
    // tearing the wintun adapter / OOM). Without this, a self-death
    // leaves the UI showing "running" forever and the three pumps
    // reconnecting in a loop. The user-initiated stop path in core_stop
    // takes the same exit_rx first, so the watcher only fires on a
    // genuine self-exit.
    let watcher_app = app.clone();
    tokio::spawn(async move {
        let rx = exit_rx_for_watcher.lock().take();
        let Some(rx) = rx else { return };
        let code = match rx.await {
            Ok(c) => c,
            Err(_) => None, // sender dropped (stop_sidecar consumed handle)
        };
        let st = watcher_app.state::<crate::state::AppState>();
        let was_running = {
            let mut g = st.core.lock();
            if !g.running {
                false
            } else {
                g.running = false;
                g.pid = None;
                g.handle = None;
                if let Some(t) = g.traffic_task.take() { t.abort(); }
                if let Some(t) = g.log_task.take() { t.abort(); }
                if let Some(t) = g.conn_task.take() { t.abort(); }
                true
            }
        };
        if was_running {
            let _ = watcher_app.emit(
                "core:state",
                serde_json::json!({ "kind": "crashed", "code": code }),
            );
        }
    });

    let _ = app.emit(
        "core:state",
        serde_json::json!({ "kind": "started", "epoch": epoch }),
    );
    Ok(CoreStatus {
        running: true,
        pid: Some(pid),
        version: Some(version_info.version),
        started_at_ms: Some(started_at_ms),
        clash_api_addr: Some(injected.addr),
        epoch,
        recent_stderr: vec![],
    })
}

#[tauri::command]
pub async fn core_stop(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (handle_opt, traffic_t, log_t, conn_t) = {
        let mut g = state.core.lock();
        let h = g.handle.take();
        let t1 = g.traffic_task.take();
        let t2 = g.log_task.take();
        let t3 = g.conn_task.take();
        g.running = false;
        g.pid = None;
        g.version = None;
        g.started_at_ms = None;
        g.clash_api_addr = None;
        g.clash_api_secret = None;
        (h, t1, t2, t3)
    };

    let pumps: Vec<_> = [traffic_t, log_t, conn_t].into_iter().flatten().collect();
    for t in &pumps {
        t.abort();
    }
    // Wait briefly for the aborted pumps to actually finish their current
    // iteration. Without this, an in-flight `app.emit` from the old session
    // can land after we've already emitted `core:state stopped`, and on a
    // restart we leak it into the new session's pump streams. 500ms is
    // generous; pumps yield at every WS frame / sleep boundary.
    if !pumps.is_empty() {
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            futures::future::join_all(pumps),
        )
        .await;
    }
    if let Some(h) = handle_opt {
        let _ = stop_sidecar(h, Duration::from_secs(3)).await;
    }
    let _ = app.emit("core:state", serde_json::json!({ "kind": "stopped" }));
    Ok(())
}

/// Snapshot the in-memory log ring (last N=2000 entries). Used by the
/// Logs page on mount to hydrate before live `logs:append` events arrive.
#[tauri::command]
pub async fn logs_recent(state: State<'_, AppState>) -> AppResult<Vec<LogEntry>> {
    Ok(state.logs.lock().snapshot())
}

/// Restart sing-box: stop (idempotent if already stopped) then start
/// against the current active config + current TUN setting. Used by the
/// "Restart core" button and after edits that need to take effect.
#[tauri::command]
pub async fn core_restart(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CoreStatus> {
    let _ = core_stop(app.clone(), state.clone()).await;
    core_start(app, state).await
}

#[derive(Debug, Serialize)]
pub struct PrivilegeReport {
    pub tun_capable: bool,
    pub hint: String,
}

/// Resolve the path to the bundled sing-box binary in priority order:
///   1. Production: next to the current executable (Tauri 2 strips the
///      target-triple suffix when bundling).
///   2. Production fallback: same dir but with the dev-style
///      target-triple suffix.
///   3. Dev: source-tree paths (`src-tauri/binaries/...`).
///
/// Returns `None` if no candidate exists. Used by the privilege check,
/// the Linux pkexec setcap command, and the macOS osascript launcher.
pub(crate) fn resolve_singbox_binary_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    let suffix = if cfg!(target_os = "linux") {
        "sing-box-x86_64-unknown-linux-gnu"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "sing-box-aarch64-apple-darwin"
    } else if cfg!(target_os = "macos") {
        "sing-box-x86_64-apple-darwin"
    } else if cfg!(target_os = "windows") {
        "sing-box-x86_64-pc-windows-msvc.exe"
    } else {
        "sing-box"
    };

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Tauri-bundled location.
            candidates.push(dir.join(if cfg!(target_os = "windows") {
                "sing-box.exe"
            } else {
                "sing-box"
            }));
            candidates.push(dir.join(suffix));
        }
    }
    // Dev source-tree fallbacks (host run + container bind-mount).
    candidates.push(PathBuf::from(format!("src-tauri/binaries/{suffix}")));
    candidates.push(PathBuf::from(format!(
        "/workspace/src-tauri/binaries/{suffix}"
    )));
    candidates.into_iter().find(|p| p.exists())
}

/// Check whether the bundled sing-box binary can bring up TUN. Linux:
/// inspect file capabilities. Windows: check whether our own process token
/// is elevated (sing-box inherits). macOS: always reports false because
/// authorization is per-session via osascript admin shell — the toggle
/// flow funnels through the privilege dialog every time TUN goes ON.
#[tauri::command]
pub async fn core_check_privilege(_app: AppHandle) -> AppResult<PrivilegeReport> {
    #[cfg(target_os = "linux")]
    {
        // The `caps` crate covers process caps but not file caps; reading
        // file caps via `security.capability` xattr requires decoding a
        // binary VFS_CAP_DATA struct. Easier: shell out to getcap.
        use caps::{has_cap, CapSet, Capability};
        use std::process::Command;

        let bin = resolve_singbox_binary_path();

        let tun_capable = match bin.as_ref() {
            Some(p) => Command::new("getcap")
                .arg(p)
                .output()
                .ok()
                .map(|o| {
                    let s = String::from_utf8_lossy(&o.stdout);
                    s.contains("cap_net_admin")
                })
                .unwrap_or(false),
            None => has_cap(None, CapSet::Effective, Capability::CAP_NET_ADMIN).unwrap_or(false),
        };

        let hint = if tun_capable {
            "TUN capable: sing-box has CAP_NET_ADMIN".into()
        } else {
            "sing-box needs CAP_NET_ADMIN. Click TUN to grant it via PolicyKit (pkexec).".into()
        };
        return Ok(PrivilegeReport { tun_capable, hint });
    }

    #[cfg(target_os = "windows")]
    {
        // Tauri runs in user space; whether TUN can come up depends on
        // whether we (and sing-box) are elevated.
        use windows::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let mut elevated = false;
        unsafe {
            let mut token = windows::Win32::Foundation::HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_ok() {
                let mut elevation = TOKEN_ELEVATION::default();
                let mut ret_len = 0u32;
                if GetTokenInformation(
                    token,
                    TokenElevation,
                    Some(&mut elevation as *mut _ as *mut _),
                    std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                    &mut ret_len,
                )
                .is_ok()
                {
                    elevated = elevation.TokenIsElevated != 0;
                }
                let _ = windows::Win32::Foundation::CloseHandle(token);
            }
        }
        let hint = if elevated {
            "TUN capable: process is elevated".into()
        } else {
            "TUN requires Administrator. Click TUN to relaunch elevated (UAC).".into()
        };
        return Ok(PrivilegeReport {
            tun_capable: elevated,
            hint,
        });
    }

    #[cfg(target_os = "macos")]
    {
        // macOS without a NetworkExtension entitlement always needs
        // per-session admin authorization to manage TUN devices. Report
        // not-capable so the toggle flow opens the privilege dialog and
        // fires osascript admin every time TUN goes ON.
        Ok(PrivilegeReport {
            tun_capable: false,
            hint: "macOS will request admin authorization (Touch ID / password) when TUN starts."
                .into(),
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Ok(PrivilegeReport {
            tun_capable: false,
            hint: "Unsupported platform for TUN".into(),
        })
    }
}

/// Linux only: invoke `pkexec setcap cap_net_admin,cap_net_bind_service=+ep
/// <bundled-singbox>` so the sidecar can open `/dev/net/tun` without root.
/// PolicyKit shows a graphical password prompt; success is persistent
/// (capability stored on the file's xattr).
#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn core_grant_tun_capability_linux(_app: AppHandle) -> AppResult<()> {
    use std::process::Command;

    let bin = resolve_singbox_binary_path()
        .ok_or_else(|| AppError::Sidecar("bundled sing-box binary not found".into()))?;
    let bin_str = bin
        .to_str()
        .ok_or_else(|| AppError::Sidecar("non-UTF-8 binary path".into()))?;

    let out = Command::new("pkexec")
        .args([
            "setcap",
            "cap_net_admin,cap_net_bind_service=+ep",
            bin_str,
        ])
        .output()
        .map_err(|e| {
            AppError::Sidecar(format!(
                "could not invoke pkexec ({e}) — is policykit-1 installed? \
                 Fallback: run `sudo bash scripts/grant-tun-cap.sh`."
            ))
        })?;
    if !out.status.success() {
        return Err(AppError::Sidecar(format!(
            "setcap failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Windows only: persist `tun_enabled=true`, stop the current sidecar,
/// then relaunch this same exe via `Start-Process -Verb RunAs` (UAC) and
/// exit. The new admin instance reads the persisted setting and brings
/// up TUN natively. UAC cancellation leaves the original process gone
/// and `tun_enabled=true` persisted; the boot-time guard re-opens the
/// dialog on the next launch.
#[cfg(target_os = "windows")]
#[tauri::command]
pub async fn core_relaunch_as_admin_windows(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    use std::process::Command;

    // Persist tun_enabled=true so the new instance auto-starts TUN.
    crate::commands::settings_cmd::force_set_tun_enabled(&app, true)?;

    // Best-effort stop so the old process releases wintun.dll before
    // the elevated instance tries to grab it.
    let _ = core_stop(app.clone(), state.clone()).await;

    let exe = std::env::current_exe()
        .map_err(|e| AppError::Sidecar(format!("current_exe: {e}")))?;
    let exe_str = exe.to_string_lossy().to_string();
    // PowerShell quoting: wrap the exe path in single-quotes so spaces
    // (Program Files) survive. Embedded apostrophes get doubled.
    let escaped = exe_str.replace('\'', "''");
    let ps_cmd = format!("Start-Process -FilePath '{escaped}' -Verb RunAs");

    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &ps_cmd,
        ])
        .spawn()
        .map_err(|e| AppError::Sidecar(format!("powershell Start-Process failed: {e}")))?;

    // Give PowerShell a beat to fire the UAC prompt before this process
    // dies — otherwise the consent dialog can be racing the parent's
    // exit and Windows may discard it.
    tokio::time::sleep(Duration::from_millis(250)).await;
    app.exit(0);
    Ok(())
}

/// macOS only: probe whether the user can authorize admin operations by
/// running an empty admin shell. Returns Ok if the user accepts the
/// Touch ID/password prompt; Err if they cancel or osascript fails.
/// The caller should then flip `tun_enabled=true` — the actual sing-box
/// admin spawn happens inside `process::spawn_run` and will re-prompt.
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn core_test_macos_admin(_app: AppHandle) -> AppResult<()> {
    use std::process::Command;

    let out = Command::new("osascript")
        .args([
            "-e",
            "do shell script \"true\" with administrator privileges",
        ])
        .output()
        .map_err(|e| AppError::Sidecar(format!("osascript: {e}")))?;
    if !out.status.success() {
        return Err(AppError::Sidecar(format!(
            "authorization declined: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}
