use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// One row in the connections snapshot. Field names mirror the Mihomo /
/// Clash.Meta API closely so we can reuse community frontend conventions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnRow {
    pub id: String,
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub chains: Vec<String>,
    #[serde(default)]
    pub rule: String,
    #[serde(default, rename = "rulePayload")]
    pub rule_payload: String,
    pub metadata: ConnMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnMeta {
    #[serde(default)]
    pub network: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default, rename = "sourceIP")]
    pub source_ip: String,
    #[serde(default, rename = "sourcePort")]
    pub source_port: String,
    #[serde(default, rename = "destinationIP")]
    pub destination_ip: String,
    #[serde(default, rename = "destinationPort")]
    pub destination_port: String,
    #[serde(default)]
    pub host: String,
    #[serde(default, rename = "processPath")]
    pub process_path: String,
    #[serde(default, rename = "process")]
    pub process: String,
    /// Sing-box / Mihomo also expose these — keep them in the wire DTO
    /// so the frontend can pick the most informative label and filter
    /// clash_api self-traffic. All optional / default empty for older
    /// sing-box versions that omit them.
    #[serde(default, rename = "sniffHost")]
    pub sniff_host: String,
    #[serde(default, rename = "remoteDestination")]
    pub remote_destination: String,
    #[serde(default, rename = "inboundIP")]
    pub inbound_ip: String,
    #[serde(default, rename = "inboundPort")]
    pub inbound_port: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnSnapshot {
    pub connections: Vec<ConnRow>,
    #[serde(default, rename = "downloadTotal")]
    pub download_total: u64,
    #[serde(default, rename = "uploadTotal")]
    pub upload_total: u64,
    #[serde(default)]
    pub memory: u64,
    /// Session epoch stamped at emit time; the inbound WS frame doesn't
    /// carry one, sing-box has no concept of "session".
    #[serde(default)]
    pub epoch: u64,
}

/// Subscribe to sing-box's `/connections` WebSocket and forward each
/// frame to the frontend as a `connections:snapshot` event.
///
/// **Why WS instead of HTTP**: sing-box's clash_api `/connections` HTTP
/// endpoint is single-shot — it returns one snapshot and closes the
/// connection. Polling it as if it were chunked streaming makes the
/// pump reconnect ~5 times per second and flood the dev console with
/// `connections stream ended; reconnecting` INFO logs. The WS variant
/// (same path, same token) is the actual streaming endpoint and emits
/// one frame per second indefinitely.
const MAX_CONSEC_FAILS: u32 = 6;

pub fn spawn_conn_pump(
    app: AppHandle,
    addr: String,
    secret: String,
    epoch: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff_ms = 200u64;
        let mut consec_fails: u32 = 0;
        loop {
            match run_once(&app, &addr, &secret, epoch).await {
                Ok(()) => {
                    // Stream ended cleanly — usually because sing-box
                    // is stopping/restarting. Reconnect on the next core.
                    tracing::info!("connections WS exited cleanly; reconnecting");
                    backoff_ms = 200;
                    consec_fails = 0;
                }
                Err(e) => {
                    consec_fails += 1;
                    tracing::warn!(
                        ?e,
                        "connections WS error (#{consec_fails}); backing off {}ms",
                        backoff_ms
                    );
                    if consec_fails >= MAX_CONSEC_FAILS {
                        let _ = app.emit(
                            "pumps:stale",
                            serde_json::json!({ "kind": "connections", "epoch": epoch }),
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
    let url = format!("ws://{addr}/connections?token={secret}");
    let request = url.into_client_request()?;
    let (ws, _resp) = tokio_tungstenite::connect_async(request).await?;
    let (_write, mut read) = ws.split();

    while let Some(msg) = read.next().await {
        match msg? {
            Message::Text(txt) => {
                if let Ok(mut snap) = serde_json::from_str::<ConnSnapshot>(&txt) {
                    snap.epoch = epoch;
                    if let Err(e) = app.emit("connections:snapshot", &snap) {
                        tracing::warn!(?e, "emit connections:snapshot failed");
                    }
                }
            }
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            Message::Close(_) => break,
        }
    }
    Ok(())
}
