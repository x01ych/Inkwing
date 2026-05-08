<div align="center">
  <img src="src-tauri/icons/icon.png" alt="Inkwing" width="120" height="120">

  <h1>Inkwing</h1>

  <p>
    <strong>A desktop client for sing-box that runs your existing config without lossy translation</strong>
  </p>

  <p>
    <a href="#overview">📖 Overview</a> •
    <a href="#features">✨ Features</a> •
    <a href="#how-it-works">🏗️ How it works</a> •
    <a href="#quick-start">🚀 Quick start</a> •
    <a href="#development">🛠️ Development</a> •
    <a href="#documentation">📚 Documentation</a>
  </p>

  <p>
    <a href="README.md">🇺🇸 English</a> •
    <a href="README.zh.md">🇨🇳 中文</a>
  </p>

  <p>
    <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue?style=for-the-badge&logo=tauri" alt="Platform">
    <img src="https://img.shields.io/badge/stack-Tauri%202%20%2B%20React%20%2B%20shadcn-purple?style=for-the-badge" alt="Stack">
    <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  </p>
</div>

---

## Overview

Inkwing is a desktop client built around the [sing-box](https://github.com/SagerNet/sing-box) core. It loads your existing sing-box JSON configuration, runs the kernel for you, and gives you a visual surface for routing rules, DNS, proxies, connections, and logs.

**Inkwing does not generate configs.** There is no step-by-step config wizard. You arrive with a complete sing-box JSON in hand — written manually or generated upstream by [sub-store](https://github.com/sub-store-org/Sub-Store), your subscription provider, or any sing-box-aware tool — and Inkwing takes it from there.

The original sing-box file you import is **never modified**. Every edit you make in the GUI lives in a separate overrides layer; at launch time Inkwing merges *source ⊕ overrides* into a runtime config and feeds that to sing-box. This keeps your hand-curated configs (comments, key order, fields Inkwing doesn't recognise) intact for the lifetime of the project.

## Features

- **Multi-config library**: Add local files, paste raw JSON, or fetch from a subscription URL. Switch between them; sing-box auto-restarts.
- **Route editor**: View / add / mask / modify the merged `route.rules` and `route.rule_set` lists. Edits to source rules are demoted to local overrides; revertable.
- **DNS editor**: Same model for `dns.servers` and `dns.rules`. Both server types and matchers are surfaced; modifications and masks are per-config.
- **Proxies**: Group cards, latency tests, ⚡ throughput speed-tests (preserving the group's original selection so a curiosity click doesn't switch your active node).
- **Live logs**: Virtualised list, level filter, payload search, CSV export.
- **Connections**: Active + closed tabs, host / process / rule columns, right-click "close". Filters out Inkwing's own clash_api traffic.
- **Subscription manager**: Save URLs, refresh on demand or on a per-source interval; each fetch becomes a new library entry rather than overwriting.
- **Tray + autostart**: Window close hides to tray (configurable), tray Quit kills sing-box cleanly, optional launch-on-login.
- **Frameless window** with custom controls on Windows / Linux; native traffic-light controls on macOS.
- **i18n**: English + 简体中文 (sidebar + Settings; remaining strings rolling out).

### What Inkwing intentionally doesn't do

- **No config builder.** Bring a finished sing-box JSON. Use sub-store or your provider's subscription to generate one.
- **No fakeip UI.** sing-box's fakeip mode (DNS server returning synthetic IPs from a pool, then routing on those) is *supported* — Inkwing will load and run a config that uses fakeip — but the DNS editor does not surface fakeip-specific fields. If you want fakeip, configure `dns.servers[].type: "fakeip"` plus the `dns.fakeip` block directly in your source config before importing.
- **No format conversion.** Inkwing only consumes sing-box-native JSON. Clash YAML / URI lists are handled upstream.

## How it works

Inkwing's design centres on **realip mode** (sing-box's default routing model — decisions are made on the actual destination IP, with the user's DNS resolving normally). Every UI control around routing, mode switching, and the local override layer is built around this assumption.

### Architecture

- **Frontend**: React 19 + TypeScript + Tailwind v3 + shadcn/ui + Zustand + react-router-dom + dnd-kit + recharts.
- **Bridge**: Tauri 2 `invoke` for command/response, `listen` for streaming events (logs, connections, traffic, override changes).
- **Backend**: Rust + Tauri 2. Modules under `src-tauri/src/core/`:
  - `process` — sing-box sidecar lifecycle (spawn, kill, exit watcher, Windows Job Object kill-on-job-close)
  - `clash_api` + three pumps (`log_pump`, `traffic_pump`, `conn_pump`)
  - `clash_inject` — runtime overlays (clash_api injection, mode, TUN, local ports, cache_file path)
  - `overrides` — local-override layer for GUI edits ([docs](docs/overrides.md))
  - `rules`, `dns`, `library`, `config`, `subscriptions`
- **Persistence**: tauri-plugin-store for library / settings / subscriptions; per-config + global JSON files under `<data_dir>/overrides/`; runtime config rewritten on every core start; `cache.db` redirected into the data dir to avoid orphan-lock collisions.

### The override model in one paragraph

Source config files are read-only from Inkwing's editors. Adding a route rule writes to `<data_dir>/overrides/<entry_id>.json` (per-config) or `overrides/global.json`. Editing a config rule writes a `modifications` entry keyed by the SHA-256 signature of the original rule (revertable). Masking a config rule adds its signature to a `masked` set. At launch time, Inkwing merges `source ⊕ overrides` into `<data_dir>/runtime/config.json` and runs sing-box against that. Your source `.json` keeps every byte. See [`docs/overrides.md`](docs/overrides.md) for the full schema.

## Supported config inputs

| Input | How |
| --- | --- |
| Local sing-box JSON file | **Add config → Add local file…** |
| Pasted raw JSON | **Add config → Add from text…** |
| Subscription URL (returning a sing-box JSON config) | **Add config → Add from subscription URL…** |

Anything else (Clash YAML, URI lists, share-links) needs to be converted upstream first. Inkwing will then run that converted JSON unmodified.

## Quick start

1. Download the latest binary from [GitHub Releases](https://github.com/x01ych/Inkwing/releases) (or build from source — see [Development](#development)).
2. Open Inkwing → **Config** → **Add config** → choose your input source (local file / paste / subscription URL).
3. Click your config card to make it active. Inkwing starts sing-box against it automatically.
4. Use the sidebar's **Mode** strip to flip between `rule / global / direct`. The **TUN** switch toggles a runtime TUN inbound (Linux: needs `CAP_NET_ADMIN`; Windows: needs Administrator; macOS: needs the bundled binary signed for network extensions).
5. Customise routing in **Route**, DNS in **DNS**. All edits live in the override layer; click **Save & Restart** for them to take effect.

## Development

Prerequisites: pnpm, Rust ≥ 1.77, platform deps for Tauri 2 (`webkit2gtk-4.1` on Linux, Xcode CLT on macOS, MSVC build tools on Windows).

```bash
# clone
git clone https://github.com/x01ych/Inkwing.git
cd Inkwing

# install JS deps
pnpm install

# fetch the sing-box sidecar binary into src-tauri/binaries/
node scripts/fetch-singbox.mjs

# Linux only: grant CAP_NET_ADMIN to the bundled sing-box so TUN works without root
sudo bash scripts/grant-tun-cap.sh

# run desktop dev
pnpm tauri dev

# frontend checks
pnpm exec tsc -b
pnpm build

# backend checks
cd src-tauri && cargo check && cargo test --lib

# release bundles
pnpm tauri build           # current OS
pnpm tauri build --debug   # debug bundle, fast
```

Bundles land under `src-tauri/target/release/bundle/`:

- macOS: `bundle/macos/Inkwing.app` (+ `.dmg` if signed)
- Windows: `bundle/nsis/Inkwing-*.exe`
- Linux: `bundle/deb/inkwing_*.deb`, `bundle/appimage/inkwing_*.AppImage`

For a Docker-based dev container (Ubuntu 22.04 + Xvfb + noVNC) see [`scripts/dev-container.sh`](scripts/dev-container.sh).

## Local data directory

All data stays on the local machine.

- **Windows**: `%APPDATA%\inkwing\Inkwing\data\`
- **Linux**: `~/.local/share/inkwing/`
- **macOS**: `~/Library/Application Support/dev.inkwing.Inkwing/`

Layout:

```
<data_dir>/
├── configs/<id>.json     managed library configs (immutable from editors)
├── overrides/
│   ├── <id>.json         per-config overrides
│   └── global.json       global appended overrides
├── runtime/config.json   merged config sing-box reads (rewritten on every start)
└── cache.db              sing-box's rule-set cache + URL-test history
```

## Documentation

- [Chinese README](README.zh.md)
- [Overrides architecture](docs/overrides.md) — how local edits stack on top of source configs
- [UI stack notes](docs/ui-stack.md) — Tailwind / shadcn conventions
- [macOS dev notes](docs/MACOS-DEV.md)

## Project layout

```
src/                       React + Tailwind + shadcn frontend
  pages/                   Dashboard / Config / Proxies / Route / DNS / Logs / Connections / Settings
  components/              ui (shadcn) + Layout + per-feature dialogs
  api/                     Thin invoke / listen wrappers around Tauri commands
  store/                   Zustand stores
src-tauri/
  src/commands/            Tauri commands by feature group
  src/core/                Sing-box plumbing
  binaries/                sing-box sidecar (gitignored — fetched by scripts/fetch-singbox.mjs)
  icons/                   App icons (all platforms)
docs/
```

## Contributing

Bug reports and pull requests welcome via the [GitHub repo](https://github.com/x01ych/Inkwing). Keep changes consistent with the existing architecture and the zero-loss / overrides-only-mutation contract.

## Acknowledgments

- [sing-box](https://github.com/SagerNet/sing-box) — the kernel
- [Tauri](https://tauri.app/) — desktop bridge
- [shadcn/ui](https://ui.shadcn.com/) — UI primitives
- [Sub-Store](https://github.com/sub-store-org/Sub-Store) — recommended upstream for config generation
- [sing-box-windows](https://github.com/xinggaoya/sing-box-windows) — README layout, feature-matrix conventions, and prior art for a Tauri-based sing-box client
- [clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev) — UI / interaction reference for desktop proxy clients

## License

MIT. See [LICENSE](LICENSE).

---

<div align="center">
  <p>
    <strong>Disclaimer:</strong> Inkwing is for personal learning and lawful use. Comply with the laws and regulations of your jurisdiction.
  </p>
</div>
