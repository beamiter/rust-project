// src/main.rs
use log::{error, info};
use std::sync::atomic::Ordering;

use xbar_core::initialize_logging;
use jwm::{jwm::SHARED_PATH, Jwm};

#[cfg(feature = "backend-x11")]
use jwm::backend::x11::backend::X11Backend;

#[cfg(feature = "backend-wayland")]
use jwm::backend::wayland::backend::WaylandBackend;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    jwm::miscellaneous::init_auto_command();
    jwm::miscellaneous::init_auto_start();
    initialize_logging("jwm", SHARED_PATH)?;
    info!("[main] main begin");

    run_jwm()?;
    Ok(())
}

fn run_jwm() -> Result<(), Box<dyn std::error::Error>> {
    info!("[main] Starting JWM instance");

    #[cfg(all(feature="backend-wayland", not(feature="backend-x11")))]
    let backend: Box<dyn jwm::backend::api::Backend> = Box::new(WaylandBackend::new()?);

    #[cfg(all(feature="backend-x11", not(feature="backend-wayland")))]
    let backend: Box<dyn jwm::backend::api::Backend> = Box::new(X11Backend::new()?);

    #[cfg(all(feature="backend-x11", feature="backend-wayland"))]
    let backend: Box<dyn jwm::backend::api::Backend> = if env::var("WAYLAND_DISPLAY").is_ok() {
        Box::new(WaylandBackend::new()?)
    } else {
        Box::new(X11Backend::new()?)
    };

    let mut jwm = Jwm::new(backend)?;
    jwm.checkotherwm()?;      // Wayland 下将 no-op 成功返回
    jwm.setup()?;
    jwm.scan()?;
    jwm.run()?;
    jwm.cleanup()?;

    if !jwm.is_restarting.load(Ordering::SeqCst) {
        if let Err(_) = std::process::Command::new("jwm-tool").arg("quit").spawn() {
            error!("[new] Failed to quit jwm daemon");
        }
    }
    Ok(())
}
