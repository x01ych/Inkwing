use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// User-facing summary derived from a parsed config. Cheap to compute, sent
/// to the frontend after `config_load`. Does NOT include the full parsed
/// config — frontend asks for raw via `config_get_raw` if it needs it.
#[derive(Debug, Serialize)]
pub struct ConfigSummary {
    pub path: String,
    pub size_bytes: u64,
    pub inbounds: Vec<InboundSummary>,
    pub outbound_tags: Vec<String>,
    pub rule_count: usize,
    pub final_outbound: Option<String>,
    pub has_clash_api: bool,
    pub has_tun: bool,
    pub log_level: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InboundSummary {
    #[serde(rename = "type")]
    pub kind: String,
    pub tag: Option<String>,
}

/// Read file bytes and parse the config. The returned `raw` is always the
/// original bytes (zero-loss round-trip depends on this — surgical edits
/// in `jsonc_edit` operate on `raw`, never on the parsed cache). The
/// `parsed` cache uses jsonc-parser so comments and trailing commas are
/// tolerated; sing-box itself accepts JSONC, and the marketed promise is
/// that any config sing-box can run, this GUI can load.
pub fn load_from_path(path: &Path) -> AppResult<(Vec<u8>, Value)> {
    let bytes = std::fs::read(path)
        .map_err(|e| AppError::Config(format!("read {}: {e}", path.display())))?;
    let parsed = parse_value(&bytes)
        .map_err(|e| AppError::Config(format!("parse {}: {e}", path.display())))?;
    Ok((bytes, parsed))
}

fn parse_value(bytes: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("not UTF-8: {e}"))?;
    let v: Option<Value> =
        jsonc_parser::parse_to_serde_value(text, &Default::default()).map_err(|e| e.to_string())?;
    v.ok_or_else(|| "empty config".to_string())
}

pub fn build_summary(path: &Path, raw: &[u8], parsed: &Value) -> ConfigSummary {
    let inbounds = parsed
        .get("inbounds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|i| InboundSummary {
                    kind: i
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    tag: i.get("tag").and_then(|v| v.as_str()).map(String::from),
                })
                .collect()
        })
        .unwrap_or_default();

    let outbound_tags = parsed
        .get("outbounds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o.get("tag").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let route = parsed.get("route");
    let rule_count = route
        .and_then(|r| r.get("rules"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let final_outbound = route
        .and_then(|r| r.get("final"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let has_clash_api = parsed
        .pointer("/experimental/clash_api")
        .is_some();
    let has_tun = parsed
        .get("inbounds")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().any(|i| i.get("type").and_then(|v| v.as_str()) == Some("tun")))
        .unwrap_or(false);
    let log_level = parsed
        .pointer("/log/level")
        .and_then(|v| v.as_str())
        .map(String::from);

    ConfigSummary {
        path: path.display().to_string(),
        size_bytes: raw.len() as u64,
        inbounds,
        outbound_tags,
        rule_count,
        final_outbound,
        has_clash_api,
        has_tun,
        log_level,
    }
}

/// Used by `config_save` to write content the frontend produced (e.g. a raw
/// edit in Monaco). Validates as JSON before touching disk so we never
/// persist garbage.
pub fn save_raw(path: &Path, content: &str) -> AppResult<()> {
    let _: Value = serde_json::from_str(content)
        .map_err(|e| AppError::Config(format!("invalid json: {e}")))?;
    crate::util::atomic_write::atomic_write(path, content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_jsonc_with_line_and_block_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        let body = br#"{
    // header comment
    "log": { "level": "info" /* inline */ },
    "inbounds": [],
    "outbounds": [{ "type": "direct", "tag": "direct" }],
    "route": { "final": "direct", "rules": [] }, // trailing comma below ok
}"#;
        std::fs::write(&path, body).unwrap();
        let (raw, parsed) = load_from_path(&path).unwrap();
        // raw must be byte-identical — surgical edit depends on this.
        assert_eq!(raw, body);
        // parsed must carry the values.
        assert_eq!(parsed.pointer("/log/level").and_then(|v| v.as_str()), Some("info"));
        assert_eq!(
            parsed.pointer("/route/final").and_then(|v| v.as_str()),
            Some("direct")
        );
    }

    #[test]
    fn rejects_truly_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"{ this is not json").unwrap();
        assert!(load_from_path(&path).is_err());
    }

    /// Regression: the path source_array follows on the Rules page.
    /// A config with `/route/rule_set` populated must round-trip through
    /// load_from_path + the `pointer("/route/rule_set")` lookup that
    /// `rules_cmd::source_array` uses, so the rule_set tab is never
    /// empty when the user's source config actually has entries.
    #[test]
    fn route_rule_set_array_survives_load_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.json");
        let body = br#"{
    "log": { "level": "info" },
    "outbounds": [{ "type": "direct", "tag": "direct" }],
    "route": {
        "rule_set": [
            { "tag": "geosite-cn", "type": "remote", "format": "binary",
              "url": "https://example.test/geosite-cn.srs", "update_interval": "1d" },
            { "tag": "geoip-cn", "type": "remote", "format": "binary",
              "url": "https://example.test/geoip-cn.srs" }
        ],
        "rules": []
    }
}"#;
        std::fs::write(&path, body).unwrap();
        let (_raw, parsed) = load_from_path(&path).unwrap();
        let rs = parsed
            .pointer("/route/rule_set")
            .and_then(|v| v.as_array())
            .expect("rule_set should be present as an array");
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0].get("tag").and_then(|v| v.as_str()), Some("geosite-cn"));
    }
}
