use dirs_next::home_dir;
use log::info;
use std::process::Command;

use crate::dwm::remove_control_characters;

pub fn for_test() {
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
}

pub fn init_auto_start() {
    match home_dir() {
        Some(path) => {
            let start_fehbg = path.as_path().join(".fehbg");
            println!("fehbg: {:?}", start_fehbg);
            if let Err(_) = Command::new(start_fehbg).status() {
                println!("[spawn] Start fehbg failed");
                info!("[spawn] Start fehbg failed");
            }
        }
        None => eprintln!("Could not find the home directory."),
    }
    let start_picom = "picom&";
    if let Err(_) = Command::new(start_picom).status() {
        println!("[spawn] Start picom failed");
        info!("[spawn] Start picom failed");
    }
}
