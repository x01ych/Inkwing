use rand::RngCore;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::util::port::pick_free_port;

/// Result of preparing a runtime config: the merged Value (user config +
/// our injected experimental.clash_api block) plus the parameters we need
/// to talk to it later.
pub struct InjectedConfig {
    pub merged: Value,
    pub addr: String,
    pub secret: String,
}

/// Force-overwrite `experimental.clash_api` on a copy of the user config so
/// the bearer secret is something we know. We do NOT trust whatever the
/// user might have put there — a stale secret would lock us out.
///
/// `external_ui` and `external_ui_download_url` are deliberately omitted:
/// we are the UI.
pub fn inject_clash_api(user_config: &Value) -> AppResult<InjectedConfig> {
    let port = pick_free_port()?;
    let addr = format!("127.0.0.1:{port}");
    let secret = random_hex_token();

    let mut merged = user_config.clone();

    // Ensure {"experimental": {"clash_api": {...}}} exists; MERGE rather
    // than replace — user may have legitimate fields like external_ui /
    // external_ui_download_url / access_control_allow_origin that we
    // shouldn't drop. We only force-overwrite the two fields that we have
    // to control (controller addr + bearer secret), and only set
    // default_mode if the user hasn't.
    let experimental = merged
        .as_object_mut()
        .ok_or_else(|| AppError::Config("config root is not an object".into()))?
        .entry("experimental".to_string())
        .or_insert_with(|| json!({}));
    let experimental_obj = experimental
        .as_object_mut()
        .ok_or_else(|| AppError::Config("experimental is not an object".into()))?;

    let clash_api_entry = experimental_obj
        .entry("clash_api".to_string())
        .or_insert_with(|| json!({}));
    let clash_api_obj = clash_api_entry
        .as_object_mut()
        .ok_or_else(|| AppError::Config("experimental.clash_api is not an object".into()))?;

    clash_api_obj.insert("external_controller".into(), json!(addr));
    clash_api_obj.insert("secret".into(), json!(secret));
    clash_api_obj
        .entry("default_mode".to_string())
        .or_insert_with(|| json!("rule"));

    Ok(InjectedConfig {
        merged,
        addr,
        secret,
    })
}

/// Force `experimental.cache_file.path` to an absolute path under our
/// data directory, so multiple GUI instances (or an orphan from a past
/// run that wasn't cleanly killed) don't fight over the same lock file
/// in the working directory. Without this, sing-box bails with
/// `start service: initialize cache-file: timeout` on macOS/Linux
/// whenever an earlier sing-box still holds the default ./cache.db.
///
/// Behaviour:
///   - if user set cache_file.enabled = false → no-op (they opted out).
///   - if user set their own cache_file.path  → respect it.
///   - otherwise                              → write absolute path.
pub fn apply_cache_file_overlay(merged: &mut Value, cache_path: &PathBuf) -> AppResult<()> {
    let root = merged
        .as_object_mut()
        .ok_or_else(|| AppError::Config("config root is not an object".into()))?;
    let experimental = root
        .entry("experimental".to_string())
        .or_insert_with(|| json!({}));
    let exp_obj = experimental
        .as_object_mut()
        .ok_or_else(|| AppError::Config("experimental is not an object".into()))?;
    let cache_file = exp_obj
        .entry("cache_file".to_string())
        .or_insert_with(|| json!({ "enabled": true }));
    let cf_obj = cache_file
        .as_object_mut()
        .ok_or_else(|| AppError::Config("experimental.cache_file is not an object".into()))?;
    // User explicitly disabled cache_file → leave it alone.
    if cf_obj.get("enabled") == Some(&json!(false)) {
        return Ok(());
    }
    if !cf_obj.contains_key("path") {
        cf_obj.insert(
            "path".into(),
            json!(cache_path.to_string_lossy().to_string()),
        );
    }
    Ok(())
}

/// Apply the runtime TUN-mode preference on top of an already
/// clash_api-injected config:
///   - `Some(true)`  → ensure exactly one TUN inbound exists (insert a
///     default one if none). Existing TUN inbounds are kept verbatim so
///     the user's tuning (interface name, address, mtu, auto_route…)
///     survives.
///   - `Some(false)` → strip every TUN inbound. Other inbounds untouched.
///   - `None`        → leave inbounds entirely alone (use whatever the
///     user wrote).
///
/// This mutates the runtime-only merged Value — it never touches the
/// user's source file.
pub fn apply_tun_overlay(merged: &mut Value, want_tun: Option<bool>) -> AppResult<()> {
    let Some(want) = want_tun else { return Ok(()) };
    let root = merged
        .as_object_mut()
        .ok_or_else(|| AppError::Config("config root is not an object".into()))?;
    let inbounds_v = root
        .entry("inbounds".to_string())
        .or_insert_with(|| json!([]));
    let inbounds = inbounds_v
        .as_array_mut()
        .ok_or_else(|| AppError::Config("inbounds is not an array".into()))?;

    if want {
        let already = inbounds
            .iter()
            .any(|i| i.get("type").and_then(|v| v.as_str()) == Some("tun"));
        if !already {
            inbounds.push(default_tun_inbound());
        }
    } else {
        inbounds.retain(|i| i.get("type").and_then(|v| v.as_str()) != Some("tun"));
    }
    Ok(())
}

/// macOS's `utun` kernel control interface only accepts TUN device names of
/// the form `utun<N>` (e.g. `utun3`). sing-box's Darwin backend rejects
/// anything else at bring-up with
/// `start inbound/tun: configure tun interface: bad tun name: <name>`,
/// which then trips our 30s clash_api readiness wait and fails the launch.
/// Our own default inbound uses the friendly `singbox0`, and user /
/// subscription configs routinely carry names like `singbox_tun` — all fine
/// on Linux/Windows, all fatal on macOS.
///
/// So, on macOS only, drop any TUN `interface_name` that isn't a valid
/// `utunN`; sing-box then auto-assigns the first free utun unit. A valid
/// `utunN` the user picked on purpose is preserved. No-op on Linux/Windows,
/// where arbitrary names are legal and the friendly name aids identification
/// and firewall rules.
///
/// Runs as the final step of config composition so it catches TUN inbounds
/// from every source — our default, the user's file, and overrides alike.
pub fn normalize_tun_interface_name(merged: &mut Value) {
    if cfg!(target_os = "macos") {
        strip_invalid_macos_tun_names(merged);
    }
}

/// Platform-agnostic worker behind [`normalize_tun_interface_name`] (split
/// out so it stays unit-testable off macOS): strip every TUN inbound's
/// `interface_name` unless it is already a valid `utunN`.
fn strip_invalid_macos_tun_names(merged: &mut Value) {
    let Some(inbounds) = merged.get_mut("inbounds").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for inbound in inbounds.iter_mut() {
        if inbound.get("type").and_then(|v| v.as_str()) != Some("tun") {
            continue;
        }
        let Some(obj) = inbound.as_object_mut() else {
            continue;
        };
        let valid = obj
            .get("interface_name")
            .and_then(|v| v.as_str())
            .is_some_and(is_valid_utun_name);
        if !valid {
            // Absent or invalid → remove so sing-box picks a free utun unit.
            obj.remove("interface_name");
        }
    }
}

/// Does `name` match macOS's required `utun<N>` TUN naming (`utun0`,
/// `utun7`, …)? Mirrors sing-box's Darwin `fmt.Sscanf(name, "utun%d")` gate,
/// but stricter: the whole string must be `utun` followed by one or more
/// decimal digits.
fn is_valid_utun_name(name: &str) -> bool {
    name.strip_prefix("utun")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Apply the user's chosen "proxy mode" (rule / global / direct) on top
/// of the merged runtime config.
///
/// **Why not clash_api PATCH /configs**: sing-box's clash_api accepts the
/// PATCH (returns 204) but its internal `mode-list` is hardcoded to
/// `["Rule"]` and the value isn't actually swapped — verified by curl
/// probe. Mihomo supports it; sing-box doesn't yet. We therefore
/// implement mode at inject time + a restart, same as the TUN overlay.
///
/// Semantics:
///   - `"rule"` → leave route.rules / route.final alone; user's existing
///      rules drive routing.
///   - `"direct"` → replace route.rules with `[]` and override
///      route.final = "direct". All traffic bypasses every proxy.
///   - `"global"` → ensure a `selector` outbound tagged `GLOBAL` exists
///      (auto-derived from existing proxy outbounds if missing), replace
///      route.rules with `[]`, override route.final = "GLOBAL". All
///      traffic goes through the GLOBAL selector's currently-picked node.
///
/// User's source file is never touched (we only edit the merged Value).
pub fn apply_mode_overlay(merged: &mut Value, mode: &str) -> AppResult<()> {
    let root = merged
        .as_object_mut()
        .ok_or_else(|| AppError::Config("config root is not an object".into()))?;

    match mode {
        "rule" => {
            // No-op: user's existing route.rules / route.final stand.
        }
        "direct" => {
            let route = root
                .entry("route".to_string())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| AppError::Config("route is not an object".into()))?;
            route.insert("rules".into(), json!([]));
            route.insert("final".into(), json!("direct"));
        }
        "global" => {
            ensure_global_selector(root)?;
            let route = root
                .entry("route".to_string())
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| AppError::Config("route is not an object".into()))?;
            route.insert("rules".into(), json!([]));
            route.insert("final".into(), json!("GLOBAL"));
        }
        other => {
            return Err(AppError::Config(format!(
                "unknown proxy mode '{other}' (expected rule/direct/global)"
            )));
        }
    }
    Ok(())
}

/// In global mode we route everything through a "GLOBAL" selector. If the
/// user's config already has one (any case-insensitive name match) we
/// re-use it; otherwise we synthesise one from every non-system outbound.
fn ensure_global_selector(root: &mut serde_json::Map<String, Value>) -> AppResult<()> {
    let outbounds = root
        .entry("outbounds".to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| AppError::Config("outbounds is not an array".into()))?;

    let already_has_global = outbounds.iter().any(|o| {
        o.get("tag").and_then(|t| t.as_str()) == Some("GLOBAL")
    });
    if already_has_global {
        return Ok(());
    }

    // Collect every "real proxy" outbound's tag — i.e. anything that
    // isn't direct / block / dns / one of the selector/group types
    // (selectors of other groups don't belong inside GLOBAL).
    const SYSTEM_TYPES: &[&str] = &["direct", "block", "dns"];
    const GROUP_TYPES: &[&str] = &["selector", "urltest", "fallback", "loadbalance"];
    let candidate_tags: Vec<String> = outbounds
        .iter()
        .filter_map(|o| {
            let kind = o.get("type").and_then(|v| v.as_str())?;
            if SYSTEM_TYPES.contains(&kind) || GROUP_TYPES.contains(&kind) {
                return None;
            }
            o.get("tag").and_then(|v| v.as_str()).map(String::from)
        })
        .collect();

    // Fallback: if there are no proxy outbounds (rare — direct-only
    // config), point GLOBAL at direct so the config still validates.
    let members: Vec<Value> = if candidate_tags.is_empty() {
        vec![json!("direct")]
    } else {
        candidate_tags.iter().map(|t| json!(t)).collect()
    };

    outbounds.push(json!({
        "type": "selector",
        "tag": "GLOBAL",
        "outbounds": members,
        "default": members[0],
    }));
    Ok(())
}

/// Runtime-overlay the user's mixed/socks/http inbounds on 127.0.0.1.
///
/// Semantics per protocol:
///   - `Some(p)` → ensure exactly one inbound of that type listens on
///     127.0.0.1:p (replace existing one of that type, or append).
///     Existing extra fields on a matching user inbound (auth, etc.) are
///     preserved verbatim — we only force `listen` and `listen_port`.
///   - `None`    → strip all inbounds of that type.
///
/// Same zero-loss principle as TUN: only the runtime config is touched.
pub fn apply_local_ports_overlay(
    merged: &mut Value,
    mixed: Option<u16>,
    socks: Option<u16>,
    http: Option<u16>,
) -> AppResult<()> {
    let root = merged
        .as_object_mut()
        .ok_or_else(|| AppError::Config("config root is not an object".into()))?;
    let inbounds_v = root
        .entry("inbounds".to_string())
        .or_insert_with(|| json!([]));
    let inbounds = inbounds_v
        .as_array_mut()
        .ok_or_else(|| AppError::Config("inbounds is not an array".into()))?;

    apply_one_local_port(inbounds, "mixed", "mixed-in", mixed);
    apply_one_local_port(inbounds, "socks", "socks-in", socks);
    apply_one_local_port(inbounds, "http", "http-in", http);
    Ok(())
}

fn apply_one_local_port(
    inbounds: &mut Vec<Value>,
    kind: &str,
    default_tag: &str,
    port: Option<u16>,
) {
    match port {
        None => {
            inbounds.retain(|i| i.get("type").and_then(|v| v.as_str()) != Some(kind));
        }
        Some(p) => {
            // Patch every user-supplied inbound of this type — the
            // semantic of the local-ports overlay is "I want this protocol
            // to listen on port P". Earlier code stopped at the first
            // match, which left a config with two `mixed` inbounds half
            // overridden (one on P, one on the user's original port).
            // Two inbounds on the same port collide at sing-box bring-up,
            // which is the right failure mode — the user is told to merge
            // them rather than silently getting an inconsistent setup.
            let mut patched = false;
            for item in inbounds.iter_mut() {
                if item.get("type").and_then(|v| v.as_str()) == Some(kind) {
                    if let Some(obj) = item.as_object_mut() {
                        obj.insert("listen".to_string(), json!("127.0.0.1"));
                        obj.insert("listen_port".to_string(), json!(p));
                        patched = true;
                    }
                }
            }
            if !patched {
                inbounds.push(json!({
                    "type": kind,
                    "tag": default_tag,
                    "listen": "127.0.0.1",
                    "listen_port": p,
                }));
            }
        }
    }
}

fn default_tun_inbound() -> Value {
    // Conservative defaults — auto_route on (so traffic actually goes
    // through), strict_route off (so we don't accidentally cut the user's
    // SSH session in dev), system stack (works on all platforms without
    // gVisor / mixed setup).
    json!({
        "type": "tun",
        "tag": "tun-in",
        "interface_name": "singbox0",
        "address": ["172.19.0.1/30"],
        "auto_route": true,
        "strict_route": false,
        "stack": "system"
    })
}

fn random_hex_token() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let mut s = String::with_capacity(64);
    for b in buf {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_into_empty_experimental() {
        let cfg = json!({"inbounds": [], "outbounds": []});
        let out = inject_clash_api(&cfg).unwrap();
        let api = out.merged.pointer("/experimental/clash_api").unwrap();
        assert_eq!(api["external_controller"].as_str().unwrap(), out.addr);
        assert_eq!(api["secret"].as_str().unwrap(), out.secret);
    }

    #[test]
    fn overwrites_existing_clash_api() {
        let cfg = json!({
            "experimental": {"clash_api": {"external_controller": "0.0.0.0:9999", "secret": "old"}}
        });
        let out = inject_clash_api(&cfg).unwrap();
        let api = out.merged.pointer("/experimental/clash_api").unwrap();
        assert_eq!(api["external_controller"].as_str().unwrap(), out.addr);
        assert_ne!(api["secret"].as_str().unwrap(), "old");
    }

    #[test]
    fn preserves_unrelated_clash_api_fields() {
        // Mirrors a realistic config where the user has set up an
        // external dashboard. We must not lose external_ui etc.
        let cfg = json!({
            "experimental": {
                "clash_api": {
                    "external_controller": "127.0.0.1:9090",
                    "secret": "user-secret",
                    "external_ui": "ui",
                    "external_ui_download_url": "https://example.com/ui.zip",
                    "external_ui_download_detour": "Default",
                    "default_mode": "global"
                }
            }
        });
        let out = inject_clash_api(&cfg).unwrap();
        let api = out.merged.pointer("/experimental/clash_api").unwrap();
        // Forced fields:
        assert_eq!(api["external_controller"].as_str().unwrap(), out.addr);
        assert_ne!(api["secret"].as_str().unwrap(), "user-secret");
        // Preserved fields:
        assert_eq!(api["external_ui"].as_str().unwrap(), "ui");
        assert_eq!(
            api["external_ui_download_url"].as_str().unwrap(),
            "https://example.com/ui.zip"
        );
        assert_eq!(api["external_ui_download_detour"].as_str().unwrap(), "Default");
        // default_mode preserved when user already set it (we do not stomp).
        assert_eq!(api["default_mode"].as_str().unwrap(), "global");
    }

    #[test]
    fn preserves_unrelated_experimental_fields() {
        let cfg = json!({"experimental": {"cache_file": {"enabled": true}}});
        let out = inject_clash_api(&cfg).unwrap();
        assert_eq!(
            out.merged.pointer("/experimental/cache_file/enabled").unwrap(),
            &json!(true)
        );
    }

    #[test]
    fn tun_overlay_inserts_when_wanted_and_absent() {
        let mut cfg = json!({
            "inbounds": [{"type": "mixed", "tag": "mx", "listen_port": 1080}]
        });
        apply_tun_overlay(&mut cfg, Some(true)).unwrap();
        let arr = cfg["inbounds"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().any(|i| i["type"] == "tun"));
        assert!(arr.iter().any(|i| i["type"] == "mixed"));
    }

    #[test]
    fn tun_overlay_keeps_user_tun_when_wanted_and_present() {
        let mut cfg = json!({
            "inbounds": [{"type": "tun", "tag": "my-tun", "mtu": 9000, "address": ["10.0.0.1/30"]}]
        });
        apply_tun_overlay(&mut cfg, Some(true)).unwrap();
        let arr = cfg["inbounds"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["mtu"], json!(9000));
        assert_eq!(arr[0]["tag"], "my-tun");
    }

    #[test]
    fn tun_overlay_strips_when_not_wanted() {
        let mut cfg = json!({
            "inbounds": [
                {"type": "tun", "tag": "tun-in"},
                {"type": "mixed", "tag": "mx"}
            ]
        });
        apply_tun_overlay(&mut cfg, Some(false)).unwrap();
        let arr = cfg["inbounds"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "mixed");
    }

    #[test]
    fn tun_overlay_none_means_no_change() {
        let mut cfg = json!({
            "inbounds": [{"type": "tun", "tag": "tun-in"}]
        });
        apply_tun_overlay(&mut cfg, None).unwrap();
        let arr = cfg["inbounds"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn local_ports_overlay_inserts_when_absent() {
        let mut cfg = json!({"inbounds": []});
        apply_local_ports_overlay(&mut cfg, Some(7890), None, Some(8080)).unwrap();
        let arr = cfg["inbounds"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().any(|i| i["type"] == "mixed" && i["listen_port"] == 7890));
        assert!(arr.iter().any(|i| i["type"] == "http" && i["listen_port"] == 8080));
        assert!(!arr.iter().any(|i| i["type"] == "socks"));
    }

    #[test]
    fn local_ports_overlay_patches_existing_preserves_extras() {
        let mut cfg = json!({
            "inbounds": [{
                "type": "mixed", "tag": "user-mixed", "listen": "0.0.0.0",
                "listen_port": 1080, "users": [{"username": "x", "password": "y"}]
            }]
        });
        apply_local_ports_overlay(&mut cfg, Some(7890), None, None).unwrap();
        let arr = cfg["inbounds"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let mx = &arr[0];
        assert_eq!(mx["listen_port"], 7890);
        assert_eq!(mx["listen"], "127.0.0.1"); // overlay forces loopback
        assert_eq!(mx["tag"], "user-mixed"); // preserved
        assert!(mx["users"].is_array()); // preserved
    }

    #[test]
    fn local_ports_overlay_patches_all_matching_inbounds() {
        // User has two `mixed` inbounds (legal — different ports). The
        // overlay should patch both, not just the first; a port collision
        // at sing-box bring-up is the right failure mode (forces the
        // user to merge), not a half-applied silent override.
        let mut cfg = json!({
            "inbounds": [
                {"type": "mixed", "tag": "mx-a", "listen_port": 7890, "users": []},
                {"type": "mixed", "tag": "mx-b", "listen_port": 7891}
            ]
        });
        apply_local_ports_overlay(&mut cfg, Some(9000), None, None).unwrap();
        let arr = cfg["inbounds"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        for inb in arr {
            assert_eq!(inb["type"], "mixed");
            assert_eq!(inb["listen"], "127.0.0.1");
            assert_eq!(inb["listen_port"], 9000);
        }
        // Extra fields preserved on the first one.
        assert!(arr[0].get("users").is_some());
    }

    #[test]
    fn local_ports_overlay_strips_when_none() {
        let mut cfg = json!({
            "inbounds": [
                {"type": "mixed", "tag": "mx", "listen_port": 7890},
                {"type": "tun", "tag": "tun-in"}
            ]
        });
        apply_local_ports_overlay(&mut cfg, None, None, None).unwrap();
        let arr = cfg["inbounds"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "tun");
    }

    #[test]
    fn mode_overlay_rule_is_noop() {
        let original = json!({
            "outbounds": [{"type":"shadowsocks","tag":"ss-1"},{"type":"direct","tag":"direct"}],
            "route": {"rules": [{"domain": ["x.com"], "outbound": "ss-1"}], "final": "ss-1"}
        });
        let mut cfg = original.clone();
        apply_mode_overlay(&mut cfg, "rule").unwrap();
        assert_eq!(cfg, original);
    }

    #[test]
    fn mode_overlay_direct_clears_rules_and_sets_final() {
        let mut cfg = json!({
            "outbounds": [{"type":"shadowsocks","tag":"ss-1"},{"type":"direct","tag":"direct"}],
            "route": {"rules": [{"domain": ["x.com"], "outbound": "ss-1"}], "final": "ss-1"}
        });
        apply_mode_overlay(&mut cfg, "direct").unwrap();
        assert_eq!(cfg["route"]["rules"], json!([]));
        assert_eq!(cfg["route"]["final"], "direct");
    }

    #[test]
    fn mode_overlay_global_synthesises_global_selector() {
        let mut cfg = json!({
            "outbounds": [
                {"type":"shadowsocks","tag":"ss-jp"},
                {"type":"shadowsocks","tag":"ss-us"},
                {"type":"direct","tag":"direct"}
            ],
            "route": {"rules": [], "final": "direct"}
        });
        apply_mode_overlay(&mut cfg, "global").unwrap();
        let global = cfg["outbounds"].as_array().unwrap().iter()
            .find(|o| o["tag"] == "GLOBAL").expect("GLOBAL synthesised");
        assert_eq!(global["type"], "selector");
        let members = global["outbounds"].as_array().unwrap();
        assert!(members.iter().any(|m| m == "ss-jp"));
        assert!(members.iter().any(|m| m == "ss-us"));
        assert!(!members.iter().any(|m| m == "direct"));  // system type filtered
        assert_eq!(cfg["route"]["final"], "GLOBAL");
    }

    #[test]
    fn mode_overlay_global_reuses_existing_global_selector() {
        let mut cfg = json!({
            "outbounds": [
                {"type":"selector","tag":"GLOBAL","outbounds":["a","b"],"default":"a"},
                {"type":"shadowsocks","tag":"a"},
                {"type":"shadowsocks","tag":"b"}
            ]
        });
        apply_mode_overlay(&mut cfg, "global").unwrap();
        let globals: Vec<_> = cfg["outbounds"].as_array().unwrap().iter()
            .filter(|o| o["tag"] == "GLOBAL").collect();
        assert_eq!(globals.len(), 1);  // didn't create a duplicate
        assert_eq!(globals[0]["outbounds"], json!(["a","b"]));  // user's config preserved
    }

    #[test]
    fn mode_overlay_unknown_mode_errors() {
        let mut cfg = json!({"outbounds": []});
        assert!(apply_mode_overlay(&mut cfg, "weird").is_err());
    }

    #[test]
    fn token_is_64_hex_chars() {
        let t = random_hex_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn utun_name_validity() {
        // Valid: `utun` + one or more digits.
        for ok in ["utun0", "utun7", "utun123"] {
            assert!(is_valid_utun_name(ok), "{ok} should be valid");
        }
        // Invalid: our old default, the bug-report name, and edge cases.
        for bad in ["singbox0", "singbox_tun", "tun0", "utun", "utunX", "utun1a", ""] {
            assert!(!is_valid_utun_name(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn strips_invalid_tun_name_but_preserves_other_fields() {
        let mut cfg = json!({
            "inbounds": [{
                "type": "tun",
                "tag": "tun-in",
                "interface_name": "singbox0",
                "address": ["172.19.0.1/30"],
                "auto_route": true,
                "stack": "system"
            }]
        });
        strip_invalid_macos_tun_names(&mut cfg);
        let inb = &cfg["inbounds"][0];
        assert!(inb.get("interface_name").is_none()); // stripped → auto-assign
        assert_eq!(inb["tag"], "tun-in"); // everything else verbatim
        assert_eq!(inb["address"], json!(["172.19.0.1/30"]));
        assert_eq!(inb["auto_route"], json!(true));
        assert_eq!(inb["stack"], "system");
    }

    #[test]
    fn strips_user_singbox_tun_name() {
        // The exact name from the macOS bug report.
        let mut cfg = json!({
            "inbounds": [{"type": "tun", "interface_name": "singbox_tun"}]
        });
        strip_invalid_macos_tun_names(&mut cfg);
        assert!(cfg["inbounds"][0].get("interface_name").is_none());
    }

    #[test]
    fn keeps_valid_utun_name() {
        let mut cfg = json!({
            "inbounds": [{"type": "tun", "interface_name": "utun9"}]
        });
        strip_invalid_macos_tun_names(&mut cfg);
        assert_eq!(cfg["inbounds"][0]["interface_name"], "utun9");
    }

    #[test]
    fn leaves_non_tun_inbounds_alone() {
        // interface_name is meaningless on a mixed inbound; don't touch it.
        let mut cfg = json!({
            "inbounds": [{"type": "mixed", "interface_name": "whatever"}]
        });
        strip_invalid_macos_tun_names(&mut cfg);
        assert_eq!(cfg["inbounds"][0]["interface_name"], "whatever");
    }

    #[test]
    fn normalize_tolerates_missing_or_nonarray_inbounds() {
        let mut no_inbounds = json!({"outbounds": []});
        strip_invalid_macos_tun_names(&mut no_inbounds); // must not panic
        assert!(no_inbounds.get("inbounds").is_none());

        let mut weird = json!({"inbounds": "not-an-array"});
        strip_invalid_macos_tun_names(&mut weird); // must not panic
        assert_eq!(weird["inbounds"], "not-an-array");
    }
}
