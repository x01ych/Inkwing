# Overrides layer

## Why

The product promise from day one is **zero-loss config round-trip** — the
user's source `.json` is never lossy-translated by this app. Loading
goes through jsonc-parser's CST (comments, indentation, key order
preserved); editing follows the same contract by routing every GUI
mutation into a separate overrides store rather than touching the
source file.

User-driven mutations (add a route rule, edit a DNS server, mask a
config rule, etc.) live in a separate **overrides** persistence layer.
At sing-box launch time, source ⊕ overrides → runtime config. sing-box
reads only the runtime copy.

## Storage layout

```
<data_dir>/
├── configs/<entry_id>.json         # source — read-only from rule editors
├── overrides/
│   ├── <entry_id>.json             # per-config overrides (one file each)
│   └── global.json                 # global "appended" overrides
└── runtime/config.json             # built fresh on every core_start
```

`<data_dir>` is platform-specific via the `directories` crate
(`ProjectDirs::from("dev", "inkwing", "Inkwing")`):

| Platform | Path |
|---|---|
| Linux | `~/.local/share/inkwing/` |
| macOS | `~/Library/Application Support/dev.inkwing.Inkwing/` |
| Windows | `%APPDATA%\inkwing\Inkwing\data\` |

## Schema

### Per-config (`overrides/<entry_id>.json`)

```jsonc
{
  "version": 1,
  "route_rules":     ArrayOverrides,
  "route_rule_set":  ArrayOverrides,
  "dns_servers":     ArrayOverrides,
  "dns_rules":       ArrayOverrides
}

// ArrayOverrides
{
  // local entries the user added (per-config-scoped). Order is
  // user-controlled; merge appends them after the source array.
  "appended": [
    { "id": "<uuid v4>", "value": <rule JSON>, "created_at_ms": 0 }
  ],
  // edits to source rules. key = SHA-256 of the canonical source rule.
  // value replaces the source rule at merge time (source still unchanged).
  "modifications": {
    "<sha256>": {
      "override_value": <rule JSON>,
      "original_signature_preview": "<first 16 hex chars>",
      "modified_at_ms": 0
    }
  },
  // signatures of source rules to skip at merge time.
  "masked": ["<sha256>", ...]
}
```

### Global (`overrides/global.json`)

```jsonc
{
  "version": 1,
  "route_rules":    [LocalEntry, ...],
  "route_rule_set": [LocalEntry, ...],
  "dns_servers":    [LocalEntry, ...],
  "dns_rules":      [LocalEntry, ...]
}
```

Global is *appended-only*. Modifications and masks must be per-config
because they reference a specific source rule.

## Identity: signature

Source rules are identified by `SHA-256(canonical_json(rule))`, where
canonical = object keys sorted alphabetically (recursive), arrays kept
in order, scalars re-encoded as `serde_json::Number::to_string()` and
strings escaped via `serde_json`. See `core/overrides.rs::signature`.

Properties:

- **stable**: identical rules hash the same regardless of key order in
  the source (e.g. JSONC vs autoformatted JSON)
- **decoupling**: if the user externally edits a source rule, its
  signature changes → the modification/mask attached to it no longer
  matches → it goes "stale". The current build doesn't surface stale
  overrides in the UI yet.

## ID convention over IPC

Every command takes `id: String` for source vs. local rules:

- **Source rule**: `id` = signature (SHA-256 hex, 64 chars, **no `-`**)
- **Local rule**: `id` = UUID v4 (36 chars, **with `-`**)

Backend disambiguates by `!id.contains('-')`. Adding new local rule
returns its UUID; the rest of the workflow uses that UUID until delete.

## Merge order

```
runtime[i] for i in path P =  source[P]   (skipping masked, replacing modifications)
                            ++ per_config[P].appended
                            ++ global[P].appended
```

Applied at `core_start` immediately before writing
`<data_dir>/runtime/config.json`. See
`commands::core_cmd::core_start` and
`core::overrides::apply_overrides_overlay`.

## Affected paths

| Path | Backend | Frontend |
|---|---|---|
| `/route/rules` | `commands/rules_cmd.rs` | `pages/Rules.tsx` |
| `/route/rule_set` | `commands/rules_cmd.rs` | `pages/Rules.tsx` |
| `/dns/servers` | `commands/dns_cmd.rs` | `pages/Dns.tsx` |
| `/dns/rules` | `commands/dns_cmd.rs` | `pages/Dns.tsx` |

If `/dns` doesn't exist in the source config but a global override adds
DNS servers, the merge step synthesises the parent object. See
`apply_one()` in `core/overrides.rs`.

## Adding a new managed array

To extend overrides to e.g. `/route/final` or another sing-box config
slice:

1. Add a new field to `LocalOverrides` and `GlobalOverrides` in
   `core/overrides.rs` (with `#[serde(default)]` for back-compat).
2. Add a corresponding `apply_one(...)` call in
   `apply_overrides_overlay`.
3. Add backend command file mirroring `commands/rules_cmd.rs` —
   `rules_list/add/update/delete/mask/unmask/revert/reorder/commit`.
4. Add frontend api + page mirroring `api/rules.ts` + `pages/Rules.tsx`.
5. Register commands in `lib.rs::invoke_handler`.

The shape is mostly mechanical; the only thinking-required piece is
the `*View / *Input / *_to_view / input_to_value` helpers in
`core/{rules,dns}.rs` — those decide which fields are editable
(`editable: bool`) and what gets surfaced vs. round-tripped as raw.

## What's intentionally NOT done

- **Stale modification UI**: when a user externally edits a source
  rule, the corresponding modification's signature key no longer
  matches anything in source. The override file still holds the entry;
  it's effectively dead. We don't surface or auto-clean it yet.
- **Cross-scope reorder**: dragging a rule from local-per to local-global
  isn't supported. The user has to delete + re-add with the new scope.
- **Logical rule editing**: source rules with `type: "logical"` (and / or
  nested) render read-only; the GUI doesn't compose them.
- **Apply overrides back to source file**: there's no "flatten this
  override into my source.json" button. Overrides and source live
  apart by design. If you want this, write the merged value to the
  storage_path via Monaco "Save raw" (which still uses
  `core/jsonc_edit.rs`).
