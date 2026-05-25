//! Read / mutate sing-box's `cache.db` (bbolt) so the UI can show
//! "last updated" timestamps per rule_set and force a refresh by
//! deleting cached entries (sing-box re-downloads on next startup).
//!
//! Schema (verified against sing-box `experimental/cachefile/cache.go`):
//!   root → bucket "<cache_id>"           // default = "default"
//!        → sub-bucket "rule_set"
//!        → key = <tag>
//!        → value = SavedBinary, marshaled as:
//!            [u8 version=1]
//!            [uvarint contentLen]
//!            [content bytes]
//!            [i64 BE lastUpdatedUnixSec]
//!            [uvarint etagLen]
//!            [etag bytes]
//!
//! Reads go through our own [`bbolt_reader`][crate::core::bbolt_reader],
//! which we wrote after `jammdb 0.11` was found to panic on sing-box
//! 1.13+ cache.db files (meta-page id 4 where jammdb asserts 3).
//!
//! Writes (refresh / invalidate) used to do surgical key deletion via
//! jammdb. Since jammdb is incompatible, we fall back to deleting the
//! whole cache.db file. sing-box rebuilds it on next start and
//! re-downloads every remote rule_set. That's heavier than necessary
//! for a single-tag refresh, but it's correct, requires no write
//! library, and is rare (manual user action).

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::core::bbolt_reader;
use crate::error::{AppError, AppResult};

const DEFAULT_CACHE_ID: &str = "default";
const SAVED_BINARY_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct RuleSetCacheStatus {
    pub tag: String,
    /// UNIX millis. 0 means "version mismatch / could not decode header".
    pub last_updated_ms: u64,
    pub etag: String,
    pub content_size: u64,
}

/// Read sing-box's runtime `experimental.cache_file.cache_id` from the
/// active parsed config, falling back to "default" (sing-box's internal
/// `cacheIDDefault`).
pub fn cache_id_for(parsed: Option<&serde_json::Value>) -> String {
    parsed
        .and_then(|v| v.pointer("/experimental/cache_file/cache_id"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| DEFAULT_CACHE_ID.to_string())
}

/// Best-effort: returns an empty map if cache.db doesn't exist yet
/// (sing-box hasn't been started against this config) or the bbolt
/// reader can't make sense of it. The UI then shows "Never" for
/// `last_updated_ms`.
///
/// Implementation note: we copy the live file into a temporary location
/// before reading. POSIX flock + Windows LockFile are advisory at the
/// filesystem-copy layer, so `fs::copy` from a file sing-box has open
/// just works. Reading the live file directly with mmap risks a torn
/// page when sing-box's write commit lands mid-iteration.
pub fn read_rule_set_status(
    db_path: &Path,
    cache_id: &str,
) -> AppResult<HashMap<String, RuleSetCacheStatus>> {
    if !db_path.exists() {
        return Ok(HashMap::new());
    }

    // Three attempts, in case the first snapshot copies a half-written
    // page set (extremely rare for our access pattern but cheap to
    // guard against). 100 ms between attempts.
    let mut last_err: Option<AppError> = None;
    for _ in 0..3 {
        let snap = match snapshot_cache_db(db_path) {
            Ok(s) => s,
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };
        match read_rule_set_status_inner(snap.path(), cache_id) {
            Ok(map) => return Ok(map),
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::Other("cache.db read exhausted retries".into())))
}

fn read_rule_set_status_inner(
    snapshot_path: &Path,
    cache_id: &str,
) -> AppResult<HashMap<String, RuleSetCacheStatus>> {
    let blobs = bbolt_reader::read_rule_set_blobs(snapshot_path, cache_id)
        .map_err(|e| AppError::Other(format!("bbolt read: {e}")))?;
    let mut out = HashMap::with_capacity(blobs.len());
    for (tag, bytes) in blobs {
        let size = bytes.len() as u64;
        match decode_saved_binary(&bytes) {
            Some((ts, etag, content_size)) => {
                out.insert(
                    tag.clone(),
                    RuleSetCacheStatus {
                        tag,
                        last_updated_ms: ts,
                        etag,
                        content_size,
                    },
                );
            }
            None => {
                tracing::debug!(
                    "cache.db: rule_set tag '{}' has unrecognised SavedBinary header (size={})",
                    tag,
                    size
                );
                out.insert(
                    tag.clone(),
                    RuleSetCacheStatus {
                        tag,
                        last_updated_ms: 0,
                        etag: String::new(),
                        content_size: size,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// Invalidate a single rule_set's cache entry. We don't have a bbolt
/// writer available (jammdb panics on sing-box's format, and writing
/// our own writer is significant work), so we degrade to nuking the
/// whole cache.db file. sing-box recreates it on next start and
/// re-downloads every remote rule_set — heavier than necessary but
/// correct, and only triggered by an explicit user-facing button.
///
/// Caller MUST have stopped sing-box first; this is enforced upstream
/// (`rule_set_refresh` runs `core_stop` before calling here).
pub fn invalidate_rule_set(
    db_path: &Path,
    _cache_id: &str,
    _tag: &str,
) -> AppResult<()> {
    delete_cache_db(db_path)
}

/// Wipe the entire cache so every remote rule_set re-downloads on the
/// next sing-box start. Same semantics as `invalidate_rule_set` —
/// we always delete the whole file because partial writes are not
/// available without a bbolt writer.
pub fn invalidate_all_rule_sets(db_path: &Path, _cache_id: &str) -> AppResult<()> {
    delete_cache_db(db_path)
}

fn delete_cache_db(db_path: &Path) -> AppResult<()> {
    if !db_path.exists() {
        return Ok(());
    }
    std::fs::remove_file(db_path)
        .map_err(|e| AppError::Other(format!("delete cache.db: {e}")))?;
    tracing::info!(
        "singbox_cache: removed {} — sing-box will rebuild + re-fetch on next start",
        db_path.display()
    );
    Ok(())
}

/// Copy the live cache.db into a `NamedTempFile` so any platform-level
/// file locks held by sing-box don't fight us at parse time. The temp
/// file is auto-deleted when the returned guard drops, even if the
/// caller panics — no leaked snapshot files in /tmp.
fn snapshot_cache_db(src: &Path) -> AppResult<tempfile::NamedTempFile> {
    let tmp = tempfile::Builder::new()
        .prefix("inkwing-cache-snapshot-")
        .suffix(".db")
        .tempfile()
        .map_err(|e| AppError::Other(format!("snapshot tempfile: {e}")))?;
    std::fs::copy(src, tmp.path())
        .map_err(|e| AppError::Other(format!("snapshot copy: {e}")))?;
    Ok(tmp)
}

/// Returns (last_updated_ms, etag, content_size) on success, None on
/// version mismatch / truncation.
fn decode_saved_binary(buf: &[u8]) -> Option<(u64, String, u64)> {
    let mut cur = std::io::Cursor::new(buf);
    let version = read_u8(&mut cur)?;
    if version != SAVED_BINARY_VERSION {
        return None;
    }
    let content_len = read_uvarint(&mut cur)?;
    skip(&mut cur, content_len)?;
    let last_updated_sec = read_i64_be(&mut cur)?;
    let etag_len = read_uvarint(&mut cur)?;
    let etag_bytes = read_n(&mut cur, etag_len)?;
    let etag = String::from_utf8(etag_bytes).unwrap_or_default();
    let last_updated_ms = if last_updated_sec <= 0 {
        0
    } else {
        (last_updated_sec as u64).saturating_mul(1000)
    };
    Some((last_updated_ms, etag, content_len))
}

fn read_u8(cur: &mut std::io::Cursor<&[u8]>) -> Option<u8> {
    use std::io::Read;
    let mut b = [0u8; 1];
    cur.read_exact(&mut b).ok()?;
    Some(b[0])
}

fn read_i64_be(cur: &mut std::io::Cursor<&[u8]>) -> Option<i64> {
    use std::io::Read;
    let mut b = [0u8; 8];
    cur.read_exact(&mut b).ok()?;
    Some(i64::from_be_bytes(b))
}

fn read_uvarint(cur: &mut std::io::Cursor<&[u8]>) -> Option<u64> {
    use std::io::Read;
    let mut x: u64 = 0;
    let mut s: u32 = 0;
    for _ in 0..10 {
        let mut b = [0u8; 1];
        cur.read_exact(&mut b).ok()?;
        let byte = b[0];
        if byte < 0x80 {
            if s == 63 && byte > 1 {
                return None;
            }
            return Some(x | ((byte as u64) << s));
        }
        x |= ((byte & 0x7f) as u64) << s;
        s += 7;
    }
    None
}

fn read_n(cur: &mut std::io::Cursor<&[u8]>, n: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut v = vec![0u8; n as usize];
    cur.read_exact(&mut v).ok()?;
    Some(v)
}

fn skip(cur: &mut std::io::Cursor<&[u8]>, n: u64) -> Option<()> {
    use std::io::Seek;
    cur.seek(std::io::SeekFrom::Current(n as i64)).ok()?;
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_saved_binary(content: &[u8], last_updated_sec: i64, etag: &str) -> Vec<u8> {
        let mut out = vec![SAVED_BINARY_VERSION];
        write_uvarint(&mut out, content.len() as u64);
        out.extend_from_slice(content);
        out.extend_from_slice(&last_updated_sec.to_be_bytes());
        write_uvarint(&mut out, etag.len() as u64);
        out.extend_from_slice(etag.as_bytes());
        out
    }

    fn write_uvarint(buf: &mut Vec<u8>, mut x: u64) {
        while x >= 0x80 {
            buf.push((x as u8) | 0x80);
            x >>= 7;
        }
        buf.push(x as u8);
    }

    #[test]
    fn decodes_known_payload() {
        let payload = encode_saved_binary(b"rule-set body bytes", 1_700_000_000, "W/\"abc\"");
        let (ts, etag, size) = decode_saved_binary(&payload).expect("decode");
        assert_eq!(ts, 1_700_000_000_000);
        assert_eq!(etag, "W/\"abc\"");
        assert_eq!(size, b"rule-set body bytes".len() as u64);
    }

    #[test]
    fn rejects_other_version() {
        let mut buf = encode_saved_binary(b"x", 1, "");
        buf[0] = 2;
        assert!(decode_saved_binary(&buf).is_none());
    }

    #[test]
    fn handles_zero_timestamp() {
        let payload = encode_saved_binary(b"", 0, "");
        let (ts, etag, size) = decode_saved_binary(&payload).expect("decode");
        assert_eq!(ts, 0);
        assert_eq!(etag, "");
        assert_eq!(size, 0);
    }

    #[test]
    fn truncated_returns_none() {
        let mut buf = encode_saved_binary(b"contents", 1, "etag");
        buf.truncate(3);
        assert!(decode_saved_binary(&buf).is_none());
    }
}
