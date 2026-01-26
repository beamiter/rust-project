#!/usr/bin/env bash
set -euo pipefail

# Start JWM on a dedicated VT so you can always switch away
# (e.g. Ctrl+Alt+F3) to inspect logs / kill the process.
#
# Usage:
#   ./scripts/start_jwm_debug_vt.sh            # default VT 8
#   ./scripts/start_jwm_debug_vt.sh 2          # run on VT 2
#   RUST_LOG=... JWM_DEBUG_KEYS=1 ./scripts/start_jwm_debug_vt.sh 8

VT="${1:-8}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v openvt >/dev/null 2>&1; then
  echo "[start_jwm_debug_vt] openvt not found (install kbd util / util-linux)" >&2
  exit 1
fi

CMD=("$ROOT_DIR/scripts/start_jwm_debug.sh")

echo "[start_jwm_debug_vt] ROOT_DIR=$ROOT_DIR" >&2
echo "[start_jwm_debug_vt] VT=$VT" >&2
echo "[start_jwm_debug_vt] cmd: ${CMD[*]}" >&2

action=(openvt -c "$VT" -s -f -- "${CMD[@]}")

# openvt often requires root to allocate/switch VTs.
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  exec sudo -E "${action[@]}"
else
  exec "${action[@]}"
fi
