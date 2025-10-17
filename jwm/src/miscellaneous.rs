use dirs_next::home_dir;
use log::{error, info};
use std::process::Command;

use crate::terminal_prober::ADVANCED_TERMINAL_PROBER;

const USE_PICOM: bool = false;

pub fn init_auto_command() {
    let prober = &*ADVANCED_TERMINAL_PROBER;
    if let Some(terminal) = prober.get_available_terminal() {
        info!("Found terminal: {}", terminal.command);
    } else {
        error!("No terminal found!");
    }
    let start_amixer = "amixer";
    if let Err(_) = Command::new(start_amixer)
        .arg("sset")
        .arg("Master")
        .arg("80%")
        .arg("unmute")
        .spawn()
    {
        error!("[spawn] Start Master volume failed");
    }
    if let Err(_) = Command::new(start_amixer)
        .arg("sset")
        .arg("Headphone")
        .arg("80%")
        .arg("unmute")
        .spawn()
    {
        error!("[spawn] Start Headphone volume failed");
    }
}

pub fn init_auto_start() {
    match home_dir() {
        Some(path) => {
            let start_fehbg = path.as_path().join(".fehbg");
            info!("fehbg: {:?}", start_fehbg);
            if let Err(_) = Command::new(start_fehbg).spawn() {
                error!("[spawn] Start fehbg failed");
            } else {
                info!("[spawn] Start fehbg succed");
            }
        }
        None => error!("Could not find the home directory."),
    }
    if USE_PICOM {
        if let Err(_) = Command::new("picom").spawn() {
            error!("[spawn] Start picom failed");
        } else {
            info!("[spawn] Start picom succed");
        }
    }
}
