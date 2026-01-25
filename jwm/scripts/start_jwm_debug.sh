#!/usr/bin/env bash
set -euo pipefail

# Start JWM with verbose input/keyboard debugging enabled.
# This keeps debug env vars scoped to this process only.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# You can override these at runtime:
#   RUST_LOG=... JWM_DEBUG_KEYS=1 ./scripts/start_jwm_debug.sh
: "${JWM_DEBUG_KEYS:=1}"
: "${RUST_LOG:=jwm=info,jwm::backend::udev=debug,smithay::backend::libinput=debug,smithay::input::keyboard=debug}"

# Optional: write logs to a file under ./logs
LOG_DIR="$ROOT_DIR/logs"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/jwm_debug_$(date +%F_%H-%M-%S).log"

# Prefer an already-built binary.
BIN_RELEASE="$ROOT_DIR/target/release/jwm"
BIN_DEBUG="$ROOT_DIR/target/debug/jwm"

cmd=()
if [[ -x "$BIN_RELEASE" ]]; then
  cmd+=("$BIN_RELEASE")
elif [[ -x "$BIN_DEBUG" ]]; then
  cmd+=("$BIN_DEBUG")
else
  # Fall back to building/running via cargo.
  # Keep feature flags aligned with TTY + udev usage.
  cmd+=(cargo run --release --features "backend-udev gtk_bar" --)
fi

export RUST_LOG
export JWM_DEBUG_KEYS

echo "[start_jwm_debug] ROOT_DIR=$ROOT_DIR" >&2
echo "[start_jwm_debug] RUST_LOG=$RUST_LOG" >&2
echo "[start_jwm_debug] JWM_DEBUG_KEYS=$JWM_DEBUG_KEYS" >&2
echo "[start_jwm_debug] LOG_FILE=$LOG_FILE" >&2
echo "[start_jwm_debug] exec: ${cmd[*]}" >&2

# Capture both stdout/stderr to the log file while also showing it on screen.
# If you don't want terminal output, replace with: exec "${cmd[@]}" >>"$LOG_FILE" 2>&1
exec "${cmd[@]}" 2>&1 | tee -a "$LOG_FILE"
