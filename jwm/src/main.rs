// src/main.rs
use jwm::{jwm::SHARED_PATH, Jwm};
use log::{error, info, warn};
use std::{env, process::Command, sync::atomic::Ordering};
use xbar_core::initialize_logging;

#[cfg(feature = "backend-wayland")]
use jwm::backend::wayland::backend::WaylandBackend;
#[cfg(feature = "backend-x11")]
use jwm::backend::x11::backend::X11Backend;

use cfg_if::cfg_if;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_locale();
    jwm::miscellaneous::init_auto_command();
    jwm::miscellaneous::init_auto_start();

    initialize_logging("jwm", SHARED_PATH)?;
    info!("[main] begin");

    run_jwm()?;
    Ok(())
}

fn run_jwm() -> Result<(), Box<dyn std::error::Error>> {
    info!("[main] Starting JWM instance");

    cfg_if! {
        if #[cfg(feature = "backend-wayland")]
        {
            let backend: Box<dyn jwm::backend::api::Backend> =
                Box::new(WaylandBackend::new()?);
        }
        else if #[cfg(feature = "backend-x11")]
        {
            let backend: Box<dyn jwm::backend::api::Backend> =
                Box::new(X11Backend::new()?);
        }
        else {
            panic!("not supported feature");
        }
    };

    let mut jwm = Jwm::new(backend)?;
    jwm.checkotherwm()?; // Wayland 下此函数仍会执行，内部是 no-op/不影响
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
