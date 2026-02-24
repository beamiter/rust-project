// src/main.rs
use jwm::{Jwm, jwm::SHARED_PATH};
use log::{error, info, warn};
use std::{env, process::Command, sync::atomic::Ordering};
use xbar_core::initialize_logging;

// 导入后端
use jwm::backend::wayland_udev::backend::UdevBackend;
use jwm::backend::wayland_winit::backend::WaylandWinitBackend;
use jwm::backend::wayland_x11::backend::WaylandX11Backend;
use jwm::backend::x11::backend::X11Backend;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    initialize_logging("jwm", SHARED_PATH)?;
    install_panic_hook();
    info!("[main] begin");

    setup_locale();
    jwm::miscellaneous::init_auto_command();
    jwm::miscellaneous::init_auto_start();

    run_jwm()?;
    Ok(())
}

fn install_panic_hook() {
    // In this workspace the release profile is configured with `panic = "abort"`.
    // That means panics often look like a “crash with no logs”.
    // Installing a hook lets us capture the panic payload + location (and a backtrace)
    // into the regular log output before abort.
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };

        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());

        let backtrace = std::backtrace::Backtrace::force_capture();

        // Best-effort: log to both stderr and our logger.
        eprintln!("[panic] {payload} @ {location}\nBacktrace:\n{backtrace:?}");
        error!("[panic] {payload} @ {location} | backtrace={backtrace:?}");
    }));
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
    WaylandUdev,
    WaylandX11,
    WaylandWinit,
}

fn select_backend() -> Result<Box<dyn jwm::backend::api::Backend>, Box<dyn std::error::Error>> {
    // Selection rule:
    // - If JWM_BACKEND is set, honor it.
    // - Otherwise, default to x11.
    let resolved = if let Ok(val) = env::var("JWM_BACKEND") {
        let val = val.to_lowercase();
        match val.as_str() {
            "x11" => BackendChoice::X11,
            "wayland-udev" | "udev" | "wayland" => BackendChoice::WaylandUdev,
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
        BackendChoice::X11
    };

    match resolved {
        BackendChoice::X11 => {
            info!("Initializing X11 Backend");
            Ok(Box::new(X11Backend::new()?))
        }
        BackendChoice::WaylandUdev => {
            info!("Initializing Wayland/Udev Backend (wayland-udev)");
            Ok(Box::new(UdevBackend::new()?))
        }
        BackendChoice::WaylandX11 => {
            info!("Initializing Wayland-on-X11 Backend (Smithay windowed)");
            Ok(Box::new(WaylandX11Backend::new()?))
        }
        BackendChoice::WaylandWinit => {
            info!("Initializing Wayland/Winit Backend (Smithay windowed)");
            Ok(Box::new(WaylandWinitBackend::new()?))
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
