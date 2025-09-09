#!/bin/bash
# install_jwm_scripts.sh - 安装JWM脚本

sudo cp target/release/jwm /usr/local/bin/
sudo cp target/release/jwm-tool /usr/local/bin/
sudo cp jwm.desktop /usr/share/xsessions/

# # 设置权限并复制脚本
# sudo cp jwm_daemon.sh /usr/local/bin/
# sudo cp jwm_control.sh /usr/local/bin/
# sudo chmod +x /usr/local/bin/jwm_daemon.sh
# sudo chmod +x /usr/local/bin/jwm_control.sh

# # 创建符号链接方便使用
# sudo ln -sf /usr/local/bin/jwm_control.sh /usr/local/bin/jwmc

# echo "安装完成！"
# echo "现在你可以使用以下命令："

# echo "  restart           - 重启JWM"
# echo "  stop              - 停止JWM"
# echo "  start             - 启动JWM"
# echo "  quit              - 退出守护进程"
# echo "  status            - 显示JWM状态"
# echo "  rebuild           - 编译并重启JWM"
# echo "  daemon-check      - 检查守护进程状态"
# echo "  daemon-restart    - 强制重启守护进程"
# echo "  help              - 显示此帮助信息"

sudo cp jwm_tool.sh /usr/local/bin/
sudo chmod +x /usr/local/bin/jwm_tool.sh
