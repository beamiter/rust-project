#!/bin/bash
# 快速调试脚本 - 启用所有调试选项

set -e

cd "$(dirname "$0")/.."

echo "🔧 Building debug version..."

echo "📋 Debug Configuration:"
echo "  - Log Level: DEBUG"
echo "  - Keyboard Debug: ON"
echo "  - Wayland Debug: ON"
echo ""
echo "⚠️  Make sure you're running from a TTY (Ctrl+Alt+F2)"

export RUST_LOG=debug
export RUST_BACKTRACE=full
export JWM_DEBUG_KEYS=0
export WAYLAND_DEBUG=0
export XDG_SESSION_TYPE=wayland
export XDG_SESSION_CLASS=user
export JWM_BACKEND=wayland-udev

LOG_FILE="/tmp/jwm_debug_$(date +%s).log"
echo "📝 Logging to: $LOG_FILE"

jwm 2>&1 | tee "$LOG_FILE"

echo ""
echo "✅ Debug session ended"
echo "📄 Log saved to: $LOG_FILE"
