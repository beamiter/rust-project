# jterm4 中文输入修复 - 更改摘要

## 问题
jterm4 可以渲染中文但无法输入中文。

## 根本原因
1. 事件传播阶段设置为 `Capture`，导致 IME 事件被窗口级控制器拦截
2. 环境变量配置错误：系统运行 `fcitx` 但环境变量设置为 `ibus`

## 修复内容

### 1. 代码修改

#### src/main.rs (第 350-358 行)
添加了 IME 配置调试输出：
```rust
// Print IME configuration for debugging
println!("=== jterm4 IME Configuration ===");
println!("GTK_IM_MODULE: {}", std::env::var("GTK_IM_MODULE")...);
println!("XMODIFIERS: {}", std::env::var("XMODIFIERS")...);
println!("QT_IM_MODULE: {}", std::env::var("QT_IM_MODULE")...);
println!("================================");
```

#### src/main.rs (第 394-398 行)
修改事件传播阶段从 `Capture` 到 `Bubble`：
```rust
// 之前:
key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);

// 之后:
key_controller.set_propagation_phase(gtk4::PropagationPhase::Bubble);
```

这允许 VTE 终端首先处理输入事件（包括 IME），只有未处理的事件才会传播到窗口级快捷键。

### 2. 环境变量配置

#### ~/.bashrc
添加了 fcitx 配置：
```bash
# Fcitx input method configuration (added by jterm4)
export GTK_IM_MODULE=fcitx
export XMODIFIERS=@im=fcitx
export QT_IM_MODULE=fcitx
```

#### ~/.config/fish/config.fish
添加了 fcitx 配置：
```fish
# Fcitx input method configuration (added by jterm4)
set -gx GTK_IM_MODULE fcitx
set -gx XMODIFIERS @im=fcitx
set -gx QT_IM_MODULE fcitx
```

### 3. 新增文件

- **run-with-fcitx.sh** - fcitx 启动脚本（设置环境变量并运行 jterm4）
- **test-ime.sh** - IME 配置检查脚本
- **IME-SETUP.md** - 完整的中文输入设置指南
- **CHANGES.md** - 本文件

## 使用方法

### 新终端中（推荐）
1. 关闭当前终端
2. 打开新终端
3. 运行: `cd ~/projects/rust-project/jterm4 && cargo run`
4. 按 `Ctrl+Space` 切换到中文输入

### 当前终端中（临时）
```bash
./run-with-fcitx.sh
```

### 验证配置
```bash
./test-ime.sh
```

## 技术细节

### 事件流程（修复后）
```
用户输入
  ↓
GTK4 事件系统
  ↓
fcitx IME 处理
  ↓
VTE 终端接收并处理（Bubble 阶段第一优先级）
  ↓
如果未处理 → 窗口级快捷键处理器
```

### 支持的功能
- ✅ 中文输入（fcitx/ibus）
- ✅ 中文渲染
- ✅ 拼音输入预览（preedit）
- ✅ 候选词选择
- ✅ 所有快捷键仍然正常工作
- ✅ 英文输入
- ✅ 其他语言 IME 支持

## 文件变更列表

```
修改:
  src/main.rs               - IME 支持代码修改
  ~/.bashrc                 - 添加 fcitx 环境变量
  ~/.config/fish/config.fish - 添加 fcitx 环境变量

新增:
  run-with-fcitx.sh         - fcitx 启动脚本
  test-ime.sh               - IME 测试脚本
  IME-SETUP.md              - 中文输入设置指南
  CHANGES.md                - 本文件
```

## 故障排除

如果中文输入仍然不工作：

1. 运行 `./test-ime.sh` 检查配置
2. 确保从新终端启动 jterm4（不是旧终端）
3. 检查 fcitx 是否在运行：`ps aux | grep fcitx`
4. 查看启动时的 IME 配置输出
5. 查阅 `IME-SETUP.md` 获取详细说明

## 参考资源

- [GTK4 输入法文档](https://docs.gtk.org/gtk4/input-method.html)
- [VTE4 文档](https://gnome.pages.gitlab.gnome.org/vte/)
- [Fcitx 文档](https://fcitx-im.org/)
