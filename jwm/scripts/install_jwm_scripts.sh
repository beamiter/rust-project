#!/bin/bash
# install_jwm_scripts.sh - 安装JWM脚本

sudo rm /usr/local/bin/jwm
sudo rm /usr/local/bin/jwm-tool
sudo install ../target/debug/jwm /usr/local/bin/
sudo install ../target/debug/jwm-tool /usr/local/bin/
sudo install jwm-x11.desktop /usr/share/xsessions/
sudo install jwm-wayland.desktop /usr/share/wayland-sessions/
# mkdir -p ~/.config/picom/
# cp picom.conf ~/.config/picom/picom.conf

echo "jwm-tool"
echo "JWM 管理工具（单二进制多子命令）"
echo "Usage: jwm-tool <COMMAND>"
echo "Commands:"
echo "  daemon          启动守护进程"
echo "  restart         向守护进程发送命令"
echo "  stop"
echo "  start"
echo "  quit"
echo "  status"
echo "  rebuild         编译并重启 JWM"
echo "  daemon-check    守护进程检查/重启"
echo "  daemon-restart"
echo "  debug           调试信息"
echo "  help            Print this message or the help of the given subcommand(s)"
echo "Options:"
echo "  -h, --help     Print help"
echo "  -V, --version  Print version"
