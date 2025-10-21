// use bar::StatusBar;
use jwm::{jwm::SHARED_PATH, Jwm};
use log::{error, info, warn};
use std::{env, process::Command, sync::atomic::Ordering};
use xbar_core::initialize_logging;

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
    setup_locale();
    jwm::miscellaneous::init_auto_command();
    jwm::miscellaneous::init_auto_start();

    initialize_logging("jwm", SHARED_PATH)?;

    info!("[main] main begin");

    // 运行窗口管理器
    run_jwm()?;

    Ok(())
}

fn run_jwm() -> Result<(), Box<dyn std::error::Error>> {
    info!("[main] Starting JWM instance");
    let backend = Box::new(X11Backend::new()?);

    let mut jwm = Jwm::new(backend)?;
    jwm.checkotherwm()?;

    jwm.setup()?;

    jwm.scan()?;

    jwm.run()?;

    jwm.cleanup()?;

    if !jwm.is_restarting.load(Ordering::SeqCst) {
        if let Err(_) = Command::new("jwm-tool").arg("quit").spawn() {
            error!("[new] Failted to quit jwm daemon");
        }
    }
    Ok(())
}
