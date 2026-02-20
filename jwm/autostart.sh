#!/bin/sh

# =========================================================
# JWM Autostart Script
# 路径: ~/.config/jwm/autostart.sh
# =========================================================

# --- 函数：如果程序未运行则启动 ---
run_if_not_running() {
    if ! pgrep -f "$1" > /dev/null; then
        "$@" &
    fi
}

# --- 1. 屏幕分辨率 (xrandr) ---
# 如果你是虚拟机或多显示器，在这里取消注释并配置
# xrandr --output Virtual-1 --mode 1920x1080
# xrandr --output HDMI-1 --right-of eDP-1 --auto
xrandr --output HDMI-1 --rotate normal --left-of eDP-1 --auto &

# --- 2. 设置壁纸 (原 .fehbg 逻辑) ---
# 这是一个更通用的写法，如果 ~/.fehbg 存在则执行它
# 否则尝试设置一个默认背景色
if [ -f "$HOME/.fehbg" ]; then
    sh "$HOME/.fehbg" &
elif command -v feh > /dev/null; then
    # 如果没有 .fehbg 但安装了 feh，可以在这里指定一张默认壁纸
    # feh --bg-fill /usr/share/backgrounds/default.jpg &
    :
else
    # 设置纯色背景作为回退 (依赖 xsetroot)
    xsetroot -solid "#2e3440" &
fi

# --- 3. 混成器 (Picom) ---
# 用于实现透明、阴影和垂直同步
# 使用 run_if_not_running 防止重复启动
if command -v picom > /dev/null; then
    run_if_not_running picom -b
fi

# --- 4. 音频设置 (原 amixer 硬编码逻辑) ---
# 将主音量和耳机音量设置为 80% 并取消静音
if command -v amixer > /dev/null; then
    amixer sset Master 70 unmute > /dev/null 2>&1
    amixer sset Headphone 70 unmute > /dev/null 2>&1
fi

# --- 5. 输入法 (Fcitx5 / IBus) ---
# 对于中文用户非常重要
export GTK_IM_MODULE=fcitx
export QT_IM_MODULE=fcitx
export XMODIFIERS=@im=fcitx
# run_if_not_running fcitx -d
run_if_not_running fcitx5 -d

# --- 6. 身份认证代理 (Polkit) ---
# 用于 GUI 程序请求 root 权限 (如 GParted, Synaptic)
# 常见的路径如下，根据你的发行版选择一个取消注释：
# /usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1 &   # Arch/Debian/Ubuntu
# /usr/libexec/polkit-gnome-authentication-agent-1 &            # Fedora/RedHat
# lxpolkit &                                                    # LXDE/通用

# --- 7. 通知守护进程 (Dunst) ---
# 用于显示桌面通知
# if command -v dunst > /dev/null; then
#     run_if_not_running dunst
# fi

# --- 8. 电源管理 (xfce4-power-manager) ---
# 防止屏幕自动休眠或处理笔记本盖子关闭
# if command -v xfce4-power-manager > /dev/null; then
#     run_if_not_running xfce4-power-manager
# fi

# --- 9. 网络管理器托盘 ---
# if command -v nm-applet > /dev/null; then
#     run_if_not_running nm-applet
# fi

echo "[autostart.sh] Initialization finished."
