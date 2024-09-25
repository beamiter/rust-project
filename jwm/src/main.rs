use std::{ffi::CString, process::exit, ptr::null_mut};

use simplelog::*;
use log::{info, warn};

use dwm::{checkotherwm, cleanup, dpy, run, scan, setup};
use libc::{setlocale, LC_CTYPE};
use x11::xlib::{XCloseDisplay, XOpenDisplay, XSupportsLocale};

mod config;
mod drw;
mod dwm;
mod xproto;

fn main() {
    let log_file = std::fs::File::create("/home/mm/jwm.log").unwrap();
    // SimpleLogger::init(LevelFilter::Info, Config::default()).unwrap();
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
        info!("checkotherwm");
        // checkotherwm();
        info!("setup");
        setup();
        info!("scan");
        scan();
        info!("run");
        run();
        info!("cleanup");
        cleanup();
        info!("XCloseDisplay");
        XCloseDisplay(dpy);
        warn!("end");
    }
}
