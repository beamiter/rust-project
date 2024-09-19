use std::{ffi::CString, process::exit, ptr::null_mut};

use dwm::{checkotherwm, cleanup, dpy, run, scan, setup};
use libc::{setlocale, LC_CTYPE};
use x11::xlib::{XCloseDisplay, XOpenDisplay, XSupportsLocale};

mod config;
mod drw;
mod dwm;
mod xproto;

fn main() {
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
        checkotherwm();
        setup();
        scan();
        run();
        cleanup();
        XCloseDisplay(dpy);
    }
}
