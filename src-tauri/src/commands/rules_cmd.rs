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
use crate::core::singbox_cache;
use crate::error::{AppError, AppResult};
use crate::paths::{cache_file_path, global_overrides_path, per_config_overrides_path};
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

/// Diagnostic probe of the active source config's `route` section so the
/// empty Rule Sets tab can explain *why* it's empty (missing route
/// entirely? rule_set under a different / mistyped key? present-but-empty?).
#[derive(Debug, Clone, Serialize)]
pub struct RouteProbeReport {
    pub config_loaded: bool,
    pub config_path: Option<String>,
    pub has_route: bool,
    /// All top-level keys directly under `route` in user's source config.
    pub route_keys: Vec<String>,
    pub has_route_rule_set: bool,
    pub route_rule_set_is_array: bool,
    pub route_rule_set_len: usize,
    /// Keys that look like a mistyped `rule_set` field (e.g. `ruleset`,
    /// `rule-set`, `RuleSet`, `rule_sets`). Empty unless we spot one.
    pub similar_route_keys: Vec<String>,
    /// First 3 chars-truncated rule entries from `route.rules`, so the
    /// user can see whether their rules use `rule_set` matchers that
    /// reference tags they haven't actually defined yet.
    pub rules_using_rule_set_matcher: usize,
    pub rules_total: usize,
}

#[tauri::command]
pub async fn rule_sets_probe(state: State<'_, AppState>) -> AppResult<RouteProbeReport> {
    let g = state.config.lock();
    let parsed = match g.parsed.as_ref() {
        None => {
            return Ok(RouteProbeReport {
                config_loaded: false,
                config_path: g.path.as_ref().map(|p| p.display().to_string()),
                has_route: false,
                route_keys: vec![],
                has_route_rule_set: false,
                route_rule_set_is_array: false,
                route_rule_set_len: 0,
                similar_route_keys: vec![],
                rules_using_rule_set_matcher: 0,
                rules_total: 0,
            });
        }
        Some(v) => v,
    };
    let route = parsed.get("route");
    let has_route = route.is_some();
    let route_keys: Vec<String> = route
        .and_then(|r| r.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let rs = route.and_then(|r| r.get("rule_set"));
    let route_rule_set_is_array = matches!(rs, Some(Value::Array(_)));
    let route_rule_set_len = rs
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    // Catch common typos / casing differences.
    let likely_typos: &[&str] = &[
        "ruleset", "rulesets", "rule_sets", "rule-set", "ruleSet", "RuleSet",
        "providers", "rule_provider", "rule-provider", "rule_providers",
    ];
    let similar_route_keys = route_keys
        .iter()
        .filter(|k| likely_typos.iter().any(|t| t.eq_ignore_ascii_case(k.as_str())))
        .cloned()
        .collect::<Vec<_>>();

    let rules = route
        .and_then(|r| r.get("rules"))
        .and_then(|v| v.as_array());
    let rules_total = rules.map(|a| a.len()).unwrap_or(0);
    let rules_using_rule_set_matcher = rules
        .map(|arr| {
            arr.iter()
                .filter(|r| r.get("rule_set").is_some())
                .count()
        })
        .unwrap_or(0);

    Ok(RouteProbeReport {
        config_loaded: true,
        config_path: g.path.as_ref().map(|p| p.display().to_string()),
        has_route,
        route_keys,
        has_route_rule_set: rs.is_some(),
        route_rule_set_is_array,
        route_rule_set_len,
        similar_route_keys,
        rules_using_rule_set_matcher,
        rules_total,
    })
}

#[tauri::command]
pub async fn rule_sets_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetViewWithBadge>> {
    let (active_id, per, global) = load_overrides_for_active(&app, &state)?;
    let source = source_array(&state, "/route/rule_set");

    // Diagnostic: surface the read-path state so an empty Rules tab is
    // easy to debug (active config? source has rule_set? overrides?).
    let parsed_state = {
        let g = state.config.lock();
        match g.parsed.as_ref() {
            None => "parsed=None",
            Some(p) => {
                if p.pointer("/route/rule_set").is_some() {
                    "parsed=Some, /route/rule_set present"
                } else if p.pointer("/route").is_some() {
                    "parsed=Some, /route present, /route/rule_set MISSING"
                } else {
                    "parsed=Some, /route MISSING"
                }
            }
        }
    };
    tracing::info!(
        active_id = ?active_id,
        parsed_state = %parsed_state,
        source_len = source.len(),
        per_appended = per.route_rule_set.appended.len(),
        per_modifications = per.route_rule_set.modifications.len(),
        per_masked = per.route_rule_set.masked.len(),
        global_appended = global.route_rule_set.len(),
        "rule_sets_list called"
    );

    let mut merged = merge_rule_sets_for_view(
        &source,
        &per.route_rule_set,
        &global.route_rule_set,
    );
    tracing::info!(merged_len = merged.len(), "rule_sets_list step 1: merge done");

    // Best-effort: enrich each row with sing-box's own last_updated /
    // etag from cache.db. Failures are non-fatal — the UI just shows
    // "Never" for those rows. Wrap the WHOLE block in
    // spawn_blocking + a timeout so a slow / blocking jammdb call on
    // Windows (cache.db is locked by sing-box, or the file is large,
    // or some FS oddity) can't hang the entire rule_sets_list command.
    let cache_id = {
        let g = state.config.lock();
        singbox_cache::cache_id_for(g.parsed.as_ref())
    };
    tracing::info!(cache_id = %cache_id, "rule_sets_list step 2: cache_id resolved");
    if let Ok(db_path) = cache_file_path() {
        tracing::info!(db_path = %db_path.display(), exists = db_path.exists(), "rule_sets_list step 3: cache_file_path");
        // Move the potentially-blocking cache read off the tokio worker
        // thread, and cap it at 2 seconds so we never hang the IPC.
        let cache_id_owned = cache_id.clone();
        let db_path_owned = db_path.clone();
        let cache_result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::task::spawn_blocking(move || {
                singbox_cache::read_rule_set_status(&db_path_owned, &cache_id_owned)
            }),
        )
        .await;
        match cache_result {
            Ok(Ok(Ok(map))) if !map.is_empty() => {
                tracing::info!(map_len = map.len(), "rule_sets_list step 4: cache enrichment");
                for row in &mut merged {
                    if let Some(st) = map.get(&row.view.tag) {
                        if st.last_updated_ms > 0 {
                            row.view.last_updated_ms = Some(st.last_updated_ms);
                        }
                        if !st.etag.is_empty() {
                            row.view.etag = Some(st.etag.clone());
                        }
                    }
                }
            }
            Ok(Ok(Ok(_))) => {
                tracing::info!("rule_sets_list step 4: cache empty");
            }
            Ok(Ok(Err(e))) => {
                tracing::warn!("rule_sets_list step 4: singbox_cache read failed: {e}");
            }
            Ok(Err(join_err)) => {
                tracing::warn!("rule_sets_list step 4: cache read task panicked: {join_err}");
            }
            Err(_) => {
                tracing::warn!("rule_sets_list step 4: cache read TIMED OUT after 2s — falling back to no enrichment");
            }
        }
    }
    tracing::info!(
        returning = merged.len(),
        first_tag = merged.first().map(|r| r.view.tag.as_str()).unwrap_or(""),
        "rule_sets_list returning"
    );
    Ok(merged)
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

// ---------- rule_set refresh (sing-box-native) -------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RuleSetRefreshResult {
    pub tag: String,
    pub ok: bool,
    pub new_last_updated_ms: Option<u64>,
    pub error: Option<String>,
}

/// Force sing-box to re-download a single remote rule_set:
///   1. stop the core (so cache.db unlocks)
///   2. delete the cached entry for `tag` from cache.db
///   3. start the core; sing-box's RemoteRuleSet.Start() sees a cache
///      miss and synchronously fetches the URL.
///
/// Errors:
///   - tag is not a remote rule_set → returns Err
///   - the cache invalidate / start step fails → returned as part of the
///     result so the UI can show it; we still try to restart the core.
#[tauri::command]
pub async fn rule_set_refresh(
    tag: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<RuleSetRefreshResult> {
    // Verify the tag exists and is remote.
    let merged = rule_sets_list(app.clone(), state.clone()).await?;
    let row = merged
        .iter()
        .find(|r| r.view.tag == tag)
        .ok_or_else(|| AppError::Config(format!("no rule_set with tag '{tag}'")))?;
    if row.view.kind != "remote" {
        return Err(AppError::Config(format!(
            "rule_set '{tag}' is type '{}' — only remote rule_sets can be refreshed",
            row.view.kind
        )));
    }

    let cache_id = {
        let g = state.config.lock();
        singbox_cache::cache_id_for(g.parsed.as_ref())
    };
    let db_path = cache_file_path()?;

    // Stop core → invalidate → start core.
    let _ = crate::commands::core_cmd::core_stop(app.clone(), state.clone()).await;
    let mut err: Option<String> = None;
    if let Err(e) = singbox_cache::invalidate_rule_set(&db_path, &cache_id, &tag) {
        err = Some(format!("invalidate failed: {e}"));
    }
    if let Err(e) = crate::commands::core_cmd::core_start(app.clone(), state.clone()).await {
        let msg = format!("core_start failed: {e}");
        err = Some(err.map(|prev| format!("{prev}; {msg}")).unwrap_or(msg));
    }

    // Re-read cache.db for the new timestamp (sing-box's Start path
    // downloads synchronously, so by this point the entry should exist).
    let mut new_ts: Option<u64> = None;
    if let Ok(map) = singbox_cache::read_rule_set_status(&db_path, &cache_id) {
        new_ts = map.get(&tag).map(|s| s.last_updated_ms).filter(|t| *t > 0);
    }

    Ok(RuleSetRefreshResult {
        tag,
        ok: err.is_none(),
        new_last_updated_ms: new_ts,
        error: err,
    })
}

/// Same as `rule_set_refresh` but wipes the whole `rule_set` sub-bucket
/// so EVERY remote rule_set gets re-downloaded on start.
#[tauri::command]
pub async fn rule_set_refresh_all(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<RuleSetRefreshResult>> {
    let merged = rule_sets_list(app.clone(), state.clone()).await?;
    let remote_tags: Vec<String> = merged
        .iter()
        .filter(|r| r.view.kind == "remote")
        .map(|r| r.view.tag.clone())
        .collect();
    if remote_tags.is_empty() {
        return Ok(vec![]);
    }

    let cache_id = {
        let g = state.config.lock();
        singbox_cache::cache_id_for(g.parsed.as_ref())
    };
    let db_path = cache_file_path()?;

    let _ = crate::commands::core_cmd::core_stop(app.clone(), state.clone()).await;
    let invalidate_err =
        singbox_cache::invalidate_all_rule_sets(&db_path, &cache_id).err().map(|e| e.to_string());
    let start_err = crate::commands::core_cmd::core_start(app.clone(), state.clone())
        .await
        .err()
        .map(|e| e.to_string());

    let map = singbox_cache::read_rule_set_status(&db_path, &cache_id).unwrap_or_default();
    Ok(remote_tags
        .into_iter()
        .map(|tag| {
            let new_ts = map.get(&tag).map(|s| s.last_updated_ms).filter(|t| *t > 0);
            let err = match (&invalidate_err, &start_err) {
                (Some(a), Some(b)) => Some(format!("invalidate failed: {a}; core_start failed: {b}")),
                (Some(a), None) => Some(format!("invalidate failed: {a}")),
                (None, Some(b)) => Some(format!("core_start failed: {b}")),
                (None, None) => None,
            };
            RuleSetRefreshResult {
                tag,
                ok: err.is_none() && new_ts.is_some(),
                new_last_updated_ms: new_ts,
                error: err,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Regression: a config with route.rule_set populated must survive
    /// merge_rule_sets_for_view → the table is built off this list.
    /// The "Last update" / "Etag" enrichment in rule_sets_list is
    /// non-fatal, so even with cache.db absent the entries must surface.
    #[test]
    fn merge_returns_all_source_entries() {
        let source = vec![
            json!({
                "tag": "geosite-cn",
                "type": "remote",
                "format": "binary",
                "url": "https://example.test/geosite-cn.srs",
                "update_interval": "1d"
            }),
            json!({
                "tag": "geoip-cn",
                "type": "remote",
                "format": "binary",
                "url": "https://example.test/geoip-cn.srs"
            }),
            json!({
                "tag": "my-local",
                "type": "local",
                "format": "source",
                "path": "/tmp/local.json"
            }),
        ];
        let per = ArrayOverrides::default();
        let global: Vec<LocalEntry> = vec![];

        let merged = merge_rule_sets_for_view(&source, &per, &global);

        assert_eq!(merged.len(), 3, "should return all 3 source rule_sets");
        assert_eq!(merged[0].view.tag, "geosite-cn");
        assert_eq!(merged[0].view.kind, "remote");
        assert!(merged[0].view.editable);
        assert!(matches!(merged[0].source, RuleSource::Config));
        assert!(!merged[0].masked);
        assert!(!merged[0].modified);
        // last_updated_ms/etag are populated by rule_sets_list's cache.db
        // enrichment, not by merge_rule_sets_for_view itself.
        assert!(merged[0].view.last_updated_ms.is_none());
        assert!(merged[0].view.etag.is_none());

        assert_eq!(merged[1].view.tag, "geoip-cn");
        assert_eq!(merged[2].view.tag, "my-local");
        assert_eq!(merged[2].view.kind, "local");
    }

    #[test]
    fn merge_returns_empty_for_empty_source() {
        let source: Vec<Value> = vec![];
        let per = ArrayOverrides::default();
        let global: Vec<LocalEntry> = vec![];
        let merged = merge_rule_sets_for_view(&source, &per, &global);
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_includes_per_config_appended_and_global() {
        let source: Vec<Value> = vec![];
        let mut per = ArrayOverrides::default();
        per.appended.push(LocalEntry {
            id: "uuid-per".into(),
            value: json!({
                "tag": "per-cfg-set",
                "type": "remote",
                "format": "binary",
                "url": "https://example.test/per.srs"
            }),
            created_at_ms: 0,
        });
        let global = vec![LocalEntry {
            id: "uuid-glob".into(),
            value: json!({
                "tag": "global-set",
                "type": "local",
                "format": "source",
                "path": "/tmp/g.json"
            }),
            created_at_ms: 0,
        }];

        let merged = merge_rule_sets_for_view(&source, &per, &global);
        assert_eq!(merged.len(), 2);
        assert!(matches!(merged[0].source, RuleSource::LocalPer));
        assert_eq!(merged[0].view.tag, "per-cfg-set");
        assert!(matches!(merged[1].source, RuleSource::LocalGlobal));
        assert_eq!(merged[1].view.tag, "global-set");
    }
}

