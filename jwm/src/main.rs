use chrono::prelude::*;
use std::{ffi::CString, process::exit, ptr::null_mut};

use log::info;
use simplelog::*;

use dwm::{checkotherwm, cleanup, dpy, run, scan, setup};
use libc::{setlocale, LC_CTYPE};
use x11::xlib::{XCloseDisplay, XOpenDisplay, XSupportsLocale};

mod config;
mod drw;
mod dwm;
mod xproto;
mod miscellaneous;

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
    miscellaneous::for_test();

    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();
    let log_filename = format!("/tmp/jwm_{}.log", timestamp);
    let _log_file = std::fs::File::create(log_filename).unwrap();
    WriteLogger::init(LevelFilter::Info, Config::default(), _log_file).unwrap();
    // WriteLogger::init(LevelFilter::Info, Config::default(), std::io::stdout()).unwrap();
    unsafe {
        let c_string = CString::new("").unwrap();
        if setlocale(LC_CTYPE, c_string.as_ptr()).is_null() || XSupportsLocale() <= 0 {
            eprintln!("warning: no locale support");
        }
        dpy = XOpenDisplay(null_mut());
        if dpy.is_null() {
            eprintln!("jwm: cannot open display");
            exit(1);
        }
        info!("[main] main begin");
        info!("[main] checkotherwm");
        checkotherwm();
        info!("[main] setup");
        setup();
        info!("[main] scan");
        scan();
        info!("[main] run");
        run();
        info!("[main] cleanup");
        cleanup();
        info!("[main] XCloseDisplay");
        XCloseDisplay(dpy);
        info!("[main] end");
    }
}
