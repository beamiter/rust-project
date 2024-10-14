use chrono::prelude::*;
use std::{ffi::CString, process::exit, ptr::null_mut};

use log::info;
use simplelog::*;

use dwm::{checkotherwm, cleanup, dpy, run, scan, setup};
use libc::{setlocale, LC_CTYPE};
use x11::xlib::{XCloseDisplay, XOpenDisplay, XSupportsLocale};

use crate::dwm::remove_control_characters;

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

// For dual monitor:
// xrandr --output HDMI-1 --rotate normal --left-of eDP-1 --auto &

fn main() {
    let tt: &str = "中国";
    println!("len: {}", tt.len());
    println!("0: {}", &tt[0..3]);
    println!(
        "0:{} len: {}, decode: {}, {:0X}",
        tt.chars().nth(0).unwrap(),
        tt.chars().nth(0).unwrap().len_utf8(),
        tt.chars().nth(0).unwrap() as u32,
        tt.chars().nth(0).unwrap() as u32
    );
    println!("1: {}", &tt[3..]);
    println!(
        "1:{} len: {}, decode: {}, {:0X}",
        tt.chars().nth(1).unwrap(),
        tt.chars().nth(1).unwrap().len_utf8(),
        tt.chars().nth(1).unwrap() as u32,
        tt.chars().nth(1).unwrap() as u32
    );
    let mut text = "\u{200d}\u{2061}\u{200d}\u{2063}\u{202c}\u{202c}\u{2064}\u{2064}\u{200d}\u{202c}\u{2063}\u{202c}\u{2063}\u{2064}\u{202c}\u{200c}\u{feff}\u{feff}\u{2061}\u{2063}\u{2061}\u{200b}\u{200c}\u{feff}\u{2063}\u{200b}\u{200b}\u{200b}\u{2061}\u{200b}\u{2063}\u{200c}\u{200b}\u{200b}\u{2063}\u{2061}\u{2062}\u{200d}\u{2064}\u{feff}\u{202c}\u{2064}\u{2063}\u{200d}\u{2061}\u{200c}\u{feff}\u{202c}\u{2062}\u{202c}CP路测跟车记录 - Feishu Docs - Google Chrome";
    println!("{}", text.len());
     let binding = &remove_control_characters(&text.to_string());
    text = binding;
    println!("{}", text.len());

    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();
    let log_filename = format!("/tmp/jwm_{}.log", timestamp);
    let _log_file = std::fs::File::create(log_filename).unwrap();
    // WriteLogger::init(LevelFilter::Info, Config::default(), _log_file).unwrap();
    WriteLogger::init(LevelFilter::Info, Config::default(), std::io::stdout()).unwrap();
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
