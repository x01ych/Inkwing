#!/usr/bin/env bash
# Grant CAP_NET_ADMIN (and CAP_NET_BIND_SERVICE) to the bundled sing-box
# binary so TUN can come up without the GUI itself running as root.
#
# Run once after `node scripts/fetch-singbox.mjs`. Re-run after every fetch
# (setcap is lost when the file is overwritten).

set -euo pipefail

cd "$(dirname "$0")/.."

BIN="src-tauri/binaries/sing-box-x86_64-unknown-linux-gnu"

if [ ! -f "$BIN" ]; then
  echo "ERROR: $BIN not found. Run: node scripts/fetch-singbox.mjs" >&2
  exit 1
fi

echo "Granting cap_net_admin,cap_net_bind_service to $BIN ..."
sudo setcap 'cap_net_admin,cap_net_bind_service=+ep' "$BIN"

echo "Verifying:"
getcap "$BIN"
