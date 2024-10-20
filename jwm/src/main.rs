use chrono::prelude::*;
use std::process::Command;
use std::{ffi::CString, process::exit, ptr::null_mut};
use std::{thread, time::Duration};

use log::info;
use simplelog::*;

use dwm::{checkotherwm, cleanup, dpy, run, scan, setup};
use libc::{setlocale, LC_CTYPE};
use x11::xlib::{XCloseDisplay, XOpenDisplay, XSupportsLocale};

mod bar;
use bar::*;
mod config;
mod drw;
mod dwm;
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
    miscellaneous::for_test();
    let black = "#222526";
    let green = "#89b482";
    let white = "#c7b89d";
    let grey = "#2b2e2f";
    let blue = "#6f8faf";
    let red = "#ec6b64";
    let darkblue = "#6080a0";

    let status_update_thread = thread::spawn(move || {
        let mut interval = 0usize;
        let mut updates_info = String::new();
        loop {
            if interval % 3600 == 0 {
                updates_info = pkg_updates(green);
            }

            let status = format!(
                "{} {} {} {} {} {} {}",
                updates_info,
                battery_capacity(blue),
                brightness(red),
                cpu_load(black, green, white, grey),
                mem_usage(blue, black),
                wlan_status(black, blue),
                current_time(black, darkblue, blue)
            );

            // Update X root window name (status bar), here we will just print to stdout
            println!("{}", status);
            // let _output = Command::new("xsetroot")
            //     .arg("-name")
            //     .arg(status)
            //     .output();

            interval += 1;
            thread::sleep(Duration::from_secs(1));
        }
    });

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

    match status_update_thread.join() {
        Ok(_) => println!("Status update thread finished successfully."),
        Err(e) => eprintln!("Error joining status update thread: {:?}", e),
    }
}
