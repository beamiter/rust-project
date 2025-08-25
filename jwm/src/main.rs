// use bar::StatusBar;
use chrono::prelude::*;
use coredump::register_panic_handler;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use jwm::{
    jwm::{RESTART_ENV_VAR, RESTART_REQUESTED, RESTART_STATE_FILE},
    Jwm,
};
use log::{error, info, warn};
use std::{collections::HashMap, env, sync::atomic::Ordering};

pub fn setup_locale() {
    // 获取当前locale
    let locale = env::var("LANG")
        .or_else(|_| env::var("LC_ALL"))
        .or_else(|_| env::var("LC_CTYPE"))
        .unwrap_or_else(|_| "C".to_string());
    info!("Using locale: {}", locale);
    // 检查UTF-8支持
    if !locale.contains("UTF-8") && !locale.contains("utf8") {
        warn!(
            "Non-UTF-8 locale detected ({}). Text display may be affected.",
            locale
        );
        warn!("Consider setting: export LANG=en_US.UTF-8");
    }
    // 确保关键的locale环境变量存在
    if env::var("LC_CTYPE").is_err() {
        if locale.contains("UTF-8") {
            env::set_var("LC_CTYPE", &locale);
        } else {
            env::set_var("LC_CTYPE", "en_US.UTF-8");
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = register_panic_handler();

    setup_locale();
    jwm::miscellaneous::init_auto_command();
    jwm::miscellaneous::init_auto_start();

    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();
    let log_filename = format!("jwm_{}", timestamp);
    Logger::try_with_str("info")
        .unwrap()
        .format(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory("/tmp/jwm")
                .basename(format!("{log_filename}"))
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Info)
        // .log_to_stdout()
        // .buffer_capacity(1024)
        // .use_background_worker(true)
        .rotate(
            Criterion::Size(10_000_000),
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .unwrap();

    info!("[main] main begin");
    info!("fuck");

    // 检查是否是重启模式
    let is_restart = env::var(RESTART_ENV_VAR).is_ok();
    if is_restart {
        info!("[main] Restart mode detected");

        // 尝试恢复环境变量
        let env_file = format!("{}.env", RESTART_STATE_FILE);
        if std::path::Path::new(&env_file).exists() {
            match std::fs::read_to_string(&env_file) {
                Ok(content) => {
                    if let Ok(env_vars) = serde_json::from_str::<HashMap<String, String>>(&content)
                    {
                        Jwm::set_x11_environment(&env_vars);
                        info!("[main] Restored environment variables from {}", env_file);
                    }
                }
                Err(e) => {
                    warn!("[main] Failed to read environment file: {}", e);
                }
            }
        }

        // 在重启模式下等待更长时间
        std::thread::sleep(std::time::Duration::from_millis(800));
    }

    loop {
        info!("[main] Starting JWM instance");

        // 重置重启标志
        RESTART_REQUESTED.store(false, Ordering::SeqCst);

        // 运行窗口管理器
        match run_jwm() {
            Ok(should_restart) => {
                if should_restart {
                    info!("[main] Restart requested, restarting...");
                    continue;
                } else {
                    info!("[main] Normal exit");
                    break;
                }
            }
            Err(e) => {
                error!("[main] JWM error: {}", e);

                // 如果是重启模式且失败，清理并退出
                if env::var(RESTART_ENV_VAR).is_ok() {
                    cleanup_restart_files();
                    env::remove_var(RESTART_ENV_VAR);
                }

                return Err(e);
            }
        }
    }

    Ok(())
}

fn run_jwm() -> Result<bool, Box<dyn std::error::Error>> {
    let mut jwm = Jwm::new()?;

    jwm.checkotherwm()?;
    jwm.setup()?;
    jwm.scan()?;

    // 如果是重启模式，恢复状态
    if env::var(RESTART_ENV_VAR).is_ok() {
        info!("[run_jwm] Recovering from restart");
        if let Err(e) = jwm.check_restart_recovery() {
            warn!("[run_jwm] Restart recovery failed: {}", e);
        }
        env::remove_var(RESTART_ENV_VAR);
    }

    // 运行主循环
    let result = jwm.run();

    // 清理
    jwm.cleanup()?;

    // 检查是否请求了重启
    let should_restart = RESTART_REQUESTED.load(Ordering::SeqCst);

    if should_restart {
        info!("[run_jwm] Restart requested");
    }

    result?;
    Ok(should_restart)
}

fn cleanup_restart_files() {
    let _ = std::fs::remove_file(RESTART_STATE_FILE);
    let _ = std::fs::remove_file(format!("{}.env", RESTART_STATE_FILE));
}
