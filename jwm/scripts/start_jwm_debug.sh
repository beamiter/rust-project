#!/usr/bin/env bash
set -euo pipefail

# Start JWM with verbose input/keyboard debugging enabled.
# This keeps debug env vars scoped to this process only.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# When invoked via sudo, PATH is often sanitized and may not include the user's
# rustup/cargo install path (~/.cargo/bin). Try to recover it so we can build.
if ! command -v cargo >/dev/null 2>&1; then
  if [[ "${EUID:-$(id -u)}" -eq 0 && -n "${SUDO_USER:-}" ]]; then
    sudo_user_home="$(getent passwd "$SUDO_USER" | cut -d: -f6 || true)"
    if [[ -n "$sudo_user_home" ]]; then
      export PATH="$sudo_user_home/.cargo/bin:$PATH"
    fi
  fi
fi

# You can override these at runtime:
#   RUST_LOG=... JWM_DEBUG_KEYS=1 JWM_DEBUG_BUTTONS=1 ./scripts/start_jwm_debug.sh
: "${JWM_DEBUG_KEYS:=1}"
: "${JWM_DEBUG_BUTTONS:=1}"
: "${RUST_LOG:=jwm=info,jwm::backend::udev=debug,smithay::backend::libinput=debug,smithay::input::keyboard=debug}"
: "${JWM_BACKEND:=wayland-udev}"

# Optional: write logs to a file under ./logs
LOG_DIR="$ROOT_DIR/logs"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/jwm_debug_$(date +%F_%H-%M-%S).log"

# Cargo may put the `target/` dir either under this crate or under a workspace
# root (one level up). Check both so we can reuse an existing build.
TARGET_DIRS=()
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  TARGET_DIRS+=("$CARGO_TARGET_DIR")
fi
TARGET_DIRS+=("$ROOT_DIR/target" "$ROOT_DIR/../target")

BIN_RELEASE=""
BIN_DEBUG=""
for target_dir in "${TARGET_DIRS[@]}"; do
  if [[ -z "$BIN_RELEASE" && -x "$target_dir/release/jwm" ]]; then
    BIN_RELEASE="$target_dir/release/jwm"
  fi
  if [[ -z "$BIN_DEBUG" && -x "$target_dir/debug/jwm" ]]; then
    BIN_DEBUG="$target_dir/debug/jwm"
  fi
done

cmd=()
if [[ -n "$BIN_RELEASE" ]]; then
  cmd+=("$BIN_RELEASE")
elif [[ -n "$BIN_DEBUG" ]]; then
  cmd+=("$BIN_DEBUG")
else
  # No prebuilt binary found: build it and run the produced binary.
  if command -v cargo >/dev/null 2>&1; then
    echo "[start_jwm_debug] building: cargo build --release" >&2
    cargo build --release
    # Re-resolve after build (target dir may differ).
    for target_dir in "${TARGET_DIRS[@]}"; do
      if [[ -x "$target_dir/release/jwm" ]]; then
        cmd+=("$target_dir/release/jwm")
        break
      fi
    done
    if [[ ${#cmd[@]} -eq 0 ]]; then
      echo "[start_jwm_debug] error: build succeeded but no 'jwm' binary was found under: ${TARGET_DIRS[*]}" >&2
      exit 1
    fi
  else
    echo "[start_jwm_debug] error: cargo not found in PATH (try: sudo -E ... or install rust/cargo for root)" >&2
    exit 127
  fi
fi

export RUST_LOG
export JWM_DEBUG_KEYS
export JWM_DEBUG_BUTTONS
export JWM_BACKEND

echo "[start_jwm_debug] ROOT_DIR=$ROOT_DIR" >&2
echo "[start_jwm_debug] RUST_LOG=$RUST_LOG" >&2
echo "[start_jwm_debug] JWM_DEBUG_KEYS=$JWM_DEBUG_KEYS" >&2
echo "[start_jwm_debug] JWM_DEBUG_BUTTONS=$JWM_DEBUG_BUTTONS" >&2
echo "[start_jwm_debug] JWM_BACKEND=$JWM_BACKEND" >&2
echo "[start_jwm_debug] LOG_FILE=$LOG_FILE" >&2
echo "[start_jwm_debug] exec: ${cmd[*]}" >&2

if [[ "${JWM_DRY_RUN:-0}" == "1" ]]; then
  exit 0
fi

# Capture both stdout/stderr to the log file while also showing it on screen.
# If you don't want terminal output, replace with: exec "${cmd[@]}" >>"$LOG_FILE" 2>&1
exec "${cmd[@]}" 2>&1 | tee -a "$LOG_FILE"
