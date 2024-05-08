extern crate gdk;
extern crate gtk;
extern crate vte;
use gdk::ffi::GdkRGBA;
use gdk::glib::ffi::GSpawnFlags;
use gdk::glib::gobject_ffi::g_signal_connect_data;
use gdk::glib::gobject_ffi::GCallback;
use gdk::glib::gobject_ffi::GObject;
use glib::*;
use glib_sys::*;
use gtk::ffi::gtk_window_new;
use gtk::ffi::gtk_window_set_title;
use gtk::ffi::GtkWidget;
use gtk::ffi::GtkWindow;
use gtk::ffi::GTK_WINDOW_TOPLEVEL;
use std::env;
use std::ffi::c_char;
use std::ffi::c_void;
use std::ffi::CStr;
use std::i8;
use std::path::PathBuf;
use std::slice;
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
    Sl(Vec<&'a str>),
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

fn cb_spawn_async(term: *mut VteTerminal, pid: Pid, err: *mut GError, data: gpointer) {}

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
                // g_free(p as *mut c_void);
                return;
            }
        }
        // g_free(p as *mut c_void);
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
                        // g_free(p as *mut c_void);
                    } else {
                        let c_str = CStr::from_ptr(p);
                        conf.v = ConfigValue::S(c_str.to_str().expect("convert fail"));
                    }
                }
            },
            ConfigItemType::StringList => unsafe {
                let mut len: usize = 0;
                let lst = g_key_file_get_string_list(
                    ini,
                    conf.s.as_ptr() as *const i8,
                    conf.n.as_ptr() as *const i8,
                    &mut len,
                    &mut err,
                );
                if !lst.is_null() {
                    let slice = slice::from_raw_parts(lst, len);
                    let mut str_vec: Vec<&str> = Vec::new();
                    for &c_str in slice {
                        let tmp = CStr::from_ptr(c_str).to_str().unwrap();
                        str_vec.push(tmp);
                    }
                    conf.v = ConfigValue::Sl(str_vec);
                    conf.l = Some(len.try_into().unwrap());
                }
            },
            ConfigItemType::Boolean => unsafe {
                let ret = g_key_file_get_boolean(
                    ini,
                    conf.s.as_ptr() as *const i8,
                    conf.n.as_ptr() as *const i8,
                    &mut err,
                );
                if err.is_null() {
                    conf.v = ConfigValue::B(ret);
                }
            },
            ConfigItemType::Int64 => unsafe {
                let int64 = g_key_file_get_int64(
                    ini,
                    conf.s.as_ptr() as *const i8,
                    conf.n.as_ptr() as *const i8,
                    &mut err,
                );
                if err.is_null() {
                    conf.v = ConfigValue::I(int64);
                }
            },
            ConfigItemType::Uint64 => unsafe {
                let uint64 = g_key_file_get_uint64(
                    ini,
                    conf.s.as_ptr() as *const i8,
                    conf.n.as_ptr() as *const i8,
                    &mut err,
                );
                if err.is_null() {
                    conf.v = ConfigValue::Ui(uint64);
                }
            },
        }
    }
}

fn safe_emsg(_: *mut GError) {}

fn sig_bell(_: *mut VteTerminal, _: gpointer) {}

fn sig_button_press(_: *mut GtkWidget, _: *mut gdk::Event, _: gpointer) {}

fn sig_child_exited(_: *mut VteTerminal, _: i64, _: gpointer) {}

fn sig_hyperlink_changed(
    _: *mut VteTerminal,
    _: *const c_char,
    _: *mut gdk::Rectangle,
    _: gpointer,
) {
}

fn sig_key_press(_: *mut GtkWidget, _: *mut gdk::Event, _: gpointer) -> gboolean {
    0
}

fn sig_window_destroy(widget: *mut GtkWidget, data: gpointer) {}

fn sig_window_resize(_: *mut VteTerminal, _: u64, _: u64, _: gpointer) {}

fn sig_window_title_changed(_: *mut VteTerminal, _: gpointer) {}

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

    // Create GKT+ widges.
    unsafe {
        (*t).win = gtk_window_new(GTK_WINDOW_TOPLEVEL);
        // Cast *mut GtkWidget to *mut GtkWindow;
        let window_ptr = (*t).win as *mut GtkWindow;
        // Convert raw pointer to a safe wrapper
        // let window: gtk::Window = from_glib_none(window_ptr);
        gtk_window_set_title(window_ptr, title.as_ptr() as *const i8);
        let object_ptr = (*t).win as *mut GObject;
        let signal: &str = "destroy";
        let callback: GCallback = Some(std::mem::transmute(sig_window_destroy as *const ()));
        g_signal_connect_data(
            object_ptr,
            signal.as_ptr() as *const i8,
            callback,
            t as *mut c_void,
            None,
            0,
        );
    }
    println!("fuck haha");
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
