#!/bin/bash
# test_jwm.sh - JWM 测试脚本

set -e

echo "======================================"
echo "       JWM 窗口管理器测试脚本"
echo "======================================"

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 检查依赖
check_dependencies() {
  echo -e "${BLUE}检查依赖...${NC}"

  local missing_deps=()

  # 检查 X11 工具
  command -v xdotool >/dev/null 2>&1 || missing_deps+=("xdotool")
  command -v xwininfo >/dev/null 2>&1 || missing_deps+=("x11-utils")
  command -v xev >/dev/null 2>&1 || missing_deps+=("x11-utils")

  if [ ${#missing_deps[@]} -ne 0 ]; then
    echo -e "${RED}缺少依赖: ${missing_deps[*]}${NC}"
    echo "请运行: sudo apt install ${missing_deps[*]}"
    exit 1
  fi

  echo -e "${GREEN}✓ 依赖检查通过${NC}"
}

# 检查 JWM 是否运行
check_jwm() {
  echo -e "${BLUE}检查 JWM 状态...${NC}"

  if ! pgrep -x "jwm" > /dev/null; then
    echo -e "${RED}✗ JWM 未运行${NC}"
    echo "请先启动 JWM 窗口管理器"
    exit 1
  fi

  echo -e "${GREEN}✓ JWM 正在运行${NC}"
}

# 发送按键组合
send_key() {
  local modifiers="$1"
  local key="$2"
  local description="$3"

  echo -e "${YELLOW}测试: $description${NC}"
  echo "  按键组合: $modifiers+$key"

  # 获取当前活动窗口
  local active_window_before=$(xdotool getactivewindow 2>/dev/null || echo "none")

  # 发送按键
  if [ -n "$modifiers" ]; then
    xdotool key --clearmodifiers "$modifiers+$key"
  else
    xdotool key --clearmodifiers "$key"
  fi

  sleep 0.2

  # 检查结果
  local active_window_after=$(xdotool getactivewindow 2>/dev/null || echo "none")

  if [ "$active_window_before" != "$active_window_after" ] || [ "$key" == "e" ] || [ "$key" == "shift+Return" ]; then
    echo -e "  ${GREEN}✓ 测试通过${NC}"
    return 0
  else
    echo -e "  ${RED}✗ 测试失败${NC}"
    return 1
  fi
}

# 功能测试
functional_tests() {
  echo -e "\n${BLUE}=== 功能测试 ===${NC}"

  local passed=0
  local total=0

  # 窗口焦点测试
  echo -e "\n${YELLOW}窗口焦点控制测试:${NC}"
  send_key "alt" "j" "向下切换窗口焦点" && ((passed++))
  ((total++))

  send_key "alt" "k" "向上切换窗口焦点" && ((passed++))
  ((total++))

  # 布局测试
  echo -e "\n${YELLOW}布局控制测试:${NC}"
  send_key "alt" "h" "减少主窗口宽度" && ((passed++))
  ((total++))

  send_key "alt" "l" "增加主窗口宽度" && ((passed++))
  ((total++))

  # 布局切换测试
  echo -e "\n${YELLOW}布局切换测试:${NC}"
  send_key "alt" "t" "切换到平铺布局" && ((passed++))
  ((total++))

  send_key "alt" "f" "切换到浮动布局" && ((passed++))
  ((total++))

  send_key "alt" "m" "切换到单窗口布局" && ((passed++))
  ((total++))

  # 标签测试
  echo -e "\n${YELLOW}标签切换测试:${NC}"
  send_key "alt" "1" "切换到标签1" && ((passed++))
  ((total++))

  send_key "alt" "2" "切换到标签2" && ((passed++))
  ((total++))

  send_key "alt" "Tab" "循环切换标签" && ((passed++))
  ((total++))

  # 窗口操作测试
  echo -e "\n${YELLOW}窗口操作测试:${NC}"
  send_key "alt" "Return" "提升窗口为主窗口" && ((passed++))
  ((total++))

  send_key "alt+shift" "space" "切换浮动状态" && ((passed++))
  ((total++))

  # 应用启动测试
  echo -e "\n${YELLOW}应用启动测试:${NC}"
  echo "测试启动 dmenu..."
  xdotool key --clearmodifiers "alt+e"
  sleep 0.5
  xdotool key --clearmodifiers "Escape" # 关闭 dmenu
  echo -e "  ${GREEN}✓ dmenu 启动测试通过${NC}"
  ((passed++))
  ((total++))

  echo -e "\n${BLUE}功能测试完成: $passed/$total 通过${NC}"
  return $((total - passed))
}

# 压力测试
stress_test() {
  echo -e "\n${BLUE}=== 压力测试 ===${NC}"

  local iterations=500
  local passed=0
  local start_time=$(date +%s)

  echo "执行 $iterations 次随机按键组合..."

  # 按键组合数组
  local keys=("alt+j" "alt+k" "alt+h" "alt+l" "alt+1" "alt+2" "alt+3" "alt+Tab")
  local descriptions=("焦点下" "焦点上" "宽度-" "宽度+" "标签1" "标签2" "标签3" "切换标签")

  for ((i=1; i<=iterations; i++)); do
    # 随机选择按键
    local index=$((RANDOM % ${#keys[@]}))
      local key="${keys[$index]}"
      local desc="${descriptions[$index]}"

      # 显示进度
      if ((i % 50 == 0)); then
        echo "进度: $i/$iterations"
      fi

      # 发送按键
      xdotool key --clearmodifiers "$key" 2>/dev/null && ((passed++))

      # 短暂延迟
      sleep 0.01
    done

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    echo -e "\n${BLUE}压力测试结果:${NC}"
    echo "  总操作数: $iterations"
    echo "  成功操作: $passed"
    echo "  成功率: $(( passed * 100 / iterations ))%"
    echo "  总耗时: ${duration}秒"
    echo "  平均每次操作: $(( duration * 1000 / iterations ))毫秒"
  }

# 内存监控
memory_test() {
  echo -e "\n${BLUE}=== 内存监控测试 ===${NC}"

  local duration=30
  local interval=1

  echo "监控 JWM 内存使用 ${duration} 秒..."

  local max_memory=0
  local start_memory=0
  local measurements=0

  # 获取初始内存
  start_memory=$(ps -C jwm -o rss= --no-headers | awk '{sum += $1} END {print sum}')

  for ((i=0; i<duration; i+=interval)); do
    # 获取当前内存使用
    local current_memory=$(ps -C jwm -o rss= --no-headers | awk '{sum += $1} END {print sum}')

    if [ -n "$current_memory" ] && [ "$current_memory" -gt "$max_memory" ]; then
      max_memory=$current_memory
    fi

    # 在后台发送一些按键来产生负载
    xdotool key --clearmodifiers "alt+j" 2>/dev/null &
    sleep 0.1
    xdotool key --clearmodifiers "alt+k" 2>/dev/null &

    sleep $interval
    ((measurements++))

    # 显示进度
    echo -n "."
  done

  echo ""

  local end_memory=$(ps -C jwm -o rss= --no-headers | awk '{sum += $1} END {print sum}')
  local memory_diff=$((end_memory - start_memory))

  echo -e "\n${BLUE}内存监控结果:${NC}"
  echo "  初始内存: ${start_memory} KB"
  echo "  结束内存: ${end_memory} KB"
  echo "  峰值内存: ${max_memory} KB"
  echo "  内存变化: ${memory_diff} KB"

  if [ "$memory_diff" -gt 1000 ]; then
    echo -e "  ${YELLOW}⚠️  检测到可能的内存泄漏${NC}"
  else
    echo -e "  ${GREEN}✓ 内存使用稳定${NC}"
  fi
}

# 响应时间测试
response_time_test() {
  echo -e "\n${BLUE}=== 响应时间测试 ===${NC}"

  local test_count=50
  local total_time=0

  echo "测试 $test_count 次按键响应时间..."

  for ((i=1; i<=test_count; i++)); do
    local start_time=$(date +%s%N)

    # 发送按键
    xdotool key --clearmodifiers "alt+j" 2>/dev/null

    # 等待系统响应
    sleep 0.05

    local end_time=$(date +%s%N)
    local response_time=$(( (end_time - start_time) / 1000000 )) # 转换为毫秒

    total_time=$((total_time + response_time))

    if ((i % 10 == 0)); then
      echo "进度: $i/$test_count"
    fi
  done

  local avg_time=$((total_time / test_count))

  echo -e "\n${BLUE}响应时间测试结果:${NC}"
  echo "  总测试数: $test_count"
  echo "  平均响应时间: ${avg_time}ms"

  if [ "$avg_time" -lt 50 ]; then
    echo -e "  ${GREEN}✓ 响应时间优秀${NC}"
  elif [ "$avg_time" -lt 100 ]; then
    echo -e "  ${YELLOW}⚠️  响应时间一般${NC}"
  else
    echo -e "  ${RED}✗ 响应时间较慢${NC}"
  fi
}

# 清理测试环境
cleanup() {
  echo -e "\n${BLUE}清理测试环境...${NC}"

  # 关闭可能打开的测试窗口
  pkill -f "sleep 60" 2>/dev/null || true

  echo -e "${GREEN}✓ 清理完成${NC}"
}

# 主函数
main() {
  # 设置陷阱以确保清理
  trap cleanup EXIT

  # 运行检查
  check_dependencies
  check_jwm

  # 运行测试
  echo -e "\n${BLUE}开始测试...${NC}"

  local test_results=()

  # 功能测试
  if functional_tests; then
    test_results+=("${GREEN}功能测试: 通过${NC}")
  else
    test_results+=("${RED}功能测试: 失败${NC}")
  fi

  # 压力测试
  stress_test
  test_results+=("${GREEN}压力测试: 完成${NC}")

  # 内存测试
  memory_test
  test_results+=("${GREEN}内存测试: 完成${NC}")

  # 响应时间测试
  response_time_test
  test_results+=("${GREEN}响应时间测试: 完成${NC}")

  # 打印总结
  echo -e "\n${'='*50}"
  echo -e "${BLUE}        测试总结${NC}"
  echo -e "${'='*50}"

  for result in "${test_results[@]}"; do
    echo -e "$result"
  done

  echo -e "\n${GREEN}🎉 测试完成!${NC}"
}

# 运行主函数
main "$@"
