#!/bin/bash
# jwm_daemon.sh - JWM守护进程脚本（修复版2）

JWM_BINARY="/usr/local/bin/jwm"
DAEMON_PID=$$
CONTROL_PIPE="/tmp/jwm_control_$DAEMON_PID"
PIDFILE="/tmp/jwm_daemon.pid"
LOG_FILE="$HOME/.local/share/jwm/jwm_daemon.log"

# 创建日志目录
mkdir -p "$(dirname "$LOG_FILE")"

log() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" | tee -a "$LOG_FILE"
}

cleanup() {
  log "开始清理资源..."
  if [ -n "$JWM_PID" ]; then
    log "终止JWM进程: $JWM_PID"
    kill -TERM "$JWM_PID" 2>/dev/null
    sleep 1
    kill -9 "$JWM_PID" 2>/dev/null
  fi
  rm -f "$CONTROL_PIPE"
  rm -f "$PIDFILE"
  log "清理完成，守护进程退出"
  exit 0
}

# 设置信号处理
trap cleanup TERM INT

# 检查是否已有守护进程运行
if [ -f "$PIDFILE" ]; then
  OLD_PID=$(cat "$PIDFILE")
  if kill -0 "$OLD_PID" 2>/dev/null; then
    echo "守护进程已在运行，PID: $OLD_PID"
    exit 1
  else
    rm -f "$PIDFILE"
  fi
fi

# 写入PID文件
echo "$DAEMON_PID" > "$PIDFILE"

# 创建控制管道
rm -f "$CONTROL_PIPE"
if ! mkfifo "$CONTROL_PIPE"; then
  log "错误: 无法创建控制管道"
  exit 1
fi

log "JWM守护进程启动，PID: $DAEMON_PID"
log "控制管道: $CONTROL_PIPE"

# 启动JWM的函数
start_jwm() {
  if [ -n "$JWM_PID" ] && kill -0 "$JWM_PID" 2>/dev/null; then
    log "JWM已在运行，PID: $JWM_PID"
    return 0
  fi

  log "启动JWM: $JWM_BINARY"
  if [ -f "$JWM_BINARY" ]; then
    "$JWM_BINARY" &
    JWM_PID=$!
    log "JWM已启动，PID: $JWM_PID"
  else
    log "错误: JWM二进制文件不存在: $JWM_BINARY"
    return 1
  fi
}

# 停止JWM的函数
stop_jwm() {
  if [ -n "$JWM_PID" ] && kill -0 "$JWM_PID" 2>/dev/null; then
    log "停止JWM进程: $JWM_PID"
    kill -TERM "$JWM_PID"

    # 等待进程退出（最多5秒）
    local count=0
    while kill -0 "$JWM_PID" 2>/dev/null && [ $count -lt 50 ]; do
      sleep 0.1
      count=$((count + 1))
    done

    # 如果还没退出，强制杀死
    if kill -0 "$JWM_PID" 2>/dev/null; then
      log "强制终止JWM进程: $JWM_PID"
      kill -9 "$JWM_PID" 2>/dev/null
    fi

    JWM_PID=""
    log "JWM进程已停止"
  else
    log "JWM进程未运行"
  fi
}

# 重启JWM的函数
restart_jwm() {
  log "重启JWM..."
  stop_jwm
  sleep 1
  start_jwm
}

# 获取状态
get_status() {
  if [ -n "$JWM_PID" ] && kill -0 "$JWM_PID" 2>/dev/null; then
    echo "JWM运行中，PID: $JWM_PID"
  else
    echo "JWM未运行"
  fi
}

# 处理命令的函数
handle_command() {
  local cmd="$1"
  log "收到命令: $cmd"

  case "$cmd" in
    "restart")
      restart_jwm
      echo "restart_done" > "${CONTROL_PIPE}_response"
      ;;
    "stop")
      stop_jwm
      echo "stop_done" > "${CONTROL_PIPE}_response"
      ;;
    "start")
      start_jwm
      echo "start_done" > "${CONTROL_PIPE}_response"
      ;;
    "quit")
      log "收到退出命令"
      echo "quit_done" > "${CONTROL_PIPE}_response"
      cleanup
      ;;
    "status")
      get_status > "${CONTROL_PIPE}_response"
      ;;
    *)
      log "未知命令: $cmd"
      echo "unknown_command" > "${CONTROL_PIPE}_response"
      ;;
  esac
}

# 主循环 - 使用非阻塞读取
main() {
  # 初始启动JWM
  start_jwm

  log "开始主循环，监听命令..."

  # 监听控制命令
  while true; do
    # 使用exec打开管道进行非阻塞读取
    if exec 3< "$CONTROL_PIPE"; then
      # 使用read -t进行带超时的读取
      if read -t 1 -r cmd <&3 2>/dev/null; then
        if [ -n "$cmd" ]; then
          handle_command "$cmd"
        fi
      fi
      exec 3<&-
    fi

    # 检查JWM是否意外退出
    if [ -n "$JWM_PID" ] && ! kill -0 "$JWM_PID" 2>/dev/null; then
      log "检测到JWM意外退出，重新启动..."
      JWM_PID=""
      sleep 1
      start_jwm
    fi

    # 短暂休眠避免CPU占用过高
    sleep 0.1
  done
}

# 启动主循环
main
