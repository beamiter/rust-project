#!/bin/bash
# 远程调试模式 - JWM 输出到文件，方便从另一个 TTY 查看

set -e

cd "$(dirname "$0")/.."

# 创建日志目录
mkdir -p /tmp/jwm_logs

LOG_FILE="/tmp/jwm_logs/debug_$(date +%Y%m%d_%H%M%S).log"
PID_FILE="/tmp/jwm_logs/jwm.pid"

echo "📝 Starting JWM in remote debug mode..."
echo "   Log file: $LOG_FILE"
echo ""
echo "📖 To view logs from another TTY, run:"
echo "   tail -f $LOG_FILE"
echo ""
echo "⚠️  To kill JWM, run:"
echo "   kill \$(cat $PID_FILE)"
echo ""

export RUST_LOG=debug
export RUST_BACKTRACE=full
export JWM_DEBUG_KEYS=1
export WAYLAND_DEBUG=1

# 在后台运行并保存 PID
(
    ./target/debug/jwm 2>&1 | tee "$LOG_FILE"
) &

echo $! > "$PID_FILE"

# 等待进程
wait $!
