use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::core::library::{
    compute_summary_fields, load as load_library, new_entry, now_ms, save as save_library,
    write_initial,
};
use crate::core::{config, validate};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, ConfigEntry, ConfigSource};

/// Native file picker for selecting a sing-box JSON config. Returns None
/// when the user cancels.
#[tauri::command]
pub async fn config_open_dialog(app: AppHandle) -> AppResult<Option<PathBuf>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("sing-box config", &["json", "jsonc"])
        .add_filter("All files", &["*"])
        .pick_file(move |selected| {
            let _ = tx.send(selected);
        });
    let chosen = rx.await.map_err(|e| AppError::Other(e.to_string()))?;
    Ok(chosen.and_then(|fp| fp.into_path().ok()))
}

/// Legacy command kept for backwards compatibility. Now: copy the picked
/// file into the managed library AND select it as active. New UI calls
/// `config_library_add_local` + `config_library_select` directly.
#[tauri::command]
pub async fn config_load(
    path: PathBuf,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<config::ConfigSummary> {
    let entry = add_local_inner(&app, &state, path).await?;
    select_inner(&app, &state, &entry.id).await?;
    let g = state.config.lock();
    let raw = g.raw.clone().unwrap_or_default();
    let parsed = g.parsed.clone().unwrap_or(serde_json::Value::Null);
    let path = g.path.clone().unwrap_or_default();
    Ok(config::build_summary(&path, &raw, &parsed))
}

#[tauri::command]
pub async fn config_validate(
    path: Option<PathBuf>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<validate::ValidationReport> {
    let target_path = match path {
        Some(p) => p,
        None => {
            let raw = state
                .config
                .lock()
                .raw
                .clone()
                .ok_or_else(|| AppError::Config("no config loaded".into()))?;
            let mut tmp = tempfile::Builder::new()
                .prefix("singbox-validate-")
                .suffix(".json")
                .tempfile()?;
            tmp.write_all(&raw)?;
            tmp.flush()?;
            let kept = tmp.into_temp_path();
            let report = validate::validate_path(&app, &kept).await;
            let _ = kept.close();
            return report;
        }
    };
    validate::validate_path(&app, &target_path).await
}

#[tauri::command]
pub async fn config_get_raw(state: State<'_, AppState>) -> AppResult<String> {
    let g = state.config.lock();
    let raw = g
        .raw
        .as_ref()
        .ok_or_else(|| AppError::Config("no config loaded".into()))?;
    // Return original bytes verbatim (preserves comments).
    String::from_utf8(raw.clone()).map_err(|e| AppError::Config(format!("not UTF-8: {e}")))
}

#[tauri::command]
pub async fn config_save(
    content: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let path = state
        .config
        .lock()
        .path
        .clone()
        .ok_or_else(|| AppError::Config("no config loaded".into()))?;
    config::save_raw(&path, &content)?;
    let (raw, parsed) = config::load_from_path(&path)?;
    let active_id = {
        let mut g = state.config.lock();
        g.raw = Some(raw.clone());
        g.parsed = Some(parsed);
        g.active_id.clone()
    };
    if let Some(id) = active_id {
        let _ = crate::core::library::refresh_entry_summary(&app, &id, &raw);
    }
    Ok(())
}

#[tauri::command]
pub async fn config_reveal(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let path = state
        .config
        .lock()
        .path
        .clone()
        .ok_or_else(|| AppError::Config("no config loaded".into()))?;
    reveal_path(&app, &path)
}

#[tauri::command]
pub async fn config_current_path(state: State<'_, AppState>) -> AppResult<Option<PathBuf>> {
    Ok(state.config.lock().path.clone())
}

// ---------------------------------------------------------------- library

/// Slim summary returned by config_library_list — no raw bytes.
#[derive(Debug, Serialize)]
pub struct ConfigEntrySummary {
    pub id: String,
    pub name: String,
    pub source: ConfigSource,
    pub storage_path: PathBuf,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub is_active: bool,
    /// Counts pulled from the on-disk file. None means parse failed.
    pub outbound_count: Option<u32>,
    pub rule_count: Option<u32>,
    pub has_tun_inbound: Option<bool>,
}

#[tauri::command]
pub async fn config_library_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<ConfigEntrySummary>> {
    let mut library = load_library(&app);
    let active_id = library.active_id.clone();

    // Lazy migration: any entry whose summary fields are still None
    // (legacy from before the cache existed) gets filled in here once,
    // and we save the library back so subsequent calls are pure
    // in-memory work. Reads happen on this Tauri command worker — but
    // they're bounded by library size and only on the initial pass,
    // not every list call.
    let mut dirty = false;
    for e in &mut library.entries {
        if e.outbound_count.is_some() && e.rule_count.is_some() && e.has_tun_inbound.is_some() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&e.storage_path) {
            let (oc, rc, tun) = compute_summary_fields(&bytes);
            if e.outbound_count.is_none() {
                e.outbound_count = oc;
            }
            if e.rule_count.is_none() {
                e.rule_count = rc;
            }
            if e.has_tun_inbound.is_none() {
                e.has_tun_inbound = tun;
            }
            dirty = true;
        }
    }
    if dirty {
        let _ = save_library(&app, &library);
    }

    // Sync state's library cache so other callers can read without
    // hitting the store.
    {
        let mut g = state.config.lock();
        g.library = library.entries.clone();
        g.active_id = active_id.clone();
    }

    Ok(library
        .entries
        .into_iter()
        .map(|e| ConfigEntrySummary {
            is_active: Some(&e.id) == active_id.as_ref(),
            outbound_count: e.outbound_count,
            rule_count: e.rule_count,
            has_tun_inbound: e.has_tun_inbound,
            id: e.id,
            name: e.name,
            source: e.source,
            storage_path: e.storage_path,
            created_at_ms: e.created_at_ms,
            updated_at_ms: e.updated_at_ms,
        })
        .collect())
}

#[tauri::command]
pub async fn config_library_add_local(
    path: PathBuf,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ConfigEntry> {
    let _g = state.library_op.lock().await;
    add_local_inner(&app, &state, path).await
}

#[tauri::command]
pub async fn config_library_add_from_text(
    name: String,
    text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ConfigEntry> {
    let _g = state.library_op.lock().await;
    add_text_inner(
        &app,
        &state,
        name,
        text,
        ConfigSource::Local { original_path: None },
    )
    .await
}

#[tauri::command]
pub async fn config_library_remove(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _g = state.library_op.lock().await;
    let mut lib = load_library(&app);
    let pos = lib.entries.iter().position(|e| e.id == id);
    if let Some(p) = pos {
        let entry = lib.entries.remove(p);
        let _ = std::fs::remove_file(&entry.storage_path);
        // If we removed the active one, pick a fallback (first remaining)
        // and re-activate it. If library is now empty, stop the core too.
        if lib.active_id.as_deref() == Some(&id) {
            lib.active_id = lib.entries.first().map(|e| e.id.clone());
            save_library(&app, &lib)?;
            if let Some(new_id) = lib.active_id.clone() {
                select_inner(&app, &state, &new_id).await?;
            } else {
                let _ = crate::commands::core_cmd::core_stop(app.clone(), state.clone()).await;
                let mut g = state.config.lock();
                g.path = None;
                g.raw = None;
                g.parsed = None;
                g.library.clear();
                g.active_id = None;
            }
        } else {
            save_library(&app, &lib)?;
            let mut g = state.config.lock();
            g.library = lib.entries.clone();
        }
    }
    let _ = app.emit("library:changed", ());
    Ok(())
}

#[tauri::command]
pub async fn config_library_rename(
    id: String,
    new_name: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _g = state.library_op.lock().await;
    let mut lib = load_library(&app);
    if let Some(e) = lib.entries.iter_mut().find(|e| e.id == id) {
        e.name = new_name;
        e.updated_at_ms = now_ms();
    }
    save_library(&app, &lib)?;
    state.config.lock().library = lib.entries;
    let _ = app.emit("library:changed", ());
    Ok(())
}

#[tauri::command]
pub async fn config_library_select(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<config::ConfigSummary>> {
    let _g = state.library_op.lock().await;
    select_inner(&app, &state, &id).await?;
    Ok(active_summary_from_cache(&state))
}

/// Build a ConfigSummary from the currently cached active config (if any).
/// Returns None when no config is active. Callers: app boot prefetch,
/// `library:changed` listener, and any "give me the current summary"
/// situation that doesn't want to round-trip through `config_load(path)`
/// (which is the legacy add+select shim).
#[tauri::command]
pub async fn config_active_summary(
    state: State<'_, AppState>,
) -> AppResult<Option<config::ConfigSummary>> {
    Ok(active_summary_from_cache(&state))
}

fn active_summary_from_cache(state: &State<'_, AppState>) -> Option<config::ConfigSummary> {
    let g = state.config.lock();
    match (g.path.as_ref(), g.raw.as_ref(), g.parsed.as_ref()) {
        (Some(p), Some(r), Some(v)) => Some(config::build_summary(p, r, v)),
        _ => None,
    }
}

/// In-place refresh: for an entry whose source = Subscription, fetch the
/// associated subscription's URL again and overwrite its storage_path.
/// If this entry is the active one, the running sing-box is restarted.
#[tauri::command]
pub async fn config_library_refresh_from_subscription(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _g = state.library_op.lock().await;
    let mut lib = crate::core::library::load(&app);
    let entry = lib
        .entries
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Other(format!("no entry {id}")))?;
    let sub_id = match &entry.source {
        ConfigSource::Subscription { sub_id, .. } => sub_id.clone(),
        ConfigSource::Local { .. } => {
            return Err(AppError::Other(
                "this config wasn't imported from a subscription".into(),
            ))
        }
    };
    let subs = crate::core::subscriptions::load_all(&app);
    let sub = subs
        .iter()
        .find(|s| s.id == sub_id)
        .ok_or_else(|| AppError::Other(format!("subscription {sub_id} no longer exists")))?
        .clone();
    let (text, _parsed) = crate::core::subscriptions::fetch_full_config(&sub.url).await?;

    // Overwrite storage. atomic_write keeps a .bak.
    crate::util::atomic_write::atomic_write(&entry.storage_path, text.as_bytes())?;
    entry.source = ConfigSource::Subscription {
        sub_id: sub.id.clone(),
        fetched_at_ms: crate::core::library::now_ms(),
    };
    entry.updated_at_ms = crate::core::library::now_ms();
    let (oc, rc, tun) = crate::core::library::compute_summary_fields(text.as_bytes());
    entry.outbound_count = oc;
    entry.rule_count = rc;
    entry.has_tun_inbound = tun;
    let was_active = lib.active_id.as_deref() == Some(&id);
    crate::core::library::save(&app, &lib)?;
    state.config.lock().library = lib.entries.clone();

    // Update the subscription's last_fetched timestamp too.
    let mut all = crate::core::subscriptions::load_all(&app);
    if let Some(s) = all.iter_mut().find(|s| s.id == sub_id) {
        s.last_fetched_at_ms = Some(crate::core::library::now_ms());
        s.last_error = None;
    }
    crate::core::subscriptions::save_all(&app, &all)?;

    let _ = app.emit("library:changed", ());

    // If this was the active one, reload the cache + restart core.
    if was_active {
        select_inner(&app, &state, &id).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn config_library_view(
    id: String,
    app: AppHandle,
) -> AppResult<String> {
    let lib = load_library(&app);
    let entry = lib
        .entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Other(format!("no entry {id}")))?;
    let bytes = std::fs::read(&entry.storage_path)
        .map_err(|e| AppError::Io(e))?;
    String::from_utf8(bytes).map_err(|e| AppError::Config(format!("not UTF-8: {e}")))
}

#[tauri::command]
pub async fn config_library_reveal(
    id: String,
    app: AppHandle,
) -> AppResult<()> {
    let lib = load_library(&app);
    let entry = lib
        .entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Other(format!("no entry {id}")))?;
    reveal_path(&app, &entry.storage_path)
}

// ---------------------------------------------------------------- internals

async fn add_local_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: PathBuf,
) -> AppResult<ConfigEntry> {
    let bytes = std::fs::read(&path)
        .map_err(|e| AppError::Config(format!("read {}: {e}", path.display())))?;
    let display_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".into());
    add_text_inner(
        app,
        state,
        display_name,
        String::from_utf8_lossy(&bytes).into_owned(),
        ConfigSource::Local {
            original_path: Some(path),
        },
    )
    .await
}

async fn add_text_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    name: String,
    text: String,
    source: ConfigSource,
) -> AppResult<ConfigEntry> {
    // Validate it parses as JSON and is an object root before we copy.
    // sing-box configs are always JSON objects; if a user pastes a JSON
    // array (or scalar) here, our jsonc_edit surgical-edit pass would
    // later replace the root with a fresh empty object on first rule
    // commit, silently destroying the imported data.
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| AppError::Config(format!("not valid JSON: {e}")))?;
    if !v.is_object() {
        return Err(AppError::Config(
            "config root must be a JSON object (sing-box configs are always {…}, not arrays)".into(),
        ));
    }
    let mut entry = new_entry(name, source)?;
    let (oc, rc, tun) = compute_summary_fields(text.as_bytes());
    entry.outbound_count = oc;
    entry.rule_count = rc;
    entry.has_tun_inbound = tun;
    write_initial(&entry.storage_path, text.as_bytes())?;
    let mut lib = load_library(app);
    lib.entries.push(entry.clone());
    let became_first = lib.active_id.is_none();
    if became_first {
        lib.active_id = Some(entry.id.clone());
    }
    save_library(app, &lib)?;
    state.config.lock().library = lib.entries;
    state.config.lock().active_id = lib.active_id.clone();
    let _ = app.emit("library:changed", ());

    // If this is the very first entry, also load it as active so the rest
    // of the UI (rules / proxies / dashboard) has something to show.
    // Awaiting select_inner here is correct now that this function is async
    // — the previous block_on parked the Tauri worker thread for the full
    // 30 s readiness probe and could deadlock the runtime.
    if became_first {
        // best-effort, swallow errors here so the Add succeeds even if
        // the activation has a hiccup.
        let _ = select_inner(app, state, &entry.id).await;
    }
    Ok(entry)
}

pub(crate) async fn select_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    id: &str,
) -> AppResult<()> {
    let mut lib = load_library(app);
    let entry = lib
        .entries
        .iter()
        .find(|e| e.id == id)
        .cloned()
        .ok_or_else(|| AppError::Other(format!("no entry {id}")))?;
    let (raw, parsed) = config::load_from_path(&entry.storage_path)?;
    {
        let mut g = state.config.lock();
        g.path = Some(entry.storage_path.clone());
        g.raw = Some(raw);
        g.parsed = Some(parsed);
        g.active_id = Some(entry.id.clone());
        g.library = lib.entries.clone();
    }
    lib.active_id = Some(entry.id.clone());
    save_library(app, &lib)?;

    // After a select, sing-box should always be running with the picked
    // config. If it was already up: stop+start to swap. If it was down:
    // start fresh. There are no Start/Stop buttons — select IS the
    // user's intent to make this proxy active. Failure of start is
    // surfaced; failure of stop on a stopped core is silent.
    //
    // Emit library:changed AFTER the core has swapped so frontend
    // listeners (App.tsx refetches summary, Proxies refetches /proxies
    // against clash_api) see the new running core, not the dying old
    // one. Otherwise Proxies briefly hits the previous session's
    // clash_api with the new active config's outbound list.
    let _ = crate::commands::core_cmd::core_stop(app.clone(), state.clone()).await;
    let _ = crate::commands::core_cmd::core_start(app.clone(), state.clone()).await?;
    let _ = app.emit("library:changed", ());
    Ok(())
}

fn reveal_path(app: &AppHandle, path: &PathBuf) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Other(format!("path has no parent: {}", path.display())))?;
    use tauri_plugin_shell::ShellExt;
    #[allow(deprecated)]
    app.shell()
        .open(parent.display().to_string(), None)
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}

/// Hydrate the active config from the persisted library at app boot. Called
/// once from lib::run setup.
pub fn hydrate_on_startup(app: &AppHandle) {
    let lib = load_library(app);
    let state = app.state::<AppState>();
    {
        let mut g = state.config.lock();
        g.library = lib.entries.clone();
        g.active_id = lib.active_id.clone();
    }
    if let Some(active_id) = lib.active_id.clone() {
        if let Some(entry) = lib.entries.iter().find(|e| e.id == active_id) {
            if let Ok((raw, parsed)) = config::load_from_path(&entry.storage_path) {
                let mut g = state.config.lock();
                g.path = Some(entry.storage_path.clone());
                g.raw = Some(raw);
                g.parsed = Some(parsed);
            } else {
                tracing::warn!("active config file missing or unreadable: {}", entry.storage_path.display());
            }
        }
    }
}
