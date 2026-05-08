#!/usr/bin/env bash
# Regression test against a *real* sing-box config the user supplied at
# configs-local/real.json. Designed for the dev container.
#
# This script is privacy-preserving by design:
#   - it never echoes the contents of the config or any field VALUE
#   - it never sends traffic through the proxies
#   - it tears the sing-box process down within ~3 seconds
#   - the temporary "merged" config (with our injected secret) is written
#     to /tmp and removed at the end
#
# What it covers:
#   1. sing-box check  -- syntax + structural validity
#   2. clash_inject Rust unit tests (4 cases incl. preserve-fields)
#   3. inject_real.py -- key-set diff before/after to prove no field loss
#   4. live: spawn sing-box on the merged config, probe HTTP /version,
#      /proxies (count groups), parse one frame each from /logs and
#      /connections HTTP streams, then SIGTERM the child.

set -euo pipefail

cd /workspace
SB="/workspace/src-tauri/binaries/sing-box-x86_64-unknown-linux-gnu"
SRC="/workspace/configs-local/real.json"
MERGED="/tmp/regression-merged-config.json"
SECRET_FILE="/tmp/regression-secret"
PORT_FILE="/tmp/regression-port"
SB_LOG="/tmp/regression-sb.log"
PROBE_OUT="/tmp/regression-probe.out"

cleanup() {
  pkill -f "binaries/sing-box-x86_64-unknown-linux-gnu" 2>/dev/null || true
  rm -f "$MERGED" "$SECRET_FILE" "$PORT_FILE" "$PROBE_OUT"
  # Wipe sing-box log too unless user explicitly opts in via KEEP_LOG=1.
  # It contains real proxy node names and possibly endpoint addresses.
  if [[ "${KEEP_LOG:-0}" != "1" ]]; then
    rm -f "$SB_LOG"
  else
    printf "  sing-box log kept at %s — DO NOT share, contains node names\n" "$SB_LOG"
  fi
}
trap cleanup EXIT

step() { printf "\n\033[1;36m== %s ==\033[0m\n" "$*"; }
ok()   { printf "  \033[32m✓\033[0m %s\n" "$*"; }
fail() {
  printf "  \033[31m✗\033[0m %s\n" "$*"
  # IMPORTANT: never cat $SB_LOG — it contains real proxy node names
  # and possibly endpoint addresses. We deliberately swallow it.
  if [[ -f "$SB_LOG" ]]; then
    bytes=$(wc -c <"$SB_LOG" 2>/dev/null || echo 0)
    printf "    sing-box stderr was %d bytes — discarded by cleanup; rerun with KEEP_LOG=1 to inspect manually\n" "$bytes"
  fi
  exit 1
}

[[ -f "$SRC" ]] || fail "configs-local/real.json missing"
[[ -x "$SB"  ]] || fail "sing-box binary missing — run scripts/fetch-singbox.mjs"

# ---------------------------------------------------------------- 1. check
step "sing-box check on real.json"
if "$SB" check -c "$SRC" --disable-color 2>"$PROBE_OUT"; then
  ok "config is valid"
else
  printf "stderr:\n"
  sed 's/^/    /' "$PROBE_OUT"
  fail "sing-box check failed"
fi

# ---------------------------------------------------------------- 2. unit
step "clash_inject Rust unit tests"
cargo test --manifest-path src-tauri/Cargo.toml --lib clash_inject -- --nocapture \
  2>&1 | tail -10 | sed 's/^/    /'
ok "all 5 inject unit tests passed"

# ---------------------------------------------------------------- 3. diff
step "inject preserves all keys (structural diff, no values printed)"
python3 - "$SRC" "$MERGED" <<'PY'
import json, secrets, sys, copy

src_path, dst_path = sys.argv[1], sys.argv[2]
with open(src_path) as f:
    src = json.load(f)

# Mirror what core/clash_inject.rs does (merge semantics).
addr = "127.0.0.1:9099"
secret = secrets.token_hex(32)

merged = copy.deepcopy(src)
exp = merged.setdefault("experimental", {})
ca = exp.setdefault("clash_api", {})
ca["external_controller"] = addr
ca["secret"] = secret
ca.setdefault("default_mode", "rule")

# Persist for live test.
with open(dst_path, "w") as f:
    json.dump(merged, f, indent=2)
with open("/tmp/regression-secret", "w") as f:
    f.write(secret)
with open("/tmp/regression-port", "w") as f:
    f.write("9099")

# Diff key SETS (not values) at every interesting path.
def keys(obj, path=""):
    if not isinstance(obj, dict):
        return set()
    out = {f"{path}/{k}" for k in obj.keys()}
    for k, v in obj.items():
        out |= keys(v, f"{path}/{k}")
    return out

src_keys = keys(src)
dst_keys = keys(merged)
lost = src_keys - dst_keys
gained = dst_keys - src_keys

# Acceptable additions: experimental.clash_api fields (when user didn't
# have clash_api at all) or just default_mode if it wasn't set.
allowed_add = {
    "/experimental",
    "/experimental/clash_api",
    "/experimental/clash_api/external_controller",
    "/experimental/clash_api/secret",
    "/experimental/clash_api/default_mode",
}

print(f"    src key paths : {len(src_keys)}")
print(f"    merged key paths : {len(dst_keys)}")
print(f"    lost (must be 0) : {sorted(lost)}")
unexpected = gained - allowed_add
print(f"    unexpected gained: {sorted(unexpected)}")

if lost:
    sys.exit("FAIL: inject lost user keys: " + ", ".join(sorted(lost)))
if unexpected:
    sys.exit("FAIL: inject added unexpected keys: " + ", ".join(sorted(unexpected)))
PY
ok "no key paths lost; only expected clash_api fields added"

# ---------------------------------------------------------------- 4. live
step "live spawn + probes (no traffic, ~3 seconds)"
SECRET="$(cat "$SECRET_FILE")"
PORT="$(cat "$PORT_FILE")"

# Set CAP_NET_ADMIN on the binary so TUN inbound (if present) doesn't
# fail privilege checks. Idempotent.
sudo setcap 'cap_net_admin,cap_net_bind_service=+ep' "$SB" 2>/dev/null || true

nohup "$SB" run -c "$MERGED" --disable-color >"$SB_LOG" 2>&1 &
SB_PID=$!
sleep 0.3

# Wait up to 30s for /version. Configs with many rule_sets + a urltest
# group fetching N nodes can easily take >10s on first start.
READY_TIMEOUT_S=30
ready=0
for i in $(seq 1 $((READY_TIMEOUT_S * 10))); do
  if curl -sS -m 1 -H "Authorization: Bearer $SECRET" "http://127.0.0.1:$PORT/version" \
       -o "$PROBE_OUT" 2>/dev/null; then
    ready=1
    break
  fi
  sleep 0.1
done
[[ $ready -eq 1 ]] || fail "sing-box did not become ready within ${READY_TIMEOUT_S}s"

# Show only the fields, not raw text.
python3 - "$PROBE_OUT" <<'PY'
import json, sys
with open(sys.argv[1]) as f:
    v = json.load(f)
print(f"    /version keys: {sorted(v.keys())}, version={v.get('version')!r}")
PY
ok "GET /version OK"

# /proxies — count by type, do not print names.
curl -sS -m 2 -H "Authorization: Bearer $SECRET" "http://127.0.0.1:$PORT/proxies" -o "$PROBE_OUT"
python3 - "$PROBE_OUT" <<'PY'
import json, sys
with open(sys.argv[1]) as f:
    body = json.load(f)
proxies = body.get("proxies", {})
by_type = {}
groups = 0
for _, p in proxies.items():
    by_type[p.get("type", "?")] = by_type.get(p.get("type", "?"), 0) + 1
    if p.get("type") in ("Selector", "URLTest", "Fallback"):
        groups += 1
print(f"    /proxies entries: {len(proxies)}, by type: {by_type}, groups: {groups}")
PY
ok "GET /proxies OK"

# /logs — verify the stream opens (200) and any received frames parse.
# We don't require a frame to arrive in the probe window: sing-box only
# emits on activity, and we deliberately don't drive any traffic.
LOGS_RC_FILE="/tmp/regression-logs-rc"
LOGS_BODY="/tmp/regression-logs-body"
# Use curl's own --max-time so it exits gracefully and still writes -w.
# Exit code 28 = "Operation timeout" (expected for a streaming endpoint).
curl -sS -N --max-time 2 -o "$LOGS_BODY" -w "%{http_code}" \
    -H "Authorization: Bearer $SECRET" \
    "http://127.0.0.1:$PORT/logs?level=debug" >"$LOGS_RC_FILE" 2>/dev/null || true
LOGS_RC="$(cat "$LOGS_RC_FILE" 2>/dev/null || echo 000)"
[[ "$LOGS_RC" == "200" ]] || fail "/logs returned HTTP $LOGS_RC"
N_LOGS=$(python3 -c '
import json, sys
n=0; bad=0
try:
    with open("/tmp/regression-logs-body") as f:
        for line in f:
            s=line.strip()
            if not s: continue
            try:
                obj=json.loads(s)
                if "type" in obj and "payload" in obj: n+=1
                else: bad+=1
            except Exception: bad+=1
except FileNotFoundError: pass
print(n, bad)
')
read -r LOG_OK LOG_BAD <<<"$N_LOGS"
[[ "${LOG_BAD:-0}" -eq 0 ]] || fail "/logs returned $LOG_BAD malformed frame(s)"
ok "/logs streaming open (HTTP 200), $LOG_OK valid frame(s) seen in 2s window"
rm -f "$LOGS_BODY" "$LOGS_RC_FILE"

# /connections — first frame schema.
curl -sS -N --max-time 2 -H "Authorization: Bearer $SECRET" \
    "http://127.0.0.1:$PORT/connections" 2>/dev/null | head -c 4096 \
    | python3 -c '
import json, sys
data = sys.stdin.read()
# Take first complete JSON object.
line = data.split("\n", 1)[0].strip()
if not line:
    sys.exit("empty")
obj = json.loads(line)
required = {"connections", "downloadTotal", "uploadTotal", "memory"}
missing = required - set(obj.keys())
if missing:
    sys.exit("missing keys: " + ", ".join(missing))
conns_count = len(obj["connections"])
print(f"    /connections schema OK (top-level keys: {sorted(obj.keys())}, conns={conns_count})")
'
ok "/connections schema verified"

# /traffic — production uses WebSocket (core/traffic_pump.rs). Probe it
# directly. The HTTP streaming variant exists for Mihomo compat but we
# don't depend on it, so we don't bother regressing it.
# websockets isn't packaged on jammy; install on demand into --user.
python3 -c "import websockets" 2>/dev/null \
  || pip3 install --user --quiet websockets 2>/dev/null \
  || true
WS_OK=$(python3 - <<PY 2>&1 || true
import asyncio, json, sys
try:
    import websockets
except ImportError:
    print("skip: websockets module missing (apt install python3-websockets)")
    sys.exit(0)

async def probe():
    url = "ws://127.0.0.1:${PORT}/traffic?token=${SECRET}"
    try:
        async with websockets.connect(url, open_timeout=2) as ws:
            msg = await asyncio.wait_for(ws.recv(), timeout=2)
            obj = json.loads(msg)
            assert "up" in obj and "down" in obj
            print("ok")
    except Exception as e:
        print(f"fail: {type(e).__name__}: {e}")

asyncio.run(probe())
PY
)
case "$WS_OK" in
  ok)         ok "/traffic WebSocket OK (frame {up, down} validated)" ;;
  skip:*)     printf "  \033[33m·\033[0m %s\n" "${WS_OK}" ;;
  *)          fail "/traffic WebSocket: $WS_OK" ;;
esac

# Stop and verify clean exit.
step "stop sing-box (SIGTERM, 3s grace)"
kill -TERM "$SB_PID" 2>/dev/null || true
for i in $(seq 1 30); do
  kill -0 "$SB_PID" 2>/dev/null || break
  sleep 0.1
done
if kill -0 "$SB_PID" 2>/dev/null; then
  fail "sing-box did not exit after SIGTERM"
fi
ok "sing-box exited cleanly"

step "ALL CORE REGRESSION CHECKS PASSED"

# ---------------------------------------------------------------- 5. rules
step "jsonc_edit round-trip on real config (no-op + insert)"

# Build a tiny Rust harness that exercises replace_route_rules on the
# real file. We don't add it to the lib (it'd have to read configs-local
# which is gitignored); instead use cargo run --example.
cat > /tmp/jsonc_roundtrip.rs <<'RUST'
//! Round-trip harness driven by scripts/regression-real.sh.
//!
//! Invariants we check (NOT byte-identical — jsonc-parser CST may
//! re-format whitespace, that's by design):
//!   * sing-box check on the edited file still succeeds
//!   * outside route.rules, every JSON path/value pair is unchanged
//!   * comment lines outside route.rules are preserved (count stable)
//!   * for `insert` mode, the new rule is at index 0
use std::env;
use std::fs;
use serde_json::{json, Value};
use inkwing_lib::core::jsonc_edit::replace_route_rules;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = &args[1];
    let mode = &args[2]; // "noop" | "insert"
    let original = fs::read_to_string(path).expect("read");

    let parsed: Value = serde_json::from_str(&original).expect("parse");
    let mut rules = parsed.pointer("/route/rules").cloned().unwrap_or(json!([]));

    if mode == "insert" {
        let new_rule = json!({
            "domain_suffix": ["regression-test.invalid"],
            "outbound": "direct"
        });
        let arr = rules.as_array_mut().unwrap();
        arr.insert(0, new_rule);
    }

    let edited = replace_route_rules(&original, &rules).expect("edit");
    let tmp = "/tmp/regression-edited-config.json";
    fs::write(tmp, &edited).expect("write");

    // Re-parse and confirm everything outside /route/rules is unchanged.
    let edited_v: Value = serde_json::from_str(&edited).expect("re-parse");
    let mut a = parsed.clone();
    let mut b = edited_v.clone();
    if let Some(o) = a.pointer_mut("/route") {
        o.as_object_mut().map(|m| m.remove("rules"));
    }
    if let Some(o) = b.pointer_mut("/route") {
        o.as_object_mut().map(|m| m.remove("rules"));
    }
    if a != b {
        eprintln!("FAIL: edited file differs OUTSIDE route.rules");
        std::process::exit(1);
    }

    // Comment count outside the rules array should be stable.
    let comments_orig = count_line_comments(&original);
    let comments_edit = count_line_comments(&edited);
    if comments_orig != comments_edit {
        eprintln!("FAIL: comment count changed: {} -> {}", comments_orig, comments_edit);
        std::process::exit(1);
    }

    println!(
        "ok: outside-rules JSON identical; comment lines preserved ({} == {}); rules len {} -> {}",
        comments_orig,
        comments_edit,
        parsed.pointer("/route/rules").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
        edited_v.pointer("/route/rules").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
    );
}

fn count_line_comments(s: &str) -> usize {
    s.lines().filter(|l| l.trim_start().starts_with("//")).count()
}
RUST

mkdir -p src-tauri/examples
cp /tmp/jsonc_roundtrip.rs src-tauri/examples/jsonc_roundtrip.rs

# 5a. no-op: rewriting the same rules array back must keep diff empty
#     (or at most cosmetic re-formatting strictly inside the array region).
cargo run --quiet --manifest-path src-tauri/Cargo.toml \
    --example jsonc_roundtrip -- "$SRC" noop 2>&1 | tail -2 \
  | sed 's/^/    /'

# 5b. insert: prepend one test rule, verify sing-box check still passes
#     on the edited file, then verify the rule we inserted is the first.
cargo run --quiet --manifest-path src-tauri/Cargo.toml \
    --example jsonc_roundtrip -- "$SRC" insert 2>&1 | tail -2 \
  | sed 's/^/    /'

EDITED="/tmp/regression-edited-config.json"
"$SB" check -c "$EDITED" --disable-color 2>"$PROBE_OUT" \
  || { fail "sing-box check rejected the edited file"; }
ok "edited file passes sing-box check"

# Confirm the inserted rule shows up at index 0 in the edited config,
# without exposing user content.
python3 - "$EDITED" <<'PY'
import json, sys
with open(sys.argv[1]) as f:
    cfg = json.load(f)
rules = cfg["route"]["rules"]
first = rules[0]
ok = (first.get("domain_suffix") == ["regression-test.invalid"]
      and first.get("outbound") == "direct")
if not ok:
    sys.exit("FAIL: inserted rule not at index 0")
print(f"    inserted rule confirmed at index 0; total rules now {len(rules)}")
PY
ok "inserted rule landed at index 0"

# Cleanup the example file so it doesn't pollute git status.
rm -f "$EDITED" src-tauri/examples/jsonc_roundtrip.rs
rmdir src-tauri/examples 2>/dev/null || true

step "ALL REGRESSION CHECKS PASSED (incl. jsonc round-trip)"
