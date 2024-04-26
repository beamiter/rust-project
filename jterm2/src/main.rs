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
use std::ffi::CString;
use std::os::raw::c_void;
use std::path::PathBuf;
use vte_sys::VteRegex;
use vte_sys::VteTerminal;

const PCRE2_CODE_UNIT_WIDTH: i8 = 8;
const NAME: &str = "jterm2";

#[derive(Debug, Clone, Copy)]
enum ConfigItemType {
    String,
    StringList,
    Boolean,
    Int64,
    Uint64,
}

// With tagged variants.
#[derive(Debug, Clone)]
enum ConfigValue<'a> {
    S(&'a str),
    Sl(&'a [&'a str]),
    B(gboolean),
    I(i64),
    Ui(u64),
}

#[derive(Debug, Clone)]
struct ConfigItem<'a> {
    s: &'a str,
    n: &'a str,
    t: ConfigItemType,
    // Only used for StringList
    l: Option<u64>,
    v: ConfigValue<'a>,
}

const config: &[ConfigItem] = &[
    ConfigItem {
        s: "Options",
        n: "login_shell",
        t: ConfigItemType::Boolean,
        l: None,
        v: ConfigValue::B(1),
    },
    ConfigItem {
        s: "Options",
        n: "bold_is_bright",
        t: ConfigItemType::Boolean,
        l: None,
        v: ConfigValue::B(0),
    },
    ConfigItem {
        s: "Options",
        n: "fonts",
        t: ConfigItemType::StringList,
        l: Some(1),
        v: ConfigValue::Sl(&["Monospace 9"]),
    },
    ConfigItem {
        s: "Options",
        n: "scrollback_lines",
        t: ConfigItemType::Int64,
        l: None,
        v: ConfigValue::I(5000),
    },
    ConfigItem {
        s: "Options",
        n: "link_regex",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("[a-z]+://[[:graph:]]+"),
    },
    ConfigItem {
        s: "Options",
        n: "link_handler",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("jterm2-link-handler"),
    },
    ConfigItem {
        s: "Options",
        n: "history_handler",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("jterm2-history-handler"),
    },
    ConfigItem {
        s: "Options",
        n: "cursor_blink_mode",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("VTE_CURSOR_BLINK_OFF"),
    },
    ConfigItem {
        s: "Options",
        n: "cursor_shape",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("VTE_CURSOR_SHAPE_BLOCK"),
    },
    ConfigItem {
        s: "Colors",
        n: "forground",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("$AAAAAA"),
    },
    ConfigItem {
        s: "Colors",
        n: "background",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("$000000"),
    },
    ConfigItem {
        s: "Colors",
        n: "cursor",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("$00FF00"),
    },
    ConfigItem {
        s: "Colors",
        n: "cursor_foreground",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("$000000"),
    },
    ConfigItem {
        s: "Colors",
        n: "bold",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S(""), // Assuming NULL equivalent to empty string
    },
    ConfigItem {
        s: "Colors",
        n: "dark_black",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("000000"),
    },
    ConfigItem {
        s: "Colors",
        n: "dark_red",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("AA0000"),
    },
    ConfigItem {
        s: "Colors",
        n: "dark_green",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("00AA00"),
    },
    ConfigItem {
        s: "Colors",
        n: "dark_yellow",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("AA5500"),
    },
    ConfigItem {
        s: "Colors",
        n: "dark_blue",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("0000AA"),
    },
    ConfigItem {
        s: "Colors",
        n: "dark_magenta",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("AA00AA"),
    },
    ConfigItem {
        s: "Colors",
        n: "dark_cyan",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("00AAAA"),
    },
    ConfigItem {
        s: "Colors",
        n: "dark_white",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("AAAAAA"),
    },
    ConfigItem {
        s: "Colors",
        n: "bright_black",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("555555"),
    },
    ConfigItem {
        s: "Colors",
        n: "bright_red",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("FF5555"),
    },
    ConfigItem {
        s: "Colors",
        n: "bright_green",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("55FF55"),
    },
    ConfigItem {
        s: "Colors",
        n: "bright_yellow",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("FFFF55"),
    },
    ConfigItem {
        s: "Colors",
        n: "bright_blue",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("5555FF"),
    },
    ConfigItem {
        s: "Colors",
        n: "bright_magenta",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("FF55FF"),
    },
    ConfigItem {
        s: "Colors",
        n: "bright_cyan",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("55FFFF"),
    },
    ConfigItem {
        s: "Colors",
        n: "bright_white",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("FFFFFF"),
    },
    ConfigItem {
        s: "Controls",
        n: "button_link",
        t: ConfigItemType::Uint64,
        l: None,
        v: ConfigValue::Ui(3),
    },
    ConfigItem {
        s: "Controls",
        n: "key_copy_to_clipboard",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("C"),
    },
    ConfigItem {
        s: "Controls",
        n: "key_paste_from_clipboard",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("V"),
    },
    ConfigItem {
        s: "Controls",
        n: "key_handle_history",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("H"),
    },
    ConfigItem {
        s: "Controls",
        n: "key_next_font",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("N"),
    },
    ConfigItem {
        s: "Controls",
        n: "key_previous_font",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("P"),
    },
    ConfigItem {
        s: "Controls",
        n: "key_zoom_in",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("I"),
    },
    ConfigItem {
        s: "Controls",
        n: "key_zoom_out",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("O"),
    },
    ConfigItem {
        s: "Controls",
        n: "key_zoom_reset",
        t: ConfigItemType::String,
        l: None,
        v: ConfigValue::S("R"),
    },
];

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
    let mut p: *mut c_char = std::ptr::null_mut();
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
