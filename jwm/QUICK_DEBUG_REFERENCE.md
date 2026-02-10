# JWM udev 后端调试速查表

## 快速开始

```bash
# 1. 检查系统
./scripts/check_system.sh

# 2. 构建调试版本
cargo build --features backend-udev

# 3. 运行调试
./scripts/debug_jwm.sh
```

## 调试模式

### 基础日志调试
```bash
RUST_LOG=debug ./target/debug/jwm 2>&1 | tee jwm.log
```

### 键盘调试
```bash
JWM_DEBUG_KEYS=1 RUST_LOG=debug ./target/debug/jwm
```

### Wayland 协议调试
```bash
WAYLAND_DEBUG=1 RUST_LOG=debug ./target/debug/jwm
```

### 全调试
```bash
RUST_LOG=trace RUST_BACKTRACE=full WAYLAND_DEBUG=1 JWM_DEBUG_KEYS=1 ./target/debug/jwm
```

## 使用 GDB

```bash
# 启动 GDB
gdb ./target/debug/jwm

# GDB 命令
(gdb) run                              # 启动
(gdb) break backend.rs:500             # 断点
(gdb) break UdevBackend::new           # 函数断点
(gdb) continue                         # 继续
(gdb) next                             # 单步（跳过函数）
(gdb) step                             # 单步（进入函数）
(gdb) print variable_name              # 打印变量
(gdb) bt                               # 回溯
(gdb) info locals                     # 本地变量
(gdb) quit                            # 退出
```

## 日志级别

| 级别 | 用途 |
|------|------|
| `error` | 仅错误 |
| `warn` | 警告 + 错误 |
| `info` | 常规信息 |
| `debug` | 详细调试信息（推荐） |
| `trace` | 最详细（性能影响大） |

## 模块化日志

```bash
# 只看特定模块
RUST_LOG=jwm::backend::wayland_udev=debug ./target/debug/jwm

# 多个模块
RUST_LOG=jwm::backend=debug,smithay=info ./target/debug/jwm

# 排除噪音模块
RUST_LOG=debug,smithay::backend::drm=warn ./target/debug/jwm
```

## 常见问题

### ❌ 无法打开 /dev/dri/card0
```bash
# 检查权限
ls -la /dev/dri/

# 添加到 video 组
sudo usermod -aG video $USER
# 重新登录
```

### ❌ Failed to initialize libinput
```bash
# 检查 input 设备
libinput list-devices

# 添加到 input 组
sudo usermod -aG input $USER
```

### ❌ No outputs detected
```bash
# 检查输出
drm_info | grep Connector

# 检查状态
cat /sys/class/drm/card*/status
```

### ❌ Keyboard not working
```bash
# 启用键盘调试
JWM_DEBUG_KEYS=1 RUST_LOG=debug ./target/debug/jwm

# 检查 libinput
libinput debug-events
```

## 性能分析

```bash
# CPU 分析
perf record -g ./target/release/jwm
perf report

# 内存分析
valgrind --leak-check=full ./target/debug/jwm
```

## 远程调试

### 方法 1: 多个 TTY
```bash
# TTY1: 运行 JWM
./scripts/debug_remote.sh

# TTY2: 查看日志
tail -f /tmp/jwm_logs/debug_*.log

# TTY3: 控制（如需要）
kill $(cat /tmp/jwm_logs/jwm.pid)
```

### 方法 2: SSH
```bash
# SSH 会话 1
ssh user@machine
RUST_LOG=debug ./target/debug/jwm 2>&1 | tee /tmp/jwm.log

# SSH 会话 2
ssh user@machine
tail -f /tmp/jwm.log
```

## 崩溃调试

### 启用 coredump
```bash
ulimit -c unlimited
echo "/tmp/core.%e.%p" | sudo tee /proc/sys/kernel/core_pattern
```

### 分析 coredump
```bash
gdb ./target/debug/jwm /tmp/core.jwm.12345
(gdb) bt full
```

## 调试检查清单

- [ ] 运行 `./scripts/check_system.sh`
- [ ] 确认在 TTY 中运行（Ctrl+Alt+F2）
- [ ] 启用 `RUST_LOG=debug`
- [ ] 检查 DRM 设备权限
- [ ] 检查 libinput 设备
- [ ] 查看日志文件中的错误
- [ ] 测试键盘输入（`JWM_DEBUG_KEYS=1`）
- [ ] 使用第二个 TTY 查看实时日志

## 关键代码位置

| 功能 | 文件 | 行数范围 |
|------|------|---------|
| 后端初始化 | `backend/wayland_udev/backend.rs` | ~700-900 |
| 窗口创建 | `backend/wayland_udev/state.rs` | ~600-700 |
| 渲染循环 | `backend/udev_kms.rs` | ~300-700 |
| 事件处理 | `backend/wayland_udev/backend.rs` | ~850-1300 |
| 布局计算 | `core/layout.rs` | 全文件 |
| WM 逻辑 | `jwm.rs` | 全文件 |

## 有用的命令

```bash
# 查找崩溃位置
RUST_BACKTRACE=1 ./target/debug/jwm

# 监控系统调用
strace -e openat,ioctl ./target/debug/jwm

# 检查依赖
ldd ./target/debug/jwm

# 检查编译选项
cargo rustc -- --print cfg
```

## 测试终端命令

```bash
# 自动启动终端
JWM_AUTOSTART_TERMINAL=1 ./target/debug/jwm

# 指定终端
JWM_AUTOSTART_TERMINAL_CMD="foot" ./target/debug/jwm

# 自定义命令
JWM_AUTOSTART_TERMINAL_CMD="alacritty -e bash" ./target/debug/jwm
```

---

**需要帮助？**
1. 查看完整文档：`DEBUGGING_UDEV.md`
2. 运行系统检查：`./scripts/check_system.sh`
3. 提交 issue：https://github.com/anthropics/jwm/issues
