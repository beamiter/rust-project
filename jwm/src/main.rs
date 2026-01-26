// src/main.rs
use jwm::{Jwm, jwm::SHARED_PATH};
use log::{error, info, warn};
use std::{env, process::Command, sync::atomic::Ordering};
use xbar_core::initialize_logging;

// 导入后端
#[cfg(feature = "backend-x11")]
use jwm::backend::x11::backend::X11Backend;

#[cfg(feature = "backend-udev")]
use jwm::backend::udev::backend::UdevBackend;

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

    // 动态选择 Backend
    let mut backend: Box<dyn jwm::backend::api::Backend>;

    #[cfg(feature = "backend-udev")]
    {
        info!("Initializing Udev Backend (Smithay)");
        backend = Box::new(UdevBackend::new()?);
    }

    #[cfg(all(not(feature = "backend-udev"), feature = "backend-x11"))]
    {
        info!("Initializing X11 Backend");
        backend = Box::new(X11Backend::new()?);
    }

    #[cfg(all(not(feature = "backend-udev"), not(feature = "backend-x11")))]
    {
        panic!("No backend enabled!");
    }

    backend.check_existing_wm()?;

    let mut jwm = Jwm::new(&mut *backend)?;
    jwm.setup(&mut *backend)?;
    jwm.setup_initial_windows(&mut *backend)?;
    jwm.run(&mut *backend)?;
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
