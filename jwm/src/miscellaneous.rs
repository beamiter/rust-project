use dirs_next::home_dir;
use log::info;
use std::{ffi::CString, process::Command};

use crate::terminal_prober::ADVANCED_TERMINAL_PROBER;

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

    let word = "goodbye";

    let count = word.chars().count();
    assert_eq!(7, count);

    let mut chars = word.chars();
    chars.next();
    let rebuilt_word: String = chars.collect();
    println!("rebuilt_word: {}", rebuilt_word);

    // let text = "en\0"; // This has a null byte in the middle.
    let text = "en\000\000"; // This has a null byte in the middle.
    let c_str = CString::new(text);

    match c_str {
        Ok(_) => println!("CString created successfully"),
        Err(e) => println!("Failed to create CString: {:?}", e),
    }

    // Remove the null bytes using filter() and collect()
    let filtered_text: String = text.chars().filter(|&c| c != '\0').collect();

    // Now we can safely create a CString since `filtered_text` has no null bytes
    match CString::new(filtered_text) {
        Ok(c_str) => println!("CString created successfully: {:?}", c_str),
        Err(e) => println!("Failed to create CString: {:?}", e),
    }

    // 基础使用
    // println!("Available terminal: {:?}", termcmd.as_slice());

    // 高级使用
    let prober = &*ADVANCED_TERMINAL_PROBER;

    // 获取可用终端
    if let Some(terminal) = prober.get_available_terminal() {
        println!("Found terminal: {}", terminal.command);

        // // 构建启动命令
        // if let Some(cmd) = prober.build_command("bash", Some("My Terminal"), Some("~/")) {
        //     println!("Launch command: {:?}", cmd);
        //
        //     // 执行终端
        //     std::process::Command::new(&cmd[0])
        //         .args(&cmd[1..])
        //         .spawn()
        //         .expect("Failed to launch terminal");
        // }
    } else {
        println!("No terminal found!");
    }
}

pub fn init_auto_start() {
    match home_dir() {
        Some(path) => {
            let start_fehbg = path.as_path().join(".fehbg");
            println!("fehbg: {:?}", start_fehbg);
            if let Err(_) = Command::new(start_fehbg).spawn() {
                println!("[spawn] Start fehbg failed");
                info!("[spawn] Start fehbg failed");
                panic!("[spawn] Start fehbg failed");
            }
        }
        None => eprintln!("Could not find the home directory."),
    }
    let start_picom = "picom";
    if let Err(_) = Command::new(start_picom)
        .arg("--backend")
        .arg("xrender")
        .spawn()
    {
        println!("[spawn] Start picom failed");
        info!("[spawn] Start picom failed");
        panic!("[spawn] Start picom failed");
    }
}
