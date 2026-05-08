//! **Overrides layer** — keeps user edits out of the source config file.
//!
//! Conceptually the user has a *source config* (JSON they own, never
//! mutated by this app) and a *runtime config* (what sing-box actually
//! reads, written to data_dir/runtime/config.json on every core_start).
//! The runtime config = source ⊕ overrides, computed at inject time.
//!
//! Overrides have two scopes:
//!
//!   - **per-config** — attached to a single ConfigEntry. Holds:
//!       - `appended`: new local entries added in this config
//!       - `modifications`: edits to source entries, keyed by signature
//!       - `masked`: signatures of source entries to skip
//!     Stored at `<data_dir>/overrides/<entry_id>.json`.
//!
//!   - **global** — applies to every ConfigEntry. Only `appended` is
//!     supported (modifications/masks reference a specific source rule
//!     so they're inherently per-config). Stored at
//!     `<data_dir>/overrides/global.json`.
//!
//! Signature scheme: SHA-256 hex of the canonical JSON of the source
//! entry. Canonical = object keys sorted alphabetically, recursively;
//! arrays keep order; numbers re-encoded as serde_json default.
//! Property: identical-meaning entries hash the same regardless of key
//! order in the source file. Caveat: if the user manually edits a
//! source entry (e.g. opens the file in vim), its signature changes
//! and any modification/mask attached to it goes "stale".

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

const OVERRIDES_VERSION: u32 = 1;

/// Per-config overrides. One file per ConfigEntry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalOverrides {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub route_rules: ArrayOverrides,
    #[serde(default)]
    pub route_rule_set: ArrayOverrides,
    #[serde(default)]
    pub dns_servers: ArrayOverrides,
    #[serde(default)]
    pub dns_rules: ArrayOverrides,
}

impl Default for LocalOverrides {
    fn default() -> Self {
        Self {
            version: OVERRIDES_VERSION,
            route_rules: ArrayOverrides::default(),
            route_rule_set: ArrayOverrides::default(),
            dns_servers: ArrayOverrides::default(),
            dns_rules: ArrayOverrides::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArrayOverrides {
    /// Local entries the user added in this config. Order is meaningful
    /// (user-controlled). Default rendering position is *after* the
    /// (filtered) source array.
    #[serde(default)]
    pub appended: Vec<LocalEntry>,
    /// Edits to source entries. Key = signature of the *original* source
    /// entry. Value carries the override JSON we replace it with at
    /// merge time.
    #[serde(default)]
    pub modifications: HashMap<String, ModificationEntry>,
    /// Source entries to skip at merge time (don't write to runtime).
    #[serde(default)]
    pub masked: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalEntry {
    pub id: String, // UUID v4
    pub value: Value,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModificationEntry {
    pub override_value: Value,
    /// First 16 hex chars of the original signature, for UI hover hint.
    pub original_signature_preview: String,
    pub modified_at_ms: u64,
}

/// Global overrides. One file shared across all ConfigEntries.
/// Only `appended` semantics — modifications/masks reference specific
/// source rules so they can't be global.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalOverrides {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub route_rules: Vec<LocalEntry>,
    #[serde(default)]
    pub route_rule_set: Vec<LocalEntry>,
    #[serde(default)]
    pub dns_servers: Vec<LocalEntry>,
    #[serde(default)]
    pub dns_rules: Vec<LocalEntry>,
}

fn default_version() -> u32 {
    OVERRIDES_VERSION
}

// ---------- persistence -------------------------------------------------

pub fn load_per_config(path: &Path) -> LocalOverrides {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<LocalOverrides>(&b).ok())
        .unwrap_or_default()
}

pub fn save_per_config(path: &PathBuf, ov: &LocalOverrides) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(ov)?;
    crate::util::atomic_write::atomic_write(path, &bytes)
}

pub fn load_global(path: &Path) -> GlobalOverrides {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<GlobalOverrides>(&b).ok())
        .unwrap_or_default()
}

pub fn save_global(path: &PathBuf, g: &GlobalOverrides) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(g)?;
    crate::util::atomic_write::atomic_write(path, &bytes)
}

// ---------- signature ---------------------------------------------------

/// Canonical sha256 hex of a JSON value. Object keys sorted recursively;
/// arrays keep order. Idempotent under any whitespace / key-order change
/// in the original source.
pub fn signature(v: &Value) -> String {
    let mut buf: Vec<u8> = Vec::new();
    write_canonical(v, &mut buf);
    let mut h = Sha256::new();
    h.update(&buf);
    hex(&h.finalize())
}

fn write_canonical(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Value::String(s) => {
            // Use serde_json to escape correctly (handles \uXXXX, control bytes, etc.).
            let s = serde_json::Value::String(s.clone()).to_string();
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(arr) => {
            out.push(b'[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        Value::Object(obj) => {
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                let ks = serde_json::Value::String(k.clone()).to_string();
                out.extend_from_slice(ks.as_bytes());
                out.push(b':');
                write_canonical(&obj[k], out);
            }
            out.push(b'}');
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

// ---------- merge -------------------------------------------------------

/// Replace `arr` with the merged result of (source-arr filtered through
/// per-config mods + masks) ++ per-config.appended ++ global.appended.
///
/// `arr` is the slice of merged config sitting at e.g. /route/rules at
/// inject time; we hand back a fresh `Value::Array` for the caller to
/// stick into the same path.
pub fn merge_array(
    source_arr: &[Value],
    per_config: &ArrayOverrides,
    global_appended: &[LocalEntry],
) -> Value {
    let mut out: Vec<Value> = Vec::with_capacity(
        source_arr.len() + per_config.appended.len() + global_appended.len(),
    );
    for item in source_arr {
        let sig = signature(item);
        if per_config.masked.contains(&sig) {
            continue;
        }
        if let Some(m) = per_config.modifications.get(&sig) {
            out.push(m.override_value.clone());
        } else {
            out.push(item.clone());
        }
    }
    for e in &per_config.appended {
        out.push(e.value.clone());
    }
    for e in global_appended {
        out.push(e.value.clone());
    }
    Value::Array(out)
}

/// Apply overrides in-place to a (clash-api-injected) merged config.
/// Touches: /route/rules, /route/rule_set, /dns/servers, /dns/rules.
/// Creates the parent objects when needed (e.g. user's source has no
/// dns block but a global override adds dns servers).
pub fn apply_overrides_overlay(
    merged: &mut Value,
    per: &LocalOverrides,
    global: &GlobalOverrides,
) -> AppResult<()> {
    apply_one(merged, &["route", "rules"], &per.route_rules, &global.route_rules)?;
    apply_one(
        merged,
        &["route", "rule_set"],
        &per.route_rule_set,
        &global.route_rule_set,
    )?;
    apply_one(
        merged,
        &["dns", "servers"],
        &per.dns_servers,
        &global.dns_servers,
    )?;
    apply_one(merged, &["dns", "rules"], &per.dns_rules, &global.dns_rules)?;
    Ok(())
}

fn apply_one(
    merged: &mut Value,
    path: &[&str],
    per: &ArrayOverrides,
    global_appended: &[LocalEntry],
) -> AppResult<()> {
    let no_changes = per.appended.is_empty()
        && per.modifications.is_empty()
        && per.masked.is_empty()
        && global_appended.is_empty();
    if no_changes {
        return Ok(());
    }

    // Walk/create the parent path.
    let mut cur = merged
        .as_object_mut()
        .ok_or_else(|| AppError::Config("config root is not an object".into()))?;
    for key in &path[..path.len() - 1] {
        let entry = cur
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(Default::default()));
        cur = entry
            .as_object_mut()
            .ok_or_else(|| AppError::Config(format!("{key} is not an object")))?;
    }
    let leaf_key = path[path.len() - 1];
    let leaf = cur
        .entry(leaf_key.to_string())
        .or_insert_with(|| Value::Array(vec![]));
    let source_arr = leaf
        .as_array()
        .ok_or_else(|| AppError::Config(format!("{leaf_key} is not an array")))?;

    *leaf = merge_array(source_arr, per, global_appended);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_array_ov() -> ArrayOverrides {
        ArrayOverrides::default()
    }

    #[test]
    fn signature_is_stable_across_key_order() {
        let a = json!({"domain": ["x.com"], "outbound": "direct"});
        let b = json!({"outbound": "direct", "domain": ["x.com"]});
        assert_eq!(signature(&a), signature(&b));
    }

    #[test]
    fn empty_overrides_are_a_noop() {
        let mut cfg = json!({
            "route": {"rules": [{"domain": ["x.com"], "outbound": "direct"}], "final": "direct"}
        });
        let before = cfg.clone();
        let per = LocalOverrides::default();
        let global = GlobalOverrides::default();
        apply_overrides_overlay(&mut cfg, &per, &global).unwrap();
        assert_eq!(cfg, before);
    }

    #[test]
    fn append_local_route_rule() {
        let mut cfg = json!({"route": {"rules": [{"domain": ["a.com"], "outbound": "direct"}]}});
        let mut per = LocalOverrides::default();
        per.route_rules.appended.push(LocalEntry {
            id: "u1".into(),
            value: json!({"domain": ["b.com"], "outbound": "block"}),
            created_at_ms: 0,
        });
        apply_overrides_overlay(&mut cfg, &per, &GlobalOverrides::default()).unwrap();
        let rules = cfg.pointer("/route/rules").unwrap().as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1]["outbound"], "block");
    }

    #[test]
    fn mask_skips_source_rule() {
        let r1 = json!({"domain": ["a.com"], "outbound": "direct"});
        let r2 = json!({"domain": ["b.com"], "outbound": "direct"});
        let mut cfg = json!({"route": {"rules": [r1.clone(), r2.clone()]}});
        let mut per = LocalOverrides::default();
        per.route_rules.masked.insert(signature(&r1));
        apply_overrides_overlay(&mut cfg, &per, &GlobalOverrides::default()).unwrap();
        let rules = cfg.pointer("/route/rules").unwrap().as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["domain"][0], "b.com");
    }

    #[test]
    fn modify_replaces_source_rule_in_place() {
        let r1 = json!({"domain": ["a.com"], "outbound": "direct"});
        let mut cfg = json!({"route": {"rules": [r1.clone()]}});
        let mut per = LocalOverrides::default();
        let sig = signature(&r1);
        per.route_rules.modifications.insert(
            sig.clone(),
            ModificationEntry {
                override_value: json!({"domain": ["a.com"], "outbound": "block"}),
                original_signature_preview: sig[..16].to_string(),
                modified_at_ms: 0,
            },
        );
        apply_overrides_overlay(&mut cfg, &per, &GlobalOverrides::default()).unwrap();
        let rules = cfg.pointer("/route/rules").unwrap().as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["outbound"], "block");
    }

    #[test]
    fn modification_decouples_when_source_changes() {
        // Modification keyed on signature(r1). User then edits the source
        // file so the rule becomes r1' (different signature). The merge
        // should leave r1' intact (override no longer matches).
        let r1 = json!({"domain": ["a.com"], "outbound": "direct"});
        let r1_prime = json!({"domain": ["a.com"], "outbound": "proxy"});
        let mut cfg = json!({"route": {"rules": [r1_prime.clone()]}});
        let mut per = LocalOverrides::default();
        per.route_rules.modifications.insert(
            signature(&r1),
            ModificationEntry {
                override_value: json!({"domain": ["a.com"], "outbound": "block"}),
                original_signature_preview: "x".into(),
                modified_at_ms: 0,
            },
        );
        apply_overrides_overlay(&mut cfg, &per, &GlobalOverrides::default()).unwrap();
        let rules = cfg.pointer("/route/rules").unwrap().as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["outbound"], "proxy", "user's external edit wins");
    }

    #[test]
    fn dns_path_creates_parent_when_missing() {
        // Source has no `dns` block; a global override adds a server.
        // Inject should synthesise dns: { servers: [...] }.
        let mut cfg = json!({"outbounds": []});
        let mut global = GlobalOverrides::default();
        global.dns_servers.push(LocalEntry {
            id: "g1".into(),
            value: json!({"type": "udp", "tag": "g", "server": "8.8.8.8"}),
            created_at_ms: 0,
        });
        apply_overrides_overlay(&mut cfg, &LocalOverrides::default(), &global).unwrap();
        let servers = cfg.pointer("/dns/servers").unwrap().as_array().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0]["tag"], "g");
    }

    #[test]
    fn merge_order_is_source_then_per_then_global() {
        let r_src = json!({"domain": ["src"], "outbound": "direct"});
        let mut cfg = json!({"route": {"rules": [r_src]}});
        let mut per = LocalOverrides::default();
        per.route_rules.appended.push(LocalEntry {
            id: "u1".into(),
            value: json!({"domain": ["per"], "outbound": "direct"}),
            created_at_ms: 0,
        });
        let mut global = GlobalOverrides::default();
        global.route_rules.push(LocalEntry {
            id: "g1".into(),
            value: json!({"domain": ["global"], "outbound": "direct"}),
            created_at_ms: 0,
        });
        apply_overrides_overlay(&mut cfg, &per, &global).unwrap();
        let rules = cfg.pointer("/route/rules").unwrap().as_array().unwrap();
        let domains: Vec<_> = rules.iter().map(|r| r["domain"][0].as_str().unwrap()).collect();
        assert_eq!(domains, vec!["src", "per", "global"]);
    }

    #[test]
    fn empty_array_ov_default_works() {
        let _ = empty_array_ov();
    }
}
