//! Subscription store: persistent list of remote sing-box JSON URLs.
//!
//! v1 only handles native sing-box JSON configs (Content-Type:
//! application/json or text/plain with a JSON body). Clash YAML / SS
//! base64 conversion is deferred — those formats need a translation
//! layer that risks the very lossy semantics we built jsonc_edit to
//! avoid.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Wry};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

const STORE_FILE: &str = "subscriptions.json";
const SUBS_KEY: &str = "subscriptions";
const USER_AGENT: &str = "Inkwing/0.1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    /// 0 = manual refresh only.
    pub interval_hours: u32,
    pub last_fetched_at_ms: Option<u64>,
    pub last_error: Option<String>,
    /// Cached outbound count from the most recent successful fetch.
    pub outbound_count: Option<u32>,
}

pub fn load_all(app: &AppHandle<Wry>) -> Vec<Subscription> {
    match app.store(STORE_FILE) {
        Ok(store) => store
            .get(SUBS_KEY)
            .and_then(|v| serde_json::from_value::<Vec<Subscription>>(v).ok())
            .unwrap_or_default(),
        Err(_) => vec![],
    }
}

pub fn save_all(app: &AppHandle<Wry>, subs: &[Subscription]) -> AppResult<()> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| AppError::Other(format!("open store: {e}")))?;
    store.set(SUBS_KEY, serde_json::to_value(subs)?);
    store
        .save()
        .map_err(|e| AppError::Other(format!("save store: {e}")))?;
    Ok(())
}

pub fn new_subscription(name: String, url: String, interval_hours: u32) -> Subscription {
    Subscription {
        id: Uuid::new_v4().to_string(),
        name,
        url,
        interval_hours,
        last_fetched_at_ms: None,
        last_error: None,
        outbound_count: None,
    }
}

/// Fetch a subscription URL and parse its body as a sing-box JSON config.
/// Returns the verbatim text (so we can store it as the new ConfigEntry
/// preserving any comments / formatting) AND the parsed Value (used for
/// quick metadata extraction like outbound count). v1 only handles
/// native sing-box JSON — Clash YAML / SS base64 are explicitly rejected.
pub async fn fetch_full_config(url: &str) -> AppResult<(String, Value)> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Other(format!("http client: {e}")))?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "subscription HTTP {}",
            resp.status()
        )));
    }
    let body = resp.text().await?;
    let parsed: Value = serde_json::from_str(&body).map_err(|e| {
        AppError::Other(format!(
            "subscription body is not sing-box JSON: {e}; v1 doesn't support Clash YAML / SS base64 yet"
        ))
    })?;
    if parsed
        .get("outbounds")
        .and_then(|v| v.as_array())
        .is_none()
    {
        return Err(AppError::Other(
            "subscription is missing an outbounds[] array".into(),
        ));
    }
    Ok((body, parsed))
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
