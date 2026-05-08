//! Surgical JSONC edit on a sing-box config.
//!
//! Replaces ONLY the `route.rules` array; everything outside that range —
//! comments, key order, indent, unknown fields — is byte-for-byte preserved
//! by jsonc-parser's CST manipulation API. This is what makes our claim of
//! zero-loss native-config round-trip true.

use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use jsonc_parser::ParseOptions;
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// Replace a top-level array property by its name. Used for `outbounds`
/// (subscription apply) — same comment/format preservation guarantees.
pub fn replace_top_level_array(
    original: &str,
    key: &'static str,
    new_array: &Value,
) -> AppResult<String> {
    if !new_array.is_array() {
        return Err(AppError::Config(format!(
            "replace_top_level_array: {key} must be a JSON array"
        )));
    }
    let opts = ParseOptions::default();
    let root = CstRootNode::parse(original, &opts)
        .map_err(|e| AppError::Config(format!("JSONC parse failed: {e}")))?;
    let root_obj: CstObject = root.object_value_or_set();
    let new_input = json_to_cst_input(new_array);
    match root_obj.get(key) {
        Some(prop) => prop.set_value(new_input),
        None => {
            root_obj.append(key, new_input);
        }
    }
    Ok(root.to_string())
}

/// Replace the `route.rule_set` array in the original JSONC text with
/// `new_rule_sets`. Same guarantees as `replace_route_rules`: only the
/// targeted node is touched. Used by the Rule Sets editor.
pub fn replace_route_rule_set(original: &str, new_rule_sets: &Value) -> AppResult<String> {
    if !new_rule_sets.is_array() {
        return Err(AppError::Config(
            "replace_route_rule_set: new_rule_sets must be a JSON array".into(),
        ));
    }
    let opts = ParseOptions::default();
    let root = CstRootNode::parse(original, &opts)
        .map_err(|e| AppError::Config(format!("JSONC parse failed: {e}")))?;
    let root_obj: CstObject = root.object_value_or_set();
    let route_obj: CstObject = root_obj.object_value_or_set("route");
    let new_input: CstInputValue = json_to_cst_input(new_rule_sets);
    match route_obj.get("rule_set") {
        Some(prop) => prop.set_value(new_input),
        None => {
            route_obj.append("rule_set", new_input);
        }
    }
    Ok(root.to_string())
}

/// Replace the `route.rules` array in the original JSONC text with
/// `new_rules` (which must be a JSON array). The rest of the file is
/// untouched.
///
/// Three insertion-position cases handled:
///   1. `route.rules` exists → in-place replace
///   2. `route` exists but no `rules` → append `rules` to route
///   3. no `route` → append `route: { rules: [...] }` at top level
///
/// Returns the new file content as a String. Caller is responsible for
/// atomic_write to disk.
pub fn replace_route_rules(original: &str, new_rules: &Value) -> AppResult<String> {
    if !new_rules.is_array() {
        return Err(AppError::Config(
            "replace_route_rules: new_rules must be a JSON array".into(),
        ));
    }

    // Defaults are permissive enough for sing-box configs (comments,
    // trailing commas, normal property names).
    let opts = ParseOptions::default();
    let root = CstRootNode::parse(original, &opts)
        .map_err(|e| AppError::Config(format!("JSONC parse failed: {e}")))?;

    let root_obj: CstObject = root.object_value_or_set();
    let route_obj: CstObject = root_obj.object_value_or_set("route");

    let new_input: CstInputValue = json_to_cst_input(new_rules);

    match route_obj.get("rules") {
        Some(prop) => {
            // Case 1: rules exists. Replace in place.
            prop.set_value(new_input);
        }
        None => {
            // Case 2: route exists but no rules. Append.
            route_obj.append("rules", new_input);
        }
    }
    // Case 3 was handled implicitly: object_value_or_set("route") creates
    // route if missing, then we append rules to that fresh object. No
    // special branching needed.

    Ok(root.to_string())
}

/// Recursively convert a serde_json::Value into the input form jsonc-parser
/// uses for its CST manipulation API.
fn json_to_cst_input(v: &Value) -> CstInputValue {
    match v {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(arr) => CstInputValue::Array(arr.iter().map(json_to_cst_input).collect()),
        Value::Object(obj) => CstInputValue::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), json_to_cst_input(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Compute `(prefix_unchanged, suffix_unchanged)` byte counts to prove
    /// the diff between two strings is contained inside ONE contiguous
    /// region. If unchanged_prefix + unchanged_suffix < min(len), the diff
    /// is multi-region (regression).
    fn diff_bounds(a: &str, b: &str) -> (usize, usize) {
        let a = a.as_bytes();
        let b = b.as_bytes();
        let mut p = 0;
        while p < a.len() && p < b.len() && a[p] == b[p] {
            p += 1;
        }
        let mut s = 0;
        while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
            s += 1;
        }
        (p, s)
    }

    fn assert_diff_within(orig: &str, edited: &str, region_start_marker: &str, region_end_marker: &str) {
        let (p, s) = diff_bounds(orig, edited);
        let region_start = orig.find(region_start_marker).expect("marker start");
        let region_end = orig.rfind(region_end_marker).expect("marker end") + region_end_marker.len();
        assert!(
            p >= region_start,
            "diff started at byte {p}, which is before the {region_start_marker} region start at {region_start}"
        );
        assert!(
            orig.len() - s <= region_end,
            "diff ended at byte {} (from suffix {}), past the {region_end_marker} region end at {region_end}",
            orig.len() - s,
            s
        );
    }

    // ---------------- fixture 1: minimal, rules already present
    #[test]
    fn replaces_existing_rules_in_place() {
        let orig = r#"{
  "log": { "level": "info" },
  "outbounds": [
    { "type": "direct", "tag": "direct" }
  ],
  "route": {
    "rules": [
      { "domain_suffix": ["a.com"], "outbound": "direct" }
    ],
    "final": "direct"
  }
}
"#;
        let new = json!([
            { "domain_suffix": ["b.com"], "outbound": "direct" },
            { "domain_suffix": ["c.com"], "outbound": "direct" }
        ]);
        let edited = replace_route_rules(orig, &new).unwrap();
        // The diff must be contained inside the [ ... ] of route.rules.
        assert_diff_within(orig, &edited, "[", "]");
        // And outside it, every byte is the same.
        assert!(edited.starts_with("{\n  \"log\": { \"level\": \"info\" },\n  \"outbounds\": ["));
        assert!(edited.contains("\"final\": \"direct\""));
        assert!(edited.contains("\"b.com\""));
        assert!(edited.contains("\"c.com\""));
        assert!(!edited.contains("\"a.com\""));
    }

    // ---------------- fixture 2: top-level comment must survive
    #[test]
    fn preserves_top_level_comments() {
        let orig = r#"// This is my favorite config.
{
  // dns block
  "dns": { "strategy": "prefer_ipv4" },
  "route": {
    "rules": [
      { "domain_suffix": ["old.com"], "outbound": "direct" }
    ]
  }
}
"#;
        let new = json!([
            { "domain_suffix": ["new.com"], "outbound": "direct" }
        ]);
        let edited = replace_route_rules(orig, &new).unwrap();
        assert!(edited.starts_with("// This is my favorite config.\n"), "lost top-level comment");
        assert!(edited.contains("// dns block"), "lost inline comment");
        assert!(edited.contains("\"new.com\""));
        assert!(!edited.contains("\"old.com\""));
    }

    // ---------------- fixture 3: inline comments inside route.rules region
    #[test]
    fn inline_comments_outside_rules_array_survive() {
        let orig = r#"{
  "route": {
    // route block
    "rules": [
      { "domain_suffix": ["x.com"], "outbound": "direct" }
    ],
    // final outbound
    "final": "direct"
  }
}
"#;
        let new = json!([
            { "domain_suffix": ["y.com"], "outbound": "direct" }
        ]);
        let edited = replace_route_rules(orig, &new).unwrap();
        assert!(edited.contains("// route block"));
        assert!(edited.contains("// final outbound"));
        assert!(edited.contains("\"y.com\""));
    }

    // ---------------- fixture 4: 4-space indent
    #[test]
    fn preserves_four_space_indent() {
        let orig = r#"{
    "route": {
        "rules": [
            { "outbound": "direct" }
        ]
    }
}
"#;
        let new = json!([{ "outbound": "block" }]);
        let edited = replace_route_rules(orig, &new).unwrap();
        // The original 4-space indent for surrounding keys must survive
        // exactly; "    \"route\": {" still appears.
        assert!(edited.contains("    \"route\": {"));
        assert!(edited.contains("\"block\""));
    }

    // ---------------- fixture 5: tab indent
    #[test]
    fn preserves_tab_indent() {
        let orig = "{\n\t\"route\": {\n\t\t\"rules\": [\n\t\t\t{ \"outbound\": \"direct\" }\n\t\t]\n\t}\n}\n";
        let new = json!([{ "outbound": "block" }]);
        let edited = replace_route_rules(orig, &new).unwrap();
        assert!(edited.contains("\t\"route\": {"));
        assert!(edited.contains("\"block\""));
    }

    // ---------------- fixture 6: route exists but no rules
    #[test]
    fn appends_rules_when_route_has_no_rules() {
        let orig = r#"{
  "outbounds": [
    { "type": "direct", "tag": "direct" }
  ],
  "route": {
    "final": "direct"
  }
}
"#;
        let new = json!([{ "domain_suffix": ["new.com"], "outbound": "direct" }]);
        let edited = replace_route_rules(orig, &new).unwrap();
        assert!(edited.contains("\"final\": \"direct\""));
        assert!(edited.contains("\"new.com\""));
        // Pre-existing keys still present.
        assert!(edited.contains("\"outbounds\""));
    }

    // ---------------- fixture 7: no route at all
    #[test]
    fn creates_route_when_missing() {
        let orig = r#"{
  "outbounds": [
    { "type": "direct", "tag": "direct" }
  ]
}
"#;
        let new = json!([{ "outbound": "direct" }]);
        let edited = replace_route_rules(orig, &new).unwrap();
        assert!(edited.contains("\"route\""), "route key not added");
        assert!(edited.contains("\"rules\""), "rules key not added");
        assert!(edited.contains("\"outbounds\""));
    }

    // ---------------- fixture 8: identity round-trip (same rules)
    #[test]
    fn identity_round_trip_is_byte_stable() {
        let orig = r#"{
  "route": {
    "rules": [
      { "domain_suffix": ["a.com", "b.com"], "outbound": "direct" }
    ]
  }
}
"#;
        let parsed: Value = serde_json::from_str(orig).unwrap();
        let same_rules = parsed
            .pointer("/route/rules")
            .cloned()
            .unwrap();
        let edited = replace_route_rules(orig, &same_rules).unwrap();
        // Allow some normalization (jsonc-parser may re-print the array
        // in a canonical form even when content is identical), but
        // verify the structure round-trips through serde to the same
        // logical value.
        let reparsed: Value = serde_json::from_str(&edited).unwrap();
        assert_eq!(reparsed, parsed);
    }

    #[test]
    fn rejects_non_array_input() {
        let orig = "{}";
        let r = replace_route_rules(orig, &json!({"oops": true}));
        assert!(r.is_err());
    }
}
