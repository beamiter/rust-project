#!/bin/bash
# 系统诊断脚本 - 检查运行 JWM 的系统条件

echo "🔍 JWM Wayland udev Backend - System Diagnostic"
echo "================================================="
echo ""

# 1. 系统信息
echo "📋 System Information:"
uname -a
echo ""

# 2. 用户权限
echo "👤 User Groups:"
groups $USER
echo ""

# 3. DRM 设备
echo "🖥️  DRM Devices:"
if [ -d /dev/dri ]; then
    ls -lh /dev/dri/
    echo ""

    echo "DRM Status:"
    for card in /sys/class/drm/card*/status; do
        if [ -f "$card" ]; then
            echo "  $(basename $(dirname $card)): $(cat $card)"
        fi
    done
else
    echo "  ❌ /dev/dri not found!"
fi
echo ""

# 4. 输入设备
echo "⌨️  Input Devices:"
if command -v libinput &> /dev/null; then
    libinput list-devices 2>/dev/null | head -30
else
    echo "  ⚠️  libinput command not found"
    ls /dev/input/ | head -10
fi
echo ""

# 5. Seat 状态
echo "💺 Seat Status:"
if command -v loginctl &> /dev/null; then
    loginctl seat-status seat0 2>/dev/null | head -10 || echo "  No seat0 info available"
else
    echo "  ⚠️  loginctl not available"
fi
echo ""

# 6. 依赖库检查
echo "📦 Required Libraries:"
for lib in libinput libseat libdrm libgbm libEGL libGLESv2; do
    if ldconfig -p | grep -q "$lib"; then
        echo "  ✅ $lib: found"
    else
        echo "  ❌ $lib: NOT FOUND"
    fi
done
echo ""

# 7. 环境变量
echo "🌍 Environment:"
env | grep -E "XDG_|WAYLAND|DISPLAY" || echo "  No relevant env vars"
echo ""

# 8. Rust 工具链
echo "🦀 Rust Toolchain:"
if command -v rustc &> /dev/null; then
    rustc --version
    cargo --version
else
    echo "  ❌ Rust not installed"
fi
echo ""

# 9. JWM 编译状态
echo "🏗️  JWM Build Status:"
cd "$(dirname "$0")/.."
if [ -f Cargo.toml ]; then
    echo "  Project: $(grep '^name' Cargo.toml | head -1)"
    echo "  Version: $(grep '^version' Cargo.toml | head -1)"

    if [ -f target/debug/jwm ]; then
        echo "  ✅ Debug build exists"
    else
        echo "  ⚠️  Debug build not found (run: cargo build)"
    fi

    if [ -f target/release/jwm ]; then
        echo "  ✅ Release build exists"
    else
        echo "  ⚠️  Release build not found"
    fi
else
    echo "  ❌ Not in JWM project directory"
fi
echo ""

# 10. 权限建议
echo "💡 Recommendations:"
if ! groups $USER | grep -q video; then
    echo "  ⚠️  User not in 'video' group. Run:"
    echo "     sudo usermod -aG video $USER"
    echo "     (then log out and back in)"
fi

if ! groups $USER | grep -q input; then
    echo "  ⚠️  User not in 'input' group. Run:"
    echo "     sudo usermod -aG input $USER"
fi

echo ""
echo "✅ Diagnostic complete!"
echo ""
echo "📄 To save this report:"
echo "   ./scripts/check_system.sh > system_report.txt 2>&1"
