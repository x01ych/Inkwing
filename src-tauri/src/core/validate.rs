use std::path::Path;

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize)]
pub struct ValidationReport {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Serialize)]
pub struct ValidationError {
    pub level: String,
    pub message: String,
}

/// Run `sing-box check -c <path> --disable-color` via the sidecar. Exit 0 =>
/// ok=true, errors=[]. Non-zero => parse stderr lines into structured
/// errors. Stripped of ANSI color codes for safety in case --disable-color
/// is ignored on some build.
pub async fn validate_path(app: &AppHandle, path: &Path) -> AppResult<ValidationReport> {
    let path_str = path
        .to_str()
        .ok_or_else(|| AppError::Config(format!("non-UTF-8 path: {}", path.display())))?;

    let cmd = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| AppError::Sidecar(e.to_string()))?
        .args(["check", "-c", path_str, "--disable-color"]);

    let (mut rx, _child) = cmd
        .spawn()
        .map_err(|e| AppError::Sidecar(e.to_string()))?;

    let mut stderr_lines: Vec<String> = Vec::new();
    let mut exit_code: Option<i32> = None;

    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stderr(line) => {
                let s = strip_ansi(&String::from_utf8_lossy(&line));
                if !s.trim().is_empty() {
                    stderr_lines.push(s);
                }
            }
            CommandEvent::Stdout(_) => {}
            CommandEvent::Terminated(p) => exit_code = p.code,
            _ => {}
        }
    }

    let ok = matches!(exit_code, Some(0));
    let errors = if ok {
        Vec::new()
    } else {
        stderr_lines
            .into_iter()
            .map(parse_log_line)
            .collect()
    };
    Ok(ValidationReport { ok, exit_code, errors })
}

fn parse_log_line(line: String) -> ValidationError {
    // Format observed: "FATAL[0000] decode config at /tmp/x.json: <msg>"
    // or "INFO[0000] ..." or plain "panic: ..."
    let trimmed = line.trim().to_string();
    let (level, message) = if let Some(rest) = trimmed.strip_prefix("FATAL") {
        ("FATAL", strip_log_prefix(rest))
    } else if let Some(rest) = trimmed.strip_prefix("ERROR") {
        ("ERROR", strip_log_prefix(rest))
    } else if let Some(rest) = trimmed.strip_prefix("WARN") {
        ("WARN", strip_log_prefix(rest))
    } else if let Some(rest) = trimmed.strip_prefix("PANIC") {
        ("PANIC", strip_log_prefix(rest))
    } else {
        ("ERROR", trimmed.clone())
    };
    ValidationError {
        level: level.to_string(),
        message,
    }
}

/// Drop the "[0000] " timestamp marker after a level token.
fn strip_log_prefix(rest: &str) -> String {
    let r = rest.trim_start();
    let r = if r.starts_with('[') {
        if let Some(end) = r.find(']') {
            &r[end + 1..]
        } else {
            r
        }
    } else {
        r
    };
    r.trim().to_string()
}

/// Minimal ANSI CSI / SGR stripper. Handles `\x1b[…m` and `\x1b[…K` etc.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // skip until a final byte (0x40..=0x7e)
            i += 2;
            while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}
