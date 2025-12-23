// src/miscellaneous.rs
use log::{error, info};
use std::process::{Command, Stdio};

use crate::terminal_prober::ADVANCED_TERMINAL_PROBER;

pub fn init_auto_command() {
    // 仅仅保留终端检测日志，这对于调试很有用
    let prober = &*ADVANCED_TERMINAL_PROBER;
    if let Some(terminal) = prober.get_available_terminal() {
        info!("Found terminal: {}", terminal.command);
    } else {
        error!("No terminal found!");
    }
}

pub fn init_auto_start() {
    // 1. 确定 autostart.sh 的路径
    // 优先查找 XDG_CONFIG_HOME/jwm/autostart.sh (通常是 ~/.config/jwm/autostart.sh)
    let config_path = dirs::config_dir()
        .map(|p| p.join("jwm").join("autostart.sh"))
        .or_else(|| {
            // 回退方案：尝试找 ~/.jwm/autostart.sh
            dirs::home_dir().map(|p| p.join(".jwm").join("autostart.sh"))
        });

    if let Some(path) = config_path {
        if path.exists() {
            info!("Found autostart script at: {:?}", path);

            // 2. 执行脚本
            // 使用 "sh" 来执行，这样即使用户忘记 chmod +x 也能运行
            match Command::new("sh")
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(_) => info!("Autostart script spawned successfully"),
                Err(e) => error!("Failed to execute autostart script: {}", e),
            }
        } else {
            info!(
                "No autostart script found at {:?}, skipping auto start tasks.",
                path
            );
        }
    } else {
        error!("Could not determine configuration directory.");
    }
}
