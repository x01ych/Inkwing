//! DNS data model — sibling of `core::rules` for the DNS editor.
//!
//! sing-box DNS has two managed arrays:
//!
//!   - `dns.servers[]` — typed (1.12+ discriminated union on `type`).
//!     Each variant has its own fields; we keep a small whitelist of
//!     "known" types we render specifically (udp/tcp/tls/https/quic/h3/
//!     local/hosts/dhcp/fakeip), everything else gets `editable=false`
//!     with raw JSON.
//!
//!   - `dns.rules[]` — match/action like route.rules but with DNS-only
//!     matchers (`query_type`) and DNS-only actions (`route` with
//!     `server`, `reject`, `predefined`, `route-options`).
//!
//! Both flow through the overrides layer (see core::overrides and
//! commands::dns_cmd) — the user's source config is never touched.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};

// ---------- DNS server -------------------------------------------------

/// Server `type` values we render with a structured form. Anything
/// outside this set falls back to read-only raw JSON. Order: most
/// common first.
pub const KNOWN_DNS_SERVER_TYPES: &[&str] = &[
    "udp", "tcp", "tls", "https", "quic", "h3", "local", "hosts", "dhcp", "fakeip",
];

/// What gets sent over IPC for display.
#[derive(Debug, Clone, Serialize)]
pub struct DnsServerView {
    pub id: String,
    pub editable: bool,
    pub tag: String,
    /// sing-box's `type` field. Renamed because `type` is reserved in
    /// many languages and the route module already uses `kind`.
    pub kind: String,
    /// For `udp`/`tcp`/`tls`/`https`/`quic`/`h3`: the address to dial.
    /// (1.12+ uses `server` + `server_port`; we collapse to a single
    /// "server" field for the UI but pass through both on save.)
    pub server: Option<String>,
    pub server_port: Option<u16>,
    /// HTTPS / H3 path (e.g. "/dns-query").
    pub path: Option<String>,
    pub detour: Option<String>,
    pub domain_resolver: Option<String>,
    pub domain_strategy: Option<String>,
    /// Legacy 1.11 form — `address: "tls://..."`. We surface but don't
    /// edit; `editable=false` if user's config still uses this shape.
    pub address: Option<String>,
    pub readonly_reason: String,
    pub raw_pretty: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DnsServerInput {
    pub tag: String,
    pub kind: String,
    pub server: Option<String>,
    pub server_port: Option<u16>,
    pub path: Option<String>,
    pub detour: Option<String>,
    pub domain_resolver: Option<String>,
    pub domain_strategy: Option<String>,
    /// Catch-all so the UI can pass through fields we haven't surfaced.
    #[serde(default)]
    pub extra: Map<String, Value>,
}

pub fn dns_server_to_view(idx: usize, v: &Value) -> DnsServerView {
    let raw_pretty = serde_json::to_string_pretty(v).unwrap_or_default();
    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            return DnsServerView {
                id: idx.to_string(),
                editable: false,
                tag: String::new(),
                kind: String::new(),
                server: None,
                server_port: None,
                path: None,
                detour: None,
                domain_resolver: None,
                domain_strategy: None,
                address: None,
                readonly_reason: "not an object".into(),
                raw_pretty,
            };
        }
    };
    let kind = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let known = KNOWN_DNS_SERVER_TYPES.iter().any(|t| *t == kind);
    let address = obj.get("address").and_then(|v| v.as_str()).map(String::from);
    let mut readonly_reason = String::new();
    let editable = if address.is_some() && !known {
        readonly_reason = "legacy `address` form — switch to typed shape on save".into();
        false
    } else if !known {
        readonly_reason = if kind.is_empty() {
            "missing `type` field".into()
        } else {
            format!("unknown server type `{kind}`")
        };
        false
    } else {
        true
    };
    DnsServerView {
        id: idx.to_string(),
        editable,
        tag: obj
            .get("tag")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        kind,
        server: obj.get("server").and_then(|v| v.as_str()).map(String::from),
        server_port: obj
            .get("server_port")
            .and_then(|v| v.as_u64())
            .and_then(|n| u16::try_from(n).ok()),
        path: obj.get("path").and_then(|v| v.as_str()).map(String::from),
        detour: obj.get("detour").and_then(|v| v.as_str()).map(String::from),
        domain_resolver: obj
            .get("domain_resolver")
            .and_then(|v| v.as_str())
            .map(String::from),
        domain_strategy: obj
            .get("domain_strategy")
            .and_then(|v| v.as_str())
            .map(String::from),
        address,
        readonly_reason,
        raw_pretty,
    }
}

pub fn dns_server_input_to_value(input: &DnsServerInput) -> AppResult<Value> {
    if input.tag.trim().is_empty() {
        return Err(AppError::Config("DNS server tag is required".into()));
    }
    if !KNOWN_DNS_SERVER_TYPES.iter().any(|t| *t == input.kind) {
        return Err(AppError::Config(format!(
            "unsupported DNS server type `{}`",
            input.kind
        )));
    }
    let mut obj = Map::new();
    // Insert known scalars in a predictable order.
    obj.insert("type".into(), json!(input.kind));
    obj.insert("tag".into(), json!(input.tag.trim()));
    if let Some(s) = &input.server {
        if !s.trim().is_empty() {
            obj.insert("server".into(), json!(s.trim()));
        }
    }
    if let Some(p) = input.server_port {
        obj.insert("server_port".into(), json!(p));
    }
    if let Some(p) = &input.path {
        if !p.trim().is_empty() {
            obj.insert("path".into(), json!(p.trim()));
        }
    }
    if let Some(d) = &input.detour {
        if !d.trim().is_empty() {
            obj.insert("detour".into(), json!(d.trim()));
        }
    }
    if let Some(r) = &input.domain_resolver {
        if !r.trim().is_empty() {
            obj.insert("domain_resolver".into(), json!(r.trim()));
        }
    }
    if let Some(s) = &input.domain_strategy {
        if !s.trim().is_empty() {
            obj.insert("domain_strategy".into(), json!(s.trim()));
        }
    }
    // Pass-through extras (so unknown 1.14+ fields the user might have
    // pre-existing don't get dropped).
    for (k, v) in &input.extra {
        if !obj.contains_key(k) {
            obj.insert(k.clone(), v.clone());
        }
    }
    Ok(Value::Object(obj))
}

// ---------- DNS rule --------------------------------------------------

/// DNS rule matchers we render structurally. Reuses route's matcher
/// list (same shape on the wire) plus DNS-only `query_type`.
pub const KNOWN_DNS_MATCHERS: &[&str] = &[
    // domain
    "domain", "domain_suffix", "domain_keyword", "domain_regex", "geosite", "rule_set",
    // IP-on-source
    "source_ip_cidr", "source_ip_is_private",
    // port
    "port", "port_range", "source_port", "source_port_range",
    // process
    "process_name", "process_path", "package_name", "user", "user_id",
    // misc
    "network", "protocol", "inbound", "clash_mode",
    // DNS-only
    "query_type",
];

/// Actions valid in DNS rule context.
pub const KNOWN_DNS_ACTIONS: &[&str] = &["route", "reject", "predefined", "route-options"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsMatcher {
    pub kind: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsRuleView {
    pub id: String,
    pub editable: bool,
    pub matchers: Vec<DnsMatcher>,
    pub server: Option<String>,
    pub action: Option<String>,
    pub invert: bool,
    pub readonly_reason: String,
    pub raw_pretty: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DnsRuleInput {
    pub matchers: Vec<DnsMatcher>,
    pub server: Option<String>,
    pub action: Option<String>,
    #[serde(default)]
    pub invert: bool,
}

pub fn dns_rule_to_view(idx: usize, v: &Value) -> DnsRuleView {
    let raw_pretty = serde_json::to_string_pretty(v).unwrap_or_default();
    let obj = match v.as_object() {
        Some(o) => o,
        None => {
            return DnsRuleView {
                id: idx.to_string(),
                editable: false,
                matchers: vec![],
                server: None,
                action: None,
                invert: false,
                readonly_reason: "not an object".into(),
                raw_pretty,
            };
        }
    };

    // Logical rules → read-only
    if obj.get("type").and_then(|v| v.as_str()) == Some("logical") {
        return DnsRuleView {
            id: idx.to_string(),
            editable: false,
            matchers: vec![],
            server: None,
            action: None,
            invert: false,
            readonly_reason: "logical rule (and/or)".into(),
            raw_pretty,
        };
    }

    let matchers: Vec<DnsMatcher> = obj
        .iter()
        .filter_map(|(k, v)| {
            if !KNOWN_DNS_MATCHERS.iter().any(|m| *m == k) {
                return None;
            }
            let values: Vec<String> = match v {
                Value::String(s) => vec![s.clone()],
                Value::Number(n) => vec![n.to_string()],
                Value::Array(arr) => arr
                    .iter()
                    .filter_map(|x| match x {
                        Value::String(s) => Some(s.clone()),
                        Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .collect(),
                _ => return None,
            };
            Some(DnsMatcher {
                kind: k.clone(),
                values,
            })
        })
        .collect();

    // Detect unknown structural keys (other than matchers + action +
    // server + invert + nested type hint) → read-only.
    let known_keys: std::collections::HashSet<&str> = {
        let mut s: std::collections::HashSet<&str> = KNOWN_DNS_MATCHERS.iter().copied().collect();
        for k in &["server", "action", "invert", "type", "disable_cache", "rewrite_ttl",
                   "client_subnet", "method", "no_drop", "rcode", "answer", "ns", "extra"] {
            s.insert(k);
        }
        s
    };
    let unknown: Vec<String> = obj
        .keys()
        .filter(|k| !known_keys.contains(k.as_str()))
        .cloned()
        .collect();

    let action = obj
        .get("action")
        .and_then(|v| v.as_str())
        .map(String::from);
    let server = obj.get("server").and_then(|v| v.as_str()).map(String::from);
    let invert = obj.get("invert").and_then(|v| v.as_bool()).unwrap_or(false);

    let known_action = match action.as_deref() {
        None => true, // implicit "route"
        Some(a) => KNOWN_DNS_ACTIONS.iter().any(|x| *x == a),
    };
    let editable = unknown.is_empty()
        && known_action
        && (matchers.is_empty() || !matchers.is_empty()) // tautology: shape OK
        && (server.is_some() || action.is_some());

    let readonly_reason = if !unknown.is_empty() {
        format!("unknown field(s): {}", unknown.join(", "))
    } else if !known_action {
        format!("unknown action `{}`", action.as_deref().unwrap_or(""))
    } else if server.is_none() && action.is_none() {
        "no server / action".into()
    } else {
        String::new()
    };

    DnsRuleView {
        id: idx.to_string(),
        editable,
        matchers,
        server,
        action,
        invert,
        readonly_reason,
        raw_pretty,
    }
}

pub fn dns_rule_input_to_value(input: &DnsRuleInput) -> AppResult<Value> {
    if input.matchers.is_empty() {
        return Err(AppError::Config(
            "DNS rule needs at least one matcher".into(),
        ));
    }
    if input.server.as_deref().unwrap_or("").trim().is_empty()
        && input.action.as_deref().map(|a| a == "route").unwrap_or(true)
    {
        return Err(AppError::Config(
            "DNS rule needs `server` (or non-route `action`)".into(),
        ));
    }
    let mut obj = Map::new();
    for m in &input.matchers {
        if !KNOWN_DNS_MATCHERS.iter().any(|x| *x == m.kind) {
            return Err(AppError::Config(format!(
                "unsupported DNS matcher `{}`",
                m.kind
            )));
        }
        if m.values.is_empty() {
            return Err(AppError::Config(format!(
                "matcher `{}` has no values",
                m.kind
            )));
        }
        // Port matchers want numbers; everything else stays as strings.
        let v = if matches!(m.kind.as_str(), "port" | "source_port") {
            Value::Array(
                m.values
                    .iter()
                    .map(|s| s.parse::<u64>().map(Value::from).unwrap_or(Value::String(s.clone())))
                    .collect(),
            )
        } else {
            Value::Array(m.values.iter().map(|s| Value::String(s.clone())).collect())
        };
        obj.insert(m.kind.clone(), v);
    }
    if let Some(s) = &input.server {
        if !s.trim().is_empty() {
            obj.insert("server".into(), json!(s.trim()));
        }
    }
    if let Some(a) = &input.action {
        if !a.trim().is_empty() && a != "route" {
            obj.insert("action".into(), json!(a));
        }
    }
    if input.invert {
        obj.insert("invert".into(), json!(true));
    }
    Ok(Value::Object(obj))
}
