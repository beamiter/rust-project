// src/main.rs
use jwm::{Jwm, jwm::SHARED_PATH};
use log::{error, info, warn};
use std::{env, process::Command, sync::atomic::Ordering};
use xbar_core::initialize_logging;

// 导入后端
#[cfg(feature = "backend-x11")]
use jwm::backend::x11::backend::X11Backend;

#[cfg(feature = "backend-udev")]
use jwm::backend::wayland_udev::backend::UdevBackend;

#[cfg(feature = "backend-wayland-x11")]
use jwm::backend::wayland_x11::backend::WaylandX11Backend;

#[cfg(feature = "backend-wayland-winit")]
use jwm::backend::wayland_winit::backend::WaylandWinitBackend;

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

    let mut backend = select_backend()?;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendChoice {
    X11,
    Udev,
    WaylandX11,
    WaylandWinit,
}

fn select_backend() -> Result<Box<dyn jwm::backend::api::Backend>, Box<dyn std::error::Error>> {
    // Selection rule:
    // - If JWM_BACKEND is set, honor it.
    // - Otherwise, pick the only backend feature that is enabled.
    // - If 0 or >1 backend features are enabled, error out to avoid confusion.
    let enabled_x11 = cfg!(feature = "backend-x11");
    let enabled_udev = cfg!(feature = "backend-udev");
    let enabled_wayland_x11 = cfg!(feature = "backend-wayland-x11");
    let enabled_wayland_winit = cfg!(feature = "backend-wayland-winit");
    let enabled_count =
        enabled_x11 as u8 + enabled_udev as u8 + enabled_wayland_x11 as u8 + enabled_wayland_winit as u8;

    let resolved = if let Ok(val) = env::var("JWM_BACKEND") {
        let val = val.to_lowercase();
        match val.as_str() {
            "x11" => BackendChoice::X11,
            "wayland-udev" | "udev" | "wayland" => BackendChoice::Udev,
            "wayland-x11" | "x11-wayland" | "windowed" => BackendChoice::WaylandX11,
            "wayland-winit" | "winit" => BackendChoice::WaylandWinit,
            other => {
                return Err(format!(
                    "Unknown JWM_BACKEND={other:?}; expected 'x11'|'wayland-udev'|'wayland-x11'|'wayland-winit'"
                )
                .into());
            }
        }
    } else {
        match enabled_count {
            0 => {
                return Err(
                    "No backend features enabled. Build with one of: backend-x11 | backend-udev | backend-wayland-x11 | backend-wayland-winit"
                        .into(),
                );
            }
            1 => {
                if enabled_x11 {
                    BackendChoice::X11
                } else if enabled_wayland_x11 {
                    BackendChoice::WaylandX11
                } else if enabled_wayland_winit {
                    BackendChoice::WaylandWinit
                } else {
                    BackendChoice::Udev
                }
            }
            _ => {
                return Err(
                    "Multiple backends are enabled; set JWM_BACKEND explicitly to one of: x11 | wayland-udev | wayland-x11 | wayland-winit"
                        .into(),
                );
            }
        }
    };

    match resolved {
        BackendChoice::X11 => {
            #[cfg(feature = "backend-x11")]
            {
                info!("Initializing X11 Backend");
                return Ok(Box::new(X11Backend::new()?));
            }
            #[cfg(not(feature = "backend-x11"))]
            {
                return Err("X11 backend requested but 'backend-x11' feature is not enabled".into());
            }
        }
        BackendChoice::Udev => {
            #[cfg(feature = "backend-udev")]
            {
                info!("Initializing Wayland/Udev Backend (wayland-udev)");
                return Ok(Box::new(UdevBackend::new()?));
            }
            #[cfg(not(feature = "backend-udev"))]
            {
                return Err("wayland-udev backend requested but 'backend-udev' feature is not enabled".into());
            }
        }
        BackendChoice::WaylandX11 => {
            #[cfg(feature = "backend-wayland-x11")]
            {
                info!("Initializing Wayland-on-X11 Backend (Smithay windowed)");
                return Ok(Box::new(WaylandX11Backend::new()?));
            }
            #[cfg(not(feature = "backend-wayland-x11"))]
            {
                return Err(
                    "wayland-x11 backend requested but 'backend-wayland-x11' feature is not enabled"
                        .into(),
                );
            }
        }
        BackendChoice::WaylandWinit => {
            #[cfg(feature = "backend-wayland-winit")]
            {
                info!("Initializing Wayland/Winit Backend (Smithay windowed)");
                return Ok(Box::new(WaylandWinitBackend::new()?));
            }
            #[cfg(not(feature = "backend-wayland-winit"))]
            {
                return Err(
                    "wayland-winit backend requested but 'backend-wayland-winit' feature is not enabled"
                        .into(),
                );
            }
        }
    }
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
