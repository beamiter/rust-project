use chrono::Local;
use clap::{Parser, Subcommand};
use glob::glob;
use nix::fcntl::{open, OFlag};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::signal::{kill, Signal};
use nix::sys::stat::Mode;
use nix::sys::wait::WaitStatus;
use nix::sys::wait::{waitpid, WaitPidFlag};
use nix::unistd::{mkfifo, read, Pid};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::collections::VecDeque;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::os::fd::OwnedFd;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// --- Runtime directory (XDG_RUNTIME_DIR) ---

fn runtime_dir() -> PathBuf {
    if let Ok(dir) = env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir)
    } else {
        // Fallback: /tmp/jwm-<uid>
        let uid = unsafe { libc::getuid() };
        let dir = PathBuf::from(format!("/tmp/jwm-{}", uid));
        let _ = fs::create_dir_all(&dir);
        dir
    }
}

fn pidfile_path() -> PathBuf {
    runtime_dir().join("jwm_daemon.pid")
}

fn control_pipe_path(daemon_pid: i32) -> PathBuf {
    runtime_dir().join(format!("jwm_control_{}", daemon_pid))
}

fn response_path(control_pipe: &Path) -> PathBuf {
    let name = control_pipe.file_name().unwrap_or_default().to_string_lossy();
    runtime_dir().join(format!("{}_response", name))
}

fn control_pipe_glob_pattern() -> String {
    format!("{}/jwm_control_*", runtime_dir().display())
}

// --- CLI ---

#[derive(Parser)]
#[command(
    name = "jwm-tool",
    version,
    about = "JWM 管理工具（单二进制多子命令）",
    long_about = "JWM 管理工具 — 通过 IPC 控制 JWM 窗口管理器。\n\
                  支持守护进程管理、窗口/标签/布局操作、事件订阅等功能。\n\
                  IPC 套接字位于 $XDG_RUNTIME_DIR/jwm-ipc.sock",
    after_help = "\x1b[1m示例:\x1b[0m\n  \
                  jwm-tool daemon                        # 启动守护进程\n  \
                  jwm-tool status                        # 查看守护进程状态\n  \
                  jwm-tool msg view --args '{\"tag\":2}'   # 切换到标签 2\n  \
                  jwm-tool msg get_windows               # 查询所有窗口\n  \
                  jwm-tool msg get_windows --raw          # 查询并输出原始 JSON\n  \
                  jwm-tool rebuild                       # 重新编译并重启 JWM",
)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动守护进程（管理 JWM 进程的生命周期）
    Daemon {
        /// 自定义JWM可执行文件路径（默认 /usr/local/bin/jwm，可用 env JWM_BINARY 覆盖）
        #[arg(long, env = "JWM_BINARY")]
        jwm_binary: Option<String>,
        /// 指定运行后端（可用 env JWM_BACKEND 覆盖）
        #[arg(long, env = "JWM_BACKEND")]
        backend: Option<String>,
    },

    /// 重启 JWM 进程
    Restart,
    /// 停止 JWM 进程（守护进程保持运行）
    Stop,
    /// 启动 JWM 进程（需先启动守护进程）
    Start,
    /// 退出守护进程及 JWM
    Quit,
    /// 查看守护进程与 JWM 运行状态
    Status,

    /// 编译并重启 JWM（cargo build --release）
    Rebuild {
        /// JWM 源码目录（默认 $HOME/jwm，可用 env JWM_DIR 覆盖）
        #[arg(long, env = "JWM_DIR", default_value_t = default_jwm_dir())]
        jwm_dir: String,
    },

    /// 安装 JWM 与桌面入口（参考 install_jwm_scripts.sh）
    Install {
        /// JWM 源码目录（默认 $HOME/jwm，可用 env JWM_DIR 覆盖）
        #[arg(long, env = "JWM_DIR", default_value_t = default_jwm_dir())]
        jwm_dir: String,
    },

    /// 检查守护进程是否存活
    DaemonCheck,
    /// 重启守护进程
    DaemonRestart,

    /// 打印调试信息（PID、套接字、控制管道等）
    Debug,

    /// 向 JWM IPC 发送消息 (JSON)
    #[command(
        long_about = "通过 Unix 套接字向 JWM 发送 IPC 消息。\n\
                      名称以 get_ 开头的自动作为查询发送，其余作为命令发送。\n\n\
                      \x1b[1m可用命令:\x1b[0m\n  \
                      窗口: focusstack, killclient, zoom, togglefloating, togglesticky,\n        \
                      togglepip, togglescratchpad, movestack\n  \
                      布局: setmfact, setcfact, incnmaster, setlayout, cyclelayout, togglebar\n  \
                      标签: view, tag, toggleview, toggletag, loopview\n  \
                      显示器: focusmon, tagmon\n  \
                      其他: spawn, quit, restart, reload_config\n\n\
                      \x1b[1m可用查询:\x1b[0m\n  \
                      get_windows, get_workspaces, get_monitors, get_tree, get_config, get_version\n\n\
                      \x1b[1m可用布局:\x1b[0m\n  \
                      tile, float, monocle, fibonacci, centered_master, bstack,\n  \
                      grid, deck, three_col, tatami, fullscreen\n\n\
                      \x1b[1m事件主题 (--subscribe):\x1b[0m\n  \
                      window (window/new, window/close, window/focus, window/title)\n  \
                      tag (tag/view), layout (layout/set), monitor (monitor/focus)\n  \
                      config (config/reload), * (订阅全部)",
        after_help = "\x1b[1m示例:\x1b[0m\n  \
                      jwm-tool msg view --args '{\"tag\":2}'              # 切换到标签 2\n  \
                      jwm-tool msg focusstack --args '{\"value\":-1}'     # 聚焦上一个窗口\n  \
                      jwm-tool msg setlayout --args '{\"layout\":\"monocle\"}' # 设置布局\n  \
                      jwm-tool msg setmfact --args '0.05'               # 调整主区比例\n  \
                      jwm-tool msg spawn --args '{\"cmd\":[\"alacritty\"]}' # 启动终端\n  \
                      jwm-tool msg killclient                           # 关闭当前窗口\n  \
                      jwm-tool msg get_windows                          # 查询所有窗口\n  \
                      jwm-tool msg get_windows --raw                    # 原始 JSON 输出\n  \
                      jwm-tool msg reload_config                        # 重新加载配置\n  \
                      jwm-tool msg \"\" --subscribe 'window,tag'          # 订阅事件流\n  \
                      jwm-tool msg \"\" --subscribe '*'                   # 订阅全部事件",
    )]
    Msg {
        /// 命令或查询名称（get_ 前缀自动识别为查询）
        #[arg(help = "命令或查询名称（get_ 前缀自动识别为查询）\n\
                      命令: view, tag, focusstack, killclient, zoom, setlayout, spawn, ...\n\
                      查询: get_windows, get_workspaces, get_monitors, get_tree, get_config, get_version")]
        name: String,
        /// JSON 参数，格式取决于命令类型
        #[arg(long, default_value = "null",
              help = "JSON 参数，格式取决于命令类型\n\
                      整数参数: '{\"value\": N}' 或直接 'N'  (focusstack, movestack, ...)\n\
                      浮点参数: '{\"value\": F}' 或直接 'F'  (setmfact, setcfact)\n\
                      标签参数: '{\"tag\": N}'               (view, tag, toggleview, ...)\n\
                      布局参数: '{\"layout\": \"name\"}'       (setlayout)\n\
                      命令参数: '{\"cmd\": [\"prog\", ...]}'   (spawn)")]
        args: String,
        /// 订阅事件流（逗号分隔的主题列表）
        #[arg(long,
              help = "订阅事件流（逗号分隔的主题列表）\n\
                      主题: window, tag, layout, monitor, config, * (全部)\n\
                      事件: window/new, window/close, window/focus, window/title,\n\
                            tag/view, layout/set, monitor/focus, config/reload")]
        subscribe: Option<String>,
        /// 输出原始 JSON（不做格式化美化）
        #[arg(long)]
        raw: bool,
    },
}

fn default_jwm_dir() -> String {
    env::var("HOME")
        .map(|h| format!("{}/jwm", h))
        .unwrap_or_else(|_| "./jwm".to_string())
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .expect("$HOME 未设置")
}

fn log_dir() -> PathBuf {
    home_dir().join(".local/share/jwm")
}

fn log_file() -> PathBuf {
    log_dir().join("jwm_daemon.log")
}

fn now_ts() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn log_line(msg: &str) {
    let _ = fs::create_dir_all(log_dir());
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
        .unwrap_or_else(|_| {
            eprintln!("[{}] 无法打开日志文件: {}", now_ts(), log_file().display());
            std::process::exit(1);
        });
    let _ = writeln!(f, "[{}] {}", now_ts(), msg);
    let _ = f.flush();
    println!("[{}] {}", now_ts(), msg);
}

// --- JwmManager ---

struct JwmManager {
    jwm_binary: PathBuf,
    backend: Option<String>,
    jwm_child: Option<Child>,
    jwm_pid: Option<i32>,
}

impl JwmManager {
    fn new(jwm_binary: PathBuf, backend: Option<String>) -> Self {
        Self {
            jwm_binary,
            backend,
            jwm_child: None,
            jwm_pid: None,
        }
    }

    fn start(&mut self) -> io::Result<()> {
        if self.is_running() {
            if let Some(pid) = self.jwm_pid {
                log_line(&format!("JWM已在运行，PID: {}", pid));
            }
            return Ok(());
        }
        if !self.jwm_binary.is_file() {
            log_line(&format!(
                "错误: JWM二进制文件不存在: {}",
                self.jwm_binary.display()
            ));
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "JWM binary not found",
            ));
        }
        log_line(&format!("启动JWM: {}", self.jwm_binary.display()));
        let mut command = Command::new(&self.jwm_binary);
        if let Some(backend) = self.backend.as_ref() {
            if !backend.trim().is_empty() {
                command.env("JWM_BACKEND", backend);
            }
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let pid = child.id() as i32;
        self.jwm_pid = Some(pid);
        self.jwm_child = Some(child);
        log_line(&format!("JWM已启动，PID: {}", pid));
        Ok(())
    }

    /// Wait for the managed process to exit within `timeout`.
    /// Uses Child::try_wait if we own the handle, otherwise waitpid + kill(0).
    /// Returns true if the process exited.
    fn wait_for_exit(&mut self, pid: i32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        if let Some(child) = self.jwm_child.as_mut() {
            while Instant::now() < deadline {
                match child.try_wait() {
                    Ok(Some(_)) => return true,
                    Ok(None) => {}
                    Err(e) => {
                        log_line(&format!("try_wait 错误: {e}"));
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
        } else {
            while Instant::now() < deadline {
                match waitpid(Pid::from_raw(pid), Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::StillAlive) => {}
                    Ok(_) => return true,
                    Err(nix::errno::Errno::ECHILD) => {
                        if kill(Pid::from_raw(pid), None).is_err() {
                            return true;
                        }
                    }
                    Err(e) => {
                        log_line(&format!("waitpid 错误: {e}"));
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        false
    }

    fn stop(&mut self) {
        let pid = match self.jwm_pid {
            Some(pid) => pid,
            None => {
                log_line("JWM进程未运行");
                return;
            }
        };

        log_line(&format!("停止JWM进程: {}", pid));

        // Phase 1: graceful SIGTERM
        let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
        let terminated = self.wait_for_exit(pid, Duration::from_secs(2));

        // Phase 2: force SIGKILL if still alive
        if !terminated {
            log_line(&format!("强制终止JWM进程: {}", pid));
            let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
            self.wait_for_exit(pid, Duration::from_secs(2));
        }

        self.jwm_pid = None;
        self.jwm_child = None;
        log_line("JWM进程已停止");
    }

    fn restart(&mut self) {
        log_line("重启JWM...");
        self.stop();
        // stop() already waited for exit, no extra sleep needed
        let _ = self.start();
    }

    fn status_str(&self) -> String {
        if let Some(pid) = self.jwm_pid {
            if self.process_exists(pid) {
                return format!("JWM运行中，PID: {}", pid);
            }
        }
        "JWM未运行".to_string()
    }

    fn is_running(&self) -> bool {
        if let Some(pid) = self.jwm_pid {
            self.process_exists(pid)
        } else {
            false
        }
    }

    fn process_exists(&self, pid: i32) -> bool {
        kill(Pid::from_raw(pid), None).is_ok()
    }
}

// --- Atomic response write ---

fn write_response(resp_file: &Path, s: &str) {
    let tmp = resp_file.with_extension("tmp");
    if fs::write(&tmp, s).is_ok() {
        let _ = fs::rename(&tmp, resp_file);
    }
}

// --- IPC helpers ---

fn write_pidfile(pid: i32) -> io::Result<()> {
    let _ = fs::create_dir_all(runtime_dir());
    fs::write(pidfile_path(), pid.to_string())
}

fn read_existing_pid() -> Option<i32> {
    let content = fs::read_to_string(pidfile_path()).ok()?;
    content.trim().parse::<i32>().ok()
}

fn cleanup_resources(control_pipe: &Path) {
    log_line("开始清理资源...");
    let resp = response_path(control_pipe);
    let _ = fs::remove_file(control_pipe);
    let _ = fs::remove_file(&resp);
    let _ = fs::remove_file(resp.with_extension("tmp"));
    let _ = fs::remove_file(pidfile_path());
    log_line("清理完成，守护进程退出");
}

fn mkfifo_safe(p: &Path) -> io::Result<()> {
    let _ = fs::remove_file(p);
    mkfifo(p, Mode::from_bits_truncate(0o600))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("mkfifo error: {e}")))
}

fn open_fifo_rdwr_nonblock(p: &Path) -> io::Result<OwnedFd> {
    open(p, OFlag::O_RDWR | OFlag::O_NONBLOCK, Mode::empty())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("打开FIFO失败: {e}")))
}

fn read_commands_from_fd<F: AsFd>(fd: F, buf: &mut String) -> io::Result<Vec<String>> {
    let mut tmp = [0u8; 1024];
    let n = match read(fd, &mut tmp) {
        Ok(0) => 0,
        Ok(n) => n,
        Err(nix::errno::Errno::EAGAIN) => 0,
        Err(e) => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("读取FIFO失败: {e}"),
            ))
        }
    };

    let mut cmds = Vec::new();
    if n > 0 {
        buf.push_str(&String::from_utf8_lossy(&tmp[..n]));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let cmd = line.trim();
            if !cmd.is_empty() {
                cmds.push(cmd.to_string());
            }
        }
    }
    Ok(cmds)
}

// --- Daemon main loop ---

fn run_daemon(jwm_binary: PathBuf, backend: Option<String>) -> io::Result<()> {
    let term_flag = Arc::new(AtomicBool::new(false));
    flag::register(SIGTERM, Arc::clone(&term_flag)).expect("注册SIGTERM失败");
    flag::register(SIGINT, Arc::clone(&term_flag)).expect("注册SIGINT失败");

    // Check for existing daemon
    if let Some(old_pid) = read_existing_pid() {
        if kill(Pid::from_raw(old_pid), None).is_ok() {
            eprintln!("守护进程已在运行，PID: {old_pid}");
            std::process::exit(1);
        } else {
            let _ = fs::remove_file(pidfile_path());
        }
    }

    let daemon_pid = std::process::id() as i32;
    write_pidfile(daemon_pid)?;

    let control_pipe = control_pipe_path(daemon_pid);
    if let Err(e) = mkfifo_safe(&control_pipe) {
        log_line(&format!(
            "错误: 无法创建控制管道 {}: {}",
            control_pipe.display(),
            e
        ));
        std::process::exit(1);
    }

    let fifo_fd = match open_fifo_rdwr_nonblock(&control_pipe) {
        Ok(fd) => fd,
        Err(e) => {
            log_line(&format!("错误: {}", e));
            std::process::exit(1);
        }
    };

    log_line(&format!("JWM守护进程启动，PID: {}", daemon_pid));
    log_line(&format!("控制管道: {}", control_pipe.display()));

    let mut mgr = JwmManager::new(jwm_binary, backend);
    let _ = mgr.start();

    log_line("开始主循环，监听命令...");

    let mut line_buf = String::new();

    loop {
        if term_flag.load(Ordering::Relaxed) {
            if let Some(pid) = mgr.jwm_pid {
                log_line(&format!("终止JWM进程: {}", pid));
            }
            mgr.stop();
            cleanup_resources(&control_pipe);
            break;
        }

        // poll() on FIFO fd — block up to 200ms instead of busy-sleep
        let mut poll_fds = [PollFd::new(fifo_fd.as_fd(), PollFlags::POLLIN)];
        let _ = poll(&mut poll_fds, PollTimeout::from(200u8));

        match read_commands_from_fd(&fifo_fd, &mut line_buf) {
            Ok(cmds) => {
                for cmd in cmds {
                    log_line(&format!("收到命令: {}", cmd));
                    let resp_path = response_path(&control_pipe);
                    match cmd.as_str() {
                        "restart" => {
                            mgr.restart();
                            write_response(&resp_path, "restart_done");
                        }
                        "stop" => {
                            mgr.stop();
                            write_response(&resp_path, "stop_done");
                        }
                        "start" => {
                            let _ = mgr.start();
                            write_response(&resp_path, "start_done");
                        }
                        "quit" => {
                            log_line("收到退出命令");
                            write_response(&resp_path, "quit_done");
                            mgr.stop();
                            cleanup_resources(&control_pipe);
                            return Ok(());
                        }
                        "status" => {
                            let s = mgr.status_str();
                            write_response(&resp_path, &s);
                        }
                        other => {
                            log_line(&format!("未知命令: {}", other));
                            write_response(&resp_path, "unknown_command");
                        }
                    }
                }
            }
            Err(e) => {
                log_line(&format!("读取命令错误: {}", e));
            }
        }

        // Check JWM process health
        if let Some(pid) = mgr.jwm_pid {
            if kill(Pid::from_raw(pid), None).is_err() {
                log_line(&format!("检测到JWM意外退出 (PID: {}), 守护进程一并退出", pid));
                mgr.jwm_pid = None;
                mgr.jwm_child = None;
                cleanup_resources(&control_pipe);
                return Ok(());
            } else {
                // Reap zombie
                let _ = waitpid(Pid::from_raw(pid), Some(WaitPidFlag::WNOHANG));
            }
        }
    }

    Ok(())
}

// --- Control client helpers ---

fn read_daemon_pid() -> Option<i32> {
    let content = fs::read_to_string(pidfile_path()).ok()?;
    content.trim().parse::<i32>().ok()
}

fn process_exists(pid: i32) -> bool {
    kill(Pid::from_raw(pid), None).is_ok()
}

fn control_pipe_for(pid: i32) -> PathBuf {
    control_pipe_path(pid)
}

fn is_fifo(p: &Path) -> bool {
    fs::metadata(p)
        .map(|m| m.file_type().is_fifo())
        .unwrap_or(false)
}

fn find_control_pipe() -> Option<PathBuf> {
    let pid = read_daemon_pid()?;
    if !process_exists(pid) {
        return None;
    }
    let pipe = control_pipe_for(pid);
    if is_fifo(&pipe) {
        Some(pipe)
    } else {
        None
    }
}

fn send_command(cmd: &str) -> io::Result<()> {
    let pipe = match find_control_pipe() {
        Some(p) => p,
        None => {
            eprintln!("错误: 未找到JWM守护进程或控制管道\n请确保JWM守护进程正在运行");
            std::process::exit(1);
        }
    };
    println!("发送命令: {}", cmd);

    let data = format!("{cmd}\n");
    let mut last_err: Option<io::Error> = None;
    for _ in 0..10 {
        match fs::write(&pipe, &data) {
            Ok(_) => {
                last_err = None;
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe || e.raw_os_error() == Some(32) => {
                last_err = Some(e);
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    if let Some(e) = last_err {
        return Err(e);
    }

    let resp_path = response_path(&pipe);

    let mut count = 0;
    while count < 20 {
        if resp_path.exists() {
            let content = fs::read_to_string(&resp_path).unwrap_or_default();
            println!("响应: {}", content.trim());
            let _ = fs::remove_file(&resp_path);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
        count += 1;
    }
    eprintln!("警告: 命令可能已发送，但未收到响应");
    Ok(())
}

fn check_daemon() -> bool {
    if let Some(pipe) = find_control_pipe() {
        println!("JWM守护进程正在运行");
        if let Some(pid) = read_daemon_pid() {
            println!("PID: {}", pid);
        }
        println!("控制管道: {}", pipe.display());
        true
    } else {
        println!("JWM守护进程未运行");
        false
    }
}

fn kill_daemon_by_pidfile() {
    if let Some(old_pid) = read_daemon_pid() {
        if process_exists(old_pid) {
            println!("终止旧的守护进程: {}", old_pid);
            let _ = kill(Pid::from_raw(old_pid), Signal::SIGTERM);
            thread::sleep(Duration::from_secs(1));
            if process_exists(old_pid) {
                let _ = kill(Pid::from_raw(old_pid), Signal::SIGKILL);
            }
        }
    }
}

fn cleanup_old_pipes_and_pidfile() {
    if let Ok(entries) = glob(&control_pipe_glob_pattern()) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry);
        }
    }
    let _ = fs::remove_file(pidfile_path());
}

fn force_restart_daemon() -> io::Result<()> {
    println!("强制重启守护进程...");
    kill_daemon_by_pidfile();
    cleanup_old_pipes_and_pidfile();

    println!("启动新的守护进程...");
    let exe = env::current_exe()?;
    let child = Command::new(exe)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let _ = child.id();

    thread::sleep(Duration::from_secs(1));
    if check_daemon() {
        println!("守护进程重启成功");
        Ok(())
    } else {
        eprintln!("守护进程重启失败");
        Err(io::Error::new(
            io::ErrorKind::Other,
            "daemon restart failed",
        ))
    }
}

// --- Build & Install ---

fn rebuild_and_restart(jwm_dir: &str) -> io::Result<()> {
    if !check_daemon() {
        println!("守护进程未运行，正在强制重启...");
        force_restart_daemon()?;
    }

    println!("开始编译JWM...");
    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .current_dir(jwm_dir)
        .status()?;
    if !status.success() {
        eprintln!("编译失败！");
        return Err(io::Error::new(io::ErrorKind::Other, "cargo build failed"));
    }

    install_jwm(jwm_dir)?;

    println!("重启JWM...");
    let _ = send_command("restart");
    println!("JWM编译并重启完成！");
    Ok(())
}

/// Run `sudo install <src> <dest_dir>` and return an error on failure.
fn sudo_install(src: &Path, dest_dir: &str) -> io::Result<()> {
    let status = Command::new("sudo")
        .arg("install")
        .arg(src)
        .arg(dest_dir)
        .status()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("install {} to {} failed", src.display(), dest_dir),
        ));
    }
    Ok(())
}

fn install_jwm(jwm_dir: &str) -> io::Result<()> {
    let jwm_dir = Path::new(jwm_dir);

    let files_to_check: &[(&str, PathBuf)] = &[
        ("jwm", jwm_dir.join("target/release/jwm")),
        ("jwm-tool", jwm_dir.join("target/release/jwm-tool")),
        ("jwm-x11.desktop", jwm_dir.join("jwm-x11.desktop")),
        (
            "jwm-wayland.desktop",
            jwm_dir.join("jwm-wayland.desktop"),
        ),
    ];

    for (name, path) in files_to_check {
        if !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} 未找到: {}", name, path.display()),
            ));
        }
    }

    println!("安装 JWM 与 jwm-tool...");

    let status = Command::new("sudo")
        .args(["rm", "-f", "/usr/local/bin/jwm", "/usr/local/bin/jwm-tool"])
        .status()?;
    if !status.success() {
        eprintln!("清理旧二进制失败！");
        return Err(io::Error::new(io::ErrorKind::Other, "sudo rm failed"));
    }

    sudo_install(&files_to_check[0].1, "/usr/local/bin/")?;
    sudo_install(&files_to_check[1].1, "/usr/local/bin/")?;
    sudo_install(&files_to_check[2].1, "/usr/share/xsessions/")?;
    sudo_install(&files_to_check[3].1, "/usr/share/wayland-sessions/")?;

    println!("安装完成");
    Ok(())
}

// --- Debug ---

/// Run `ps aux | grep <pattern>` and print matching lines.
fn ps_grep(pattern: &str) {
    let _ = Command::new("ps")
        .arg("aux")
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut ps| {
            let mut grep = Command::new("grep")
                .arg(pattern)
                .stdin(ps.stdout.take().expect("ps stdout"))
                .stdout(Stdio::inherit())
                .spawn()?;
            let _ = ps.wait();
            let _ = grep.wait();
            Ok(())
        });
}

fn tail_lines(p: &Path, n: usize) -> io::Result<Vec<String>> {
    use std::io::{BufRead, BufReader};
    let f = fs::File::open(p)?;
    let reader = BufReader::new(f);
    let mut buf: VecDeque<String> = VecDeque::with_capacity(n + 1);
    for line in reader.lines() {
        if let Ok(l) = line {
            buf.push_back(l);
            if buf.len() > n {
                buf.pop_front();
            }
        }
    }
    Ok(buf.into())
}

fn debug_info() {
    println!("=== JWM守护进程调试信息 ===");
    println!("时间: {}", Local::now().format("%Y-%m-%d %H:%M:%S"));
    println!("运行目录: {}", runtime_dir().display());
    println!();

    println!("1. 检查守护进程:");
    ps_grep("jwm-tool");

    println!("\n2. 检查PID文件:");
    if let Ok(pid) = fs::read_to_string(pidfile_path()) {
        println!("PID文件存在: {}", pid.trim());
    } else {
        println!("PID文件不存在");
    }

    println!("\n3. 检查控制管道:");
    let mut found = false;
    if let Ok(entries) = glob(&control_pipe_glob_pattern()) {
        for entry in entries.flatten() {
            found = true;
            if let Ok(meta) = fs::metadata(&entry) {
                println!(
                    "{}  {}",
                    if meta.file_type().is_fifo() {
                        "FIFO"
                    } else {
                        "NOT_FIFO"
                    },
                    entry.display()
                );
            }
        }
    }
    if !found {
        println!("未找到控制管道");
    }

    println!("\n4. 检查JWM进程:");
    ps_grep("-E \"jwm[^_]\"");

    println!("\n5. 检查日志:");
    let lf = log_file();
    if lf.exists() {
        println!("最近的日志:");
        match tail_lines(&lf, 10) {
            Ok(lines) => {
                for l in lines {
                    println!("{}", l);
                }
            }
            Err(_) => println!("读取日志失败"),
        }
    } else {
        println!("日志文件不存在");
    }

    println!("\n6. X11信息:");
    println!("DISPLAY: {}", env::var("DISPLAY").unwrap_or_default());
    ps_grep(" X");
}

// --- main ---

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Commands::Daemon {
            jwm_binary,
            backend,
        } => {
            let jwm_bin = jwm_binary
                .or_else(|| env::var("JWM_BINARY").ok())
                .unwrap_or_else(|| "/usr/local/bin/jwm".to_string());
            let backend = backend.or_else(|| env::var("JWM_BACKEND").ok());
            run_daemon(PathBuf::from(jwm_bin), backend)?;
        }

        Commands::Restart => send_command("restart")?,
        Commands::Stop => send_command("stop")?,
        Commands::Start => send_command("start")?,
        Commands::Quit => send_command("quit")?,
        Commands::Status => send_command("status")?,

        Commands::Rebuild { jwm_dir } => {
            rebuild_and_restart(&jwm_dir)?;
        }

        Commands::Install { jwm_dir } => {
            install_jwm(&jwm_dir)?;
        }

        Commands::DaemonCheck => {
            let _ = check_daemon();
        }
        Commands::DaemonRestart => {
            let _ = force_restart_daemon()?;
        }

        Commands::Debug => debug_info(),

        Commands::Msg { name, args, subscribe, raw } => {
            run_ipc_msg(&name, &args, subscribe.as_deref(), raw)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// IPC msg subcommand
// ---------------------------------------------------------------------------

fn ipc_socket_path() -> PathBuf {
    let runtime = env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/tmp/jwm-{}", unsafe { libc::getuid() }));
    Path::new(&runtime).join("jwm-ipc.sock")
}

fn run_ipc_msg(name: &str, args_str: &str, subscribe: Option<&str>, raw: bool) -> io::Result<()> {
    let sock_path = ipc_socket_path();
    if !sock_path.exists() {
        eprintln!("Error: IPC socket not found at {}", sock_path.display());
        eprintln!("Is JWM running?");
        std::process::exit(1);
    }

    let mut stream = UnixStream::connect(&sock_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    // Handle subscribe mode
    if let Some(topics) = subscribe {
        let topic_list: Vec<String> = topics.split(',').map(|s| s.trim().to_string()).collect();
        let msg = serde_json::json!({ "subscribe": topic_list });
        let mut line = serde_json::to_string(&msg).unwrap();
        line.push('\n');
        stream.write_all(line.as_bytes())?;

        // Read subscription confirmation
        let resp = read_ipc_line(&mut stream)?;
        if !raw {
            eprintln!("Subscribed: {}", resp.trim());
        }

        // Continuously read events (no timeout)
        stream.set_read_timeout(None)?;
        loop {
            match read_ipc_line(&mut stream) {
                Ok(line) => {
                    if raw {
                        println!("{}", line.trim());
                    } else {
                        match serde_json::from_str::<serde_json::Value>(line.trim()) {
                            Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(line)),
                            Err(_) => println!("{}", line.trim()),
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Connection closed: {e}");
                    break;
                }
            }
        }
        return Ok(());
    }

    // Parse args
    let args: serde_json::Value = serde_json::from_str(args_str).unwrap_or_else(|_| {
        eprintln!("Warning: could not parse args as JSON, using null");
        serde_json::Value::Null
    });

    // Determine if this is a command or query
    let msg = if name.starts_with("get_") {
        serde_json::json!({ "query": name, "args": args })
    } else {
        serde_json::json!({ "command": name, "args": args })
    };

    let mut line = serde_json::to_string(&msg).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes())?;

    // Read response
    let resp = read_ipc_line(&mut stream)?;
    if raw {
        print!("{}", resp);
    } else {
        match serde_json::from_str::<serde_json::Value>(resp.trim()) {
            Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or(resp)),
            Err(_) => print!("{}", resp),
        }
    }

    Ok(())
}

fn read_ipc_line(stream: &mut UnixStream) -> io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match io::Read::read(stream, &mut byte) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed")),
            Ok(_) => {
                if byte[0] == b'\n' {
                    return Ok(String::from_utf8_lossy(&buf).to_string());
                }
                buf.push(byte[0]);
            }
            Err(e) => return Err(e),
        }
    }
}
