use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::core::log_pump::LogEntry;
use crate::core::process::SidecarHandle;
use crate::util::ring_buffer::RingBuffer;

/// Top-level app state. Held inside Tauri's `State<AppState>`.
pub struct AppState {
    pub core: Arc<Mutex<CoreState>>,
    pub config: Arc<Mutex<ConfigState>>,
    pub logs: Arc<Mutex<RingBuffer<LogEntry>>>,
    /// Serialises settings_set: each call holds this for the entire
    /// merge → persist → core_restart window so a fast double-toggle
    /// (TUN, then mode, then TUN again) can't overlap and lose the
    /// later-arriving overlay change. Tokio mutex (not parking_lot)
    /// because we need to hold it across awaits.
    pub settings_op: Arc<tokio::sync::Mutex<()>>,
    /// Same shape, for library mutations (add_local / add_from_text /
    /// remove / rename / select / refresh / subs_apply). Without it, two
    /// concurrent library mutations both load_library, mutate their
    /// local copies, and save — the later writer clobbers the earlier
    /// writer's entry. Held across the load → mutate → save → emit
    /// window.
    pub library_op: Arc<tokio::sync::Mutex<()>>,
    /// Monotonically increasing counter, bumped on every successful
    /// `core_start`. Each pump stamps its emits with the current value;
    /// the frontend stores drop events whose epoch != currentEpoch. This
    /// guards against a stale emit from a just-stopped session leaking
    /// into the just-started one (abort() doesn't drain Tauri's emit
    /// queue, so without this the new traffic chart can briefly show
    /// dead-session bytes).
    pub session_epoch: Arc<AtomicU64>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            core: Arc::new(Mutex::new(CoreState::default())),
            config: Arc::new(Mutex::new(ConfigState::default())),
            logs: Arc::new(Mutex::new(RingBuffer::new(2000))),
            settings_op: Arc::new(tokio::sync::Mutex::new(())),
            library_op: Arc::new(tokio::sync::Mutex::new(())),
            session_epoch: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Default)]
pub struct CoreState {
    pub running: bool,
    pub pid: Option<u32>,
    pub clash_api_addr: Option<String>,
    pub clash_api_secret: Option<String>,
    pub version: Option<String>,
    pub started_at_ms: Option<u64>,
    pub handle: Option<SidecarHandle>,
    pub traffic_task: Option<JoinHandle<()>>,
    pub log_task: Option<JoinHandle<()>>,
    pub conn_task: Option<JoinHandle<()>>,
}

/// Config state.
///
/// `library` + `active_id` is the source of truth. The
/// `path/raw/parsed` triple is a *cache* mirroring whichever library
/// entry is currently active — commands like rules_cmd / subs_cmd /
/// validate / save / reveal read from it. Switching active simply
/// re-loads that cache from disk.
#[derive(Default)]
pub struct ConfigState {
    /// Cache of the currently active config (mirrors library[active_id]).
    pub path: Option<PathBuf>,
    pub raw: Option<Vec<u8>>,
    pub parsed: Option<serde_json::Value>,

    /// Library of managed configs. Persisted to a tauri-plugin-store file.
    pub library: Vec<ConfigEntry>,
    pub active_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub id: String,
    pub name: String,
    pub source: ConfigSource,
    /// Where the config text actually lives on disk (always under
    /// `configs_dir/<id>.json`). All edits happen against this path; the
    /// user's original local file (when source = Local) is never modified.
    pub storage_path: PathBuf,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// Cached summary fields. Computed on add / refresh / rules_commit
    /// so config_library_list doesn't have to re-read+parse every entry
    /// on each UI refresh. None = unknown (legacy entries before this
    /// caching was added; will be filled in lazily on first list).
    #[serde(default)]
    pub outbound_count: Option<u32>,
    #[serde(default)]
    pub rule_count: Option<u32>,
    #[serde(default)]
    pub has_tun_inbound: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigSource {
    /// User picked a file via OS dialog or Add-from-text.
    Local {
        /// Filesystem path the user originally pointed us at, if any
        /// (None for paste-from-text). Informational only — we don't
        /// write back to it.
        original_path: Option<PathBuf>,
    },
    /// Auto-generated from a subscription fetch.
    Subscription {
        sub_id: String,
        fetched_at_ms: u64,
    },
}
