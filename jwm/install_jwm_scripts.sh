#!/bin/bash
# install_jwm_scripts.sh - 安装JWM脚本

# 设置权限并复制脚本
sudo cp jwm_daemon.sh /usr/local/bin/
sudo cp jwm_control.sh /usr/local/bin/
sudo chmod +x /usr/local/bin/jwm_daemon.sh
sudo chmod +x /usr/local/bin/jwm_control.sh

# 创建符号链接方便使用
sudo ln -sf /usr/local/bin/jwm_control.sh /usr/local/bin/jwmc

echo "安装完成！"
echo "现在你可以使用以下命令："
echo "  jwmc restart   - 重启JWM"
echo "  jwmc rebuild   - 编译并重启JWM"
echo "  jwmc status    - 查看状态"
echo "  jwmc quit      - 退出守护进程"
