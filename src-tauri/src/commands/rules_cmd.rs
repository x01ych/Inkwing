//! Route rules + rule_sets editing through the overrides layer.
//!
//! `state.config.parsed` is the user's source config and is **never
//! modified** by these commands. Every mutation writes to per-config or
//! global override files (see `core/overrides.rs`). On `core_start` the
//! source is cloned, overrides are merged in, and the result is written
//! to `data_dir/runtime/config.json` for sing-box to read.
//!
//! ID convention used over the IPC boundary:
//!   - **Source-config rules**: `id` = SHA-256 hex signature (64 chars, no dashes).
//!   - **Local rules** (per-config or global): `id` = UUID v4 (36 chars, with dashes).
//! `id_is_signature(id)` distinguishes by `!id.contains('-')`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::core::overrides::{
    load_global, load_per_config, save_global, save_per_config, signature, ArrayOverrides,
    GlobalOverrides, LocalEntry, LocalOverrides, ModificationEntry,
};
use crate::core::rules::{
    input_to_value, rule_set_input_to_value, rule_set_to_view, rule_to_view, RuleInput,
    RuleSetInput, RuleSetView, RuleView,
};
use crate::error::{AppError, AppResult};
use crate::paths::{global_overrides_path, per_config_overrides_path};
use crate::state::AppState;

// ---------- public DTOs -------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    Config,
    LocalPer,
    LocalGlobal,
}

/// Snapshot a rule with the metadata the UI needs to render the right
/// badges and action buttons.
#[derive(Debug, Clone, Serialize)]
pub struct RuleViewWithBadge {
    /// Stable id over IPC. Signature for config rules, UUID for local.
    pub id: String,
    pub view: RuleView,
    pub source: RuleSource,
    /// True iff this is a config rule that has a modification override.
    /// `view` already reflects the override values; the original is in
    /// `original_signature` for revert.
    pub modified: bool,
    /// True iff this is a config rule whose signature is in `masked`.
    /// `view` is rendered grey but otherwise untouched.
    pub masked: bool,
    /// Only present when `modified` is true — the source's original
    /// signature so revert can find the modification entry.
    pub original_signature: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    PerConfig,
    Global,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleSetViewWithBadge {
    pub id: String,
    pub view: RuleSetView,
    pub source: RuleSource,
    pub modified: bool,
    pub masked: bool,
    pub original_signature: Option<String>,
}

// ---------- helpers ----------------------------------------------------

fn id_is_signature(id: &str) -> bool {
    !id.contains('-')
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Load overrides for the currently-active ConfigEntry (if any) plus the
/// shared global file. If no active id, both are defaults.
fn load_overrides_for_active(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> AppResult<(Option<String>, LocalOverrides, GlobalOverrides)> {
    let active_id = state.config.lock().active_id.clone();
    let per = match active_id.as_ref() {
        Some(id) => load_per_config(&per_config_overrides_path(id)?),
        None => LocalOverrides::default(),
    };
    let global = load_global(&global_overrides_path()?);
    let _ = app; // kept for future emit hooks
    Ok((active_id, per, global))
}

fn save_per(app: &AppHandle, id: &str, ov: &LocalOverrides) -> AppResult<()> {
    let path = per_config_overrides_path(id)?;
    save_per_config(&path, ov)?;
    let _ = app.emit("overrides:changed", id);
    Ok(())
}

fn save_glob(app: &AppHandle, ov: &GlobalOverrides) -> AppResult<()> {
    save_global(&global_overrides_path()?, ov)?;
    let _ = app.emit("overrides:changed", "__global__");
    Ok(())
}

/// Read the source array at `path` from cached parsed config.
/// Empty Vec when no config is loaded (boot before select).
fn source_array(state: &State<'_, AppState>, ptr: &str) -> Vec<Value> {
    let g = state.config.lock();
    g.parsed
        .as_ref()
        .and_then(|p| p.pointer(ptr))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

// ---------- route.rules commands ----------------------------------------

#[tauri::command]
pub async fn rules_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleViewWithBadge>> {
    let (_active, per, global) = load_overrides_for_active(&app, &state)?;
    let source = source_array(&state, "/route/rules");
    Ok(merge_rules_for_view(
        &source,
        &per.route_rules,
        &global.route_rules,
    ))
}

fn merge_rules_for_view(
    source: &[Value],
    per: &ArrayOverrides,
    global_appended: &[LocalEntry],
) -> Vec<RuleViewWithBadge> {
    let mut out: Vec<RuleViewWithBadge> = Vec::with_capacity(
        source.len() + per.appended.len() + global_appended.len(),
    );
    for item in source {
        let sig = signature(item);
        let masked = per.masked.contains(&sig);
        let modified = per.modifications.contains_key(&sig);
        // What do we show for the view? When modified, render the
        // override values; otherwise the source values.
        let display_value = if let Some(m) = per.modifications.get(&sig) {
            &m.override_value
        } else {
            item
        };
        let view = rule_to_view(out.len(), display_value);
        out.push(RuleViewWithBadge {
            id: sig.clone(),
            view,
            source: RuleSource::Config,
            modified,
            masked,
            original_signature: if modified { Some(sig) } else { None },
        });
    }
    for e in &per.appended {
        let view = rule_to_view(out.len(), &e.value);
        out.push(RuleViewWithBadge {
            id: e.id.clone(),
            view,
            source: RuleSource::LocalPer,
            modified: false,
            masked: false,
            original_signature: None,
        });
    }
    for e in global_appended {
        let view = rule_to_view(out.len(), &e.value);
        out.push(RuleViewWithBadge {
            id: e.id.clone(),
            view,
            source: RuleSource::LocalGlobal,
            modified: false,
            masked: false,
            original_signature: None,
        });
    }
    out
}

#[tauri::command]
pub async fn rules_add(
    rule: RuleInput,
    scope: Scope,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleViewWithBadge>> {
    let value = input_to_value(&rule)?;
    let entry = LocalEntry {
        id: Uuid::new_v4().to_string(),
        value,
        created_at_ms: now_ms(),
    };
    match scope {
        Scope::PerConfig => {
            let active_id = state
                .config
                .lock()
                .active_id
                .clone()
                .ok_or_else(|| AppError::Config("no active config; can't add per-config rule".into()))?;
            let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
            per.route_rules.appended.push(entry);
            save_per(&app, &active_id, &per)?;
        }
        Scope::Global => {
            let mut g = load_global(&global_overrides_path()?);
            g.route_rules.push(entry);
            save_glob(&app, &g)?;
        }
    }
    rules_list(app, state).await
}

#[tauri::command]
pub async fn rules_update(
    id: String,
    rule: RuleInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleViewWithBadge>> {
    let new_value = input_to_value(&rule)?;
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;

    if id_is_signature(&id) {
        // Update of a source-config rule → write into modifications
        // (demote to local override; source untouched).
        let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
        per.route_rules.modifications.insert(
            id.clone(),
            ModificationEntry {
                override_value: new_value,
                original_signature_preview: id.chars().take(16).collect(),
                modified_at_ms: now_ms(),
            },
        );
        // If the source rule was masked, unmask it now (mod implies "use the override").
        per.route_rules.masked.remove(&id);
        save_per(&app, &active_id, &per)?;
    } else {
        // Local rule update — find by UUID in per-config first, then global.
        let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
        if let Some(e) = per.route_rules.appended.iter_mut().find(|e| e.id == id) {
            e.value = new_value;
            save_per(&app, &active_id, &per)?;
        } else {
            let mut g = load_global(&global_overrides_path()?);
            if let Some(e) = g.route_rules.iter_mut().find(|e| e.id == id) {
                e.value = new_value;
                save_glob(&app, &g)?;
            } else {
                return Err(AppError::Config(format!("no local rule with id {id}")));
            }
        }
    }
    rules_list(app, state).await
}

#[tauri::command]
pub async fn rules_delete(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleViewWithBadge>> {
    if id_is_signature(&id) {
        return Err(AppError::Config(
            "config rules can't be deleted — use mask instead".into(),
        ));
    }
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    let pre_len = per.route_rules.appended.len();
    per.route_rules.appended.retain(|e| e.id != id);
    if per.route_rules.appended.len() != pre_len {
        save_per(&app, &active_id, &per)?;
    } else {
        let mut g = load_global(&global_overrides_path()?);
        let pre = g.route_rules.len();
        g.route_rules.retain(|e| e.id != id);
        if g.route_rules.len() != pre {
            save_glob(&app, &g)?;
        } else {
            return Err(AppError::Config(format!("no rule with id {id}")));
        }
    }
    rules_list(app, state).await
}

#[tauri::command]
pub async fn rules_mask(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleViewWithBadge>> {
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    per.route_rules.masked.insert(signature_id);
    save_per(&app, &active_id, &per)?;
    rules_list(app, state).await
}

#[tauri::command]
pub async fn rules_unmask(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleViewWithBadge>> {
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    per.route_rules.masked.remove(&signature_id);
    save_per(&app, &active_id, &per)?;
    rules_list(app, state).await
}

#[tauri::command]
pub async fn rules_revert(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleViewWithBadge>> {
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    per.route_rules.modifications.remove(&signature_id);
    save_per(&app, &active_id, &per)?;
    rules_list(app, state).await
}

#[tauri::command]
pub async fn rules_reorder(
    ids: Vec<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleViewWithBadge>> {
    // Reorder applies only to local-appended subsets, and only within
    // the same scope (per-config OR global). Source rules can't be
    // reordered — their order is the source file's. The frontend
    // should detect a cross-source drag and refuse.
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    let mut g = load_global(&global_overrides_path()?);

    // Separate ids into per/global based on which list contains them.
    // Anything not found is ignored (likely a signature, which means
    // "don't move me anyway").
    let per_target: Vec<LocalEntry> = ids
        .iter()
        .filter_map(|id| per.route_rules.appended.iter().find(|e| e.id == *id).cloned())
        .collect();
    if per_target.len() == per.route_rules.appended.len() && !per_target.is_empty() {
        per.route_rules.appended = per_target;
        save_per(&app, &active_id, &per)?;
    }
    let global_target: Vec<LocalEntry> = ids
        .iter()
        .filter_map(|id| g.route_rules.iter().find(|e| e.id == *id).cloned())
        .collect();
    if global_target.len() == g.route_rules.len() && !global_target.is_empty() {
        g.route_rules = global_target;
        save_glob(&app, &g)?;
    }
    rules_list(app, state).await
}

/// Persist nothing — overrides are written incrementally on every
/// mutation. The only point of this command is to optionally restart
/// sing-box so the new merged runtime config is in effect.
#[tauri::command]
pub async fn rules_commit(
    restart: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _ = app.emit("config:saved", "overrides");
    if restart {
        let _ = crate::commands::core_cmd::core_stop(app.clone(), state.clone()).await;
        let _ = crate::commands::core_cmd::core_start(app, state).await?;
    }
    Ok(())
}

// ---------- route.rule_set commands -------------------------------------

#[tauri::command]
pub async fn rule_sets_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetViewWithBadge>> {
    let (_active, per, global) = load_overrides_for_active(&app, &state)?;
    let source = source_array(&state, "/route/rule_set");
    Ok(merge_rule_sets_for_view(
        &source,
        &per.route_rule_set,
        &global.route_rule_set,
    ))
}

fn merge_rule_sets_for_view(
    source: &[Value],
    per: &ArrayOverrides,
    global_appended: &[LocalEntry],
) -> Vec<RuleSetViewWithBadge> {
    let mut out: Vec<RuleSetViewWithBadge> = Vec::with_capacity(
        source.len() + per.appended.len() + global_appended.len(),
    );
    for item in source {
        let sig = signature(item);
        let masked = per.masked.contains(&sig);
        let modified = per.modifications.contains_key(&sig);
        let display = if let Some(m) = per.modifications.get(&sig) {
            &m.override_value
        } else {
            item
        };
        let view = rule_set_to_view(out.len(), display);
        out.push(RuleSetViewWithBadge {
            id: sig.clone(),
            view,
            source: RuleSource::Config,
            modified,
            masked,
            original_signature: if modified { Some(sig) } else { None },
        });
    }
    for e in &per.appended {
        let view = rule_set_to_view(out.len(), &e.value);
        out.push(RuleSetViewWithBadge {
            id: e.id.clone(),
            view,
            source: RuleSource::LocalPer,
            modified: false,
            masked: false,
            original_signature: None,
        });
    }
    for e in global_appended {
        let view = rule_set_to_view(out.len(), &e.value);
        out.push(RuleSetViewWithBadge {
            id: e.id.clone(),
            view,
            source: RuleSource::LocalGlobal,
            modified: false,
            masked: false,
            original_signature: None,
        });
    }
    out
}

#[tauri::command]
pub async fn rule_sets_add(
    rule_set: RuleSetInput,
    scope: Scope,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetViewWithBadge>> {
    let value = rule_set_input_to_value(&rule_set)?;
    let entry = LocalEntry {
        id: Uuid::new_v4().to_string(),
        value,
        created_at_ms: now_ms(),
    };
    match scope {
        Scope::PerConfig => {
            let active_id = state
                .config
                .lock()
                .active_id
                .clone()
                .ok_or_else(|| AppError::Config("no active config".into()))?;
            let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
            per.route_rule_set.appended.push(entry);
            save_per(&app, &active_id, &per)?;
        }
        Scope::Global => {
            let mut g = load_global(&global_overrides_path()?);
            g.route_rule_set.push(entry);
            save_glob(&app, &g)?;
        }
    }
    rule_sets_list(app, state).await
}

#[tauri::command]
pub async fn rule_sets_update(
    id: String,
    rule_set: RuleSetInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetViewWithBadge>> {
    let new_value = rule_set_input_to_value(&rule_set)?;
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    if id_is_signature(&id) {
        let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
        per.route_rule_set.modifications.insert(
            id.clone(),
            ModificationEntry {
                override_value: new_value,
                original_signature_preview: id.chars().take(16).collect(),
                modified_at_ms: now_ms(),
            },
        );
        per.route_rule_set.masked.remove(&id);
        save_per(&app, &active_id, &per)?;
    } else {
        let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
        if let Some(e) = per.route_rule_set.appended.iter_mut().find(|e| e.id == id) {
            e.value = new_value;
            save_per(&app, &active_id, &per)?;
        } else {
            let mut g = load_global(&global_overrides_path()?);
            if let Some(e) = g.route_rule_set.iter_mut().find(|e| e.id == id) {
                e.value = new_value;
                save_glob(&app, &g)?;
            } else {
                return Err(AppError::Config(format!("no local rule_set with id {id}")));
            }
        }
    }
    rule_sets_list(app, state).await
}

#[tauri::command]
pub async fn rule_sets_delete(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetViewWithBadge>> {
    if id_is_signature(&id) {
        return Err(AppError::Config(
            "config rule_sets can't be deleted — use mask instead".into(),
        ));
    }
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    let pre_len = per.route_rule_set.appended.len();
    per.route_rule_set.appended.retain(|e| e.id != id);
    if per.route_rule_set.appended.len() != pre_len {
        save_per(&app, &active_id, &per)?;
    } else {
        let mut g = load_global(&global_overrides_path()?);
        let pre = g.route_rule_set.len();
        g.route_rule_set.retain(|e| e.id != id);
        if g.route_rule_set.len() != pre {
            save_glob(&app, &g)?;
        } else {
            return Err(AppError::Config(format!("no rule_set with id {id}")));
        }
    }
    rule_sets_list(app, state).await
}

#[tauri::command]
pub async fn rule_sets_mask(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetViewWithBadge>> {
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    per.route_rule_set.masked.insert(signature_id);
    save_per(&app, &active_id, &per)?;
    rule_sets_list(app, state).await
}

#[tauri::command]
pub async fn rule_sets_unmask(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetViewWithBadge>> {
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    per.route_rule_set.masked.remove(&signature_id);
    save_per(&app, &active_id, &per)?;
    rule_sets_list(app, state).await
}

#[tauri::command]
pub async fn rule_sets_revert(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetViewWithBadge>> {
    let active_id = state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))?;
    let mut per = load_per_config(&per_config_overrides_path(&active_id)?);
    per.route_rule_set.modifications.remove(&signature_id);
    save_per(&app, &active_id, &per)?;
    rule_sets_list(app, state).await
}

#[tauri::command]
pub async fn rule_sets_commit(
    restart: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _ = app.emit("config:saved", "overrides");
    if restart {
        let _ = crate::commands::core_cmd::core_stop(app.clone(), state.clone()).await;
        let _ = crate::commands::core_cmd::core_start(app, state).await?;
    }
    Ok(())
}

