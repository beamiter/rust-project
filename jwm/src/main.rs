// use bar::StatusBar;
use chrono::prelude::*;
use coredump::register_panic_handler;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use jwm::{config::CONFIG, Jwm};
use log::{error, info, warn};
use std::{env, process::Command, sync::atomic::Ordering};

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

    // 运行窗口管理器
    run_jwm()?;

    Ok(())
}

fn run_jwm() -> Result<(), Box<dyn std::error::Error>> {
    info!("[main] Starting JWM instance");
    let mut jwm = Jwm::new()?;
    jwm.checkotherwm()?;
    if let Err(_) = Command::new("pkill")
        .arg("-9")
        .arg(CONFIG.status_bar_base_name())
        .spawn()
    {
        error!("[new] Clear status bar failed");
    }
    jwm.setup()?;
    jwm.scan()?;

    // 运行主循环
    jwm.run()?;
    // 清理
    jwm.cleanup()?;

    if jwm.is_restarting.load(Ordering::SeqCst) {
        if let Err(_) = Command::new("jwmc").arg("restart").spawn() {
            error!("[new] Failted to quit jwmc");
        }
    } else {
        if let Err(_) = Command::new("jwmc").arg("quit").spawn() {
            error!("[new] Failted to quit jwmc");
        }
    }
    Ok(())
}
