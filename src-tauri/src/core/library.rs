//! Persistence + helper API for the multi-config library.
//!
//! Persisted shape (in tauri-plugin-store at `library.json`):
//!   {
//!     "active_id": "<uuid>" | null,
//!     "entries": [ ConfigEntry, ... ]
//!   }

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Wry};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::paths::configs_dir;
use crate::state::{ConfigEntry, ConfigSource};

const STORE_FILE: &str = "library.json";
const KEY: &str = "library";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedLibrary {
    #[serde(default)]
    pub active_id: Option<String>,
    #[serde(default)]
    pub entries: Vec<ConfigEntry>,
}

pub fn load(app: &AppHandle<Wry>) -> PersistedLibrary {
    match app.store(STORE_FILE) {
        Ok(s) => s
            .get(KEY)
            .and_then(|v| serde_json::from_value::<PersistedLibrary>(v).ok())
            .unwrap_or_default(),
        Err(_) => PersistedLibrary::default(),
    }
}

pub fn save(app: &AppHandle<Wry>, lib: &PersistedLibrary) -> AppResult<()> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| AppError::Other(format!("open library store: {e}")))?;
    store.set(KEY, serde_json::to_value(lib)?);
    store
        .save()
        .map_err(|e| AppError::Other(format!("save library store: {e}")))?;
    Ok(())
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn new_entry(name: String, source: ConfigSource) -> AppResult<ConfigEntry> {
    let id = Uuid::new_v4().to_string();
    let storage_path = configs_dir()?.join(format!("{id}.json"));
    let now = now_ms();
    Ok(ConfigEntry {
        id,
        name,
        source,
        storage_path,
        created_at_ms: now,
        updated_at_ms: now,
        outbound_count: None,
        rule_count: None,
        has_tun_inbound: None,
    })
}

/// Recompute the cached summary fields on a single entry (looked up by
/// id) and persist the library back. Call sites: rules_commit /
/// rule_sets_commit / config_save / refresh_from_subscription —
/// anything that rewrites the active config's bytes on disk.
pub fn refresh_entry_summary(
    app: &AppHandle<Wry>,
    id: &str,
    bytes: &[u8],
) -> AppResult<()> {
    let mut lib = load(app);
    if let Some(e) = lib.entries.iter_mut().find(|e| e.id == id) {
        let (oc, rc, tun) = compute_summary_fields(bytes);
        e.outbound_count = oc;
        e.rule_count = rc;
        e.has_tun_inbound = tun;
        e.updated_at_ms = now_ms();
        save(app, &lib)?;
    }
    Ok(())
}

/// Compute (outbound_count, rule_count, has_tun_inbound) from raw config
/// bytes, parsing leniently as JSONC. Returns all-None on parse failure
/// — list still shows the entry, just without counts.
pub fn compute_summary_fields(
    bytes: &[u8],
) -> (Option<u32>, Option<u32>, Option<bool>) {
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return (None, None, None),
    };
    let parsed: serde_json::Value =
        match jsonc_parser::parse_to_serde_value(text, &Default::default()) {
            Ok(Some(v)) => v,
            _ => return (None, None, None),
        };
    let outbound_count = parsed
        .get("outbounds")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as u32);
    let rule_count = parsed
        .pointer("/route/rules")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as u32);
    let has_tun_inbound = parsed.get("inbounds").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .any(|i| i.get("type").and_then(|t| t.as_str()) == Some("tun"))
    });
    (outbound_count, rule_count, has_tun_inbound)
}

/// Copy bytes to the entry's storage path, creating the configs dir on
/// demand. Atomic write isn't strictly necessary here (this file is
/// brand-new each time we Add) but we still go through the helper so
/// behaviour is uniform with rules_commit / subs_apply.
pub fn write_initial(path: &PathBuf, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::util::atomic_write::atomic_write(path, bytes)
}
