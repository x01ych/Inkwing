#!/usr/bin/env bash
# Wrapper around docker compose for the dev container. Pass UID/GID so the
# container user matches the host and bind-mounted file ownership stays sane.
set -euo pipefail

cd "$(dirname "$0")/.."
export UID GID
GID="$(id -g)"

CMD="${1:-help}"
shift || true

case "$CMD" in
  build)
    docker compose -f docker-compose.dev.yml build "$@"
    ;;
  up)
    docker compose -f docker-compose.dev.yml up -d --build "$@"
    echo
    echo "Container is up. Useful next steps:"
    echo "  bash scripts/dev-container.sh shell         # open a bash inside"
    echo "  bash scripts/dev-container.sh tauri-dev     # run pnpm tauri dev"
    echo
    echo "noVNC: http://<host>:6080/vnc.html  (ssh -L 6080:localhost:6080 if remote)"
    ;;
  down)
    docker compose -f docker-compose.dev.yml down "$@"
    ;;
  shell)
    docker compose -f docker-compose.dev.yml exec dev bash
    ;;
  exec)
    docker compose -f docker-compose.dev.yml exec dev "$@"
    ;;
  logs)
    docker compose -f docker-compose.dev.yml logs -f "$@"
    ;;
  install)
    docker compose -f docker-compose.dev.yml exec dev bash -lc "pnpm install"
    ;;
  setcap)
    docker compose -f docker-compose.dev.yml exec dev bash -lc "bash scripts/grant-tun-cap.sh"
    ;;
  cargo-check)
    docker compose -f docker-compose.dev.yml exec dev bash -lc "cargo check --manifest-path src-tauri/Cargo.toml"
    ;;
  tauri-dev)
    docker compose -f docker-compose.dev.yml exec dev bash -lc "pnpm tauri dev"
    ;;
  reset)
    docker compose -f docker-compose.dev.yml down -v
    ;;
  help|--help|-h|"")
    cat <<EOF
Usage: $0 <command>

  build         Build the dev image
  up            Start the container in background (auto-builds on change)
  down          Stop the container (volumes preserved)
  reset         Stop AND remove cargo/node_modules volumes (fresh start)
  shell         Open a bash inside the container
  exec <cmd>    Run an arbitrary command inside
  logs          Tail container logs

  install       pnpm install (inside)
  setcap        Run scripts/grant-tun-cap.sh (inside, needs sudo in container)
  cargo-check   cargo check on src-tauri (inside)
  tauri-dev     pnpm tauri dev (inside) — view via noVNC at :6080

EOF
    ;;
  *)
    echo "unknown command: $CMD" >&2
    "$0" help
    exit 1
    ;;
esac
