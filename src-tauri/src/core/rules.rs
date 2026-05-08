//! sing-box `route.rules` business model + Value <-> RuleView round-trip.
//!
//! - Reads existing rules out of a cached config (a serde_json::Value).
//! - Editable rules become a flat shape (matchers + outbound/action) the
//!   UI can render in form fields. Logical / action-only / unknown-shape
//!   rules are surfaced as read-only views the user can drag/delete but
//!   not edit field-by-field.
//! - Writing back to the user's source file is done elsewhere via
//!   `jsonc_edit::replace_route_rules` so comments/format are preserved.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

/// All matcher kinds the editor knows how to round-trip safely. Anything
/// outside this set forces a rule into read-only mode so we don't drop
/// fields silently.
pub const KNOWN_MATCHERS: &[&str] = &[
    "domain",
    "domain_suffix",
    "domain_keyword",
    "domain_regex",
    "geosite",
    "geoip",
    "ip_cidr",
    "source_ip_cidr",
    "ip_is_private",
    "source_ip_is_private",
    "port",
    "port_range",
    "source_port",
    "source_port_range",
    "network",
    "protocol",
    "process_name",
    "process_path",
    "package_name",
    "user",
    "user_id",
    "inbound",
    "clash_mode",
    "rule_set",
    "rule_set_ip_cidr_match_source",
    "rule_set_ip_cidr_accept_empty",
];

/// Field names that are control fields (target / action / inversion),
/// not matchers. Carried separately on RuleView.
const CONTROL_FIELDS: &[&str] = &["outbound", "action", "invert"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matcher {
    pub kind: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleView {
    /// Stable position id. Rules don't have a sing-box-side id; we use the
    /// array index of the snapshot (becomes the natural drag-and-drop key).
    pub id: String,
    /// True when this rule's shape is one we can edit field-by-field.
    pub editable: bool,
    /// Matchers (only populated when editable).
    pub matchers: Vec<Matcher>,
    /// Routing target tag, if applicable.
    pub outbound: Option<String>,
    /// `action` field (sing-box 1.11+: route/block/reject/sniff/resolve/hijack-dns).
    pub action: Option<String>,
    /// Whether match is inverted (sing-box `invert: true`).
    pub invert: bool,
    /// Why this rule is read-only (e.g. "logical rule", "unknown field 'foo'").
    /// Empty when editable.
    pub readonly_reason: String,
    /// Pretty-printed full rule body, shown in the read-only badge.
    pub raw_pretty: String,
}

/// Input the UI hands us for add / update. Matcher shape is identical
/// to the read-side type — we just reuse it.
#[derive(Debug, Clone, Deserialize)]
pub struct RuleInput {
    pub matchers: Vec<Matcher>,
    #[serde(default)]
    pub outbound: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub invert: bool,
}

/// Convert one rule Value into a UI-friendly view at array position `idx`.
pub fn rule_to_view(idx: usize, v: &Value) -> RuleView {
    let raw_pretty =
        serde_json::to_string_pretty(v).unwrap_or_else(|_| "<unserializable>".into());

    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            return RuleView {
                id: idx.to_string(),
                editable: false,
                matchers: vec![],
                outbound: None,
                action: None,
                invert: false,
                readonly_reason: "rule is not a JSON object".into(),
                raw_pretty,
            };
        }
    };

    // Logical rules are always read-only in v1.
    if obj.get("type").and_then(|t| t.as_str()) == Some("logical") {
        return RuleView {
            id: idx.to_string(),
            editable: false,
            matchers: vec![],
            outbound: obj.get("outbound").and_then(|v| v.as_str()).map(String::from),
            action: obj.get("action").and_then(|v| v.as_str()).map(String::from),
            invert: obj.get("invert").and_then(|v| v.as_bool()).unwrap_or(false),
            readonly_reason: "logical rule (and/or composition)".into(),
            raw_pretty,
        };
    }

    // Detect unknown fields so we never silently drop them on round-trip.
    let known: BTreeSet<&str> = KNOWN_MATCHERS
        .iter()
        .copied()
        .chain(CONTROL_FIELDS.iter().copied())
        .collect();
    let unknown: Vec<&str> = obj
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .map(String::as_str)
        .collect();

    let outbound = obj.get("outbound").and_then(|v| v.as_str()).map(String::from);
    let action = obj.get("action").and_then(|v| v.as_str()).map(String::from);
    let invert = obj.get("invert").and_then(|v| v.as_bool()).unwrap_or(false);

    if !unknown.is_empty() {
        return RuleView {
            id: idx.to_string(),
            editable: false,
            matchers: vec![],
            outbound,
            action,
            invert,
            readonly_reason: format!("unknown field(s): {}", unknown.join(", ")),
            raw_pretty,
        };
    }

    // Build matchers from each known matcher key. Sing-box accepts both
    // scalar and array; we normalize to a Vec<String>.
    let mut matchers = Vec::new();
    for k in KNOWN_MATCHERS {
        if let Some(field) = obj.get(*k) {
            let values = value_to_string_vec(field);
            matchers.push(Matcher {
                kind: (*k).into(),
                values,
            });
        }
    }

    // A rule with neither outbound nor action and no matchers is degenerate;
    // sing-box 1.11+ allows `action`-only rules (e.g. sniff). Treat as
    // read-only since the editor doesn't yet ask for "action" without
    // matchers.
    if matchers.is_empty() && action.is_none() && outbound.is_none() {
        return RuleView {
            id: idx.to_string(),
            editable: false,
            matchers,
            outbound,
            action,
            invert,
            readonly_reason: "rule has no matchers / outbound / action".into(),
            raw_pretty,
        };
    }

    RuleView {
        id: idx.to_string(),
        editable: true,
        matchers,
        outbound,
        action,
        invert,
        readonly_reason: String::new(),
        raw_pretty,
    }
}

fn value_to_string_vec(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Number(n) => vec![n.to_string()],
        Value::Bool(b) => vec![b.to_string()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| match x {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

/// Build a sing-box rule Value from a UI RuleInput. Single-element value
/// arrays are kept as arrays for consistency — sing-box accepts both.
pub fn input_to_value(input: &RuleInput) -> AppResult<Value> {
    let mut obj = serde_json::Map::new();
    let mut seen_kinds = BTreeSet::new();
    for m in &input.matchers {
        if !KNOWN_MATCHERS.contains(&m.kind.as_str()) {
            return Err(AppError::Config(format!(
                "rule_input: unknown matcher kind '{}'",
                m.kind
            )));
        }
        if !seen_kinds.insert(m.kind.clone()) {
            return Err(AppError::Config(format!(
                "rule_input: duplicate matcher kind '{}'; merge values into a single matcher",
                m.kind
            )));
        }
        if m.values.is_empty() {
            return Err(AppError::Config(format!(
                "rule_input: matcher '{}' has no values",
                m.kind
            )));
        }
        // Numeric kinds keep their values as numbers when possible so the
        // serialized JSON matches what sing-box check expects.
        let json_vals: Vec<Value> = m
            .values
            .iter()
            .map(|s| coerce_for_kind(&m.kind, s))
            .collect();
        obj.insert(m.kind.clone(), json!(json_vals));
    }
    if let Some(out) = &input.outbound {
        obj.insert("outbound".into(), json!(out));
    }
    if let Some(act) = &input.action {
        obj.insert("action".into(), json!(act));
    }
    if input.invert {
        obj.insert("invert".into(), json!(true));
    }
    if obj.is_empty() {
        return Err(AppError::Config("rule_input: empty rule".into()));
    }
    if !obj.contains_key("outbound") && !obj.contains_key("action") {
        return Err(AppError::Config(
            "rule_input: must have either outbound or action".into(),
        ));
    }
    Ok(Value::Object(obj))
}

// ---------------------------------------------------------------- rule_set

/// View of a sing-box `route.rule_set[i]` entry. Both local and remote
/// rule-sets fit; UI distinguishes via `kind`.
#[derive(Debug, Clone, Serialize)]
pub struct RuleSetView {
    pub id: String,
    pub editable: bool,
    pub tag: String,
    pub kind: String, // "local" | "remote" | unknown
    pub format: String, // "binary" | "source" | unknown
    pub url: Option<String>,
    pub path: Option<String>,
    pub download_detour: Option<String>,
    pub update_interval: Option<String>,
    pub readonly_reason: String,
    pub raw_pretty: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleSetInput {
    pub tag: String,
    pub kind: String,   // "local" | "remote"
    pub format: String, // "binary" | "source"
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub download_detour: Option<String>,
    #[serde(default)]
    pub update_interval: Option<String>,
}

const RULE_SET_KNOWN_FIELDS: &[&str] = &[
    "tag",
    "type",
    "format",
    "url",
    "path",
    "download_detour",
    "update_interval",
];

pub fn rule_set_to_view(idx: usize, v: &Value) -> RuleSetView {
    let raw_pretty =
        serde_json::to_string_pretty(v).unwrap_or_else(|_| "<unserializable>".into());
    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            return RuleSetView {
                id: idx.to_string(),
                editable: false,
                tag: String::new(),
                kind: String::new(),
                format: String::new(),
                url: None,
                path: None,
                download_detour: None,
                update_interval: None,
                readonly_reason: "rule_set entry is not an object".into(),
                raw_pretty,
            };
        }
    };

    let tag = obj.get("tag").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let kind = obj.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let format = obj.get("format").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let url = obj.get("url").and_then(|v| v.as_str()).map(String::from);
    let path = obj.get("path").and_then(|v| v.as_str()).map(String::from);
    let download_detour = obj
        .get("download_detour")
        .and_then(|v| v.as_str())
        .map(String::from);
    let update_interval = obj
        .get("update_interval")
        .and_then(|v| v.as_str())
        .map(String::from);

    let unknown: Vec<&str> = obj
        .keys()
        .filter(|k| !RULE_SET_KNOWN_FIELDS.contains(&k.as_str()))
        .map(String::as_str)
        .collect();
    let editable = unknown.is_empty()
        && !tag.is_empty()
        && (kind == "local" || kind == "remote");
    let readonly_reason = if !editable {
        if !unknown.is_empty() {
            format!("unknown field(s): {}", unknown.join(", "))
        } else if tag.is_empty() {
            "missing required field: tag".into()
        } else {
            format!("unsupported type: '{kind}' (only local/remote)")
        }
    } else {
        String::new()
    };

    RuleSetView {
        id: idx.to_string(),
        editable,
        tag,
        kind,
        format,
        url,
        path,
        download_detour,
        update_interval,
        readonly_reason,
        raw_pretty,
    }
}

pub fn rule_set_input_to_value(input: &RuleSetInput) -> AppResult<Value> {
    if input.tag.trim().is_empty() {
        return Err(AppError::Config("rule_set tag is required".into()));
    }
    if input.kind != "local" && input.kind != "remote" {
        return Err(AppError::Config(format!(
            "rule_set type must be 'local' or 'remote', got '{}'",
            input.kind
        )));
    }
    if input.format != "binary" && input.format != "source" {
        return Err(AppError::Config(format!(
            "rule_set format must be 'binary' or 'source', got '{}'",
            input.format
        )));
    }
    let mut obj = serde_json::Map::new();
    obj.insert("tag".into(), json!(input.tag));
    obj.insert("type".into(), json!(input.kind));
    obj.insert("format".into(), json!(input.format));
    if input.kind == "remote" {
        let url = input
            .url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::Config("remote rule_set requires url".into()))?;
        obj.insert("url".into(), json!(url));
        if let Some(d) = input.download_detour.as_deref().filter(|s| !s.is_empty()) {
            obj.insert("download_detour".into(), json!(d));
        }
        if let Some(d) = input.update_interval.as_deref().filter(|s| !s.is_empty()) {
            obj.insert("update_interval".into(), json!(d));
        }
    } else {
        let path = input
            .path
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::Config("local rule_set requires path".into()))?;
        obj.insert("path".into(), json!(path));
    }
    Ok(Value::Object(obj))
}

fn coerce_for_kind(kind: &str, s: &str) -> Value {
    let numeric = matches!(
        kind,
        "port" | "port_range" | "source_port" | "source_port_range"
    );
    if numeric {
        if let Ok(n) = s.parse::<u64>() {
            return json!(n);
        }
    }
    json!(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_outbound_rule() {
        let v = json!({"domain_suffix": ["a.com", "b.com"], "outbound": "proxy"});
        let view = rule_to_view(0, &v);
        assert!(view.editable);
        assert_eq!(view.outbound.as_deref(), Some("proxy"));
        assert_eq!(view.matchers.len(), 1);
        assert_eq!(view.matchers[0].kind, "domain_suffix");
        assert_eq!(view.matchers[0].values, vec!["a.com", "b.com"]);
    }

    #[test]
    fn scalar_matcher_value_is_normalized_to_vec() {
        let v = json!({"domain_suffix": "single.com", "outbound": "p"});
        let view = rule_to_view(0, &v);
        assert!(view.editable);
        assert_eq!(view.matchers[0].values, vec!["single.com"]);
    }

    #[test]
    fn logical_rule_is_readonly() {
        let v = json!({
            "type": "logical",
            "mode": "or",
            "rules": [{"domain_suffix": ["a.com"]}, {"domain_suffix": ["b.com"]}],
            "outbound": "block"
        });
        let view = rule_to_view(0, &v);
        assert!(!view.editable);
        assert!(view.readonly_reason.contains("logical"));
    }

    #[test]
    fn unknown_field_makes_readonly() {
        let v = json!({"foo_bar": ["x"], "outbound": "p"});
        let view = rule_to_view(0, &v);
        assert!(!view.editable);
        assert!(view.readonly_reason.contains("foo_bar"));
    }

    #[test]
    fn action_only_rule_is_readonly() {
        let v = json!({"action": "sniff", "sniffer": ["http"], "timeout": "200ms"});
        let view = rule_to_view(0, &v);
        // sniffer + timeout are unknown to KNOWN_MATCHERS so this falls
        // into "unknown field" branch — that's the safe behaviour.
        assert!(!view.editable);
    }

    #[test]
    fn input_to_value_basic() {
        let inp = RuleInput {
            matchers: vec![Matcher {
                kind: "domain_suffix".into(),
                values: vec!["a.com".into(), "b.com".into()],
            }],
            outbound: Some("proxy".into()),
            action: None,
            invert: false,
        };
        let v = input_to_value(&inp).unwrap();
        assert_eq!(v["domain_suffix"], json!(["a.com", "b.com"]));
        assert_eq!(v["outbound"], json!("proxy"));
    }

    #[test]
    fn input_port_values_become_numbers() {
        let inp = RuleInput {
            matchers: vec![Matcher {
                kind: "port".into(),
                values: vec!["80".into(), "443".into()],
            }],
            outbound: Some("proxy".into()),
            action: None,
            invert: false,
        };
        let v = input_to_value(&inp).unwrap();
        assert_eq!(v["port"], json!([80, 443]));
    }

    #[test]
    fn input_rejects_unknown_kind() {
        let inp = RuleInput {
            matchers: vec![Matcher {
                kind: "bogus".into(),
                values: vec!["x".into()],
            }],
            outbound: Some("p".into()),
            action: None,
            invert: false,
        };
        assert!(input_to_value(&inp).is_err());
    }

    #[test]
    fn input_requires_outbound_or_action() {
        let inp = RuleInput {
            matchers: vec![Matcher {
                kind: "domain_suffix".into(),
                values: vec!["a.com".into()],
            }],
            outbound: None,
            action: None,
            invert: false,
        };
        assert!(input_to_value(&inp).is_err());
    }
}
