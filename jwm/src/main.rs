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
    Auto,
    X11,
    Udev,
}

fn select_backend() -> Result<Box<dyn jwm::backend::api::Backend>, Box<dyn std::error::Error>> {
    let choice = env::var("JWM_BACKEND")
        .unwrap_or_else(|_| "auto".to_string())
        .to_lowercase();

    let choice = match choice.as_str() {
        "auto" | "" => BackendChoice::Auto,
        "x11" => BackendChoice::X11,
        "udev" | "wayland" => BackendChoice::Udev,
        other => {
            warn!(
                "Unknown JWM_BACKEND={other:?}; expected 'auto'|'x11'|'udev'. Falling back to auto."
            );
            BackendChoice::Auto
        }
    };

    // Heuristic: if we're clearly inside an X11 session, prefer X11 backend.
    // Using udev/KMS in an X11 session commonly fails to acquire DRM (device busy)
    // and may also steal input devices via libseat/libinput.
    let session_type = env::var("XDG_SESSION_TYPE").ok();
    let in_x11_session = session_type.as_deref() == Some("x11") || env::var("DISPLAY").is_ok();

    let resolved = match choice {
        BackendChoice::Auto => {
            if in_x11_session {
                BackendChoice::X11
            } else {
                BackendChoice::Udev
            }
        }
        other => other,
    };

    match resolved {
        BackendChoice::X11 => {
            #[cfg(feature = "backend-x11")]
            {
                info!("Initializing X11 Backend (selected via JWM_BACKEND / session type)");
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
                if in_x11_session {
                    warn!(
                        "Starting udev/KMS backend inside an X11 session. If you see 'KMS init failed (running headless)', run JWM from a TTY or a Wayland session instead, or set JWM_BACKEND=x11."
                    );
                }
                info!("Initializing Udev Backend (Smithay)");
                return Ok(Box::new(UdevBackend::new()?));
            }
            #[cfg(not(feature = "backend-udev"))]
            {
                return Err("udev backend requested but 'backend-udev' feature is not enabled".into());
            }
        }
        BackendChoice::Auto => unreachable!("Auto backend must be resolved"),
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
