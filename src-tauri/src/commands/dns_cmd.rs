//! DNS editor commands. Parallels rules_cmd: every mutation goes
//! through the overrides layer (per-config or global), the source
//! config file is never touched.
//!
//! Two managed arrays: `/dns/servers` and `/dns/rules`. Same id
//! convention as rules_cmd — signature for source rows, UUID for local.

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::commands::rules_cmd::{RuleSource, Scope};
use crate::core::dns::{
    dns_rule_input_to_value, dns_rule_to_view, dns_server_input_to_value, dns_server_to_view,
    DnsRuleInput, DnsRuleView, DnsServerInput, DnsServerView,
};
use crate::core::overrides::{
    load_global, load_per_config, save_global, save_per_config, signature, ArrayOverrides,
    LocalEntry, ModificationEntry,
};
use crate::error::{AppError, AppResult};
use crate::paths::{global_overrides_path, per_config_overrides_path};
use crate::state::AppState;

// ---------- helpers (mirror rules_cmd) ---------------------------------

fn id_is_signature(id: &str) -> bool {
    !id.contains('-')
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn source_array(state: &State<'_, AppState>, ptr: &str) -> Vec<Value> {
    let g = state.config.lock();
    g.parsed
        .as_ref()
        .and_then(|p| p.pointer(ptr))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn require_active(state: &State<'_, AppState>) -> AppResult<String> {
    state
        .config
        .lock()
        .active_id
        .clone()
        .ok_or_else(|| AppError::Config("no active config".into()))
}

// ---------- shared view wrappers ---------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DnsServerViewWithBadge {
    pub id: String,
    pub view: DnsServerView,
    pub source: RuleSource,
    pub modified: bool,
    pub masked: bool,
    pub original_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsRuleViewWithBadge {
    pub id: String,
    pub view: DnsRuleView,
    pub source: RuleSource,
    pub modified: bool,
    pub masked: bool,
    pub original_signature: Option<String>,
}

// ---------- /dns/servers ----------------------------------------------

#[tauri::command]
pub async fn dns_servers_list(state: State<'_, AppState>) -> AppResult<Vec<DnsServerViewWithBadge>> {
    let active = state.config.lock().active_id.clone();
    let per = match active {
        Some(ref id) => load_per_config(&per_config_overrides_path(id)?),
        None => Default::default(),
    };
    let global = load_global(&global_overrides_path()?);
    let source = source_array(&state, "/dns/servers");
    Ok(merge_servers(&source, &per.dns_servers, &global.dns_servers))
}

fn merge_servers(
    source: &[Value],
    per: &ArrayOverrides,
    global_appended: &[LocalEntry],
) -> Vec<DnsServerViewWithBadge> {
    let mut out = Vec::with_capacity(source.len() + per.appended.len() + global_appended.len());
    for item in source {
        let sig = signature(item);
        let masked = per.masked.contains(&sig);
        let modified = per.modifications.contains_key(&sig);
        let display = if let Some(m) = per.modifications.get(&sig) {
            &m.override_value
        } else {
            item
        };
        let view = dns_server_to_view(out.len(), display);
        out.push(DnsServerViewWithBadge {
            id: sig.clone(),
            view,
            source: RuleSource::Config,
            modified,
            masked,
            original_signature: if modified { Some(sig) } else { None },
        });
    }
    for e in &per.appended {
        let view = dns_server_to_view(out.len(), &e.value);
        out.push(DnsServerViewWithBadge {
            id: e.id.clone(),
            view,
            source: RuleSource::LocalPer,
            modified: false,
            masked: false,
            original_signature: None,
        });
    }
    for e in global_appended {
        let view = dns_server_to_view(out.len(), &e.value);
        out.push(DnsServerViewWithBadge {
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
pub async fn dns_servers_add(
    server: DnsServerInput,
    scope: Scope,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsServerViewWithBadge>> {
    let value = dns_server_input_to_value(&server)?;
    let entry = LocalEntry {
        id: Uuid::new_v4().to_string(),
        value,
        created_at_ms: now_ms(),
    };
    match scope {
        Scope::PerConfig => {
            let active = require_active(&state)?;
            let mut per = load_per_config(&per_config_overrides_path(&active)?);
            per.dns_servers.appended.push(entry);
            save_per_config(&per_config_overrides_path(&active)?, &per)?;
        }
        Scope::Global => {
            let mut g = load_global(&global_overrides_path()?);
            g.dns_servers.push(entry);
            save_global(&global_overrides_path()?, &g)?;
        }
    }
    let _ = app.emit("overrides:changed", "dns_servers");
    dns_servers_list(state).await
}

#[tauri::command]
pub async fn dns_servers_update(
    id: String,
    server: DnsServerInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsServerViewWithBadge>> {
    let new_value = dns_server_input_to_value(&server)?;
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    if id_is_signature(&id) {
        per.dns_servers.modifications.insert(
            id.clone(),
            ModificationEntry {
                override_value: new_value,
                original_signature_preview: id.chars().take(16).collect(),
                modified_at_ms: now_ms(),
            },
        );
        per.dns_servers.masked.remove(&id);
        save_per_config(&per_config_overrides_path(&active)?, &per)?;
    } else if let Some(e) = per.dns_servers.appended.iter_mut().find(|e| e.id == id) {
        e.value = new_value;
        save_per_config(&per_config_overrides_path(&active)?, &per)?;
    } else {
        let mut g = load_global(&global_overrides_path()?);
        if let Some(e) = g.dns_servers.iter_mut().find(|e| e.id == id) {
            e.value = new_value;
            save_global(&global_overrides_path()?, &g)?;
        } else {
            return Err(AppError::Config(format!("no DNS server with id {id}")));
        }
    }
    let _ = app.emit("overrides:changed", "dns_servers");
    dns_servers_list(state).await
}

#[tauri::command]
pub async fn dns_servers_delete(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsServerViewWithBadge>> {
    if id_is_signature(&id) {
        return Err(AppError::Config(
            "config DNS servers can't be deleted — use mask".into(),
        ));
    }
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    let pre = per.dns_servers.appended.len();
    per.dns_servers.appended.retain(|e| e.id != id);
    if per.dns_servers.appended.len() != pre {
        save_per_config(&per_config_overrides_path(&active)?, &per)?;
    } else {
        let mut g = load_global(&global_overrides_path()?);
        let pg = g.dns_servers.len();
        g.dns_servers.retain(|e| e.id != id);
        if g.dns_servers.len() != pg {
            save_global(&global_overrides_path()?, &g)?;
        } else {
            return Err(AppError::Config(format!("no DNS server with id {id}")));
        }
    }
    let _ = app.emit("overrides:changed", "dns_servers");
    dns_servers_list(state).await
}

#[tauri::command]
pub async fn dns_servers_mask(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsServerViewWithBadge>> {
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    per.dns_servers.masked.insert(signature_id);
    save_per_config(&per_config_overrides_path(&active)?, &per)?;
    let _ = app.emit("overrides:changed", "dns_servers");
    dns_servers_list(state).await
}

#[tauri::command]
pub async fn dns_servers_unmask(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsServerViewWithBadge>> {
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    per.dns_servers.masked.remove(&signature_id);
    save_per_config(&per_config_overrides_path(&active)?, &per)?;
    let _ = app.emit("overrides:changed", "dns_servers");
    dns_servers_list(state).await
}

#[tauri::command]
pub async fn dns_servers_revert(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsServerViewWithBadge>> {
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    per.dns_servers.modifications.remove(&signature_id);
    save_per_config(&per_config_overrides_path(&active)?, &per)?;
    let _ = app.emit("overrides:changed", "dns_servers");
    dns_servers_list(state).await
}

// ---------- /dns/rules -------------------------------------------------

#[tauri::command]
pub async fn dns_rules_list(state: State<'_, AppState>) -> AppResult<Vec<DnsRuleViewWithBadge>> {
    let active = state.config.lock().active_id.clone();
    let per = match active {
        Some(ref id) => load_per_config(&per_config_overrides_path(id)?),
        None => Default::default(),
    };
    let global = load_global(&global_overrides_path()?);
    let source = source_array(&state, "/dns/rules");
    Ok(merge_dns_rules(&source, &per.dns_rules, &global.dns_rules))
}

fn merge_dns_rules(
    source: &[Value],
    per: &ArrayOverrides,
    global_appended: &[LocalEntry],
) -> Vec<DnsRuleViewWithBadge> {
    let mut out = Vec::with_capacity(source.len() + per.appended.len() + global_appended.len());
    for item in source {
        let sig = signature(item);
        let masked = per.masked.contains(&sig);
        let modified = per.modifications.contains_key(&sig);
        let display = if let Some(m) = per.modifications.get(&sig) {
            &m.override_value
        } else {
            item
        };
        let view = dns_rule_to_view(out.len(), display);
        out.push(DnsRuleViewWithBadge {
            id: sig.clone(),
            view,
            source: RuleSource::Config,
            modified,
            masked,
            original_signature: if modified { Some(sig) } else { None },
        });
    }
    for e in &per.appended {
        let view = dns_rule_to_view(out.len(), &e.value);
        out.push(DnsRuleViewWithBadge {
            id: e.id.clone(),
            view,
            source: RuleSource::LocalPer,
            modified: false,
            masked: false,
            original_signature: None,
        });
    }
    for e in global_appended {
        let view = dns_rule_to_view(out.len(), &e.value);
        out.push(DnsRuleViewWithBadge {
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
pub async fn dns_rules_add(
    rule: DnsRuleInput,
    scope: Scope,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsRuleViewWithBadge>> {
    let value = dns_rule_input_to_value(&rule)?;
    let entry = LocalEntry {
        id: Uuid::new_v4().to_string(),
        value,
        created_at_ms: now_ms(),
    };
    match scope {
        Scope::PerConfig => {
            let active = require_active(&state)?;
            let mut per = load_per_config(&per_config_overrides_path(&active)?);
            per.dns_rules.appended.push(entry);
            save_per_config(&per_config_overrides_path(&active)?, &per)?;
        }
        Scope::Global => {
            let mut g = load_global(&global_overrides_path()?);
            g.dns_rules.push(entry);
            save_global(&global_overrides_path()?, &g)?;
        }
    }
    let _ = app.emit("overrides:changed", "dns_rules");
    dns_rules_list(state).await
}

#[tauri::command]
pub async fn dns_rules_update(
    id: String,
    rule: DnsRuleInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsRuleViewWithBadge>> {
    let new_value = dns_rule_input_to_value(&rule)?;
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    if id_is_signature(&id) {
        per.dns_rules.modifications.insert(
            id.clone(),
            ModificationEntry {
                override_value: new_value,
                original_signature_preview: id.chars().take(16).collect(),
                modified_at_ms: now_ms(),
            },
        );
        per.dns_rules.masked.remove(&id);
        save_per_config(&per_config_overrides_path(&active)?, &per)?;
    } else if let Some(e) = per.dns_rules.appended.iter_mut().find(|e| e.id == id) {
        e.value = new_value;
        save_per_config(&per_config_overrides_path(&active)?, &per)?;
    } else {
        let mut g = load_global(&global_overrides_path()?);
        if let Some(e) = g.dns_rules.iter_mut().find(|e| e.id == id) {
            e.value = new_value;
            save_global(&global_overrides_path()?, &g)?;
        } else {
            return Err(AppError::Config(format!("no DNS rule with id {id}")));
        }
    }
    let _ = app.emit("overrides:changed", "dns_rules");
    dns_rules_list(state).await
}

#[tauri::command]
pub async fn dns_rules_delete(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsRuleViewWithBadge>> {
    if id_is_signature(&id) {
        return Err(AppError::Config(
            "config DNS rules can't be deleted — use mask".into(),
        ));
    }
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    let pre = per.dns_rules.appended.len();
    per.dns_rules.appended.retain(|e| e.id != id);
    if per.dns_rules.appended.len() != pre {
        save_per_config(&per_config_overrides_path(&active)?, &per)?;
    } else {
        let mut g = load_global(&global_overrides_path()?);
        let pg = g.dns_rules.len();
        g.dns_rules.retain(|e| e.id != id);
        if g.dns_rules.len() != pg {
            save_global(&global_overrides_path()?, &g)?;
        } else {
            return Err(AppError::Config(format!("no DNS rule with id {id}")));
        }
    }
    let _ = app.emit("overrides:changed", "dns_rules");
    dns_rules_list(state).await
}

#[tauri::command]
pub async fn dns_rules_mask(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsRuleViewWithBadge>> {
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    per.dns_rules.masked.insert(signature_id);
    save_per_config(&per_config_overrides_path(&active)?, &per)?;
    let _ = app.emit("overrides:changed", "dns_rules");
    dns_rules_list(state).await
}

#[tauri::command]
pub async fn dns_rules_unmask(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsRuleViewWithBadge>> {
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    per.dns_rules.masked.remove(&signature_id);
    save_per_config(&per_config_overrides_path(&active)?, &per)?;
    let _ = app.emit("overrides:changed", "dns_rules");
    dns_rules_list(state).await
}

#[tauri::command]
pub async fn dns_rules_revert(
    signature_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<DnsRuleViewWithBadge>> {
    let active = require_active(&state)?;
    let mut per = load_per_config(&per_config_overrides_path(&active)?);
    per.dns_rules.modifications.remove(&signature_id);
    save_per_config(&per_config_overrides_path(&active)?, &per)?;
    let _ = app.emit("overrides:changed", "dns_rules");
    dns_rules_list(state).await
}

#[tauri::command]
pub async fn dns_commit(
    restart: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _ = app.emit("config:saved", "dns_overrides");
    if restart {
        let _ = crate::commands::core_cmd::core_stop(app.clone(), state.clone()).await;
        let _ = crate::commands::core_cmd::core_start(app, state).await?;
    }
    Ok(())
}
