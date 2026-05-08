use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// Connect to ws://<addr>/traffic?token=<secret> and forward each frame to
/// the frontend as a `traffic:tick` Tauri event. Each emit carries the
/// session `epoch` so the frontend store can drop stragglers from a
/// previous core session.
/// After this many consecutive connect/auth failures we give up and
/// emit `pumps:stale {kind: "traffic"}`. Crash detection (the watcher
/// in core_cmd::core_start) covers the "sing-box died" case; this one
/// covers "sing-box is alive but our HTTP path is broken" (auth
/// mismatch, secret rotated by a future config change, port collision).
const MAX_CONSEC_FAILS: u32 = 6;

pub fn spawn_traffic_pump(
    app: AppHandle,
    addr: String,
    secret: String,
    epoch: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // Reconnect loop — single-flight; if WS closes (sing-box restart in
        // dev), wait then retry. Bounded backoff so we don't spin.
        let mut backoff_ms = 200u64;
        let mut consec_fails: u32 = 0;
        loop {
            match run_once(&app, &addr, &secret, epoch).await {
                Ok(()) => {
                    tracing::info!("traffic WS exited cleanly; reconnecting");
                    backoff_ms = 200;
                    consec_fails = 0;
                }
                Err(e) => {
                    consec_fails += 1;
                    tracing::warn!(
                        ?e,
                        "traffic WS error (#{consec_fails}); backing off {}ms",
                        backoff_ms
                    );
                    if consec_fails >= MAX_CONSEC_FAILS {
                        let _ = app.emit(
                            "pumps:stale",
                            serde_json::json!({ "kind": "traffic", "epoch": epoch }),
                        );
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms * 2).min(5_000);
                }
            }
        }
    })
}

async fn run_once(
    app: &AppHandle,
    addr: &str,
    secret: &str,
    epoch: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("ws://{addr}/traffic?token={secret}");
    let request = url.into_client_request()?;
    let (ws, _resp) = tokio_tungstenite::connect_async(request).await?;
    let (_write, mut read) = ws.split();

    while let Some(msg) = read.next().await {
        match msg? {
            Message::Text(txt) => {
                if let Ok(parsed) = serde_json::from_str::<TrafficFrame>(&txt) {
                    let payload = TrafficTick {
                        up: parsed.up,
                        down: parsed.down,
                        ts_ms: now_ms(),
                        epoch,
                    };
                    if let Err(e) = app.emit("traffic:tick", &payload) {
                        tracing::warn!(?e, "emit traffic:tick failed");
                    }
                }
            }
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            Message::Close(_) => break,
        }
    }
    Ok(())
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Deserialize)]
struct TrafficFrame {
    up: u64,
    down: u64,
}

#[derive(Serialize, Clone)]
pub struct TrafficTick {
    pub up: u64,
    pub down: u64,
    pub ts_ms: u64,
    pub epoch: u64,
}
