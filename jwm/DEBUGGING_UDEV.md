# JWM Wayland udev 后端调试指南

## 1. 基础日志调试

### 1.1 启用日志输出

在 TTY 中运行 JWM 时直接看到日志：

```bash
# 进入 TTY (Ctrl+Alt+F2)
cd ~/projects/rust-project/jwm

# 运行时指定日志级别
RUST_LOG=debug ./target/release/jwm 2>&1 | tee jwm.log

# 或只看特定模块的日志
RUST_LOG=jwm=debug ./target/release/jwm

# 更详细的日志
RUST_LOG=trace ./target/release/jwm
```

### 1.2 代码中的调试日志点

已有的日志位置（参考搜索）：
- `backend.rs`: 事件处理、窗口创建/销毁、DRM 初始化
- `state.rs`: Wayland 协议事件、surface 管理
- `udev_kms.rs`: 渲染帧、KMS 设备状态
- `jwm.rs`: 窗口管理、布局计算、快捷键处理

### 1.3 添加调试日志

```rust
// 在你想调试的地方添加
use log::{debug, info, warn, error};

debug!("[udev:backend] Event loop iteration");
info!("[udev:output] Added output: {:?}", output_info);
warn!("[udev:drm] Failed to set mode, trying fallback");
error!("[udev:rendering] Render frame failed: {:?}", e);
```

## 2. 远程调试（推荐）

### 2.1 使用第二个 TTY 运行 JWM

这是**最安全的方法**：

```bash
# TTY1: 启动一个基本的 X11 窗口管理器或其他 WM
Xvfb :1 -screen 0 1920x1080x24 &
export DISPLAY=:1

# TTY2: 运行 JWM（从 TTY 启动）
cd ~/projects/rust-project/jwm
RUST_LOG=debug ./target/release/jwm 2>&1 | tee jwm.log

# TTY3: 查看日志、杀进程、调试
tail -f jwm.log
ps aux | grep jwm
kill <pid>
```

### 2.2 SSH 远程调试

如果机器支持 SSH：

```bash
# 本地机器上
ssh user@remote_machine

# 远程机器上
RUST_LOG=debug ./target/release/jwm 2>&1 | tee /tmp/jwm.log

# 另一个 SSH 会话
ssh user@remote_machine
tail -f /tmp/jwm.log
```

## 3. 特定功能调试

### 3.1 调试键盘输入

```bash
# 启用键盘调试
JWM_DEBUG_KEYS=1 RUST_LOG=debug ./target/release/jwm

# 日志会显示：
# [key] keycode=KEY evdev_keycode=X mods_raw=0x1 mods_clean=0x1
```

### 3.2 调试显示器/输出

```rust
// 在 backend.rs 中查找输出初始化
info!("[udev:output] Enumerated output: name={}, geo={:?}",
      output_info.name, (output_info.x, output_info.y, output_info.width, output_info.height));
```

添加临时日志：
```bash
RUST_LOG=smithay=debug ./target/release/jwm  # Smithay 库的日志
```

### 3.3 调试窗口创建

```bash
RUST_LOG=jwm::backend::wayland_udev=debug ./target/release/jwm
```

会看到：
```
[udev/wayland] toplevel_created win=...
[udev/wayland] toplevel_configured ...
```

### 3.4 调试渲染

```bash
RUST_LOG=jwm::backend::udev_kms=debug ./target/release/jwm
```

## 4. 低级调试工具

### 4.1 DRM 设备检查

```bash
# 查看可用的 DRM 设备
ls -la /dev/dri/

# 查看输出连接器
drm_info

# 监控 DRM 事件
drm_monitor
```

### 4.2 libinput 调试

```bash
# 查看输入设备
libinput list-devices

# 监控输入事件
libinput debug-events

# 测试键盘
libinput debug-keyboard
```

### 4.3 Wayland 协议跟踪

```bash
# 安装 wlr-protocols-debug 工具
WAYLAND_DEBUG=1 ./target/release/jwm 2>&1 | head -100

# 会显示所有 Wayland 消息
```

## 5. 构建调试版本

### 5.1 启用调试符号和优化

```bash
# 调试版本（有符号，无优化）
cargo build

# 发布版本但带调试符号
cargo build --release

# 设置 strip=false 保留符号
RUSTFLAGS="-g" cargo build --release
```

### 5.2 使用 gdb

```bash
# 准备调试版本
cargo build

# 用 gdb 启动
gdb --args ./target/debug/jwm

# gdb 命令
(gdb) run                    # 启动程序
(gdb) break backend.rs:500  # 设置断点
(gdb) continue              # 继续
(gdb) next                  # 单步
(gdb) print var_name        # 打印变量
(gdb) bt                    # 显示回溯
```

## 6. 环境变量控制

### 6.1 调试环保变量

```bash
# 键盘调试
JWM_DEBUG_KEYS=1

# 自动启动终端（方便测试）
JWM_AUTOSTART_TERMINAL=1

# 指定后端
JWM_BACKEND=udev

# 日志级别
RUST_LOG=debug
RUST_BACKTRACE=1
RUST_BACKTRACE=full

# Wayland 协议调试
WAYLAND_DEBUG=1

# Smithay 特定
SMITHAY_DEBUG=1
```

### 6.2 完整调试启动

```bash
#!/bin/bash
# debug_jwm.sh

export RUST_LOG=debug
export RUST_BACKTRACE=full
export JWM_DEBUG_KEYS=1
export JWM_AUTOSTART_TERMINAL=1
export WAYLAND_DEBUG=1

cd ~/projects/rust-project/jwm
./target/debug/jwm 2>&1 | tee debug_$(date +%s).log
```

## 7. 常见问题排查

### 7.1 无法初始化 DRM

日志特征：
```
[udev:kms] Failed to initialize KMS: ...
```

排查步骤：
```bash
# 检查权限
ls -la /dev/dri/renderD128
groups $USER

# 添加到 video 组
sudo usermod -aG video $USER

# 检查 seat 权限
loginctl seat-status
```

### 7.2 没有输出显示

日志特征：
```
[udev:output] No outputs detected
```

排查步骤：
```bash
# 检查连接器
drm_info | grep "^Connector"

# 检查 DRM 模式
cat /sys/class/drm/*/status

# 测试 DRM 模式设置
modetest -M amdgpu  # 或其他驱动
```

### 7.3 键盘/鼠标无反应

启用日志：
```bash
JWM_DEBUG_KEYS=1 RUST_LOG=debug ./target/release/jwm
```

检查点：
```bash
# 检查 libinput 设备
libinput list-devices | grep -E "Keyboard|Pointer"

# 检查事件接收
libinput debug-events
```

### 7.4 窗口无法创建

日志特征：
```
[udev/wayland] toplevel_created failed
```

添加临时日志到 `state.rs:new_toplevel()`:
```rust
info!("[debug] new_toplevel called for surface {:?}", surface.wl_surface().id());
```

## 8. 性能分析

### 8.1 使用 perf

```bash
# 记录性能数据
sudo perf record -g ./target/release/jwm

# 分析
sudo perf report

# 火焰图
sudo perf script | stackcollapse-perf.pl | flamegraph.pl > out.svg
```

### 8.2 渲染性能

在 `udev_kms.rs` 中添加时间戳：
```rust
let start = std::time::Instant::now();
// ... render frame ...
info!("[perf] Frame rendered in {:?}", start.elapsed());
```

## 9. 快速调试清单

```
□ 启用 RUST_LOG=debug
□ 检查 DRM 设备和权限
□ 检查 libinput 设备
□ 查看 jwm.log 中的错误
□ 启用 JWM_DEBUG_KEYS=1 测试输入
□ 在第二个 TTY 中运行，方便查看日志
□ 使用 gdb 设置断点
□ 检查 Wayland socket 权限
```

## 10. 生成完整的调试报告

```bash
#!/bin/bash
# collect_debug_info.sh

echo "=== System Info ==="
uname -a
lsb_release -a

echo "=== DRM Devices ==="
ls -la /dev/dri/
drm_info 2>/dev/null || echo "drm_info not available"

echo "=== Input Devices ==="
libinput list-devices 2>/dev/null || echo "libinput not available"

echo "=== Environment ==="
env | grep -E "DISPLAY|WAYLAND|XDG"

echo "=== JWM Build Info ==="
cd ~/projects/rust-project/jwm
rustc --version
cargo --version

echo "=== Running JWM with debug ==="
RUST_LOG=debug RUST_BACKTRACE=full ./target/debug/jwm 2>&1 | head -200
```

运行生成报告：
```bash
bash collect_debug_info.sh > jwm_debug_report.txt 2>&1
```

这样可以快速诊断问题并提交 issue！
