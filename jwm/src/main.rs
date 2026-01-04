// src/main.rs
use jwm::{Jwm, jwm::SHARED_PATH};
use log::{error, info, warn};
use std::{env, process::Command, sync::atomic::Ordering};
use xbar_core::initialize_logging;

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

    // 1. 创建 Backend 所有权
    let mut backend: Box<dyn jwm::backend::api::Backend> = Box::new(X11Backend::new()?);

    // 2. 创建 Jwm，传入 backend 引用
    let mut jwm = Jwm::new(&mut *backend)?;

    // 3. 调用生命周期方法，现在需要手动传 backend
    jwm.checkotherwm(&mut *backend)?;
    jwm.setup(&mut *backend)?;
    jwm.scan(&mut *backend)?;

    // 4. 启动循环
    // backend 拥有控制权，jwm 借用给它
    backend.run(&mut jwm)?;

    // 5. 清理
    jwm.cleanup(&mut *backend)?;

    if !jwm.is_restarting.load(Ordering::SeqCst) {
        if let Err(_) = Command::new("jwm-tool").arg("quit").spawn() {
            error!("[new] Failted to quit jwm daemon");
        }
    }
    Ok(())
}

pub fn setup_locale() {
    let locale = env::var("LANG")
        .or_else(|_| env::var("LC_ALL"))
        .or_else(|_| env::var("LC_CTYPE"))
        .unwrap_or_else(|_| "C".to_string());
    info!("Using locale: {}", locale);
    if !locale.contains("UTF-8") && !locale.contains("utf8") {
        warn!(
            "Non-UTF-8 locale detected ({}). Text display may be affected.",
            locale
        );
        warn!("Consider setting: export LANG=en_US.UTF-8");
    }
    if env::var("LC_CTYPE").is_err() {
        if locale.contains("UTF-8") {
            unsafe {
                env::set_var("LC_CTYPE", &locale);
            }
        } else {
            unsafe {
                env::set_var("LC_CTYPE", "en_US.UTF-8");
            }
        }
    }
}
