//! Minimal read-only parser for bbolt files (the format sing-box's
//! `experimental.cache_file` uses). Just enough surface to walk down
//! `<cache_id> -> rule_set -> <tag>` and pull out the SavedBinary blob
//! we need for the "Last update" column.
//!
//! We had to roll this ourselves because `jammdb 0.11` asserts a
//! specific meta-page page-id layout that sing-box 1.13+ doesn't
//! honour (meta page id 4 where jammdb expects 3) — `DB::open` panics
//! before any read can happen. The bbolt on-disk layout itself is
//! stable across Go-bbolt and sing-box's fork, so we only need to
//! decode the page header, meta, branch, and leaf elements ourselves.
//!
//! Format reference: <https://github.com/etcd-io/bbolt/blob/main/page.go>.
//! Implementation is read-only and ignores everything we don't need
//! (freelist, write transactions, version-2 page split policy, etc.).
//!
//! All page IDs are interpreted as offsets `pgid * page_size` into a
//! byte slice held by the caller (mmap or read-to-vec). Bucket values
//! inline below `bucket_inline_max_threshold` are returned as the
//! bytes following the bucket header; otherwise the bucket's root
//! page is fetched recursively.

use std::collections::HashMap;
use std::path::Path;

const BBOLT_MAGIC: u32 = 0xED0CDAED;
// Page flag bits.
const PAGE_FLAG_BRANCH: u16 = 0x01;
const PAGE_FLAG_LEAF: u16 = 0x02;
const PAGE_FLAG_META: u16 = 0x04;
// Leaf-element flag bits.
const LEAF_FLAG_BUCKET: u32 = 0x01;
const PAGE_HEADER_SIZE: usize = 16;
const META_OFFSET: usize = PAGE_HEADER_SIZE; // meta data follows the header
// Default page size when we can't read it from the meta yet. Both
// meta pages live at the start of the file, so we only need to read
// past 4 KiB to find the page-size field on the vast majority of
// installs.
const DEFAULT_PAGE_SIZE: usize = 4096;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    InvalidMagic,
    Truncated,
    Other(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::InvalidMagic => write!(f, "not a bbolt file (bad magic)"),
            Error::Truncated => write!(f, "bbolt file is truncated"),
            Error::Other(s) => f.write_str(s),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Open `path`, parse the meta to discover root + page size, and
/// walk every bucket / leaf relevant to the caller. The whole file is
/// read into memory; cache.db is typically <10 MiB so this is fine.
#[derive(Debug)]
pub struct BoltReader {
    bytes: Vec<u8>,
    page_size: usize,
    root: BucketHeader,
}

#[derive(Debug, Clone, Copy)]
pub struct BucketHeader {
    root: u64,
    _sequence: u64,
}

impl BoltReader {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < DEFAULT_PAGE_SIZE {
            return Err(Error::Truncated);
        }
        // Read both meta pages and pick the one with the higher txid +
        // matching magic + matching checksum-isn't-checked (we don't
        // need to validate, we just need to read).
        let candidates = [
            try_meta(&bytes, 0, DEFAULT_PAGE_SIZE),
            try_meta(&bytes, 1, DEFAULT_PAGE_SIZE),
        ];
        let meta = candidates
            .iter()
            .flatten()
            .max_by_key(|m| m.txid)
            .ok_or(Error::InvalidMagic)?
            .clone();
        // If the on-disk page_size differs from our default, both
        // meta candidates above might have been wrong. Re-read using
        // the actual page_size.
        let actual_page_size = meta.page_size as usize;
        let meta = if actual_page_size != DEFAULT_PAGE_SIZE && actual_page_size > 0 {
            let again = [
                try_meta(&bytes, 0, actual_page_size),
                try_meta(&bytes, 1, actual_page_size),
            ];
            again
                .into_iter()
                .flatten()
                .max_by_key(|m| m.txid)
                .ok_or(Error::InvalidMagic)?
        } else {
            meta
        };

        Ok(BoltReader {
            bytes,
            page_size: meta.page_size as usize,
            root: BucketHeader {
                root: meta.root,
                _sequence: meta.sequence,
            },
        })
    }

    /// Discovered page size from the meta — useful for diagnostics.
    pub fn page_size(&self) -> usize {
        self.page_size
    }

    /// Iterate the database's top-level root bucket. Most useful for
    /// "what's actually in this file" exploration. Production code
    /// usually wants `get_bucket(name)` to descend by key.
    pub fn for_each_root<F>(&self, f: F) -> Result<(), Error>
    where
        F: FnMut(&[u8], LeafValue<'_>),
    {
        self.for_each(self.root, f)
    }

    /// Find a top-level bucket by name; the value at that key must be
    /// a sub-bucket (leaf flag = 1).
    pub fn get_bucket(&self, name: &[u8]) -> Option<BucketHeader> {
        self.find_in_bucket(self.root, name)
            .and_then(|v| match v {
                LeafValue::Bucket(b) => Some(b),
                LeafValue::Bytes(_) => None,
            })
    }

    /// Find a sub-bucket of an already-resolved bucket.
    pub fn get_sub_bucket(&self, parent: BucketHeader, name: &[u8]) -> Option<BucketHeader> {
        self.find_in_bucket(parent, name).and_then(|v| match v {
            LeafValue::Bucket(b) => Some(b),
            LeafValue::Bytes(_) => None,
        })
    }

    /// Iterate every (key, value) pair in `bucket`. Sub-buckets are
    /// yielded as `(key, LeafValue::Bucket(...))`; data entries as
    /// `(key, LeafValue::Bytes(...))`. We don't recurse — the caller
    /// asks for sub-buckets explicitly when needed.
    pub fn for_each<F>(&self, bucket: BucketHeader, mut f: F) -> Result<(), Error>
    where
        F: FnMut(&[u8], LeafValue<'_>),
    {
        self.walk_pages(bucket.root, &mut f)
    }

    fn find_in_bucket(&self, bucket: BucketHeader, key: &[u8]) -> Option<LeafValue<'_>> {
        // Inline page-walk specialised for the lookup case so we don't
        // have to thread a lifetime back out of a closure.
        self.walk_for_lookup(bucket.root, key)
    }

    fn walk_for_lookup(&self, page_id: u64, key: &[u8]) -> Option<LeafValue<'_>> {
        let page = self.read_page(page_id).ok()?;
        if page.flags & PAGE_FLAG_LEAF != 0 {
            for i in 0..page.count as usize {
                let elem = leaf_elem(self.bytes_ref(), &page, i).ok()?;
                let k = key_bytes(self.bytes_ref(), &page, i, &elem).ok()?;
                if k != key {
                    continue;
                }
                let v = value_bytes(self.bytes_ref(), &page, i, &elem).ok()?;
                if elem.flags & LEAF_FLAG_BUCKET != 0 {
                    return parse_bucket_header(v).ok().map(LeafValue::Bucket);
                }
                return Some(LeafValue::Bytes(v));
            }
            None
        } else if page.flags & PAGE_FLAG_BRANCH != 0 {
            for i in 0..page.count as usize {
                let elem = branch_elem(self.bytes_ref(), &page, i).ok()?;
                if let Some(v) = self.walk_for_lookup(elem.pgid, key) {
                    return Some(v);
                }
            }
            None
        } else {
            None
        }
    }

    /// Depth-first walk through every leaf page reachable from the
    /// given root page. Branch pages are followed transparently.
    fn walk_pages<F>(&self, page_id: u64, f: &mut F) -> Result<(), Error>
    where
        F: FnMut(&[u8], LeafValue<'_>),
    {
        let page = self.read_page(page_id)?;
        match page.flags {
            x if x & PAGE_FLAG_LEAF != 0 => {
                for i in 0..page.count as usize {
                    let elem = leaf_elem(self.bytes_ref(), &page, i)?;
                    let key = key_bytes(self.bytes_ref(), &page, i, &elem)?;
                    let value = value_bytes(self.bytes_ref(), &page, i, &elem)?;
                    if elem.flags & LEAF_FLAG_BUCKET != 0 {
                        let bucket = parse_bucket_header(value)?;
                        f(key, LeafValue::Bucket(bucket));
                    } else {
                        f(key, LeafValue::Bytes(value));
                    }
                }
            }
            x if x & PAGE_FLAG_BRANCH != 0 => {
                for i in 0..page.count as usize {
                    let elem = branch_elem(self.bytes_ref(), &page, i)?;
                    self.walk_pages(elem.pgid, f)?;
                }
            }
            _ => {
                // Inline bucket (root page is page 0 → bucket value is
                // packed right after the bucket header). Other flag
                // combinations are ignored.
            }
        }
        Ok(())
    }

    fn bytes_ref(&self) -> &[u8] {
        &self.bytes
    }

    fn read_page(&self, pgid: u64) -> Result<PageHeader, Error> {
        let off = pgid
            .checked_mul(self.page_size as u64)
            .ok_or(Error::Truncated)? as usize;
        if off + PAGE_HEADER_SIZE > self.bytes.len() {
            return Err(Error::Truncated);
        }
        let p = parse_page_header(&self.bytes[off..]);
        Ok(PageHeader { offset: off, ..p })
    }
}

#[derive(Debug, Clone)]
pub enum LeafValue<'a> {
    Bytes(&'a [u8]),
    Bucket(BucketHeader),
}

#[derive(Debug)]
struct PageHeader {
    offset: usize,
    _id: u64,
    flags: u16,
    count: u16,
    _overflow: u32,
}

#[derive(Debug, Clone)]
struct Meta {
    page_size: u32,
    root: u64,
    sequence: u64,
    txid: u64,
}

fn parse_page_header(buf: &[u8]) -> PageHeader {
    PageHeader {
        offset: 0,
        _id: u64::from_le_bytes(buf[0..8].try_into().unwrap()),
        flags: u16::from_le_bytes(buf[8..10].try_into().unwrap()),
        count: u16::from_le_bytes(buf[10..12].try_into().unwrap()),
        _overflow: u32::from_le_bytes(buf[12..16].try_into().unwrap()),
    }
}

fn try_meta(bytes: &[u8], pgid: u64, page_size: usize) -> Option<Meta> {
    let off = (pgid as usize).checked_mul(page_size)?;
    if off + PAGE_HEADER_SIZE + 32 > bytes.len() {
        return None;
    }
    let hdr = parse_page_header(&bytes[off..]);
    if hdr.flags & PAGE_FLAG_META == 0 {
        return None;
    }
    let m = &bytes[off + META_OFFSET..];
    if m.len() < 8 {
        return None;
    }
    let magic = u32::from_le_bytes(m[0..4].try_into().ok()?);
    if magic != BBOLT_MAGIC {
        return None;
    }
    let _version = u32::from_le_bytes(m[4..8].try_into().ok()?);
    if m.len() < 56 {
        return None;
    }
    let page_size = u32::from_le_bytes(m[8..12].try_into().ok()?);
    let _flags = u32::from_le_bytes(m[12..16].try_into().ok()?);
    let root = u64::from_le_bytes(m[16..24].try_into().ok()?);
    let sequence = u64::from_le_bytes(m[24..32].try_into().ok()?);
    // freelist@32..40 (skip), pgid@40..48 (skip), txid@48..56
    let txid = u64::from_le_bytes(m[48..56].try_into().ok()?);
    Some(Meta {
        page_size,
        root,
        sequence,
        txid,
    })
}

#[derive(Debug)]
struct BranchElement {
    _pos: u32,
    _ksize: u32,
    pgid: u64,
}

#[derive(Debug)]
struct LeafElement {
    flags: u32,
    pos: u32,
    ksize: u32,
    vsize: u32,
}

const BRANCH_ELEM_SIZE: usize = 16;
const LEAF_ELEM_SIZE: usize = 16;

fn branch_elem(bytes: &[u8], page: &PageHeader, i: usize) -> Result<BranchElement, Error> {
    let base = page.offset + PAGE_HEADER_SIZE + i * BRANCH_ELEM_SIZE;
    if base + BRANCH_ELEM_SIZE > bytes.len() {
        return Err(Error::Truncated);
    }
    Ok(BranchElement {
        _pos: u32::from_le_bytes(bytes[base..base + 4].try_into().unwrap()),
        _ksize: u32::from_le_bytes(bytes[base + 4..base + 8].try_into().unwrap()),
        pgid: u64::from_le_bytes(bytes[base + 8..base + 16].try_into().unwrap()),
    })
}

fn leaf_elem(bytes: &[u8], page: &PageHeader, i: usize) -> Result<LeafElement, Error> {
    let base = page.offset + PAGE_HEADER_SIZE + i * LEAF_ELEM_SIZE;
    if base + LEAF_ELEM_SIZE > bytes.len() {
        return Err(Error::Truncated);
    }
    Ok(LeafElement {
        flags: u32::from_le_bytes(bytes[base..base + 4].try_into().unwrap()),
        pos: u32::from_le_bytes(bytes[base + 4..base + 8].try_into().unwrap()),
        ksize: u32::from_le_bytes(bytes[base + 8..base + 12].try_into().unwrap()),
        vsize: u32::from_le_bytes(bytes[base + 12..base + 16].try_into().unwrap()),
    })
}

fn key_bytes<'a>(
    bytes: &'a [u8],
    page: &PageHeader,
    i: usize,
    elem: &LeafElement,
) -> Result<&'a [u8], Error> {
    // `pos` is relative to the element's offset within the page.
    let elem_off = PAGE_HEADER_SIZE + i * LEAF_ELEM_SIZE;
    let key_off = page.offset + elem_off + elem.pos as usize;
    let key_end = key_off + elem.ksize as usize;
    if key_end > bytes.len() {
        return Err(Error::Truncated);
    }
    Ok(&bytes[key_off..key_end])
}

fn value_bytes<'a>(
    bytes: &'a [u8],
    page: &PageHeader,
    i: usize,
    elem: &LeafElement,
) -> Result<&'a [u8], Error> {
    let elem_off = PAGE_HEADER_SIZE + i * LEAF_ELEM_SIZE;
    let val_off = page.offset + elem_off + elem.pos as usize + elem.ksize as usize;
    let val_end = val_off + elem.vsize as usize;
    if val_end > bytes.len() {
        return Err(Error::Truncated);
    }
    Ok(&bytes[val_off..val_end])
}

fn parse_bucket_header(buf: &[u8]) -> Result<BucketHeader, Error> {
    if buf.len() < 16 {
        return Err(Error::Other("bucket header too short"));
    }
    Ok(BucketHeader {
        root: u64::from_le_bytes(buf[0..8].try_into().unwrap()),
        _sequence: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
    })
}

/// Convenience: walk the on-disk `rule_set` bucket and return a map
/// from tag → raw `SavedBinary` bytes. Caller decodes the bytes (this
/// reader doesn't know the SavedBinary format).
///
/// Two layouts in the wild:
///   1. Default (no `experimental.cache_file.cache_id` in user config):
///      sing-box puts buckets straight at the root, so we walk
///      `root/rule_set/<tag>`.
///   2. Explicit cache_id set: sing-box wraps everything one level
///      deeper as `root/<cache_id>/rule_set/<tag>`.
///
/// We try (1) first since it matches stock configs, then fall through
/// to (2) if the caller supplied a non-empty cache_id and we couldn't
/// find a top-level rule_set bucket.
pub fn read_rule_set_blobs(
    path: &Path,
    cache_id: &str,
) -> Result<HashMap<String, Vec<u8>>, Error> {
    let reader = BoltReader::open(path)?;
    let mut out = HashMap::new();

    // Layout (1): rule_set at root.
    if let Some(rs_bucket) = reader.get_bucket(b"rule_set") {
        reader.for_each(rs_bucket, |k, v| {
            if let LeafValue::Bytes(b) = v {
                out.insert(String::from_utf8_lossy(k).into_owned(), b.to_vec());
            }
        })?;
        if !out.is_empty() {
            return Ok(out);
        }
    }

    // Layout (2): nested under <cache_id>. We only descend here if the
    // caller named a non-default cache_id (otherwise layout-(1) was
    // already authoritative and an empty map is the truth).
    if !cache_id.is_empty() && cache_id != "default" {
        if let Some(parent) = reader.get_bucket(cache_id.as_bytes()) {
            if let Some(rs_bucket) = reader.get_sub_bucket(parent, b"rule_set") {
                reader.for_each(rs_bucket, |k, v| {
                    if let LeafValue::Bytes(b) = v {
                        out.insert(String::from_utf8_lossy(k).into_owned(), b.to_vec());
                    }
                })?;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-crafted minimal bbolt file: meta@page0, one leaf@page2 with
    /// one entry. Stripped to the bytes our reader actually inspects.
    fn synth_bolt(page_size: usize, key: &[u8], val: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; page_size * 4];

        // page 0 — meta
        // page header
        out[0..8].copy_from_slice(&0u64.to_le_bytes()); // id
        out[8..10].copy_from_slice(&PAGE_FLAG_META.to_le_bytes());
        // count + overflow zero
        // meta body
        let meta_off = PAGE_HEADER_SIZE;
        out[meta_off..meta_off + 4].copy_from_slice(&BBOLT_MAGIC.to_le_bytes());
        out[meta_off + 4..meta_off + 8].copy_from_slice(&2u32.to_le_bytes()); // version
        out[meta_off + 8..meta_off + 12].copy_from_slice(&(page_size as u32).to_le_bytes());
        // flags zero
        out[meta_off + 16..meta_off + 24].copy_from_slice(&2u64.to_le_bytes()); // root → page 2
        // sequence, freelist, pgid zero
        out[meta_off + 48..meta_off + 56].copy_from_slice(&5u64.to_le_bytes()); // txid

        // page 2 — leaf with one entry
        let p2 = page_size * 2;
        out[p2..p2 + 8].copy_from_slice(&2u64.to_le_bytes()); // id
        out[p2 + 8..p2 + 10].copy_from_slice(&PAGE_FLAG_LEAF.to_le_bytes());
        out[p2 + 10..p2 + 12].copy_from_slice(&1u16.to_le_bytes()); // count = 1

        // leaf element 0: flags=0, pos=16 (just past this element),
        // ksize=key.len(), vsize=val.len()
        let elem_base = p2 + PAGE_HEADER_SIZE;
        out[elem_base..elem_base + 4].copy_from_slice(&0u32.to_le_bytes()); // flags
        out[elem_base + 4..elem_base + 8].copy_from_slice(&(LEAF_ELEM_SIZE as u32).to_le_bytes()); // pos
        out[elem_base + 8..elem_base + 12]
            .copy_from_slice(&(key.len() as u32).to_le_bytes());
        out[elem_base + 12..elem_base + 16]
            .copy_from_slice(&(val.len() as u32).to_le_bytes());

        // key + value follow
        let kv_off = elem_base + LEAF_ELEM_SIZE;
        out[kv_off..kv_off + key.len()].copy_from_slice(key);
        out[kv_off + key.len()..kv_off + key.len() + val.len()].copy_from_slice(val);

        out
    }

    #[test]
    fn reads_a_synthetic_bbolt_file() {
        let bytes = synth_bolt(4096, b"hello", b"world");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synth.db");
        std::fs::write(&path, &bytes).unwrap();
        let r = BoltReader::open(&path).expect("open");
        let mut seen: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        r.for_each(r.root, |k, v| {
            if let LeafValue::Bytes(b) = v {
                seen.push((k.to_vec(), b.to_vec()));
            }
        })
        .unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, b"hello");
        assert_eq!(seen[0].1, b"world");
    }

    #[test]
    fn rejects_non_bbolt_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("junk.db");
        std::fs::write(&path, vec![0u8; 8192]).unwrap();
        match BoltReader::open(&path) {
            Err(Error::InvalidMagic) => {}
            other => panic!("expected InvalidMagic, got {other:?}"),
        }
    }
}
