# JWM Wayland udev 后端调试资源

## 📚 文档

| 文档 | 描述 |
|------|------|
| **DEBUGGING_UDEV.md** | 完整调试指南（10个章节，详细说明） |
| **QUICK_DEBUG_REFERENCE.md** | 快速参考卡片（常用命令和技巧） |
| **.gdbinit** | GDB 调试器配置文件 |

## 🛠️ 调试脚本

所有脚本位于 `scripts/` 目录：

| 脚本 | 用途 | 使用方法 |
|------|------|----------|
| **check_system.sh** | 系统诊断（检查权限、设备、依赖） | `./scripts/check_system.sh` |
| **debug_jwm.sh** | 快速启动调试模式 | `./scripts/debug_jwm.sh` |
| **debug_remote.sh** | 远程调试（日志输出到文件） | `./scripts/debug_remote.sh` |

## 🚀 快速开始

### 1. 检查系统

```bash
./scripts/check_system.sh
```

这会检查：
- DRM 设备和权限
- Input 设备
- 用户组成员
- 必要的库
- JWM 构建状态

### 2. 第一次调试

```bash
# 构建调试版本
cargo build

# 从 TTY 运行（Ctrl+Alt+F2 进入 TTY）
./scripts/debug_jwm.sh
```

### 3. 查看问题

调试脚本会：
- 自动启用所有日志
- 自动启动终端（方便测试）
- 将日志保存到 `/tmp/jwm_debug_*.log`

## 🔍 常用调试场景

### 场景 1: 键盘不工作

```bash
JWM_DEBUG_KEYS=1 RUST_LOG=debug ./target/debug/jwm
```

日志会显示每个按键事件。

### 场景 2: 窗口创建失败

```bash
RUST_LOG=jwm::backend::wayland_udev=debug ./target/debug/jwm
```

查找 `[udev/wayland] toplevel_created` 日志。

### 场景 3: 渲染问题

```bash
RUST_LOG=jwm::backend::udev_kms=debug ./target/debug/jwm
```

查找 `[KmsState]` 和渲染相关日志。

### 场景 4: 使用 GDB

```bash
gdb ./target/debug/jwm
(gdb) run
# 等待崩溃
(gdb) bt      # 查看调用栈
```

## 📊 诊断流程

```
1. check_system.sh
   ↓
   系统正常？
   ↓ 是
2. debug_jwm.sh
   ↓
   看到错误？
   ↓ 是
3. 查看 DEBUGGING_UDEV.md 对应章节
   ↓
4. 根据错误类型：
   - DRM 错误 → 检查权限和设备
   - Input 错误 → 检查 libinput
   - Wayland 错误 → 启用 WAYLAND_DEBUG=1
   - 崩溃 → 使用 GDB
```

## ⚡ 最快的调试方法

**方法 1: 单个 TTY**（推荐新手）

```bash
# 切到 TTY2 (Ctrl+Alt+F2)
cd ~/projects/rust-project/jwm
./scripts/debug_jwm.sh
# 直接看日志输出
```

**方法 2: 多个 TTY**（推荐高级用户）

```bash
# TTY2: 运行 JWM
./scripts/debug_remote.sh

# TTY3 (Ctrl+Alt+F3): 实时查看日志
tail -f /tmp/jwm_logs/debug_*.log

# TTY4 (Ctrl+Alt+F4): 控制/诊断
ps aux | grep jwm
kill $(cat /tmp/jwm_logs/jwm.pid)
```

**方法 3: SSH 远程**（如果可用）

```bash
# SSH 会话 1: 运行
ssh user@machine
./scripts/debug_remote.sh

# SSH 会话 2: 监控
ssh user@machine
tail -f /tmp/jwm_logs/debug_*.log
```

## 🆘 常见问题速查

| 症状 | 可能原因 | 解决方法 |
|------|----------|----------|
| `Permission denied /dev/dri` | 用户不在 video 组 | `sudo usermod -aG video $USER` + 重新登录 |
| `Failed to initialize libinput` | 权限问题 | `sudo usermod -aG input $USER` |
| `No outputs detected` | DRM 配置问题 | 运行 `drm_info` 检查 |
| 键盘无反应 | libinput 未检测到设备 | `libinput list-devices` |
| 窗口显示空白 | 渲染问题 | 检查 OpenGL ES 驱动 |
| 立即崩溃 | 依赖缺失 | `ldd ./target/debug/jwm` |

## 📝 提交 Bug 报告

如果需要报告问题：

```bash
# 1. 收集系统信息
./scripts/check_system.sh > system_info.txt 2>&1

# 2. 运行调试版本
RUST_LOG=debug RUST_BACKTRACE=full ./target/debug/jwm 2>&1 | tee debug.log

# 3. 附加文件
#    - system_info.txt
#    - debug.log (前 200 行)
#    - 错误描述
```

## 🔧 高级调试

### 性能分析

```bash
# CPU profiling
perf record -g ./target/release/jwm
perf report

# 内存泄漏
valgrind --leak-check=full ./target/debug/jwm
```

### 协议追踪

```bash
# Wayland 协议消息
WAYLAND_DEBUG=1 ./target/debug/jwm 2>&1 | head -500

# DRM ioctl 调用
strace -e ioctl ./target/debug/jwm 2>&1 | grep DRM
```

### 自定义日志

在代码中添加：

```rust
use log::{debug, info, warn, error};

debug!("[mymodule] Variable x = {:?}", x);
info!("[mymodule] Important event happened");
warn!("[mymodule] Something unexpected");
error!("[mymodule] Critical error: {:?}", err);
```

## 📖 推荐阅读顺序

1. **新手**:
   - `QUICK_DEBUG_REFERENCE.md` → 快速上手
   - 运行 `./scripts/check_system.sh`
   - 运行 `./scripts/debug_jwm.sh`

2. **有问题时**:
   - `DEBUGGING_UDEV.md` → 查找对应章节
   - 章节 7（常见问题排查）

3. **深入调试**:
   - `DEBUGGING_UDEV.md` 章节 4-6（特定功能调试）
   - 使用 GDB（章节 5）

## 🎯 调试目标

- [ ] 系统检查通过
- [ ] JWM 能启动（即使有错误）
- [ ] 能看到日志输出
- [ ] 识别具体错误类型
- [ ] 根据文档解决问题

## 💡 提示

- **总是从 TTY 运行** (Ctrl+Alt+F2)，不要在 X11/Wayland 会话中
- **使用调试版本** (`cargo build` 不加 `--release`)
- **先运行 check_system.sh** 排除系统问题
- **保存日志** 方便后续分析
- **使用多个 TTY** 方便实时监控

---

**需要更多帮助？**

- 详细文档：`DEBUGGING_UDEV.md`
- 快速参考：`QUICK_DEBUG_REFERENCE.md`
- 系统检查：`./scripts/check_system.sh`
