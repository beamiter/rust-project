extern crate gdk;
extern crate gtk;
extern crate vte;
use gdk::ffi::GdkRGBA;
use gdk::glib::ffi::GSpawnFlags;
use glib::*;
use glib_sys::*;
use gtk::ffi::GtkWidget;
use std::env;
use std::ffi::c_char;
use std::ffi::CStr;
use std::os::raw::c_void;
use std::path::PathBuf;
use vte_sys::VteRegex;
use vte_sys::VteTerminal;

mod config;
use crate::config::CONFIG;

const PCRE2_CODE_UNIT_WIDTH: i8 = 8;
const NAME: &str = "jterm2";

#[derive(Debug, Clone, Copy)]
pub enum ConfigItemType {
    String,
    StringList,
    Boolean,
    Int64,
    Uint64,
}

// With tagged variants.
#[derive(Debug, Clone)]
pub enum ConfigValue<'a> {
    S(&'a str),
    Sl(&'a [&'a str]),
    B(gboolean),
    I(i64),
    Ui(u64),
}

#[derive(Debug, Clone)]
pub struct ConfigItem<'a> {
    s: &'a str,
    n: &'a str,
    t: ConfigItemType,
    // Only used for StringList
    l: Option<u64>,
    v: ConfigValue<'a>,
}

struct Terminal {
    hold: gboolean,
    term: *mut GtkWidget,
    win: *mut GtkWidget,
    has_child_exit_status: gboolean,
    child_exit_status: i64,
    current_font: usize,
}

impl Terminal {
    fn new() -> Self {
        Terminal {
            hold: 0,
            term: std::ptr::null_mut(),
            win: std::ptr::null_mut(),
            has_child_exit_status: 0,
            child_exit_status: 0,
            current_font: 0,
        }
    }
}

fn cb_spawn_async(term: *mut VteTerminal, pid: Pid, err: *mut GError, data: Pointer) {}

fn cfg(s: *const c_char, n: *const c_char) {}

fn get_cursor_blink_mode() {}

fn get_cursor_shape() {}

fn get_keyval() -> u64 {
    0
}

fn handle_history(term: *mut VteTerminal) {}

fn ini_load(config_file: *mut c_char) {
    let mut p: *const c_char = std::ptr::null_mut();
    if config_file.is_null() {
        unsafe {
            let c_str = CStr::from_ptr(g_get_user_config_dir());
            let config_dir = c_str.to_str().unwrap();
            let mut file_path = PathBuf::from(config_dir);
            file_path.push(NAME);
            file_path.push("config.ini");
            println!("File path is {:?}", file_path);
            p = file_path.to_str().unwrap().as_ptr() as *mut c_char;
        }
    } else {
        unsafe {
            p = g_strup(config_file);
        }
    }

    let mut ini = unsafe { g_key_file_new() };
    unsafe {
        if g_key_file_load_from_file(ini, p, G_KEY_FILE_NONE, std::ptr::null_mut()) <= 0 {
            if !config_file.is_null() || g_file_test(p, G_FILE_TEST_EXISTS) > 0 {
                eprintln!(":Config could not be loaded");
                g_free(p as *mut c_void);
                return;
            }
        }
        g_free(p as *mut c_void);
    }

    let mut err: *mut GError = std::ptr::null_mut();
    let mut Config = CONFIG.lock().unwrap();
    for mut conf in (*Config).iter_mut() {
        // println!("{:?}", config);
        match conf.t {
            ConfigItemType::String => unsafe {
                p = g_key_file_get_string(
                    ini,
                    conf.s.as_ptr() as *const i8,
                    conf.n.as_ptr() as *const i8,
                    &mut err,
                );
                if !p.is_null() {
                    if p == "NULL".as_ptr() as *const i8 {
                        conf.v = ConfigValue::S("");
                        g_free(p as *mut c_void);
                    } else {
                        let c_str = CStr::from_ptr(p);
                        conf.v = ConfigValue::S(c_str.to_str().expect("convert fail"));
                    }
                }
            },
            ConfigItemType::StringList => {}
            ConfigItemType::Boolean => {}
            ConfigItemType::Int64 => {}
            ConfigItemType::Uint64 => {}
        }
    }
}

fn safe_emsg(_: *mut GError) {}

fn sig_bell(_: *mut VteTerminal, _: Pointer) {}

fn sig_button_press(_: *mut GtkWidget, _: *mut gdk::Event, _: Pointer) {}

fn sig_child_exited(_: *mut VteTerminal, _: i64, _: Pointer) {}

fn sig_hyperlink_changed(
    _: *mut VteTerminal,
    _: *const c_char,
    _: *mut gdk::Rectangle,
    _: Pointer,
) {
}

fn sig_key_press(_: *mut GtkWidget, _: *mut gdk::Event, _: Pointer) -> gboolean {
    0
}

fn sig_window_destroy(_: *mut GtkWidget, _: Pointer) {}

fn sig_window_resize(_: *mut VteTerminal, _: u64, _: u64, _: Pointer) {}

fn sig_window_title_changed(_: *mut VteTerminal, _: Pointer) {}

fn term_new(t: *mut Terminal) {
    let mut title: &str = "jterm2";
    let mut res_class: &str = "Jterm2";
    let mut res_name: &str = "jterm2";
    let c_foreground_gdk = GdkRGBA {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 0.0,
    };
    let c_background_gdk = GdkRGBA {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 0.0,
    };
    let c_gdk = GdkRGBA {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 0.0,
    };
    let url_vregex: *mut VteRegex = std::ptr::null_mut();
    let err: *mut GError = std::ptr::null_mut();
    let spawn_flags: GSpawnFlags = 0;
    let standard16order: Vec<&str> = vec![
        "dark_black",
        "dark_red",
        "dark_green",
        "dark_yellow",
        "dark_blue",
        "dark_magenta",
        "dark_cyan",
        "dark_white",
        "bright_black",
        "bright_red",
        "bright_green",
        "bright_yellow",
        "bright_blue",
        "bright_magenta",
        "bright_cyan",
        "bright_white",
    ];

    // Handle arguments.
    unsafe {
        (*t).current_font = 0;
    }
    let args: Vec<String> = env::args().collect();
    println!(
        "Number of arguments (excluding program name): {}",
        args.len() - 1
    );
    let mut config_file: *mut c_char = std::ptr::null_mut();
    let mut iter = args.iter().enumerate().skip(1);
    while let Some((_, arg)) = iter.next() {
        println!("{}", arg);
        if arg == "-class" {
            if let Some((_, res_class)) = iter.next() {
                println!("set res_class: {}", res_class);
            }
        } else if arg == "-hold" {
            unsafe {
                (*t).hold = 1;
            }
        } else if arg == "-name" {
            if let Some((_, res_name)) = iter.next() {
                println!("set res_name: {}", res_name);
            }
        } else if arg == "-title" {
            if let Some((_, title)) = iter.next() {
                println!("set title: {}", title);
            }
        } else if arg == "--config" {
            if let Some((_, config_file)) = iter.next() {
                println!("set config_file: {}", config_file);
            }
        } else if arg == "--fontindex" {
            if let Some((_, current_font)) = iter.next() {
                if let Ok(current_font) = current_font.parse::<usize>() {
                    unsafe {
                        println!("current_font: {}", current_font);
                        (*t).current_font = current_font;
                    }
                }
            }
        } else if arg == "-e" {
            if let Some((_, argv_cmdline)) = iter.next() {
                break;
            }
        } else {
            eprintln!("invalid arguments, check manpage");
        }
    }

    ini_load(config_file);
}

fn term_activate_current_font(_: *mut Terminal, _: gboolean) {}

fn term_change_font_scale(_: *mut Terminal, _: i64) {}

fn term_set_size(_: *mut Terminal, _: u64, _: u64, _: gboolean) {}

fn main() {
    let mut t = Terminal::new();
    if gtk::init().is_err() {
        eprintln!("Failed to initialize GTK.");
        return;
    }
    term_new(&mut t);
}
