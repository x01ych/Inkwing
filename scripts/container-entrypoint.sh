#!/usr/bin/env bash
# Boots Xvfb + fluxbox + x11vnc + noVNC, then exec's the CMD.
# Default CMD is `sleep infinity` so the container stays up; users
# `docker compose exec dev bash` to do real work.
#
# NOTE: VNC stack failures must not kill the container — without `|| true`
# a stale lockfile would prevent ssh shell access. Errors are logged.

DISPLAY_NUM="${DISPLAY_NUM:-1}"
SCREEN_GEOMETRY="${SCREEN_GEOMETRY:-1280x800x24}"
VNC_PORT="${VNC_PORT:-5900}"
NOVNC_PORT="${NOVNC_PORT:-6080}"
export DISPLAY=":${DISPLAY_NUM}"

log() { echo "[entrypoint] $*"; }

# Docker creates named volumes as root; chown back to dev so pnpm/cargo can
# write inside them. Cheap (no-op) once ownership is correct.
fix_volume_ownership() {
  for d in /workspace/node_modules /workspace/src-tauri/target \
           /home/dev/.cargo/registry /home/dev/.cargo/git; do
    if [ -d "$d" ] && [ "$(stat -c %u "$d")" != "$(id -u)" ]; then
      log "chowning $d to $(id -un)"
      sudo chown -R "$(id -u):$(id -g)" "$d" || log "WARN: chown failed for $d"
    fi
  done
}

prep_x11() {
  # /tmp/.X11-unix is normally created by systemd at host boot; in a fresh
  # container we create it with the canonical 1777 perms so Xvfb can put its
  # socket there.
  if [ ! -d /tmp/.X11-unix ]; then
    sudo mkdir -p /tmp/.X11-unix
    sudo chmod 1777 /tmp/.X11-unix
  fi
  # Stale lockfiles linger across container restarts (writable layer is
  # preserved on `docker restart`). Remove them.
  rm -f /tmp/.X${DISPLAY_NUM}-lock 2>/dev/null || true
  sudo rm -f /tmp/.X11-unix/X${DISPLAY_NUM} 2>/dev/null || true
}

start_xvfb() {
  log "starting Xvfb on ${DISPLAY} (${SCREEN_GEOMETRY})"
  Xvfb "${DISPLAY}" -screen 0 "${SCREEN_GEOMETRY}" -nolisten tcp -ac \
    >/tmp/xvfb.log 2>&1 &
  for _ in $(seq 1 50); do
    if xdpyinfo -display "${DISPLAY}" >/dev/null 2>&1; then
      log "Xvfb ready"
      return 0
    fi
    sleep 0.1
  done
  log "WARN: Xvfb did not become ready (see /tmp/xvfb.log)"
  return 1
}

start_fluxbox() {
  log "starting fluxbox"
  fluxbox >/tmp/fluxbox.log 2>&1 &
}

start_vnc() {
  log "starting x11vnc on :${VNC_PORT}"
  x11vnc -display "${DISPLAY}" -rfbport "${VNC_PORT}" \
    -nopw -listen 127.0.0.1 -forever -shared -bg \
    -o /tmp/x11vnc.log >/tmp/x11vnc-stdout.log 2>&1 \
    || log "WARN: x11vnc failed (see /tmp/x11vnc*.log)"
}

start_novnc() {
  log "starting noVNC websockify on :${NOVNC_PORT} -> 127.0.0.1:${VNC_PORT}"
  websockify --web=/usr/share/novnc/ \
    "0.0.0.0:${NOVNC_PORT}" "127.0.0.1:${VNC_PORT}" \
    >/tmp/novnc.log 2>&1 &
}

fix_volume_ownership
prep_x11
start_xvfb && {
  start_fluxbox
  start_vnc
  start_novnc
  log "VNC ready. open http://<host>:${NOVNC_PORT}/vnc.html"
} || log "VNC stack disabled; container still usable for headless work"

# Persist DISPLAY for interactive shells.
grep -q "export DISPLAY=" /home/dev/.bashrc 2>/dev/null \
  || echo "export DISPLAY=${DISPLAY}" >> /home/dev/.bashrc

exec "$@"
