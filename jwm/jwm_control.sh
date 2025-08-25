#!/bin/bash
# jwm_control.sh - JWM控制脚本（修复版2）

PIDFILE="/tmp/jwm_daemon.pid"
JWM_DIR="$HOME/jwm"  # 修改为你的JWM源码目录

# 查找守护进程的控制管道
find_control_pipe() {
    if [ ! -f "$PIDFILE" ]; then
        return 1
    fi
    
    local daemon_pid=$(cat "$PIDFILE")
    if [ -z "$daemon_pid" ]; then
        return 1
    fi
    
    # 检查进程是否存在
    if ! kill -0 "$daemon_pid" 2>/dev/null; then
        return 1
    fi
    
    local pipe="/tmp/jwm_control_$daemon_pid"
    if [ -p "$pipe" ]; then
        echo "$pipe"
        return 0
    else
        return 1
    fi
}

# 发送命令到守护进程
send_command() {
    local cmd="$1"
    local pipe=$(find_control_pipe)
    
    if [ -z "$pipe" ]; then
        echo "错误: 未找到JWM守护进程或控制管道"
        echo "请确保JWM守护进程正在运行"
        return 1
    fi
    
    echo "发送命令: $cmd"
    
    # 发送命令
    echo "$cmd" > "$pipe" &
    local send_pid=$!
    
    # 等待发送完成
    sleep 0.5
    
    # 检查响应
    local response_file="${pipe}_response"
    local count=0
    while [ $count -lt 20 ]; do  # 等待最多2秒
        if [ -f "$response_file" ]; then
            echo "响应: $(cat "$response_file")"
            rm -f "$response_file"
            return 0
        fi
        sleep 0.1
        count=$((count + 1))
    done
    
    echo "警告: 命令可能已发送，但未收到响应"
    return 0
}

# 检查守护进程状态
check_daemon() {
    local pipe=$(find_control_pipe)
    if [ -n "$pipe" ]; then
        echo "JWM守护进程正在运行"
        if [ -f "$PIDFILE" ]; then
            echo "PID: $(cat "$PIDFILE")"
        fi
        echo "控制管道: $pipe"
        return 0
    else
        echo "JWM守护进程未运行"
        return 1
    fi
}

# 强制重启守护进程
force_restart_daemon() {
    echo "强制重启守护进程..."
    
    # 杀死现有的守护进程
    if [ -f "$PIDFILE" ]; then
        local old_pid=$(cat "$PIDFILE")
        if kill -0 "$old_pid" 2>/dev/null; then
            echo "终止旧的守护进程: $old_pid"
            kill -TERM "$old_pid"
            sleep 2
            kill -9 "$old_pid" 2>/dev/null
        fi
    fi
    
    # 清理旧文件
    rm -f /tmp/jwm_control_*
    rm -f "$PIDFILE"
    
    # 启动新的守护进程
    echo "启动新的守护进程..."
    nohup jwm_daemon.sh > /dev/null 2>&1 &
    
    sleep 3
    
    if check_daemon > /dev/null 2>&1; then
        echo "守护进程重启成功"
    else
        echo "守护进程重启失败"
        return 1
    fi
}

# 编译并重启JWM
rebuild_and_restart() {
    # 确保守护进程运行
    if ! check_daemon > /dev/null 2>&1; then
        echo "守护进程未运行，正在强制重启..."
        if ! force_restart_daemon; then
            return 1
        fi
    fi
    
    echo "开始编译JWM..."
    
    cd "$JWM_DIR" || {
        echo "错误: 无法进入JWM目录: $JWM_DIR"
        return 1
    }
    
    if ! cargo build --release; then
        echo "编译失败！"
        return 1
    fi
    
    echo "安装新的JWM二进制文件..."
    if ! sudo cp target/release/jwm /usr/local/bin/jwm; then
        echo "安装失败！"
        return 1
    fi
    
    echo "重启JWM..."
    send_command "restart"
    
    echo "✅ JWM编译并重启完成！"
}

# 显示帮助信息
show_help() {
    echo "JWM控制脚本"
    echo "用法: $0 [命令]"
    echo ""
    echo "命令:"
    echo "  restart           - 重启JWM"
    echo "  stop              - 停止JWM"
    echo "  start             - 启动JWM"
    echo "  quit              - 退出守护进程"
    echo "  status            - 显示JWM状态"
    echo "  rebuild           - 编译并重启JWM"
    echo "  daemon-check      - 检查守护进程状态"
    echo "  daemon-restart    - 强制重启守护进程"
    echo "  help              - 显示此帮助信息"
}

# 主程序
case "${1:-help}" in
    "restart")
        send_command "restart"
        ;;
    "stop")
        send_command "stop"
        ;;
    "start")
        send_command "start"
        ;;
    "quit")
        send_command "quit"
        ;;
    "status")
        send_command "status"
        ;;
    "rebuild")
        rebuild_and_restart
        ;;
    "daemon-check")
        check_daemon
        ;;
    "daemon-restart")
        force_restart_daemon
        ;;
    "help"|*)
        show_help
        ;;
esac
