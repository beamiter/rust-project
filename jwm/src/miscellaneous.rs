use dirs_next::home_dir;
use log::info;
use std::process::Command;

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
}

pub fn init_auto_start() {
    match home_dir() {
        Some(path) => {
            let start_fehbg = path.as_path().join(".fehbg");
            println!("fehbg: {:?}", start_fehbg);
            if let Err(_) = Command::new(start_fehbg).spawn() {
                println!("[spawn] Start fehbg failed");
                info!("[spawn] Start fehbg failed");
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
