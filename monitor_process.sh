#!/bin/bash

# 增强版监控脚本，带简单图形化显示
monitor_process_enhanced() {
  local PROCESS=$1
  local DURATION=${2:-60}
  local INTERVAL=${3:-2}

  echo "================================================"
  echo "增强版进程监控器"
  echo "进程: $PROCESS | 时长: ${DURATION}s | 间隔: ${INTERVAL}s"
  echo "================================================"

  # 数组存储历史数据
  declare -a CPU_HISTORY
  declare -a MEM_HISTORY

  START_TIME=$(date +%s)
  INDEX=0

  while true; do
    CURRENT_TIME=$(date +%s)
    ELAPSED=$((CURRENT_TIME - START_TIME))

    if [ $ELAPSED -ge $DURATION ]; then
      break
    fi

    # 获取进程信息
    if [[ $PROCESS =~ ^[0-9]+$ ]]; then
      PROC_INFO=$(ps -p $PROCESS -o pid,comm,%cpu,%mem,rss --no-headers 2>/dev/null)
    else
      PROC_INFO=$(ps -C $PROCESS -o pid,comm,%cpu,%mem,rss --no-headers 2>/dev/null | head -1)
    fi

    if [ -n "$PROC_INFO" ]; then
      CPU=$(echo $PROC_INFO | awk '{print $3}')
      MEM_PERCENT=$(echo $PROC_INFO | awk '{print $4}')
      RSS=$(echo $PROC_INFO | awk '{print $5}')
      MEM_MB=$(echo "scale=1; $RSS / 1024" | bc)

      # 存储历史数据
      CPU_HISTORY[$INDEX]=$CPU
      MEM_HISTORY[$INDEX]=$MEM_PERCENT

      # 清屏并显示
      clear
      echo "================================================"
      echo "进程监控 | 剩余时间: $((DURATION - ELAPSED))秒"
      echo "================================================"
      echo "当前状态:"
      echo "  CPU使用率: ${CPU}%"
      echo "  内存使用率: ${MEM_PERCENT}%"
      echo "  内存使用量: ${MEM_MB}MB"
      echo ""

      # 简单的条形图显示
      echo "CPU使用率趋势 (最近10次):"
      show_bar_chart "CPU_HISTORY" $INDEX 10
      echo ""
      echo "内存使用率趋势 (最近10次):"
      show_bar_chart "MEM_HISTORY" $INDEX 10

      INDEX=$((INDEX + 1))
    else
      echo "进程 '$PROCESS' 未找到"
    fi

    sleep $INTERVAL
  done
}

# 显示简单条形图
show_bar_chart() {
  local -n arr=$1
  local current_index=$2
  local max_bars=$3

  local start_index=$((current_index - max_bars + 1))
  if [ $start_index -lt 0 ]; then
    start_index=0
  fi

  for ((i=start_index; i<=current_index; i++)); do
    if [ -n "${arr[$i]}" ]; then
      local value=${arr[$i]}
      local bar_length=$(echo "$value / 5" | bc 2>/dev/null || echo "1")
      printf "%2d: %5.1f%% " $((i+1)) $value

      # 绘制条形图
      for ((j=0; j<bar_length; j++)); do
        printf "█"
      done
      printf "\n"
    fi
  done
}

# 主函数
main() {
  if [ $# -eq 0 ]; then
    echo "使用方法: $0 <进程名或PID> [监控时长(秒)] [采样间隔(秒)]"
    echo "示例:"
    echo "  $0 nginx"
    echo "  $0 1234 120 3"
    exit 1
  fi

  # 检查依赖
  if ! command -v bc &> /dev/null; then
    echo "错误: 需要安装 bc 计算器"
    echo "Ubuntu/Debian: sudo apt-get install bc"
    echo "CentOS/RHEL: sudo yum install bc"
    exit 1
  fi

  monitor_process_enhanced "$@"
}

# 运行主函数
main "$@"
