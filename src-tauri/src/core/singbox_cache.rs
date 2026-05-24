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
//! Locking notes:
//! - sing-box opens cache.db with an exclusive flock, so we can't open
//!   the live file with jammdb while sing-box is running. For READS we
//!   copy the file to a temp snapshot first (POSIX flock is advisory —
//!   copying is unaffected) and open the copy.
//! - For WRITES (invalidate_*) the caller MUST have stopped sing-box,
//!   otherwise jammdb will fail to acquire the lock and we propagate
//!   the error.

use std::collections::HashMap;
use std::path::Path;

use jammdb::{DB, Error as DbError};
use serde::Serialize;

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
/// (sing-box hasn't been started against this config) or if the schema
/// doesn't match the documented version-1 layout. The UI then shows
/// "Never" for last_updated.
///
/// sing-box mmap-writes the file from another process, so a snapshot we
/// take mid-flush can briefly look torn (bbolt page checksum failures
/// in jammdb). Retry the snapshot+open up to 3× with a 100ms backoff to
/// absorb that — if it still fails, treat as "no data" rather than
/// propagating to the UI.
pub fn read_rule_set_status(
    db_path: &Path,
    cache_id: &str,
) -> AppResult<HashMap<String, RuleSetCacheStatus>> {
    if !db_path.exists() {
        return Ok(HashMap::new());
    }
    let mut last_err: Option<AppError> = None;
    for attempt in 0..3 {
        let snap = match snapshot_cache_db(db_path) {
            Ok(s) => s,
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };
        // `snap` is a NamedTempFile that cleans up on drop, so we
        // don't leak even if read_rule_set_status_inner panics.
        match read_rule_set_status_inner(snap.path(), cache_id) {
            Ok(map) => return Ok(map),
            Err(e) => {
                tracing::debug!("cache.db snapshot read attempt {} failed: {e}", attempt + 1);
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
    let db = DB::open(snapshot_path).map_err(map_db_err)?;
    let tx = db.tx(false).map_err(map_db_err)?;
    let mut out = HashMap::new();
    let cache_bucket = match tx.get_bucket(cache_id.as_bytes()) {
        Ok(b) => b,
        // No bucket for this cache_id yet → no rule_sets cached.
        Err(DbError::BucketMissing) => return Ok(out),
        Err(e) => return Err(map_db_err(e)),
    };
    let rs_bucket = match cache_bucket.get_bucket("rule_set") {
        Ok(b) => b,
        Err(DbError::BucketMissing) => return Ok(out),
        Err(e) => return Err(map_db_err(e)),
    };
    for kv in rs_bucket.kv_pairs() {
        let tag = String::from_utf8_lossy(kv.key()).into_owned();
        let value: &[u8] = kv.value();
        match decode_saved_binary(value) {
            Some(s) => {
                out.insert(tag.clone(), RuleSetCacheStatus {
                    tag,
                    last_updated_ms: s.0,
                    etag: s.1,
                    content_size: s.2,
                });
            }
            None => {
                tracing::warn!("cache.db: rule_set tag '{}' has unrecognised header", tag);
                out.insert(
                    tag.clone(),
                    RuleSetCacheStatus {
                        tag,
                        last_updated_ms: 0,
                        etag: String::new(),
                        content_size: value.len() as u64,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// Delete the cached entry for one rule_set tag. Caller must have
/// stopped sing-box first or this will fail with a lock error.
pub fn invalidate_rule_set(
    db_path: &Path,
    cache_id: &str,
    tag: &str,
) -> AppResult<()> {
    if !db_path.exists() {
        // Nothing to invalidate — sing-box has never written cache.
        return Ok(());
    }
    let db = DB::open(db_path).map_err(map_db_err)?;
    let tx = db.tx(true).map_err(map_db_err)?;
    let cache_bucket = match tx.get_bucket(cache_id.as_bytes()) {
        Ok(b) => b,
        Err(DbError::BucketMissing) => return Ok(()),
        Err(e) => return Err(map_db_err(e)),
    };
    let rs_bucket = match cache_bucket.get_bucket("rule_set") {
        Ok(b) => b,
        Err(DbError::BucketMissing) => return Ok(()),
        Err(e) => return Err(map_db_err(e)),
    };
    match rs_bucket.delete(tag.as_bytes()) {
        Ok(_) | Err(DbError::KeyValueMissing) => {}
        Err(e) => return Err(map_db_err(e)),
    }
    tx.commit().map_err(map_db_err)?;
    Ok(())
}

/// Wipe all cached rule_set entries (sing-box re-downloads everything
/// on next start). Same locking caveat as `invalidate_rule_set`.
pub fn invalidate_all_rule_sets(db_path: &Path, cache_id: &str) -> AppResult<()> {
    if !db_path.exists() {
        return Ok(());
    }
    let db = DB::open(db_path).map_err(map_db_err)?;
    let tx = db.tx(true).map_err(map_db_err)?;
    let cache_bucket = match tx.get_bucket(cache_id.as_bytes()) {
        Ok(b) => b,
        Err(DbError::BucketMissing) => return Ok(()),
        Err(e) => return Err(map_db_err(e)),
    };
    match cache_bucket.delete_bucket("rule_set") {
        Ok(()) | Err(DbError::BucketMissing) => {}
        Err(e) => return Err(map_db_err(e)),
    }
    tx.commit().map_err(map_db_err)?;
    Ok(())
}

/// Copy the live cache.db into a `NamedTempFile` so jammdb's exclusive
/// flock doesn't fight sing-box's lock on the original. The temp file
/// is auto-deleted when the returned guard drops, even if the caller
/// panics — no leaked snapshot files in /tmp.
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

fn map_db_err(e: DbError) -> AppError {
    AppError::Other(format!("bbolt: {e}"))
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
            // Overflow guard for the final byte.
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
