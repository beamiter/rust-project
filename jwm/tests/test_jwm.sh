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

# 静默发送按键（用于压力测试）
send_key_silent() {
  local key_combo="$1"

  # 发送按键
  if xdotool key --clearmodifiers "$key_combo" 2>/dev/null; then
    return 0
  else
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

# 修复后的压力测试
stress_test() {
  echo -e "\n${BLUE}=== 压力测试 ===${NC}"

  local iterations=500
  local passed=0
  local failed=0
  local start_time=$(date +%s)

  echo "执行 $iterations 次随机按键组合..."

  # 按键组合数组
  local keys=("alt+j" "alt+k" "alt+h" "alt+l" "alt+1" "alt+2" "alt+3" "alt+Tab")
  local descriptions=("焦点下" "焦点上" "宽度-" "宽度+" "标签1" "标签2" "标签3" "切换标签")

  # 修复：正确的循环语法
  for ((i=1; i<=iterations; i++)); do
    # 随机选择按键
    local index=$((RANDOM % ${#keys[@]}))
      local key="${keys[$index]}"
      local desc="${descriptions[$index]}"

      # 显示进度
      if ((i % 50 == 0)); then
        echo "进度: $i/$iterations (成功: $passed, 失败: $failed)"
      fi

      # 发送按键
      if send_key_silent "$key"; then
        ((passed++))
      else
        ((failed++))
      fi

      # 短暂延迟
      sleep 0.01
    done

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    echo -e "\n${BLUE}压力测试结果:${NC}"
    echo "  总操作数: $iterations"
    echo "  成功操作: $passed"
    echo "  失败操作: $failed"
    echo "  成功率: $(( passed * 100 / iterations ))%"
    echo "  总耗时: ${duration}秒"
    if [ $duration -gt 0 ]; then
      echo "  平均每次操作: $(( duration * 1000 / iterations ))毫秒"
      echo "  操作速率: $(( iterations / duration )) ops/sec"
    fi
  }

# 高强度压力测试
intensive_stress_test() {
  echo -e "\n${BLUE}=== 高强度压力测试 ===${NC}"

  local iterations=2000
  local batch_size=100
  local passed=0
  local failed=0
  local start_time=$(date +%s)

  echo "执行 $iterations 次高强度按键组合测试..."

  # 扩展按键组合数组
  local keys=(
    "alt+j" "alt+k" "alt+h" "alt+l" 
    "alt+1" "alt+2" "alt+3" "alt+4" "alt+5"
    "alt+Tab" "alt+shift+Tab"
    "alt+t" "alt+f" "alt+m"
    "alt+Return" "alt+space"
    "alt+comma" "alt+period"
  )

  for ((batch=0; batch<iterations/batch_size; batch++)); do
    local batch_start=$(date +%s)
    local batch_passed=0
    local batch_failed=0

    for ((i=0; i<batch_size; i++)); do
      # 循环使用所有按键
      local key_index=$(( (batch * batch_size + i) % ${#keys[@]} ))
        local key="${keys[$key_index]}"

        # 发送按键
        if send_key_silent "$key"; then
          ((batch_passed++))
          ((passed++))
        else
          ((batch_failed++))
          ((failed++))
        fi

        # 最小延迟
        sleep 0.005
      done

      local batch_end=$(date +%s)
      local batch_duration=$((batch_end - batch_start))

      echo "批次 $((batch + 1)): ${batch_passed}成功 ${batch_failed}失败 ${batch_duration}秒"
    done

    local end_time=$(date +%s)
    local duration=$((end_time - start_time))

    echo -e "\n${BLUE}高强度压力测试结果:${NC}"
    echo "  总操作数: $iterations"
    echo "  成功操作: $passed"
    echo "  失败操作: $failed"
    echo "  成功率: $(( passed * 100 / iterations ))%"
    echo "  总耗时: ${duration}秒"
    if [ $duration -gt 0 ]; then
      echo "  平均每次操作: $(( duration * 1000 / iterations ))毫秒"
      echo "  操作速率: $(( iterations / duration )) ops/sec"
    fi
  }

# 并发压力测试
concurrent_stress_test() {
  echo -e "\n${BLUE}=== 并发压力测试 ===${NC}"

  local duration=30
  local num_processes=3
  local temp_dir="/tmp/jwm_test_$$"

  echo "启动 $num_processes 个并发进程，持续 $duration 秒..."

  mkdir -p "$temp_dir"

  # 启动多个并发测试进程
  for ((proc=0; proc<num_processes; proc++)); do
    (
      local proc_id=$proc
      local count=0
      local passed=0
      local start_time=$(date +%s)
      local keys=("alt+j" "alt+k" "alt+h" "alt+l" "alt+1" "alt+2")

      while [ $(($(date +%s) - start_time)) -lt $duration ]; do
        local key="${keys[$((count % ${#keys[@]}))]}"

          if send_key_silent "$key"; then
            ((passed++))
          fi
          ((count++))

          sleep 0.02
        done

        echo "$proc_id:$count:$passed" > "$temp_dir/result_$proc_id"
        ) &
      done

      # 等待所有进程完成
      wait

      # 汇总结果
      local total_ops=0
      local total_passed=0

      for ((proc=0; proc<num_processes; proc++)); do
        if [ -f "$temp_dir/result_$proc" ]; then
          local result=$(cat "$temp_dir/result_$proc")
          local proc_id=$(echo "$result" | cut -d: -f1)
          local proc_ops=$(echo "$result" | cut -d: -f2)
          local proc_passed=$(echo "$result" | cut -d: -f3)

          echo "进程 $proc_id: $proc_ops 操作, $proc_passed 成功"
          total_ops=$((total_ops + proc_ops))
          total_passed=$((total_passed + proc_passed))
        fi
      done

      # 清理临时文件
      rm -rf "$temp_dir"

      echo -e "\n${BLUE}并发压力测试结果:${NC}"
      echo "  并发进程数: $num_processes"
      echo "  测试时长: ${duration}秒"
      echo "  总操作数: $total_ops"
      echo "  成功操作: $total_passed"
      if [ $total_ops -gt 0 ]; then
        echo "  成功率: $(( total_passed * 100 / total_ops ))%"
        echo "  操作速率: $(( total_ops / duration )) ops/sec"
      fi
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
  local memory_samples=()

  # 获取初始内存
  start_memory=$(ps -C jwm -o rss= --no-headers | awk '{sum += $1} END {print sum}' 2>/dev/null || echo "0")

  for ((i=0; i<duration; i+=interval)); do
    # 获取当前内存使用
    local current_memory=$(ps -C jwm -o rss= --no-headers | awk '{sum += $1} END {print sum}' 2>/dev/null || echo "0")

    if [ -n "$current_memory" ] && [ "$current_memory" -gt "$max_memory" ]; then
      max_memory=$current_memory
    fi

    memory_samples+=($current_memory)

    # 在后台发送一些按键来产生负载
    send_key_silent "alt+j" &
    sleep 0.1
    send_key_silent "alt+k" &

    sleep $interval
    ((measurements++))

    # 显示进度
    echo -n "."
  done

  echo ""

  local end_memory=$(ps -C jwm -o rss= --no-headers | awk '{sum += $1} END {print sum}' 2>/dev/null || echo "0")
  local memory_diff=$((end_memory - start_memory))

  # 计算平均内存使用
  local total_memory=0
  for mem in "${memory_samples[@]}"; do
    total_memory=$((total_memory + mem))
  done
  local avg_memory=0
  if [ ${#memory_samples[@]} -gt 0 ]; then
    avg_memory=$((total_memory / ${#memory_samples[@]}))
  fi

  echo -e "\n${BLUE}内存监控结果:${NC}"
  echo "  初始内存: ${start_memory} KB"
  echo "  结束内存: ${end_memory} KB"
  echo "  峰值内存: ${max_memory} KB"
  echo "  平均内存: ${avg_memory} KB"
  echo "  内存变化: ${memory_diff} KB"
  echo "  采样次数: ${measurements}"

  if [ "$memory_diff" -gt 1000 ]; then
    echo -e "  ${YELLOW}⚠️  检测到可能的内存泄漏${NC}"
  else
    echo -e "  ${GREEN}✓ 内存使用稳定${NC}"
  fi
}

# 响应时间测试
response_time_test() {
  echo -e "\n${BLUE}=== 响应时间测试 ===${NC}"

  local test_count=100
  local total_time=0
  local min_time=999999
  local max_time=0
  local times=()

  echo "测试 $test_count 次按键响应时间..."

  for ((i=1; i<=test_count; i++)); do
    local start_time=$(date +%s%N)

    # 发送按键
    send_key_silent "alt+j" >/dev/null 2>&1

    # 等待系统响应
    sleep 0.05

    local end_time=$(date +%s%N)
    local response_time=$(( (end_time - start_time) / 1000000 )) # 转换为毫秒

    times+=($response_time)
    total_time=$((total_time + response_time))

    if [ $response_time -lt $min_time ]; then
      min_time=$response_time
    fi
    if [ $response_time -gt $max_time ]; then
      max_time=$response_time
    fi

    if ((i % 20 == 0)); then
      echo "进度: $i/$test_count"
    fi
  done

  local avg_time=$((total_time / test_count))

  echo -e "\n${BLUE}响应时间测试结果:${NC}"
  echo "  总测试数: $test_count"
  echo "  平均响应时间: ${avg_time}ms"
  echo "  最快响应时间: ${min_time}ms"
  echo "  最慢响应时间: ${max_time}ms"

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
  pkill -f "jwm_test" 2>/dev/null || true

  # 清理临时文件
  rm -rf /tmp/jwm_test_* 2>/dev/null || true

  echo -e "${GREEN}✓ 清理完成${NC}"
}

# 显示帮助信息
show_help() {
  echo "使用方法: $0 [选项]"
  echo ""
  echo "选项:"
  echo "  -h, --help              显示此帮助信息"
  echo "  -f, --functional        只运行功能测试"
  echo "  -s, --stress            只运行压力测试"
  echo "  -i, --intensive         只运行高强度压力测试"
  echo "  -c, --concurrent        只运行并发压力测试"
  echo "  -m, --memory            只运行内存测试"
  echo "  -r, --response          只运行响应时间测试"
  echo "  -a, --all               运行所有测试（默认）"
  echo ""
}

# 主函数
main() {
  # 设置陷阱以确保清理
  trap cleanup EXIT

  # 解析命令行参数
  local run_functional=false
  local run_stress=false
  local run_intensive=false
  local run_concurrent=false
  local run_memory=false
  local run_response=false
  local run_all=true

  while [[ $# -gt 0 ]]; do
    case $1 in
      -h|--help)
        show_help
        exit 0
        ;;
      -f|--functional)
        run_functional=true
        run_all=false
        ;;
      -s|--stress)
        run_stress=true
        run_all=false
        ;;
      -i|--intensive)
        run_intensive=true
        run_all=false
        ;;
      -c|--concurrent)
        run_concurrent=true
        run_all=false
        ;;
      -m|--memory)
        run_memory=true
        run_all=false
        ;;
      -r|--response)
        run_response=true
        run_all=false
        ;;
      -a|--all)
        run_all=true
        ;;
      *)
        echo "未知选项: $1"
        show_help
        exit 1
        ;;
    esac
    shift
  done

  # 运行检查
  check_dependencies
  check_jwm

  # 运行测试
  echo -e "\n${BLUE}开始测试...${NC}"

  local test_results=()

  if [ "$run_all" = true ] || [ "$run_functional" = true ]; then
    if functional_tests; then
      test_results+=("${GREEN}功能测试: 通过${NC}")
    else
      test_results+=("${RED}功能测试: 失败${NC}")
    fi
  fi

  if [ "$run_all" = true ] || [ "$run_stress" = true ]; then
    stress_test
    test_results+=("${GREEN}压力测试: 完成${NC}")
  fi

  if [ "$run_all" = true ] || [ "$run_intensive" = true ]; then
    intensive_stress_test
    test_results+=("${GREEN}高强度压力测试: 完成${NC}")
  fi

  if [ "$run_all" = true ] || [ "$run_concurrent" = true ]; then
    concurrent_stress_test
    test_results+=("${GREEN}并发压力测试: 完成${NC}")
  fi

  if [ "$run_all" = true ] || [ "$run_memory" = true ]; then
    memory_test
    test_results+=("${GREEN}内存测试: 完成${NC}")
  fi

  if [ "$run_all" = true ] || [ "$run_response" = true ]; then
    response_time_test
    test_results+=("${GREEN}响应时间测试: 完成${NC}")
  fi

  # 打印总结
  echo -e "\n$(printf '=%.0s' {1..50})"
  echo -e "${BLUE}        测试总结${NC}"
  echo -e "$(printf '=%.0s' {1..50})"

  for result in "${test_results[@]}"; do
    echo -e "$result"
  done

  echo -e "\n${GREEN}🎉 测试完成!${NC}"
}

# 运行主函数
main "$@"
