# jterm4 中文输入法 (IME) 设置指南

## 问题诊断

jterm4 支持中文输入，但需要正确的环境变量配置。

### 检查你的输入法

首先确认你正在使用哪个输入法：

```bash
ps aux | grep -E "fcitx|ibus" | grep -v grep
```

### 检查环境变量

```bash
echo "GTK_IM_MODULE=$GTK_IM_MODULE"
echo "XMODIFIERS=$XMODIFIERS"
echo "QT_IM_MODULE=$QT_IM_MODULE"
```

**重要**: 环境变量必须与你实际运行的输入法匹配！

---

## 解决方案

### 已完成：环境变量已添加到配置文件

fcitx 环境变量已经添加到：
- ✅ `~/.bashrc`
- ✅ `~/.config/fish/config.fish`

### 立即生效的方法

**重要**：配置文件的修改只对**新启动的终端**生效。

#### 选项 1: 重启终端 (推荐)

关闭当前所有终端窗口，然后打开一个新终端，环境变量会自动生效。

#### 选项 2: 重新登录

注销并重新登录你的桌面会话，所有应用都会使用新的环境变量。

#### 选项 3: 在当前 shell 中手动设置 (临时)

如果不想重启终端，在当前 shell 中运行：

**Bash:**
```bash
export GTK_IM_MODULE=fcitx
export XMODIFIERS=@im=fcitx
export QT_IM_MODULE=fcitx
```

**Fish:**
```fish
set -gx GTK_IM_MODULE fcitx
set -gx XMODIFIERS @im=fcitx
set -gx QT_IM_MODULE fcitx
```

#### 选项 4: 使用启动脚本

```bash
./run-with-fcitx.sh
```

### 验证配置

运行测试脚本检查环境变量是否正确：

```bash
./test-ime.sh
```

你应该看到：
```
✓ fcitx is running
✓ GTK_IM_MODULE is correctly set to fcitx
```

### 方案 1: 使用启动脚本 (推荐 for fcitx)

如果你使用 **fcitx**:

```bash
./run-with-fcitx.sh
```

这个脚本会自动设置正确的环境变量。

### 方案 2: 手动设置环境变量

#### 对于 fcitx 用户:

```bash
export GTK_IM_MODULE=fcitx
export XMODIFIERS=@im=fcitx
export QT_IM_MODULE=fcitx

cargo run
```

#### 对于 ibus 用户:

```bash
export GTK_IM_MODULE=ibus
export XMODIFIERS=@im=ibus
export QT_IM_MODULE=ibus

cargo run
```

### 方案 3: 永久设置 (推荐)

在你的 `~/.bashrc` 或 `~/.profile` 中添加:

**对于 fcitx:**
```bash
export GTK_IM_MODULE=fcitx
export XMODIFIERS=@im=fcitx
export QT_IM_MODULE=fcitx
```

**对于 ibus:**
```bash
export GTK_IM_MODULE=ibus
export XMODIFIERS=@im=ibus
export QT_IM_MODULE=ibus
```

然后重新登录或运行:
```bash
source ~/.bashrc
```

---

## 技术细节

jterm4 使用 GTK4 和 VTE4 构建。这些库依赖环境变量来选择正确的输入法模块。

### 事件传播顺序:

1. 键盘输入 → GTK4 事件系统
2. IME 系统 (fcitx/ibus) 处理输入
3. IME 生成 commit 事件
4. VTE 终端接收并显示字符

### 关键代码设置:

```rust
// src/main.rs:397
key_controller.set_propagation_phase(gtk4::PropagationPhase::Bubble);
```

这个设置使用 **Bubble 传播阶段**，确保:
- VTE 终端首先接收输入事件
- IME 可以正常处理中文输入
- 快捷键只在终端未处理时才触发

---

## 故障排除

### 1. 中文输入仍然不工作

确保:
- ✅ fcitx/ibus 正在运行
- ✅ 环境变量正确设置
- ✅ 从设置了正确环境变量的 shell 启动 jterm4
- ✅ 不是从桌面环境启动（桌面启动器可能不会继承 shell 环境变量）

### 2. 查看当前 IME 配置

运行 jterm4，它会在启动时打印 IME 配置:

```
=== jterm4 IME Configuration ===
GTK_IM_MODULE: fcitx
XMODIFIERS: @im=fcitx
QT_IM_MODULE: fcitx
================================
```

### 3. 从终端启动

不要从桌面图标启动。始终从配置了正确环境变量的终端启动:

```bash
# 从配置了 IME 环境变量的终端运行
./run-with-fcitx.sh
```

---

## 测试中文输入

1. 启动 jterm4
2. 按 `Ctrl+Space` 或你的 IME 切换键
3. 输入 "nihao" 应该出现中文候选词 "你好"
4. 选择候选词，字符应该出现在终端中

---

## 支持的快捷键

即使启用 IME，所有快捷键仍然工作:

- `Ctrl+Shift+T`: 新标签页
- `Ctrl+Shift+W`: 关闭标签页
- `Ctrl+Shift+C`: 复制
- `Ctrl+Shift+V`: 粘贴
- `Ctrl+Shift++`: 增大字体
- `Ctrl+Shift+I`: 减小字体
- `Ctrl+Shift+J/K`: 调整透明度
- `Ctrl+PageUp/PageDown`: 切换标签页

---

## 相关资源

- [GTK4 输入法文档](https://docs.gtk.org/gtk4/input-method.html)
- [Fcitx 文档](https://fcitx-im.org/)
- [IBus 文档](https://github.com/ibus/ibus)
