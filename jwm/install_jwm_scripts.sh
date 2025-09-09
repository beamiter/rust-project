#!/bin/bash
# install_jwm_scripts.sh - 安装JWM脚本

sudo cp target/release/jwm /usr/local/bin/
sudo cp target/release/jwm-tool /usr/local/bin/
sudo cp jwm.desktop /usr/local/share/xsessions/
sudo cp jwm.desktop /usr/share/xsessions/

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
