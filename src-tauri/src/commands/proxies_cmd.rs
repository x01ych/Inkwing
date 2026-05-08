use futures::stream::{self, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::core::clash_api::{ClashClient, DelayResult, SpeedTestResult};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn client_from(state: &State<'_, AppState>) -> AppResult<ClashClient> {
    let g = state.core.lock();
    let addr = g
        .clash_api_addr
        .clone()
        .ok_or_else(|| AppError::ClashApi("core not running".into()))?;
    let secret = g
        .clash_api_secret
        .clone()
        .ok_or_else(|| AppError::ClashApi("core not running".into()))?;
    Ok(ClashClient::new(&addr, &secret))
}

#[tauri::command]
pub async fn proxies_list(state: State<'_, AppState>) -> AppResult<Value> {
    let c = client_from(&state)?;
    c.proxies().await
}

#[tauri::command]
pub async fn proxies_select(
    group: String,
    name: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let c = client_from(&state)?;
    c.select_proxy(&group, &name).await
}

#[tauri::command]
pub async fn proxies_test(
    name: String,
    url: String,
    timeout_ms: u32,
    state: State<'_, AppState>,
) -> AppResult<DelayResult> {
    let c = client_from(&state)?;
    c.proxy_delay(&name, &url, timeout_ms).await
}

#[derive(Serialize)]
pub struct GroupTestResult {
    pub name: String,
    pub delay_ms: Option<u32>,
}

/// Throughput test for a single node. Selects the node first, then drives
/// real bytes through sing-box's SOCKS5 inbound (mixed / socks). After
/// the download we restore the group's prior selection so a curiosity
/// click on ⚡ doesn't permanently switch the user's active outbound.
/// The active config MUST contain a mixed or socks inbound listening on
/// 127.0.0.1, otherwise we return an actionable error.
#[tauri::command]
pub async fn proxies_speedtest(
    group: String,
    name: String,
    url: String,
    max_bytes: u64,
    state: State<'_, AppState>,
) -> AppResult<SpeedTestResult> {
    let c = client_from(&state)?;

    // Snapshot the group's currently active member so we can restore it
    // afterwards. Best-effort: if the GET /proxies fails we proceed
    // without a restore plan — better to give the user the speedtest
    // result than to fail the whole call.
    let prev_now = match c.proxies().await {
        Ok(v) => v
            .pointer(&format!("/proxies/{group}/now"))
            .and_then(|n| n.as_str())
            .map(String::from),
        Err(_) => None,
    };

    // Find a SOCKS5-capable inbound port (mixed counts) bound to 127.0.0.1.
    let socks_addr = {
        let cfg = state.config.lock();
        let parsed = cfg
            .parsed
            .as_ref()
            .ok_or_else(|| AppError::Config("no config loaded".into()))?;
        find_socks_inbound(parsed).ok_or_else(|| {
            AppError::Other(
                "no mixed/socks inbound on 127.0.0.1 in active config — speedtest needs one"
                    .into(),
            )
        })?
    };

    // Switch to the candidate node so the download actually flows through
    // it, run the test, then put back whatever was selected before. The
    // restore is best-effort and intentionally doesn't shadow the
    // download error — if select_proxy back fails we still return the
    // speedtest result (the user can re-select manually from the UI).
    c.select_proxy(&group, &name).await?;
    let res = c.speedtest_via_socks(&socks_addr, &url, max_bytes).await;
    if let Some(orig) = prev_now {
        if orig != name {
            let _ = c.select_proxy(&group, &orig).await;
        }
    }
    res
}

fn find_socks_inbound(parsed: &Value) -> Option<String> {
    let arr = parsed.get("inbounds")?.as_array()?;
    for ib in arr {
        let kind = ib.get("type")?.as_str()?;
        if kind != "mixed" && kind != "socks" {
            continue;
        }
        let listen = ib.get("listen").and_then(|v| v.as_str()).unwrap_or("127.0.0.1");
        // Only accept localhost — we won't reach LAN-only inbounds.
        let host = match listen {
            "127.0.0.1" | "::1" | "localhost" => "127.0.0.1",
            _ => continue,
        };
        let port = ib.get("listen_port")?.as_u64()?;
        return Some(format!("{host}:{port}"));
    }
    None
}

/// Parallel delay test (max 8 in flight) for a list of proxy names.
#[tauri::command]
pub async fn proxies_test_many(
    names: Vec<String>,
    url: String,
    timeout_ms: u32,
    state: State<'_, AppState>,
) -> AppResult<Vec<GroupTestResult>> {
    let c = client_from(&state)?;
    let results = stream::iter(names.into_iter())
        .map(|name| {
            let c = c.clone();
            let url = url.clone();
            async move {
                let res = c.proxy_delay(&name, &url, timeout_ms).await;
                GroupTestResult {
                    name,
                    delay_ms: res.ok().and_then(|r| r.delay_ms),
                }
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;
    Ok(results)
}
