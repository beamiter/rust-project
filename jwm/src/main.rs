// src/main.rs
use jwm::{Jwm, jwm::SHARED_PATH};
use log::{error, info, warn};
use std::{env, process::Command, sync::atomic::Ordering};
use xbar_core::initialize_logging;

#[cfg(feature = "backend-wayland")]
use jwm::backend::wayland::backend::WaylandBackend;
#[cfg(feature = "backend-wayland")]
use jwm::backend::wayland::test_main;
#[cfg(feature = "backend-x11")]
use jwm::backend::x11::backend::X11Backend;

use cfg_if::cfg_if;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_locale();
    jwm::miscellaneous::init_auto_command();
    jwm::miscellaneous::init_auto_start();

    initialize_logging("jwm", SHARED_PATH)?;
    info!("[main] begin");

    #[cfg(feature = "backend-wayland")]
    test_main()?;

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
