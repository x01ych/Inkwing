use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::AppHandle;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio::sync::oneshot;

use crate::error::{AppError, AppResult};
use crate::util::ring_buffer::RingBuffer;

/// Whether to launch sing-box wrapped in a per-platform privilege
/// elevation shell. Currently only used on macOS, where without a
/// NetworkExtension entitlement the only way to bring up TUN is to
/// run sing-box itself as root via `osascript ... with administrator
/// privileges`. Linux relies on file capabilities (setcap) and
/// Windows on the parent process being elevated, so they ignore this
/// flag.
#[derive(Copy, Clone, Debug)]
pub enum ElevationMode {
    None,
    #[cfg(target_os = "macos")]
    MacosAdmin,
}

/// One-shot run of `<binary> <args>`, capture stdout, return on exit.
/// Used by `validate_with_binary` so the Apply flow can dry-run a
/// candidate version's `sing-box check` against the merged runtime
/// config before swapping the live core.
pub async fn run_binary_oneshot(binary: &Path, args: &[&str]) -> AppResult<String> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut child = tokio::process::Command::new(binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Sidecar(format!("spawn {}: {}", binary.display(), e)))?;

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    if let Some(out) = child.stdout.take() {
        let mut r = BufReader::new(out).lines();
        while let Ok(Some(line)) = r.next_line().await {
            stdout_buf.push_str(&line);
            stdout_buf.push('\n');
        }
    }
    if let Some(err) = child.stderr.take() {
        let mut r = BufReader::new(err).lines();
        while let Ok(Some(line)) = r.next_line().await {
            stderr_buf.push_str(&line);
            stderr_buf.push('\n');
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|e| AppError::Sidecar(format!("wait {}: {}", binary.display(), e)))?;
    if status.success() {
        Ok(stdout_buf.trim_end().to_string())
    } else {
        Err(AppError::SingBoxFailed(format!(
            "exit {}\nstderr:\n{}",
            status.code().unwrap_or(-1),
            stderr_buf.trim_end()
        )))
    }
}

/// One-shot run of `sing-box <args>`, capture stdout, return on exit.
/// Used for `version` and `check`. NOT for `run`.
pub async fn run_sidecar_oneshot(app: &AppHandle, args: &[&str]) -> AppResult<String> {
    let cmd = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| AppError::Sidecar(e.to_string()))?
        .args(args);

    let (mut rx, _child) = cmd
        .spawn()
        .map_err(|e| AppError::Sidecar(e.to_string()))?;

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    let mut exit_code: Option<i32> = None;

    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(line) => {
                stdout_buf.push_str(&String::from_utf8_lossy(&line));
                stdout_buf.push('\n');
            }
            CommandEvent::Stderr(line) => {
                stderr_buf.push_str(&String::from_utf8_lossy(&line));
                stderr_buf.push('\n');
            }
            CommandEvent::Terminated(payload) => {
                exit_code = payload.code;
            }
            _ => {}
        }
    }

    match exit_code {
        Some(0) => Ok(stdout_buf.trim_end().to_string()),
        Some(code) => Err(AppError::SingBoxFailed(format!(
            "exit {code}\nstderr:\n{}",
            stderr_buf.trim_end()
        ))),
        None => Err(AppError::SingBoxFailed(format!(
            "no exit code\nstderr:\n{}",
            stderr_buf.trim_end()
        ))),
    }
}

/// Two flavours of running child: the normal Tauri sidecar (preferred)
/// and a native std-process child. The native variant is used in two
/// cases: macOS admin (osascript wrapper) and user-selected non-bundled
/// sing-box versions on any platform. Uses
/// `Arc<Mutex<Option<Child>>>` so the kill path and the exit-watcher
/// thread can both operate on the same Child without `Clone`.
pub enum ChildHandle {
    Sidecar(CommandChild),
    NativeShared(std::sync::Arc<std::sync::Mutex<Option<std::process::Child>>>),
}

impl ChildHandle {
    /// Best-effort terminate. Consumes the handle either way.
    pub fn kill(self) -> Result<(), String> {
        match self {
            ChildHandle::Sidecar(c) => c.kill().map_err(|e| e.to_string()),
            ChildHandle::NativeShared(arc) => {
                let mut g = arc.lock().map_err(|e| e.to_string())?;
                if let Some(mut c) = g.take() {
                    c.kill().map_err(|e| e.to_string())?;
                }
                Ok(())
            }
        }
    }
}

/// Live handle on a running sing-box. `core_stop` consumes one of these.
pub struct SidecarHandle {
    pub child: ChildHandle,
    pub pid: u32,
    /// stderr ring (last N=500 lines). Useful for crash diagnosis.
    pub stderr_ring: Arc<Mutex<RingBuffer<String>>>,
    /// Fires when the child exits on its own; consumers can listen to detect crashes.
    pub exit_rx: Arc<Mutex<Option<oneshot::Receiver<Option<i32>>>>>,
}

impl SidecarHandle {
    pub fn snapshot_stderr(&self) -> Vec<String> {
        self.stderr_ring.lock().snapshot()
    }
}

/// Spawn `sing-box run -c <config_path> --disable-color`, return a handle.
/// This task does NOT block until exit — caller drives readiness elsewhere.
///
/// `binary_override = None` → the Tauri-bundled sidecar.
/// `binary_override = Some(path)` → that explicit binary via
/// `std::process::Command` (used by the Dashboard's "switch sing-box
/// version" flow).
pub fn spawn_run(
    app: &AppHandle,
    config_path: &Path,
    elevation: ElevationMode,
    binary_override: Option<&Path>,
) -> AppResult<SidecarHandle> {
    let path_str = config_path
        .to_str()
        .ok_or_else(|| AppError::Config(format!("non-UTF-8 path: {}", config_path.display())))?;

    match elevation {
        ElevationMode::None => match binary_override {
            None => spawn_run_sidecar(app, path_str),
            Some(bin) => spawn_run_native(bin, path_str),
        },
        #[cfg(target_os = "macos")]
        ElevationMode::MacosAdmin => spawn_run_macos_admin(binary_override, path_str),
    }
}

fn spawn_run_sidecar(app: &AppHandle, path_str: &str) -> AppResult<SidecarHandle> {
    let cmd = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| AppError::Sidecar(e.to_string()))?
        .args(["run", "-c", path_str, "--disable-color"]);

    let (mut rx, child) = cmd
        .spawn()
        .map_err(|e| AppError::Sidecar(e.to_string()))?;

    let pid = child.pid();

    // Windows: drop the child into a Job Object with KILL_ON_JOB_CLOSE.
    // When this GUI process exits (any path — graceful, panic, crash,
    // taskkill /F), the kernel auto-closes our handle to the job, and
    // KILL_ON_JOB_CLOSE then terminates every process in it. Without
    // this, tauri-plugin-shell leaves sing-box.exe orphaned on hard
    // exit because it doesn't set up child-process job control.
    #[cfg(target_os = "windows")]
    if let Err(e) = win_assign_to_job(pid) {
        tracing::warn!(?e, "failed to assign sing-box pid {} to job object", pid);
    }

    let stderr_ring = Arc::new(Mutex::new(RingBuffer::<String>::new(500)));
    let stderr_ring_for_task = stderr_ring.clone();

    let (exit_tx, exit_rx) = oneshot::channel::<Option<i32>>();

    // Pump stdout/stderr lines and surface exit via oneshot.
    tokio::spawn(async move {
        let mut exit_code: Option<i32> = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stdout(line) | CommandEvent::Stderr(line) => {
                    let s = String::from_utf8_lossy(&line).to_string();
                    if !s.trim().is_empty() {
                        stderr_ring_for_task.lock().push(s);
                    }
                }
                CommandEvent::Terminated(payload) => {
                    exit_code = payload.code;
                }
                _ => {}
            }
        }
        // rx closed → child exited. Send is best-effort — receiver may
        // already have been taken by stop_sidecar.
        let _ = exit_tx.send(exit_code);
    });

    Ok(SidecarHandle {
        child: ChildHandle::Sidecar(child),
        pid,
        stderr_ring,
        exit_rx: Arc::new(Mutex::new(Some(exit_rx))),
    })
}

/// Spawn an arbitrary sing-box binary via `std::process::Command`.
/// Used when the user picks a non-bundled version on the Dashboard.
/// stdout / stderr are piped back into the same RingBuffer the
/// sidecar path uses, exit detected via try_wait poll → oneshot.
fn spawn_run_native(binary: &Path, config_path_str: &str) -> AppResult<SidecarHandle> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let mut child = Command::new(binary)
        .args(["run", "-c", config_path_str, "--disable-color"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Sidecar(format!("spawn {}: {}", binary.display(), e)))?;

    let pid = child.id();
    #[cfg(target_os = "windows")]
    if let Err(e) = win_assign_to_job(pid) {
        tracing::warn!(?e, "failed to assign sing-box pid {} to job object", pid);
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stderr_ring = Arc::new(Mutex::new(RingBuffer::<String>::new(500)));
    let ring_stdout = stderr_ring.clone();
    let ring_stderr = stderr_ring.clone();
    if let Some(out) = stdout {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    ring_stdout.lock().push(line);
                }
            }
        });
    }
    if let Some(err) = stderr {
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    ring_stderr.lock().push(line);
                }
            }
        });
    }

    let (exit_tx, exit_rx) = oneshot::channel::<Option<i32>>();
    let child_arc = std::sync::Arc::new(std::sync::Mutex::new(Some(child)));
    let child_for_watcher = child_arc.clone();
    std::thread::spawn(move || loop {
        let status = {
            let mut g = child_for_watcher.lock().unwrap();
            match g.as_mut() {
                Some(c) => c.try_wait(),
                None => return,
            }
        };
        match status {
            Ok(Some(s)) => {
                let _ = exit_tx.send(s.code());
                return;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => return,
        }
    });

    Ok(SidecarHandle {
        child: ChildHandle::NativeShared(child_arc),
        pid,
        stderr_ring,
        exit_rx: Arc::new(Mutex::new(Some(exit_rx))),
    })
}

/// macOS-only: launch sing-box wrapped in `osascript` so it runs as
/// root. The user sees Touch ID/password prompt before the binary
/// starts. We do NOT use the Tauri sidecar here — `tauri-plugin-shell`
/// can't set the AppleScript privilege wrapping. Stderr/stdout are
/// merged and piped back; we read them on a background thread and
/// push lines into the same ring buffer the sidecar path uses.
///
/// `binary_override` is the user's chosen non-bundled sing-box, if
/// any; falls back to the bundled binary.
#[cfg(target_os = "macos")]
fn spawn_run_macos_admin(
    binary_override: Option<&Path>,
    path_str: &str,
) -> AppResult<SidecarHandle> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let bin = match binary_override {
        Some(p) => p.to_path_buf(),
        None => crate::commands::core_cmd::resolve_singbox_binary_path()
            .ok_or_else(|| AppError::Sidecar("bundled sing-box binary not found".into()))?,
    };
    let bin_str = bin
        .to_str()
        .ok_or_else(|| AppError::Sidecar("non-UTF-8 sing-box path".into()))?;

    // AppleScript-quote the inner shell command. Embedded double-quotes
    // and backslashes inside the binary path need escaping for both the
    // outer `osascript -e "..."` shell-arg layer (we sidestep that by
    // passing the whole script as a single argv element) and the inner
    // `do shell script "..."` AppleScript-string layer.
    fn applescript_quote(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '\\' | '"' => {
                    out.push('\\');
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    let inner = format!(
        "{} run -c {} --disable-color 2>&1",
        applescript_quote(bin_str),
        applescript_quote(path_str)
    );
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        inner.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let mut child = Command::new("osascript")
        .args(["-e", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Sidecar(format!("osascript spawn: {e}")))?;

    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stderr_ring = Arc::new(Mutex::new(RingBuffer::<String>::new(500)));
    let ring_stdout = stderr_ring.clone();
    let ring_stderr = stderr_ring.clone();

    if let Some(out) = stdout {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    ring_stdout.lock().push(line);
                }
            }
        });
    }
    if let Some(err) = stderr {
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    ring_stderr.lock().push(line);
                }
            }
        });
    }

    let (exit_tx, exit_rx) = oneshot::channel::<Option<i32>>();
    // Move ownership into a watcher thread so we can `wait()` without
    // blocking the runtime, then store the SidecarHandle separately.
    // We need to keep the Child around for kill() — so we DON'T move it
    // into the watcher. Instead poll via a kill-channel pattern: the
    // watcher periodically peeks try_wait().
    // Simpler: use a second thread that watches via try_wait every 200ms;
    // when exit detected, fire oneshot.
    //
    // We can't keep the same Child in both the watcher and the
    // ChildHandle::Native (Child is not Clone). So we accept that
    // explicit `kill()` from stop_sidecar happens on the ChildHandle's
    // Child, and the watcher uses an Arc<Mutex<Option<Child>>> to take
    // ownership only after exit.
    //
    // Final shape: ChildHandle::Native owns the Child via Mutex.
    let child_arc = std::sync::Arc::new(std::sync::Mutex::new(Some(child)));
    let child_for_watcher = child_arc.clone();
    std::thread::spawn(move || {
        loop {
            // Peek without consuming.
            let status = {
                let mut g = child_for_watcher.lock().unwrap();
                match g.as_mut() {
                    Some(c) => c.try_wait(),
                    None => return, // killed elsewhere
                }
            };
            match status {
                Ok(Some(s)) => {
                    let _ = exit_tx.send(s.code());
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                Err(_) => return,
            }
        }
    });

    Ok(SidecarHandle {
        child: ChildHandle::NativeShared(child_arc),
        pid,
        stderr_ring,
        exit_rx: Arc::new(Mutex::new(Some(exit_rx))),
    })
}

/// Best-effort graceful stop: send SIGTERM (kill() on Tauri's CommandChild
/// uses SIGKILL on Unix and TerminateProcess on Windows; sing-box handles
/// both — it always cleans up its TUN device). Wait up to `grace`, then
/// drop the handle.
pub async fn stop_sidecar(handle: SidecarHandle, grace: Duration) -> AppResult<Option<i32>> {
    let exit_rx = handle.exit_rx.lock().take();
    let _ = handle.child.kill();
    if let Some(rx) = exit_rx {
        match tokio::time::timeout(grace, rx).await {
            Ok(Ok(code)) => Ok(code),
            Ok(Err(_)) => Ok(None),
            Err(_) => {
                tracing::warn!("sing-box did not exit within {:?}, leaving zombie reaping to OS", grace);
                Ok(None)
            }
        }
    } else {
        Ok(None)
    }
}

/// Windows-only: create a Job Object with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
/// and assign `pid` to it. The job handle is intentionally leaked into
/// the process — that keeps the job alive for the rest of the GUI's
/// lifetime. When the GUI process terminates (graceful or hard kill),
/// the kernel auto-closes its handle, which fires KILL_ON_JOB_CLOSE and
/// kills every process in the job (i.e. our sing-box sidecar).
///
/// This is the only reliable defence against the "GUI dies → sing-box
/// orphan" scenario. Userspace shutdown hooks (RunEvent::Exit, Drop
/// impls, panic catchers) can all be skipped if Windows kills the
/// process abruptly enough; Job Objects can't.
#[cfg(target_os = "windows")]
fn win_assign_to_job(pid: u32) -> windows::core::Result<()> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    unsafe {
        let job = CreateJobObjectW(None, windows::core::PCWSTR::null())?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;
        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)?;
        let assign_res = AssignProcessToJobObject(job, process);
        // Close our handle to the *process* either way — the job holds
        // its own reference and will keep tracking it.
        let _ = CloseHandle(process);
        assign_res?;
        // Intentionally do NOT close the job handle. windows::HANDLE is
        // a `Copy` int wrapper with no Drop — letting it fall out of
        // scope leaves the OS handle open. The kernel auto-closes it
        // when our own process exits, which fires KILL_ON_JOB_CLOSE
        // and tears down every process in the job.
        let _ = job;
    }
    Ok(())
}
