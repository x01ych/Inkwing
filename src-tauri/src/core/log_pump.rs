use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use parking_lot::Mutex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::util::ring_buffer::RingBuffer;

/// In-memory log entry. `ts_ms` is the wall-clock time when the GUI
/// received the line (sing-box's clash_api /logs payload doesn't carry
/// its own timestamp).
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub ts_ms: u64,
    pub level: String,
    pub payload: String,
}

#[derive(Deserialize)]
struct LogFrame {
    #[serde(rename = "type")]
    kind: String,
    payload: String,
}

/// Wrapper emitted on the `logs:append` event so the frontend can drop
/// straggler batches from a previous core session.
#[derive(Serialize)]
pub struct LogBatch<'a> {
    pub epoch: u64,
    pub entries: &'a [LogEntry],
}

/// Spawn a reqwest streaming GET on /logs and forward each line to:
///   1. the in-memory ring (kept in AppState), so the Logs page can hydrate
///      on mount via `logs_recent`;
///   2. the frontend, batched: every ~100ms or 200 entries (whichever
///      first), as a `logs:append` Tauri event with a session epoch tag.
///
/// On disconnect we reconnect with bounded backoff. core_stop aborts.
pub fn spawn_log_pump(
    app: AppHandle,
    addr: String,
    secret: String,
    ring: Arc<Mutex<RingBuffer<LogEntry>>>,
    epoch: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::channel::<LogEntry>(2048);

        // Batch flusher: drains rx, accumulates, flushes on size or time.
        let app_for_flush = app.clone();
        let ring_for_flush = ring.clone();
        tokio::spawn(async move {
            let mut batch: Vec<LogEntry> = Vec::with_capacity(256);
            let flush_interval = Duration::from_millis(100);
            let max_batch = 200usize;
            loop {
                tokio::select! {
                    maybe = rx.recv() => {
                        match maybe {
                            Some(e) => {
                                ring_for_flush.lock().push(e.clone());
                                batch.push(e);
                                if batch.len() >= max_batch {
                                    let _ = app_for_flush.emit(
                                        "logs:append",
                                        &LogBatch { epoch, entries: &batch },
                                    );
                                    batch.clear();
                                }
                            }
                            None => break,
                        }
                    }
                    _ = tokio::time::sleep(flush_interval) => {
                        if !batch.is_empty() {
                            let _ = app_for_flush.emit(
                                "logs:append",
                                &LogBatch { epoch, entries: &batch },
                            );
                            batch.clear();
                        }
                    }
                }
            }
        });

        let client = Client::builder()
            .no_proxy()
            .build()
            .expect("reqwest");

        const MAX_CONSEC_FAILS: u32 = 6;
        let mut backoff_ms = 200u64;
        let mut consec_fails: u32 = 0;
        loop {
            match run_once(&client, &addr, &secret, &tx).await {
                Ok(()) => {
                    tracing::info!("logs stream ended; reconnecting");
                    backoff_ms = 200;
                    consec_fails = 0;
                }
                Err(e) => {
                    consec_fails += 1;
                    tracing::warn!(
                        ?e,
                        "logs stream error (#{consec_fails}); backing off {}ms",
                        backoff_ms
                    );
                    if consec_fails >= MAX_CONSEC_FAILS {
                        let _ = app.emit(
                            "pumps:stale",
                            serde_json::json!({ "kind": "logs", "epoch": epoch }),
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
    client: &Client,
    addr: &str,
    secret: &str,
    tx: &mpsc::Sender<LogEntry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("http://{addr}/logs?level=debug");
    let resp = client.get(&url).bearer_auth(secret).send().await?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(8192);

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.extend_from_slice(&chunk);
        // Drain complete lines.
        loop {
            let nl = match buf.iter().position(|b| *b == b'\n') {
                Some(i) => i,
                None => break,
            };
            let line = buf.drain(..=nl).collect::<Vec<_>>();
            let trimmed = std::str::from_utf8(&line[..line.len() - 1])
                .unwrap_or("")
                .trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(frame) = serde_json::from_str::<LogFrame>(trimmed) {
                let entry = LogEntry {
                    ts_ms: now_ms(),
                    level: frame.kind,
                    payload: frame.payload,
                };
                if tx.try_send(entry).is_err() {
                    // Frontend / batcher behind; drop oldest by losing this
                    // one. Backpressure is intentional — UI shouldn't lock
                    // up the pump.
                }
            }
        }
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
