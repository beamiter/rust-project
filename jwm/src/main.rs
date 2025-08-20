// use bar::StatusBar;
use chrono::prelude::*;
use coredump::register_panic_handler;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use jwm::Jwm;
use libc::{setlocale, LC_CTYPE};
use log::info;
use std::sync::mpsc;
use std::{ffi::CString, process::exit};
use std::{thread, time::Duration};
use x11::xlib::{XCloseDisplay, XSupportsLocale};

#[tokio::main]
async fn main()-> Result<(), Box<dyn std::error::Error>> {
    let _ = register_panic_handler();
    jwm::miscellaneous::init_auto_command();
    jwm::miscellaneous::init_auto_start();
    let (tx, rx) = mpsc::channel();

    let mut jwm = Jwm::new(tx);
    if jwm.x11_dpy.is_null() {
        eprintln!("jwm: cannot open display");
        exit(1);
    }

    let status_update_thread = thread::spawn(move || {
        // let mut status_bar = StatusBar::new();
        loop {
            let mut need_sleep = true;
            match rx.try_recv() {
                Ok(mut latest_value) => {
                    while let Ok(value) = rx.try_recv() {
                        latest_value = value;
                    }
                    match latest_value {
                        0 => {
                            info!("Recieve {}, shut down", latest_value);
                            break;
                        }
                        1 => {
                            need_sleep = false;
                            // status_bar.update_icon_list();
                        }
                        _ => {
                            break;
                        }
                    }
                }
                Err(_) => {}
            }
            // let status = status_bar.broadcast_string();
            // info!("status string: {}", status);
            // let _output = Command::new("xsetroot").arg("-name").arg(status).output();
            if need_sleep {
                thread::sleep(Duration::from_millis(500));
            }
        }
    });

    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();
    let log_filename = format!("jwm_{}", timestamp);
    Logger::try_with_str("info")
        .unwrap()
        .format(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory("/tmp/jwm")
                .basename(format!("{log_filename}"))
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Info)
        // .log_to_stdout()
        // .buffer_capacity(1024)
        // .use_background_worker(true)
        .rotate(
            Criterion::Size(10_000_000),
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .unwrap();
    unsafe {
        let c_string = CString::new("").unwrap();
        if setlocale(LC_CTYPE, c_string.as_ptr()).is_null() || XSupportsLocale() <= 0 {
            eprintln!("warning: no locale support");
        }
        info!("[main] main begin");
        info!("[main] checkotherwm");
        jwm.checkotherwm();
        info!("[main] setup");
        jwm.setup()?;
        info!("[main] scan");
        let _ = jwm.scan();
        info!("[main] run");
        jwm.run_async().await?;
        info!("[main] cleanup");
        jwm.cleanup();
        info!("[main] XCloseDisplay");
        XCloseDisplay(jwm.x11_dpy);
        info!("[main] end");
    }

    match status_update_thread.join() {
        Ok(_) => println!("Status update thread finished successfully."),
        Err(e) => eprintln!("Error joining status update thread: {:?}", e),
    }

    Ok(())
}
