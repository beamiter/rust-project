# GDB 配置文件 for JWM debugging

# 启用 Rust pretty-printing
set print pretty on
set print array on
set print array-indexes on

# 自动加载 Rust backtrace
set backtrace limit 100

# 常用断点位置
# 取消注释你需要的：

# 后端初始化
# break backend::wayland_udev::backend::UdevBackend::new

# 窗口创建
# break backend::wayland_udev::state::JwmWaylandState::new_toplevel

# 事件循环
# break backend::wayland_udev::backend::UdevBackend::run

# 渲染
# break backend::udev_kms::KmsState::render_frame

# 布局计算
# break core::layout::calculate_tile

# 快捷键处理
# break jwm::Jwm::on_key_press_internal

# 显示启动信息
echo \n
echo ========================================\n
echo JWM Wayland udev Backend Debugger\n
echo ========================================\n
echo \n
echo Useful commands:\n
echo   run                    - Start JWM\n
echo   break file.rs:line    - Set breakpoint\n
echo   continue              - Continue execution\n
echo   next                  - Step over\n
echo   step                  - Step into\n
echo   print var             - Print variable\n
echo   bt                    - Show backtrace\n
echo   info locals           - Show local variables\n
echo \n
echo Common breakpoints (uncomment in .gdbinit):\n
echo   - UdevBackend::new    - Backend init\n
echo   - new_toplevel        - Window creation\n
echo   - render_frame        - Rendering\n
echo   - on_key_press        - Key handling\n
echo \n
echo ========================================\n
echo \n
