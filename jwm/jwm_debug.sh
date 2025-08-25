#!/bin/bash
# jwm_debug.sh - JWM调试脚本

echo "=== JWM守护进程调试信息 ==="
echo "时间: $(date)"
echo ""

echo "1. 检查守护进程:"
ps aux | grep jwm_daemon | grep -v grep

echo ""
echo "2. 检查PID文件:"
if [ -f "/tmp/jwm_daemon.pid" ]; then
    echo "PID文件存在: $(cat /tmp/jwm_daemon.pid)"
else
    echo "PID文件不存在"
fi

echo ""
echo "3. 检查控制管道:"
ls -la /tmp/jwm_control_* 2>/dev/null || echo "未找到控制管道"

echo ""
echo "4. 检查JWM进程:"
ps aux | grep -E "jwm[^_]" | grep -v grep

echo ""
echo "5. 检查日志:"
if [ -f "$HOME/.local/share/jwm/jwm_daemon.log" ]; then
    echo "最近的日志:"
    tail -10 "$HOME/.local/share/jwm/jwm_daemon.log"
else
    echo "日志文件不存在"
fi

echo ""
echo "6. X11信息:"
echo "DISPLAY: $DISPLAY"
ps aux | grep X | grep -v grep
