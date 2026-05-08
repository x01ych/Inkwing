use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// Thin client for sing-box's Clash-compatible HTTP API. WebSocket streams
/// (/logs, /traffic, /connections) are handled by per-stream pump modules,
/// not here — this is HTTP-only.
#[derive(Clone)]
pub struct ClashClient {
    base: String,
    secret: String,
    http: Client,
}

impl ClashClient {
    pub fn new(addr: &str, secret: &str) -> Self {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .no_proxy() // 127.0.0.1 should never go through HTTP_PROXY
            .build()
            .expect("reqwest client");
        Self {
            base: format!("http://{addr}"),
            secret: secret.to_string(),
            http,
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn secret(&self) -> &str {
        &self.secret
    }

    /// Returns once `/version` answers 200. Polls every 100ms up to
    /// `timeout`. Used as readiness probe right after starting sing-box.
    pub async fn wait_ready(&self, timeout: Duration) -> AppResult<VersionInfo> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match self.version().await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(AppError::ClashApi(format!(
                            "sing-box not ready within {:?}: {}",
                            timeout, e
                        )));
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    pub async fn version(&self) -> AppResult<VersionInfo> {
        let url = format!("{}/version", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.secret)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AppError::ClashApi(format!(
                "GET /version returned {}",
                resp.status()
            )));
        }
        Ok(resp.json::<VersionInfo>().await?)
    }

    /// Mihomo / Clash.Meta API: returns the proxies map. We pass the raw
    /// value through to the frontend so we don't need to mirror sing-box's
    /// proxy taxonomy in Rust — UI groups them by `type`/`all` heuristics.
    pub async fn proxies(&self) -> AppResult<Value> {
        let url = format!("{}/proxies", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.secret)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AppError::ClashApi(format!(
                "GET /proxies returned {}",
                resp.status()
            )));
        }
        Ok(resp.json::<Value>().await?)
    }

    /// PUT /proxies/{group} { "name": "<node>" } — picks `node` as the
    /// active outbound for selector `group`.
    pub async fn select_proxy(&self, group: &str, name: &str) -> AppResult<()> {
        let url = format!(
            "{}/proxies/{}",
            self.base,
            urlencoding::encode(group)
        );
        let body = serde_json::json!({ "name": name });
        let resp = self
            .http
            .put(&url)
            .bearer_auth(&self.secret)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AppError::ClashApi(format!(
                "PUT /proxies/{group} returned {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }

    /// GET /proxies/{name}/delay?url=...&timeout=ms → { delay: u32 } or 5xx.
    /// We translate failure into Ok(None) so the UI can render "timeout".
    pub async fn proxy_delay(
        &self,
        name: &str,
        url_target: &str,
        timeout_ms: u32,
    ) -> AppResult<DelayResult> {
        let url = format!(
            "{}/proxies/{}/delay?url={}&timeout={timeout_ms}",
            self.base,
            urlencoding::encode(name),
            urlencoding::encode(url_target),
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.secret)
            .timeout(Duration::from_millis((timeout_ms + 1500) as u64))
            .send()
            .await?;
        if resp.status().is_success() {
            #[derive(Deserialize)]
            struct Body {
                delay: u32,
            }
            let body = resp.json::<Body>().await?;
            Ok(DelayResult { delay_ms: Some(body.delay) })
        } else {
            // sing-box returns 5xx with {message: "..."} on timeout/unreachable.
            Ok(DelayResult { delay_ms: None })
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DelayResult {
    pub delay_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpeedTestResult {
    pub bytes: u64,
    pub duration_ms: u64,
    /// throughput in bytes/sec
    pub bytes_per_sec: u64,
}

impl ClashClient {
    /// Drive a real download through a SOCKS5 proxy that sing-box exposes
    /// (typically a `mixed` inbound on 127.0.0.1) — the only way to
    /// throughput-test a node, since clash_api offers latency only. Caller
    /// must pick a node first via `select_proxy`.
    pub async fn speedtest_via_socks(
        &self,
        socks_addr: &str,
        url: &str,
        max_bytes: u64,
    ) -> AppResult<SpeedTestResult> {
        let proxy = reqwest::Proxy::all(format!("socks5h://{socks_addr}"))
            .map_err(|e| AppError::Other(format!("invalid socks proxy: {e}")))?;
        let client = reqwest::Client::builder()
            .proxy(proxy)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| AppError::Other(format!("speedtest http client: {e}")))?;
        let started = std::time::Instant::now();
        let mut resp = client.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::Other(format!(
                "speedtest target returned {}",
                resp.status()
            )));
        }
        let mut total: u64 = 0;
        while let Some(chunk) = resp.chunk().await? {
            total += chunk.len() as u64;
            if total >= max_bytes {
                break;
            }
        }
        let duration_ms = started.elapsed().as_millis() as u64;
        let bytes_per_sec = if duration_ms == 0 {
            0
        } else {
            (total as u128 * 1000 / duration_ms as u128) as u64
        };
        Ok(SpeedTestResult {
            bytes: total,
            duration_ms,
            bytes_per_sec,
        })
    }

    /// DELETE /connections/{id} — close one live connection. sing-box/Mihomo
    /// returns 204 on success, 404 if the id is already gone (treat as ok).
    pub async fn close_connection(&self, id: &str) -> AppResult<()> {
        let url = format!("{}/connections/{}", self.base, urlencoding::encode(id));
        let resp = self
            .http
            .delete(&url)
            .bearer_auth(&self.secret)
            .send()
            .await?;
        let st = resp.status();
        if st.is_success() || st.as_u16() == 404 {
            Ok(())
        } else {
            Err(AppError::ClashApi(format!(
                "DELETE /connections/{id} returned {st}"
            )))
        }
    }

    /// DELETE /connections — close all live connections.
    pub async fn close_all_connections(&self) -> AppResult<()> {
        let url = format!("{}/connections", self.base);
        let resp = self
            .http
            .delete(&url)
            .bearer_auth(&self.secret)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AppError::ClashApi(format!(
                "DELETE /connections returned {}",
                resp.status()
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    pub version: String,
    #[serde(default)]
    pub premium: bool,
    #[serde(default)]
    pub meta: bool,
}
