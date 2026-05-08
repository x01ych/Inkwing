use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Number of `.bak` generations to keep. The freshest is `<path>.bak`,
/// then `<path>.bak.1`, `<path>.bak.2`. Anything older is dropped on
/// rotation.
const BAK_GENS: usize = 3;

/// Atomically write `bytes` to `path`. If `path` already exists, the
/// existing content is rotated through 3 backup generations (`.bak`,
/// `.bak.1`, `.bak.2`) before the new content is moved into place. Uses
/// a temp file in the same directory + persist for cross-platform atomic
/// rename.
///
/// On Windows the persist step uses MoveFileEx(REPLACE_EXISTING) — atomic
/// at the rename level for local NTFS. On SMB shares or under aggressive
/// antivirus interceptors it can degrade to a copy+delete; the data still
/// gets there, just without the all-or-nothing guarantee.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Other(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent)?;

    if path.exists() {
        rotate_backups(path)?;
    }

    let mut tmp = tempfile::Builder::new()
        .prefix(".inkwing-tmp-")
        .tempfile_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| AppError::Io(e.error))?;
    Ok(())
}

/// Promote `.bak.(N-1) → .bak.N`, …, `.bak → .bak.1`, then `path → .bak`.
/// Older-than-cap slots fall off. The freshest snapshot uses `fs::copy`
/// so the source file remains in place for the upcoming temp-file
/// persist; everything else is `fs::rename` (atomic on the same fs).
/// Copy failure on the freshest slot is propagated — silently swallowing
/// it (as the old single-`.bak` code did) means a single bad save can
/// poison the only rollback without the user knowing.
fn rotate_backups(path: &Path) -> AppResult<()> {
    // Walk from the oldest kept slot down to slot 0. For each pair
    // (src=n-1, dst=n): drop dst if present, then rename src into it.
    // Slot indices: 0 = .bak (newest), BAK_GENS-1 = .bak.{BAK_GENS-1}
    // (oldest kept).
    for n in (1..BAK_GENS).rev() {
        let dst = bak_path(path, n);
        let src = bak_path(path, n - 1);
        // dst falls off the end here; safe to remove unconditionally so
        // the rename below can claim its name.
        let _ = fs::remove_file(&dst);
        if src.exists() {
            if let Err(e) = fs::rename(&src, &dst) {
                tracing::warn!(?e, "rotate {} → {} failed", src.display(), dst.display());
            }
        }
    }
    // path → .bak: copy (not move) so the caller can still atomically
    // replace `path` via tempfile.persist() right after we return.
    let bak0 = bak_path(path, 0);
    fs::copy(path, &bak0).map_err(|e| {
        AppError::Io(std::io::Error::new(
            e.kind(),
            format!("failed to write .bak for {}: {e}", path.display()),
        ))
    })?;
    Ok(())
}

/// `n == 0` → `<path>.bak`. `n >= 1` → `<path>.bak.<n>`.
fn bak_path(path: &Path, n: usize) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    if n == 0 {
        s.push(".bak");
    } else {
        s.push(format!(".bak.{n}"));
    }
    s.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(p: &Path) -> Vec<u8> {
        fs::read(p).expect("read")
    }

    #[test]
    fn rotates_three_generations_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");

        // Gen 0
        atomic_write(&path, b"v0").unwrap();
        assert_eq!(read(&path), b"v0");
        assert!(!bak_path(&path, 0).exists());

        // Gen 1: .bak should now be v0
        atomic_write(&path, b"v1").unwrap();
        assert_eq!(read(&path), b"v1");
        assert_eq!(read(&bak_path(&path, 0)), b"v0");

        // Gen 2: .bak.1 should be v0, .bak should be v1
        atomic_write(&path, b"v2").unwrap();
        assert_eq!(read(&path), b"v2");
        assert_eq!(read(&bak_path(&path, 0)), b"v1");
        assert_eq!(read(&bak_path(&path, 1)), b"v0");

        // Gen 3: .bak.2 = v0, .bak.1 = v1, .bak = v2
        atomic_write(&path, b"v3").unwrap();
        assert_eq!(read(&path), b"v3");
        assert_eq!(read(&bak_path(&path, 0)), b"v2");
        assert_eq!(read(&bak_path(&path, 1)), b"v1");
        assert_eq!(read(&bak_path(&path, 2)), b"v0");

        // Gen 4: oldest (v0) falls off
        atomic_write(&path, b"v4").unwrap();
        assert_eq!(read(&path), b"v4");
        assert_eq!(read(&bak_path(&path, 0)), b"v3");
        assert_eq!(read(&bak_path(&path, 1)), b"v2");
        assert_eq!(read(&bak_path(&path, 2)), b"v1");
    }
}
