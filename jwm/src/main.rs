use chrono::prelude::*;
use coredump::register_panic_handler;
use dwm::Dwm;
use std::process::Command;
use std::sync::mpsc;
use std::{ffi::CString, process::exit, ptr::null_mut};
use std::{thread, time::Duration};

use log::info;
use simplelog::*;

use libc::{setlocale, LC_CTYPE};
use x11::xlib::{XCloseDisplay, XOpenDisplay, XSupportsLocale};

mod bar;
use bar::*;
mod config;
mod drw;
mod dwm;
mod icon_gallery;
mod miscellaneous;
mod xproto;

mod tests;

// Xnest and Xephyr is all you need!
// Xnest:
// Xnest :2 -geometry 1024x768 &
// export DISPLAY=:2
// exec jwm

// Xephyr:
// Xephyr :2 -screen 1024x768 &
// DISPLAY=:2 jwm

// For dual monitor:
// xrandr --output HDMI-1 --rotate normal --left-of eDP-1 --auto &

fn main() {
    let _ = register_panic_handler();
    miscellaneous::for_test();
    miscellaneous::init_auto_start();

    let (tx, rx) = mpsc::channel();

    let mut dwm = Dwm::new(tx);

    let mut status_bar = StatusBar::new();
    let status_update_thread = thread::spawn(move || {
        loop {
            match rx.try_recv() {
                Ok(val) => match val {
                    0 => {
                        info!("Recieve {}, shut down", val);
                        break;
                    }
                    1 => {
                        status_bar.update_icon_list();
                    }
                    _ => {
                        break;
                    }
                },
                Err(_) => {}
            }
            let status = status_bar.broadcast_string();
            // info!("status string: {}", status);
            // Update X root window name (status bar), here we will just print to stdout
            let _output = Command::new("xsetroot").arg("-name").arg(status).output();
            thread::sleep(Duration::from_millis(500));
        }
    });

    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();
    let log_filename = format!("/tmp/jwm_{}.log", timestamp);
    let _log_file = std::fs::File::create(log_filename).unwrap();
    // WriteLogger::init(LevelFilter::Warn, Config::default(), _log_file).unwrap();
    // WriteLogger::init(LevelFilter::Info, Config::default(), _log_file).unwrap();
    WriteLogger::init(LevelFilter::Info, Config::default(), std::io::stdout()).unwrap();
    unsafe {
        let c_string = CString::new("").unwrap();
        if setlocale(LC_CTYPE, c_string.as_ptr()).is_null() || XSupportsLocale() <= 0 {
            eprintln!("warning: no locale support");
        }
        dwm.dpy = XOpenDisplay(null_mut());
        if dwm.dpy.is_null() {
            eprintln!("jwm: cannot open display");
            exit(1);
        }
        info!("[main] main begin");
        info!("[main] checkotherwm");
        dwm.checkotherwm();
        info!("[main] setup");
        dwm.setup();
        info!("[main] scan");
        dwm.scan();
        info!("[main] run");
        dwm.run();
        info!("[main] cleanup");
        dwm.cleanup();
        info!("[main] XCloseDisplay");
        XCloseDisplay(dwm.dpy);
        info!("[main] end");
    }

    match status_update_thread.join() {
        Ok(_) => println!("Status update thread finished successfully."),
        Err(e) => eprintln!("Error joining status update thread: {:?}", e),
    }
}
