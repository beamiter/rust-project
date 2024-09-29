use std::{ffi::CString, process::exit, ptr::null_mut};

use log::{info, warn};
use simplelog::*;

use dwm::{checkotherwm, cleanup, dpy, run, scan, setup};
use libc::{setlocale, LC_CTYPE};
use x11::xlib::{XCloseDisplay, XOpenDisplay, XSupportsLocale};

mod config;
mod drw;
mod dwm;
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

fn main() {
    let log_file = std::fs::File::create("/home/mm/jwm.log").unwrap();
    WriteLogger::init(LevelFilter::Info, Config::default(), log_file).unwrap();
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
        warn!("begin");
        println!("checkotherwm");
        checkotherwm();
        println!("setup");
        setup();
        println!("scan");
        scan();
        println!("run");
        run();
        println!("cleanup");
        cleanup();
        println!("XCloseDisplay");
        XCloseDisplay(dpy);
        warn!("end");
    }
}
