//! Manage user-downloaded sing-box binaries.
//!
//! By default Inkwing ships with one bundled sing-box (the Tauri
//! sidecar). The Dashboard's version-picker lets the user fetch other
//! releases from GitHub on demand and switch the running core to them
//! without re-installing the app.
//!
//! On-disk layout:
//!   <data_dir>/binaries/<version>/sing-box[.exe]
//!
//! The version string is exactly what GitHub releases use (`v1.10.7`).
//! `Settings.selected_singbox_version = Some("v1.10.7")` ⇒ that binary
//! is used at `core_start`. `None` ⇒ falls back to the bundled sidecar.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::commands::settings_cmd::Settings;
use crate::error::{AppError, AppResult};
use crate::paths::data_dir;

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/SagerNet/sing-box/releases?per_page=30";
const USER_AGENT: &str = "Inkwing/0.1";

#[derive(Debug, Clone, Serialize)]
pub struct InstalledBinary {
    /// "v1.10.7" or "bundled".
    pub version: String,
    pub path: PathBuf,
    pub is_bundled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseAsset {
    pub version: String,
    pub asset_url: String,
    pub asset_name: String,
    pub size: u64,
    pub published_at: String,
    pub prerelease: bool,
    /// True if this exact version is already installed locally.
    pub installed: bool,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    published_at: Option<String>,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub fn binaries_dir() -> AppResult<PathBuf> {
    let d = data_dir()?.join("binaries");
    if !d.exists() {
        std::fs::create_dir_all(&d)
            .map_err(|e| AppError::Other(format!("create binaries dir: {e}")))?;
    }
    Ok(d)
}

pub fn version_dir(version: &str) -> AppResult<PathBuf> {
    Ok(binaries_dir()?.join(version))
}

fn binary_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "sing-box.exe"
    } else {
        "sing-box"
    }
}

/// Match `<asset_name>` against the running platform/arch. Returns true
/// for assets like `sing-box-<version>-linux-amd64.tar.gz` (Linux x86_64),
/// `darwin-arm64.tar.gz`, `windows-amd64.zip`, etc.
pub fn asset_matches_platform(asset_name: &str) -> bool {
    let n = asset_name.to_ascii_lowercase();
    let os_token = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        return false;
    };
    let arch_token = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        return false;
    };
    n.contains(os_token)
        && n.contains(arch_token)
        && (n.ends_with(".tar.gz") || n.ends_with(".tgz") || n.ends_with(".zip"))
        && !n.contains("legacy")
}

/// List all locally available sing-box binaries, including the bundled
/// one. The bundled entry is synthesised from
/// `commands::core_cmd::resolve_singbox_binary_path` and always sorts
/// first.
pub fn list_installed() -> AppResult<Vec<InstalledBinary>> {
    let mut out: Vec<InstalledBinary> = Vec::new();
    if let Some(p) = crate::commands::core_cmd::resolve_singbox_binary_path() {
        out.push(InstalledBinary {
            version: "bundled".into(),
            path: p,
            is_bundled: true,
        });
    }
    let dir = binaries_dir()?;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let bin = p.join(binary_filename());
            if bin.is_file() {
                out.push(InstalledBinary {
                    version: name.to_string(),
                    path: bin,
                    is_bundled: false,
                });
            }
        }
    }
    Ok(out)
}

/// Return the absolute path to the binary the user has selected via
/// settings. None means "use the Tauri-bundled sidecar".
pub fn resolve_selected_binary(settings: &Settings) -> Option<PathBuf> {
    let version = settings.selected_singbox_version.as_deref()?;
    if version.is_empty() || version == "bundled" {
        return None;
    }
    let dir = binaries_dir().ok()?;
    let path = dir.join(version).join(binary_filename());
    if path.is_file() {
        Some(path)
    } else {
        tracing::warn!(
            "selected_singbox_version='{}' but {} doesn't exist; falling back to bundled",
            version,
            path.display()
        );
        None
    }
}

/// Fetch the GitHub releases JSON and return one `ReleaseAsset` per
/// release that has a downloadable asset matching the current
/// platform. Sorted newest-first as GitHub returns them.
pub async fn list_remote() -> AppResult<Vec<ReleaseAsset>> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Other(format!("http client: {e}")))?;
    let resp = client
        .get(GITHUB_RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "GitHub releases HTTP {}",
            resp.status()
        )));
    }
    let releases: Vec<GhRelease> = resp.json().await?;
    let installed = list_installed().unwrap_or_default();
    let installed_versions: std::collections::HashSet<String> = installed
        .iter()
        .filter(|b| !b.is_bundled)
        .map(|b| b.version.clone())
        .collect();
    let mut out = Vec::new();
    for r in releases {
        let Some(asset) = r.assets.iter().find(|a| asset_matches_platform(&a.name)) else {
            continue;
        };
        out.push(ReleaseAsset {
            version: r.tag_name.clone(),
            asset_url: asset.browser_download_url.clone(),
            asset_name: asset.name.clone(),
            size: asset.size,
            published_at: r.published_at.clone().unwrap_or_default(),
            prerelease: r.prerelease,
            installed: installed_versions.contains(&r.tag_name),
        });
    }
    Ok(out)
}

/// Download `asset_url` and extract the `sing-box[.exe]` binary into
/// `<data_dir>/binaries/<version>/`. Returns the resulting
/// `InstalledBinary` so the caller can confirm in the UI.
pub async fn download(version: &str, asset_url: &str) -> AppResult<InstalledBinary> {
    if version.is_empty() {
        return Err(AppError::Other("version is empty".into()));
    }
    let dest_dir = version_dir(version)?;
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| AppError::Other(format!("create {}: {e}", dest_dir.display())))?;

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::Other(format!("http client: {e}")))?;
    let resp = client.get(asset_url).send().await?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "download HTTP {} for {asset_url}",
            resp.status()
        )));
    }
    let bytes = resp.bytes().await?;
    let archive_lower = asset_url.to_ascii_lowercase();

    let binary_path = dest_dir.join(binary_filename());
    if archive_lower.ends_with(".zip") {
        extract_zip(&bytes, &dest_dir)?;
    } else if archive_lower.ends_with(".tar.gz") || archive_lower.ends_with(".tgz") {
        extract_tar_gz(&bytes, &dest_dir)?;
    } else {
        return Err(AppError::Other(format!(
            "unsupported asset extension: {asset_url}"
        )));
    }

    if !binary_path.is_file() {
        // Some releases put the binary in a sub-directory like
        // `sing-box-<version>-linux-amd64/sing-box`. Search up to 3
        // levels deep for the executable and lift it up to dest_dir.
        if let Some(found) = find_binary_recursive(&dest_dir, 3) {
            std::fs::rename(&found, &binary_path).or_else(|_| std::fs::copy(&found, &binary_path).map(|_| ()))
                .map_err(|e| AppError::Other(format!("place binary: {e}")))?;
        }
    }
    if !binary_path.is_file() {
        return Err(AppError::Other(format!(
            "extracted archive but {} not found",
            binary_path.display()
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&binary_path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&binary_path, perms);
        }
    }

    Ok(InstalledBinary {
        version: version.to_string(),
        path: binary_path,
        is_bundled: false,
    })
}

pub fn delete(version: &str) -> AppResult<()> {
    if version == "bundled" {
        return Err(AppError::Other("can't delete the bundled binary".into()));
    }
    let d = version_dir(version)?;
    if d.exists() {
        std::fs::remove_dir_all(&d)
            .map_err(|e| AppError::Other(format!("remove {}: {e}", d.display())))?;
    }
    Ok(())
}

fn find_binary_recursive(root: &Path, depth: u32) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                let n = name.to_ascii_lowercase();
                if n == "sing-box" || n == "sing-box.exe" {
                    return Some(p);
                }
            }
        } else if p.is_dir() {
            if let Some(found) = find_binary_recursive(&p, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> AppResult<()> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    archive.set_preserve_permissions(false);
    archive.set_overwrite(true);
    let dest_abs = std::fs::canonicalize(dest).unwrap_or_else(|_| dest.to_path_buf());
    for entry in archive
        .entries()
        .map_err(|e| AppError::Other(format!("tar.gz entries: {e}")))?
    {
        let mut entry = entry.map_err(|e| AppError::Other(format!("tar.gz entry: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| AppError::Other(format!("tar.gz entry path: {e}")))?
            .into_owned();
        // Reject absolute paths and any `..` segment — classic tar-slip.
        if path.is_absolute() {
            tracing::warn!(
                "tar.gz: skipping absolute-path entry {}",
                path.display()
            );
            continue;
        }
        if path.components().any(|c| {
            matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir)
        }) {
            tracing::warn!(
                "tar.gz: skipping entry with parent/root component: {}",
                path.display()
            );
            continue;
        }
        let target = dest.join(&path);
        // Belt-and-braces: even if components() didn't catch it, verify
        // the joined path canonicalizes inside dest. (Canonicalize only
        // works on existing paths, so check the parent.)
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Other(format!("tar.gz mkdir {}: {e}", parent.display()))
            })?;
            if let Ok(parent_abs) = std::fs::canonicalize(parent) {
                if !parent_abs.starts_with(&dest_abs) {
                    tracing::warn!(
                        "tar.gz: refusing to extract outside dest: {}",
                        target.display()
                    );
                    continue;
                }
            }
        }
        entry
            .unpack(&target)
            .map_err(|e| AppError::Other(format!("tar.gz unpack {}: {e}", target.display())))?;
    }
    Ok(())
}

fn extract_zip(bytes: &[u8], dest: &Path) -> AppResult<()> {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor)
        .map_err(|e| AppError::Other(format!("zip open: {e}")))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Other(format!("zip entry {i}: {e}")))?;
        let raw_name = entry.name();
        // Reject zip-slip paths (.., absolute paths).
        let safe_name = raw_name.replace('\\', "/");
        if safe_name.starts_with('/')
            || safe_name.split('/').any(|seg| seg == ".." || seg.is_empty())
        {
            continue;
        }
        let outpath = dest.join(&safe_name);
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath)
                .map_err(|e| AppError::Other(format!("zip mkdir: {e}")))?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Other(format!("zip mkdir parent: {e}")))?;
        }
        let mut out = std::fs::File::create(&outpath)
            .map_err(|e| AppError::Other(format!("zip create: {e}")))?;
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| AppError::Other(format!("zip read: {e}")))?;
        std::io::copy(&mut buf.as_slice(), &mut out)
            .map_err(|e| AppError::Other(format!("zip write: {e}")))?;
    }
    Ok(())
}
