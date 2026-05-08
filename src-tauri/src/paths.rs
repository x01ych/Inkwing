use std::path::PathBuf;

use directories::ProjectDirs;
use once_cell::sync::Lazy;

use crate::error::{AppError, AppResult};

/// Project data dirs:
///   macOS:   ~/Library/Application Support/dev.inkwing.Inkwing/
///   Linux:   ~/.local/share/inkwing/
///   Windows: %APPDATA%\inkwing\Inkwing\data\
static PROJECT_DIRS: Lazy<Option<ProjectDirs>> =
    Lazy::new(|| ProjectDirs::from("dev", "inkwing", "Inkwing"));

pub fn data_dir() -> AppResult<PathBuf> {
    PROJECT_DIRS
        .as_ref()
        .map(|d| d.data_dir().to_path_buf())
        .ok_or_else(|| AppError::Other("could not determine data directory".into()))
}

/// Where we write the merged runtime config (user config + injected
/// experimental.clash_api). NEVER the user's source file.
pub fn runtime_config_path() -> AppResult<PathBuf> {
    Ok(data_dir()?.join("runtime").join("config.json"))
}

/// Storage root for managed config library entries. Each entry lives at
/// `<configs_dir>/<uuid>.json`. We copy local-source files in here so
/// edits don't touch the user's hard-disk file (and so the storage
/// location is uniform regardless of source).
pub fn configs_dir() -> AppResult<PathBuf> {
    Ok(data_dir()?.join("configs"))
}

/// Where sing-box's `experimental.cache_file` (rule-set cache + URL-test
/// latency history) lives. Forced to an absolute path under our data
/// dir so a stale cache.db in the working directory (left by an orphan
/// from a previous run) can't lock the new instance.
pub fn cache_file_path() -> AppResult<PathBuf> {
    Ok(data_dir()?.join("cache.db"))
}

/// Storage root for `LocalOverrides` (per-config) and `GlobalOverrides` files.
pub fn overrides_dir() -> AppResult<PathBuf> {
    Ok(data_dir()?.join("overrides"))
}

/// Per-config overrides file: `<data_dir>/overrides/<entry_id>.json`.
pub fn per_config_overrides_path(entry_id: &str) -> AppResult<PathBuf> {
    Ok(overrides_dir()?.join(format!("{entry_id}.json")))
}

/// Global overrides file: `<data_dir>/overrides/global.json`.
pub fn global_overrides_path() -> AppResult<PathBuf> {
    Ok(overrides_dir()?.join("global.json"))
}
