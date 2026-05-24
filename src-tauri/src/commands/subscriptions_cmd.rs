use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

use crate::core::library::{new_entry, write_initial};
use crate::core::subscriptions::{
    fetch_full_config, load_all, new_subscription, now_ms, save_all, Subscription,
};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, ConfigSource};

#[tauri::command]
pub async fn subs_list(app: AppHandle) -> AppResult<Vec<Subscription>> {
    Ok(load_all(&app))
}

#[derive(Deserialize)]
pub struct SubInput {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub interval_hours: u32,
    #[serde(default)]
    pub daily_update_at: Option<String>,
    #[serde(default)]
    pub keep_last_n: Option<u32>,
    /// Default true if absent (matches `Subscription::default_auto_switch`).
    #[serde(default)]
    pub auto_switch_to_new: Option<bool>,
}

fn validate_daily_at(v: &Option<String>) -> AppResult<()> {
    let Some(s) = v.as_ref() else { return Ok(()) };
    let parts: Vec<&str> = s.split(':').collect();
    let bad = || AppError::Other(format!("daily_update_at must be HH:MM, got '{s}'"));
    if parts.len() != 2 {
        return Err(bad());
    }
    let h: u32 = parts[0].parse().map_err(|_| bad())?;
    let m: u32 = parts[1].parse().map_err(|_| bad())?;
    if h >= 24 || m >= 60 {
        return Err(bad());
    }
    Ok(())
}

#[tauri::command]
pub async fn subs_add(
    input: SubInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Subscription> {
    validate_daily_at(&input.daily_update_at)?;
    let _g = state.subs_op.lock().await;
    let mut all = load_all(&app);
    let mut s = new_subscription(input.name, input.url, input.interval_hours);
    s.daily_update_at = input.daily_update_at;
    s.keep_last_n = input.keep_last_n;
    if let Some(b) = input.auto_switch_to_new {
        s.auto_switch_to_new = b;
    }
    let out = s.clone();
    all.push(s);
    save_all(&app, &all)?;
    drop(_g);
    state.subs_wakeup.notify_one();
    Ok(out)
}

/// Edit an existing subscription source. Doesn't touch fetched data —
/// caller can chain a refresh / apply if needed.
#[tauri::command]
pub async fn subs_update(
    id: String,
    input: SubInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Subscription> {
    validate_daily_at(&input.daily_update_at)?;
    let _g = state.subs_op.lock().await;
    let mut all = load_all(&app);
    let pos = all
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| AppError::Other(format!("no subscription with id {id}")))?;
    all[pos].name = input.name;
    all[pos].url = input.url;
    all[pos].interval_hours = input.interval_hours;
    all[pos].daily_update_at = input.daily_update_at;
    all[pos].keep_last_n = input.keep_last_n;
    if let Some(b) = input.auto_switch_to_new {
        all[pos].auto_switch_to_new = b;
    }
    save_all(&app, &all)?;
    let out = all[pos].clone();
    drop(_g);
    state.subs_wakeup.notify_one();
    Ok(out)
}

#[tauri::command]
pub async fn subs_remove(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _g = state.subs_op.lock().await;
    let mut all = load_all(&app);
    all.retain(|s| s.id != id);
    save_all(&app, &all)?;
    drop(_g);
    state.subs_wakeup.notify_one();
    Ok(())
}

/// Fetch the subscription URL (validate it parses + count outbounds) and
/// update its last_fetched / last_error timestamps. Does NOT touch the
/// active config and does NOT add anything to the library.
#[tauri::command]
pub async fn subs_refresh(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Subscription> {
    // Snapshot the URL under the lock so concurrent edits can't tear it.
    let (url, _) = {
        let _g = state.subs_op.lock().await;
        let all = load_all(&app);
        let s = all
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| AppError::Other(format!("no subscription with id {id}")))?;
        (s.url.clone(), ())
    };

    let fetch_result = fetch_full_config(&url).await;

    let _g = state.subs_op.lock().await;
    let mut all = load_all(&app);
    let pos = all
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| AppError::Other(format!("no subscription with id {id}")))?;
    match fetch_result {
        Ok((_text, parsed)) => {
            all[pos].last_fetched_at_ms = Some(now_ms());
            all[pos].last_error = None;
            all[pos].outbound_count = parsed
                .get("outbounds")
                .and_then(|v| v.as_array())
                .map(|a| a.len() as u32);
        }
        Err(e) => {
            all[pos].last_error = Some(e.to_string());
        }
    }
    save_all(&app, &all)?;
    Ok(all[pos].clone())
}

/// Fetch a subscription URL and add the result to the library as a NEW
/// independent ConfigEntry (add-as-new semantics). The active config is
/// NOT changed; user has to Select the new entry on the Config page to
/// make it active.
#[tauri::command]
pub async fn subs_apply(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let _g = state.library_op.lock().await;
    let new_id = apply_subscription_inner(&app, &state, &id).await?;

    // If the library was previously empty, apply_subscription_inner made the
    // new entry implicitly active — but we haven't loaded its cache or
    // started sing-box yet. Mirror what config_cmd::add_text_inner does:
    // explicitly invoke select_inner so the cache is hydrated and
    // sing-box comes up. Without this, the user sees the new card marked
    // "active" but the proxy isn't actually running.
    if state.config.lock().path.is_none() {
        crate::commands::config_cmd::select_inner(&app, &state, &new_id).await?;
    }
    Ok(new_id)
}

/// Core fetch → write entry → push library → save → emit cycle.
/// Caller is responsible for holding `state.library_op` and for any
/// post-apply policy (auto-select, restart, pruning). Returns the new
/// entry id on success.
///
/// On error: subscription's `last_error` is updated and persisted,
/// `last_fetched_at_ms` is **not** advanced, and the error propagates.
/// Each subscriptions.json mutation acquires `state.subs_op` so a
/// concurrent CRUD command can't clobber our write.
pub async fn apply_subscription_inner(
    app: &AppHandle,
    state: &State<'_, AppState>,
    sub_id: &str,
) -> AppResult<String> {
    // Snapshot just the URL + name under the lock so a concurrent edit
    // can't tear them between the read and the fetch.
    let sub = {
        let _g = state.subs_op.lock().await;
        load_all(app)
            .into_iter()
            .find(|s| s.id == sub_id)
            .ok_or_else(|| AppError::Other(format!("no subscription with id {sub_id}")))?
    };

    let (text, parsed) = match fetch_full_config(&sub.url).await {
        Ok(t) => t,
        Err(e) => {
            // Persist error under the lock and re-raise so the scheduler
            // can count the failure.
            let _g = state.subs_op.lock().await;
            let mut all = load_all(app);
            if let Some(s) = all.iter_mut().find(|s| s.id == sub_id) {
                s.last_error = Some(e.to_string());
                s.last_attempt_at_ms = Some(now_ms());
            }
            let _ = save_all(app, &all);
            return Err(e);
        }
    };
    let outbound_count = parsed
        .get("outbounds")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as u32);

    let when = chrono_like_iso(now_ms());
    let entry_name = format!("{} @ {}", sub.name, when);
    let mut entry = new_entry(
        entry_name,
        ConfigSource::Subscription {
            sub_id: sub.id.clone(),
            fetched_at_ms: now_ms(),
        },
    )?;
    let (oc, rc, tun) = crate::core::library::compute_summary_fields(text.as_bytes());
    entry.outbound_count = oc;
    entry.rule_count = rc;
    entry.has_tun_inbound = tun;
    write_initial(&entry.storage_path, text.as_bytes())?;

    // Append to library (caller holds library_op).
    let mut lib = crate::core::library::load(app);
    let new_id = entry.id.clone();
    let was_empty = lib.active_id.is_none();
    lib.entries.push(entry);
    if was_empty {
        lib.active_id = Some(new_id.clone());
    }
    crate::core::library::save(app, &lib)?;
    state.config.lock().library = lib.entries;

    // Update subscription record under subs_op.
    {
        let _g = state.subs_op.lock().await;
        let mut all = load_all(app);
        if let Some(s) = all.iter_mut().find(|s| s.id == sub_id) {
            let now = now_ms();
            s.last_fetched_at_ms = Some(now);
            s.last_attempt_at_ms = Some(now);
            s.last_error = None;
            s.outbound_count = outbound_count;
        }
        save_all(app, &all)?;
    }

    // Don't emit library:changed when this was the empty-library case:
    // the caller will select_inner the new entry and that already emits.
    if !was_empty {
        let _ = app.emit("library:changed", ());
    }
    Ok(new_id)
}

/// Tiny ISO-8601-ish formatter without pulling in chrono. Just enough for
/// human-readable entry names ("2026-05-05 13:42:11Z").
fn chrono_like_iso(ms: u64) -> String {
    let secs = ms / 1000;
    // Days since 1970-01-01.
    let days = secs / 86_400;
    let mut y = 1970u32;
    let mut d = days;
    loop {
        let leap = is_leap(y);
        let yd = if leap { 366 } else { 365 } as u64;
        if d < yd {
            break;
        }
        d -= yd;
        y += 1;
    }
    let mdays: [u64; 12] = [
        31,
        if is_leap(y) { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut m = 0u32;
    while (m as usize) < 12 && d >= mdays[m as usize] {
        d -= mdays[m as usize];
        m += 1;
    }
    let day = d + 1;
    let hour = (secs / 3600) % 24;
    let min = (secs / 60) % 60;
    let sec = secs % 60;
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}Z",
        y,
        m + 1,
        day,
        hour,
        min,
        sec
    )
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
