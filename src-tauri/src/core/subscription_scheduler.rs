//! Background task that polls subscriptions on their configured
//! interval (`every_n_hours` or `daily_at HH:MM`) and applies the result
//! to the library. Without this, `Subscription.interval_hours` /
//! `daily_update_at` are dead fields the user can set but nothing
//! consumes.
//!
//! Triggered next-fire times are recomputed every loop iteration from
//! the live `subscriptions.json`, so an edit through `subs_update`
//! takes effect on the next wake (the CRUD commands also nudge
//! `subs_wakeup` to avoid waiting up to a full sleep window).
//!
//! Failure handling: errors are persisted into `Subscription.last_error`
//! and counted in `consecutive_failures`. Each failure also bumps
//! `last_attempt_at_ms`, and `next_fire_at` applies an exponential
//! backoff so a permanently-broken endpoint stops getting hit every
//! MIN_SLEEP. When the failure counter crosses from <3 to ≥3 on a
//! single tick, a desktop notification is fired (URL secrets are
//! stripped before formatting the body). The counter resets on the
//! next success.

use std::time::Duration;

use chrono::{Local, NaiveTime, TimeZone, Timelike};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::commands::subscriptions_cmd::apply_subscription_inner;
use crate::core::library::{load as load_library, prune_subscription_entries};
use crate::core::subscriptions::{load_all, now_ms, save_all, Subscription};
use crate::state::{AppState, ConfigSource};

/// Floor on how often we re-evaluate the schedule. Even if every
/// subscription claims "fire in 6 hours" we still wake on this cadence
/// so a manual `notify_one()` doesn't have to fight a long sleep.
const MAX_SLEEP: Duration = Duration::from_secs(15 * 60);

/// Minimum sleep — avoids spinning if computation says "fire now" but
/// we just failed to apply and need to back off slightly.
const MIN_SLEEP: Duration = Duration::from_secs(5);

/// Failures must cross this threshold for the notification to fire.
const FAILURE_NOTIFY_THRESHOLD: u32 = 3;

/// Base for exponential failure backoff. 5 min, doubled per failure,
/// capped at `FAILURE_BACKOFF_MAX`. So:
///   1 failure  → wait 5 min before retry
///   2 failures → 10 min
///   3 failures → 20 min
///   4+         → capped at FAILURE_BACKOFF_MAX (1 h)
const FAILURE_BACKOFF_BASE_MS: u64 = 5 * 60 * 1000;
const FAILURE_BACKOFF_MAX_MS: u64 = 60 * 60 * 1000;

pub fn spawn(app: AppHandle) {
    // tauri::async_runtime::spawn (NOT tokio::spawn) because this is
    // called from Tauri's setup hook, which runs outside the Tokio
    // runtime — plain tokio::spawn panics there. Tauri's wrapper picks
    // up its own multi-threaded runtime, the same one the #[command]
    // handlers use.
    tauri::async_runtime::spawn(async move {
        // Brief settle on startup so library / settings hydrate first.
        tokio::time::sleep(Duration::from_secs(3)).await;
        tracing::info!(
            "subscription_scheduler: starting loop ({} subscription(s) loaded)",
            load_all(&app).len()
        );
        run_forever(app).await;
    });
}

async fn run_forever(app: AppHandle) {
    loop {
        let due = collect_due(&app);
        for sub_id in due {
            run_one(&app, &sub_id).await;
        }
        let sleep_for = compute_next_sleep(&app);
        let notify = {
            let state = app.state::<AppState>();
            state.subs_wakeup.clone()
        };
        tokio::select! {
            _ = tokio::time::sleep(sleep_for) => {}
            _ = notify.notified() => {
                tracing::debug!("subscription_scheduler woken by notify");
            }
        }
    }
}

/// Iterate persisted subscriptions and decide which are due NOW.
fn collect_due(app: &AppHandle) -> Vec<String> {
    let all = load_all(app);
    let now = now_ms();
    all.into_iter()
        .filter_map(|s| match next_fire_at(&s, now) {
            Some(t) if t <= now => Some(s.id),
            _ => None,
        })
        .collect()
}

/// Compute the sleep until the next-soonest scheduled fire, clamped to
/// [MIN_SLEEP, MAX_SLEEP].
fn compute_next_sleep(app: &AppHandle) -> Duration {
    let all = load_all(app);
    let now = now_ms();
    let next = all
        .iter()
        .filter_map(|s| next_fire_at(s, now))
        .filter(|t| *t > now)
        .min();
    match next {
        Some(t) => Duration::from_millis(t - now)
            .clamp(MIN_SLEEP, MAX_SLEEP),
        None => MAX_SLEEP,
    }
}

/// Compute when this subscription should next fire (unix ms).
/// `None` means "never auto-fire" (manual-only mode).
///
/// Precedence: `daily_update_at` wins over `interval_hours`.
/// If the sub has recent failures, push the scheduled fire forward by
/// `backoff_for_failures` from `last_attempt_at_ms` so a broken endpoint
/// stops getting retried every MIN_SLEEP.
fn next_fire_at(s: &Subscription, now_ms: u64) -> Option<u64> {
    let scheduled = if let Some(hhmm) = s.daily_update_at.as_deref() {
        next_daily_at(hhmm, s.last_fetched_at_ms, now_ms)?
    } else if s.interval_hours > 0 {
        let interval = (s.interval_hours as u64) * 3_600_000;
        match s.last_fetched_at_ms {
            Some(last) => last.saturating_add(interval),
            None => now_ms, // never fetched → fire now
        }
    } else {
        return None;
    };

    // Failure backoff: hold off until at least (last_attempt + backoff).
    if s.consecutive_failures > 0 {
        if let Some(last_attempt) = s.last_attempt_at_ms {
            let earliest_retry =
                last_attempt.saturating_add(backoff_for_failures(s.consecutive_failures));
            return Some(scheduled.max(earliest_retry));
        }
    }
    Some(scheduled)
}

fn backoff_for_failures(failures: u32) -> u64 {
    if failures == 0 {
        return 0;
    }
    let shift = (failures - 1).min(20); // saturating cap below u64 overflow
    FAILURE_BACKOFF_BASE_MS
        .saturating_mul(1u64 << shift)
        .min(FAILURE_BACKOFF_MAX_MS)
}

/// Parse "HH:MM" and return the next wall-clock instant matching it in
/// the local timezone, taking `last_fetched_at_ms` into account so we
/// don't double-fire when the app is restarted right after an apply.
fn next_daily_at(hhmm: &str, last_fetched: Option<u64>, now_ms: u64) -> Option<u64> {
    let nt = parse_hhmm(hhmm)?;
    let now = Local.timestamp_millis_opt(now_ms as i64).single()?;
    let today = now
        .with_time(nt)
        .single()
        .or_else(|| {
            // Local::with_time can be None across DST transitions —
            // fall back to constructing the date+time manually and
            // accepting whichever offset the OS picks.
            now.date_naive()
                .and_hms_opt(nt.hour(), nt.minute(), 0)
                .and_then(|d| Local.from_local_datetime(&d).single())
        })?;
    let candidate = if today <= now {
        today + chrono::Duration::days(1)
    } else {
        today
    };
    let mut candidate_ms = candidate.timestamp_millis() as u64;
    // Defensive: if we already fired within the last hour, push the
    // candidate one full day forward. Prevents a "we just started, the
    // daily fire time is 1 minute ago" loop where the app keeps
    // applying repeatedly on every restart within the same hour.
    if let Some(last) = last_fetched {
        if candidate_ms.saturating_sub(last) < 60 * 60 * 1000 && candidate_ms <= now_ms {
            candidate_ms = (candidate + chrono::Duration::days(1)).timestamp_millis() as u64;
        }
    }
    Some(candidate_ms)
}

fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let mut it = s.split(':');
    let h: u32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    NaiveTime::from_hms_opt(h, m, 0)
}

/// Apply one subscription and handle its auto-switch + pruning policy.
async fn run_one(app: &AppHandle, sub_id: &str) {
    let state = app.state::<AppState>();
    // Serialise against manual library mutations.
    let _lib_guard = state.library_op.lock().await;

    // Race guard: between collect_due() picking this sub and us taking
    // library_op, a concurrent path (subs_apply called from the Add
    // dialog, or another scheduler tick) may have already fetched.
    // Re-read the sub under the lock and skip if it's no longer due.
    // Without this, "Add subscription" would produce two identical
    // library entries: one from subs_apply, one from the scheduler
    // that subs_add's notify_one() kicks off.
    let (prev_failures, sub_name) = {
        let now = now_ms();
        let all = load_all(app);
        let Some(s) = all.iter().find(|s| s.id == sub_id) else {
            return;
        };
        let still_due = next_fire_at(s, now).map(|t| t <= now).unwrap_or(false);
        if !still_due {
            tracing::debug!(
                "subscription_scheduler: sub {} no longer due (someone else fetched it first), skipping",
                sub_id
            );
            return;
        }
        (s.consecutive_failures, s.name.clone())
    };

    let apply_result = apply_subscription_inner(app, &state, sub_id).await;

    match apply_result {
        Ok(new_entry_id) => {
            on_success(app, &state, sub_id, &new_entry_id).await;
        }
        Err(e) => {
            on_failure(app, &state, sub_id, &sub_name, prev_failures, &e.to_string()).await;
        }
    }
}

async fn on_success(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    sub_id: &str,
    new_entry_id: &str,
) {
    // Reset failure counter + record success attempt under subs_op.
    let (auto_switch, keep_last_n) = {
        let _g = state.subs_op.lock().await;
        let mut all = load_all(app);
        let mut policy = (true, 5u32);
        if let Some(s) = all.iter_mut().find(|s| s.id == sub_id) {
            s.consecutive_failures = 0;
            s.last_attempt_at_ms = Some(now_ms());
            policy = (s.auto_switch_to_new, s.keep_last_n.unwrap_or(5));
        }
        let _ = save_all(app, &all);
        policy
    };

    // Auto-switch if the previously-active config came from THIS
    // subscription (otherwise the user is running a different one and
    // we shouldn't disturb them).
    if auto_switch {
        let should_switch = {
            let g = state.config.lock();
            match (
                g.active_id.as_deref(),
                g.library
                    .iter()
                    .find(|e| Some(e.id.as_str()) == g.active_id.as_deref()),
            ) {
                (Some(_), Some(active)) => matches!(
                    &active.source,
                    ConfigSource::Subscription { sub_id: id, .. } if id == sub_id
                ),
                _ => false,
            }
        };
        if should_switch {
            tracing::info!(
                "subscription_scheduler: auto-switching active config to new entry {}",
                new_entry_id
            );
            if let Err(e) =
                crate::commands::config_cmd::select_inner(app, state, new_entry_id).await
            {
                tracing::warn!("auto-switch select_inner failed: {e}");
            }
        }
    }

    // Prune older entries from this subscription (preserves active).
    match prune_subscription_entries(app, sub_id, keep_last_n) {
        Ok(deleted) if !deleted.is_empty() => {
            tracing::info!(
                "subscription_scheduler: pruned {} old entries for sub {}",
                deleted.len(),
                sub_id
            );
            // Refresh in-memory mirror for any UI tab querying state.
            let lib = load_library(app);
            state.config.lock().library = lib.entries;
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("prune_subscription_entries failed: {e}"),
    }
}

async fn on_failure(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    sub_id: &str,
    sub_name: &str,
    prev_failures: u32,
    err_msg: &str,
) {
    let new_count = prev_failures.saturating_add(1);
    {
        let _g = state.subs_op.lock().await;
        let mut all = load_all(app);
        if let Some(s) = all.iter_mut().find(|s| s.id == sub_id) {
            s.consecutive_failures = new_count;
            s.last_attempt_at_ms = Some(now_ms());
        }
        let _ = save_all(app, &all);
    }

    tracing::warn!(
        "subscription_scheduler: sub {} failed ({}/{}): {}",
        sub_id,
        new_count,
        FAILURE_NOTIFY_THRESHOLD,
        err_msg
    );

    // Only notify when we *cross* the threshold (avoids a notification
    // every tick).
    if prev_failures < FAILURE_NOTIFY_THRESHOLD && new_count >= FAILURE_NOTIFY_THRESHOLD {
        let scrubbed = scrub_urls(err_msg);
        let body = format!(
            "Subscription \"{}\" failed {} times in a row:\n{}",
            sub_name, new_count, scrubbed
        );
        if let Err(e) = app
            .notification()
            .builder()
            .title("Inkwing — subscription update failed")
            .body(body)
            .show()
        {
            tracing::warn!("failed to show notification: {e}");
        }
    }
}

/// Replace any "http(s)://host/path?query#frag" occurrences in `s` with
/// "http(s)://host/path" so subscription tokens don't end up in the OS
/// notification history / system logs. The original error is still
/// stored in `Subscription.last_error` for in-app diagnostics.
fn scrub_urls(s: &str) -> String {
    // Cheap state machine; we don't need a full URL parser.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        // Look for "http://" or "https://" prefix.
        let is_url_start = (c == 'h' || c == 'H')
            && {
                let tail: String = chars.clone().take(7).collect();
                let lower = tail.to_ascii_lowercase();
                lower.starts_with("ttp://") || lower.starts_with("ttps://")
            };
        if !is_url_start {
            out.push(c);
            continue;
        }
        // Emit the URL, but stop at '?' / '#' / whitespace.
        out.push(c);
        while let Some(&nc) = chars.peek() {
            if nc == '?' || nc == '#' || nc.is_whitespace() {
                break;
            }
            out.push(nc);
            chars.next();
        }
        // Drop the query/fragment.
        while let Some(&nc) = chars.peek() {
            if nc.is_whitespace() {
                break;
            }
            chars.next();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_sub() -> Subscription {
        Subscription {
            id: "id".into(),
            name: "name".into(),
            url: "https://example.test/sub".into(),
            interval_hours: 0,
            daily_update_at: None,
            last_fetched_at_ms: None,
            last_error: None,
            outbound_count: None,
            keep_last_n: None,
            auto_switch_to_new: true,
            consecutive_failures: 0,
            last_attempt_at_ms: None,
        }
    }

    #[test]
    fn parse_hhmm_accepts_valid() {
        assert!(parse_hhmm("03:00").is_some());
        assert!(parse_hhmm("23:59").is_some());
        assert!(parse_hhmm("00:00").is_some());
    }

    #[test]
    fn parse_hhmm_rejects_invalid() {
        assert!(parse_hhmm("24:00").is_none());
        assert!(parse_hhmm("12:60").is_none());
        assert!(parse_hhmm("12").is_none());
        assert!(parse_hhmm("12:00:00").is_none());
        assert!(parse_hhmm("ab:cd").is_none());
    }

    #[test]
    fn next_fire_manual_returns_none() {
        let s = mk_sub();
        assert!(next_fire_at(&s, 1_700_000_000_000).is_none());
    }

    #[test]
    fn next_fire_interval_first_run_is_now() {
        let mut s = mk_sub();
        s.interval_hours = 6;
        let now = 1_700_000_000_000;
        assert_eq!(next_fire_at(&s, now), Some(now));
    }

    #[test]
    fn next_fire_interval_uses_last_fetched() {
        let mut s = mk_sub();
        s.interval_hours = 2;
        s.last_fetched_at_ms = Some(1_700_000_000_000);
        let now = 1_700_000_100_000;
        // expected = 1_700_000_000_000 + 2h
        assert_eq!(
            next_fire_at(&s, now),
            Some(1_700_000_000_000 + 2 * 3_600_000)
        );
    }

    #[test]
    fn backoff_for_failures_grows() {
        assert_eq!(backoff_for_failures(0), 0);
        assert_eq!(backoff_for_failures(1), FAILURE_BACKOFF_BASE_MS);
        assert_eq!(backoff_for_failures(2), FAILURE_BACKOFF_BASE_MS * 2);
        assert_eq!(backoff_for_failures(3), FAILURE_BACKOFF_BASE_MS * 4);
        // Cap.
        assert_eq!(backoff_for_failures(20), FAILURE_BACKOFF_MAX_MS);
        assert_eq!(backoff_for_failures(u32::MAX), FAILURE_BACKOFF_MAX_MS);
    }

    #[test]
    fn next_fire_failures_push_past_scheduled() {
        // interval=1h, last_fetched=t0, last_attempt=t0+10min, failures=2.
        // Scheduled = t0 + 1h. Backoff = t0+10min + 10min = t0+20min.
        // The backoff is < scheduled, so scheduled wins.
        let mut s = mk_sub();
        let t0 = 1_700_000_000_000u64;
        s.interval_hours = 1;
        s.last_fetched_at_ms = Some(t0);
        s.last_attempt_at_ms = Some(t0 + 10 * 60 * 1000);
        s.consecutive_failures = 2;
        let now = t0 + 12 * 60 * 1000;
        assert_eq!(next_fire_at(&s, now), Some(t0 + 60 * 60 * 1000));
    }

    #[test]
    fn next_fire_backoff_when_scheduled_is_past() {
        // interval=1h, last_fetched=t0, failed every 10min for 30min,
        // failures=3, last_attempt=t0+30min.
        //   scheduled = t0 + 1h
        //   backoff   = t0+30min + 20min = t0+50min
        // scheduled wins again.
        let mut s = mk_sub();
        let t0 = 1_700_000_000_000u64;
        s.interval_hours = 1;
        s.last_fetched_at_ms = Some(t0);
        s.last_attempt_at_ms = Some(t0 + 30 * 60 * 1000);
        s.consecutive_failures = 3;
        let now = t0 + 35 * 60 * 1000;
        assert_eq!(next_fire_at(&s, now), Some(t0 + 60 * 60 * 1000));
    }

    #[test]
    fn next_fire_backoff_dominates_when_scheduled_in_past() {
        // interval=1h and a 5-failure history should make the backoff
        // window dominate (cap = 1h).
        // last_attempt = t0+3h → earliest_retry = t0+3h + 1h = t0+4h.
        // scheduled = t0+1h. max(...) = t0+4h.
        let mut s = mk_sub();
        let t0 = 1_700_000_000_000u64;
        s.interval_hours = 1;
        s.last_fetched_at_ms = Some(t0);
        s.last_attempt_at_ms = Some(t0 + 3 * 60 * 60 * 1000);
        s.consecutive_failures = 5;
        let now = t0 + 3 * 60 * 60 * 1000 + 30 * 60 * 1000; // 3h30m in
        let fire = next_fire_at(&s, now).unwrap();
        assert_eq!(fire, t0 + 3 * 60 * 60 * 1000 + FAILURE_BACKOFF_MAX_MS);
        assert!(fire > now);
    }

    #[test]
    fn scrub_urls_strips_query() {
        let s = "fetch failed: GET https://provider.example/sub?token=abc123#frag → 500";
        let out = scrub_urls(s);
        assert!(!out.contains("token"));
        assert!(!out.contains("?"));
        assert!(!out.contains("#"));
        assert!(out.contains("https://provider.example/sub"));
    }

    #[test]
    fn scrub_urls_leaves_non_urls_alone() {
        let s = "no urls here";
        assert_eq!(scrub_urls(s), s);
    }

    #[test]
    fn scrub_urls_handles_trailing_whitespace() {
        let s = "see https://x.test/p?k=v for details";
        let out = scrub_urls(s);
        assert_eq!(out, "see https://x.test/p for details");
    }
}
