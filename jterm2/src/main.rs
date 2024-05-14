use gdk_sys::gdk_rgba_parse;
use gdk_sys::GdkEvent;
use gdk_sys::GdkRGBA;
use gdk_sys::GdkRectangle;
use glib_sys::g_file_test;
use glib_sys::g_get_user_config_dir;
use glib_sys::g_key_file_get_boolean;
use glib_sys::g_key_file_get_int64;
use glib_sys::g_key_file_get_string;
use glib_sys::g_key_file_get_string_list;
use glib_sys::g_key_file_get_uint64;
use glib_sys::g_key_file_load_from_file;
use glib_sys::g_key_file_new;
use glib_sys::g_set_prgname;
use glib_sys::g_strup;
use glib_sys::gboolean;
use glib_sys::gpointer;
use glib_sys::GError;
use glib_sys::GPid;
use glib_sys::GSpawnFlags;
use glib_sys::GTRUE;
use glib_sys::G_FILE_TEST_EXISTS;
use glib_sys::G_KEY_FILE_NONE;
use gobject_sys::g_signal_connect_data;
use gobject_sys::GCallback;
use gobject_sys::GObject;
use gtk_sys::gtk_container_add;
use gtk_sys::gtk_init;
use gtk_sys::gtk_widget_get_preferred_size;
use gtk_sys::gtk_widget_show_all;
use gtk_sys::gtk_window_new;
use gtk_sys::gtk_window_resize;
use gtk_sys::gtk_window_set_title;
use gtk_sys::GtkContainer;
use gtk_sys::GtkRequisition;
use gtk_sys::GtkWidget;
use gtk_sys::GtkWindow;
use gtk_sys::GTK_WINDOW_TOPLEVEL;
use pango_sys::pango_font_description_free;
use pango_sys::pango_font_description_from_string;
use pango_sys::PangoFontDescription;
use std::env;
use std::ffi::c_char;
use std::ffi::c_void;
use std::ffi::CStr;
use std::ffi::CString;
use std::i8;
use std::path::PathBuf;
use std::slice;
use vte_sys::vte_terminal_get_column_count;
use vte_sys::vte_terminal_get_row_count;
use vte_sys::vte_terminal_new;
use vte_sys::vte_terminal_set_allow_hyperlink;
use vte_sys::vte_terminal_set_bold_is_bright;
use vte_sys::vte_terminal_set_colors;
use vte_sys::vte_terminal_set_cursor_blink_mode;
use vte_sys::vte_terminal_set_cursor_shape;
use vte_sys::vte_terminal_set_font;
use vte_sys::vte_terminal_set_font_scale;
use vte_sys::vte_terminal_set_mouse_autohide;
use vte_sys::vte_terminal_set_scrollback_lines;
use vte_sys::vte_terminal_set_size;
use vte_sys::VteCursorBlinkMode;
use vte_sys::VteCursorShape;
use vte_sys::VteRegex;
use vte_sys::VteTerminal;
use vte_sys::VTE_CURSOR_BLINK_OFF;
use vte_sys::VTE_CURSOR_BLINK_ON;
use vte_sys::VTE_CURSOR_BLINK_SYSTEM;
use vte_sys::VTE_CURSOR_SHAPE_BLOCK;
use vte_sys::VTE_CURSOR_SHAPE_IBEAM;
use vte_sys::VTE_CURSOR_SHAPE_UNDERLINE;

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

fn cb_spawn_async(term: *mut VteTerminal, pid: GPid, err: *mut GError, data: gpointer) {}

fn cfg<'a>(s: &'a str, n: &'a str) -> Option<ConfigItem<'a>> {
    let config = CONFIG.lock().unwrap();
    for conf in (*config).iter() {
        if conf.s == s && conf.n == n {
            println!("s: {}, n: {}", s, n);
            return Some(conf.clone());
        }
    }
    return None;
}

fn get_cursor_blink_mode() -> VteCursorBlinkMode {
    if let ConfigValue::S(s) = cfg("Options", "cursor_blink_mode").unwrap().v {
        if s == "VTE_CURSOR_BLINK_SYSTEM" {
            return VTE_CURSOR_BLINK_SYSTEM;
        } else if s == "VTE_CURSOR_BLINK_OFF" {
            return VTE_CURSOR_BLINK_OFF;
        } else {
            return VTE_CURSOR_BLINK_ON;
        }
    }
    return VTE_CURSOR_BLINK_ON;
}

fn get_cursor_shape() -> VteCursorShape {
    if let ConfigValue::S(s) = cfg("Options", "cursor_shape").unwrap().v {
        if s == "VTE_CURSOR_SHAPE_IBEAM" {
            return VTE_CURSOR_SHAPE_IBEAM;
        } else if s == "VTE_CURSOR_SHAPE_UNDERLINE" {
            return VTE_CURSOR_SHAPE_UNDERLINE;
        } else {
            return VTE_CURSOR_SHAPE_BLOCK;
        }
    }
    return VTE_CURSOR_SHAPE_BLOCK;
}

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

    let mut err: *mut GError;
    let mut config = CONFIG.lock().unwrap();
    let mut c_string_s: CString;
    let mut c_string_n: CString;
    for conf in (*config).iter_mut() {
        // Free any existing error before attemping to reuse the GError* variable.
        err = std::ptr::null_mut();
        // println!("{:?}", config);
        c_string_s = CString::new(conf.s).expect("failed to convert");
        c_string_n = CString::new(conf.n).expect("failed to convert");
        match conf.t {
            ConfigItemType::String => unsafe {
                p = g_key_file_get_string(ini, c_string_s.as_ptr(), c_string_n.as_ptr(), &mut err);
                if !p.is_null() {
                    let c_string = CString::new("NULL").unwrap();
                    if p == c_string.as_ptr() {
                        conf.v = ConfigValue::S("");
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
                    c_string_s.as_ptr(),
                    c_string_n.as_ptr(),
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
                let ret =
                    g_key_file_get_boolean(ini, c_string_s.as_ptr(), c_string_n.as_ptr(), &mut err);
                if err.is_null() {
                    conf.v = ConfigValue::B(ret);
                }
            },
            ConfigItemType::Int64 => unsafe {
                let int64 =
                    g_key_file_get_int64(ini, c_string_s.as_ptr(), c_string_n.as_ptr(), &mut err);
                if err.is_null() {
                    conf.v = ConfigValue::I(int64);
                }
            },
            ConfigItemType::Uint64 => unsafe {
                let uint64 =
                    g_key_file_get_uint64(ini, c_string_s.as_ptr(), c_string_n.as_ptr(), &mut err);
                if err.is_null() {
                    conf.v = ConfigValue::Ui(uint64);
                }
            },
        }
    }
}

fn safe_emsg(_: *mut GError) {}

fn sig_bell(_: *mut VteTerminal, _: gpointer) {}

fn sig_button_press(_: *mut GtkWidget, _: *mut GdkEvent, _: gpointer) {}

fn sig_child_exited(_: *mut VteTerminal, _: i64, _: gpointer) {}

fn sig_hyperlink_changed(_: *mut VteTerminal, _: *const c_char, _: *mut GdkRectangle, _: gpointer) {
}

fn sig_key_press(_: *mut GtkWidget, _: *mut GdkEvent, _: gpointer) -> gboolean {
    0
}

fn sig_window_destroy(widget: *mut GtkWidget, data: gpointer) {}

fn sig_window_resize(_: *mut VteTerminal, _: u64, _: u64, _: gpointer) {}

fn sig_window_title_changed(_: *mut VteTerminal, _: gpointer) {}

fn term_new(t: *mut Terminal) {
    let mut title: &str = "jterm2";
    let mut res_class: &str = "Jterm2";
    let mut res_name: &str = "jterm2";
    let mut c_foreground_gdk: GdkRGBA = GdkRGBA {
        red: 0.,
        green: 0.,
        blue: 0.,
        alpha: 0.,
    };
    let mut c_background_gdk: GdkRGBA = GdkRGBA {
        red: 0.,
        green: 0.,
        blue: 0.,
        alpha: 0.,
    };
    let mut c_gdk: GdkRGBA = GdkRGBA {
        red: 0.,
        green: 0.,
        blue: 0.,
        alpha: 0.,
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
    let config_file: *mut c_char = std::ptr::null_mut();
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
            if let Some((_, _)) = iter.next() {
                break;
            }
        } else {
            eprintln!("invalid arguments, check manpage");
        }
    }

    ini_load(config_file);

    // Create GKT+ widges.
    unsafe {
        let mut c_string: CString;
        (*t).win = gtk_window_new(GTK_WINDOW_TOPLEVEL);
        // Cast *mut GtkWidget to *mut GtkWindow;
        let window_ptr = (*t).win as *mut GtkWindow;
        // Convert raw pointer to a safe wrapper
        // let window: gtk::Window = from_glib_none(window_ptr);
        c_string = CString::new(title).expect("failed to convert");
        gtk_window_set_title(window_ptr, c_string.as_ptr());
        let object_ptr = (*t).win as *mut GObject;
        let callback: GCallback = Some(std::mem::transmute(sig_window_destroy as *const ()));
        c_string = CString::new("destroy").expect("failed to convert");
        g_signal_connect_data(
            object_ptr,
            c_string.as_ptr(),
            callback,
            t as *mut c_void,
            None,
            0,
        );
        let app_id = format!("{}.{}", res_name, res_class);
        println!("{}", app_id);
        c_string = CString::new(app_id).expect("failed to convert");
        g_set_prgname(c_string.as_ptr());

        (*t).term = vte_terminal_new() as *mut GtkWidget;
        gtk_container_add((*t).win as *mut GtkContainer, (*t).term);

        // Appearance
        term_activate_current_font(t, 0);
        gtk_widget_show_all((*t).win);

        if let ConfigValue::B(b) = cfg("Options", "bold_is_bright").unwrap().v {
            vte_terminal_set_bold_is_bright((*t).term as *mut VteTerminal, b);
        }
        vte_terminal_set_cursor_blink_mode((*t).term as *mut VteTerminal, get_cursor_blink_mode());
        vte_terminal_set_cursor_shape((*t).term as *mut VteTerminal, get_cursor_shape());
        vte_terminal_set_mouse_autohide((*t).term as *mut VteTerminal, GTRUE);
        if let ConfigValue::I(i) = cfg("Options", "scrollback_lines").unwrap().v {
            vte_terminal_set_scrollback_lines((*t).term as *mut VteTerminal, i);
        }
        vte_terminal_set_allow_hyperlink((*t).term as *mut VteTerminal, GTRUE);

        // In Rust, to convert a &str (a string slice) into a *const c_char (a pointer to a C-style character array), you need to use the CString type provided by the standard library's std::ffi module. A CString is an owned, null-terminated string that can be used with FFI (Foreign Function Interface).
        let mut c_string: CString;
        if let ConfigValue::S(s) = cfg("Colors", "foreground").unwrap().v {
            c_string = CString::new(s).expect("failed");
            gdk_rgba_parse(&mut c_foreground_gdk, c_string.as_ptr());
            println!("{} foreground: {:?}", s, c_foreground_gdk);
        }
        if let ConfigValue::S(s) = cfg("Colors", "background").unwrap().v {
            c_string = CString::new(s).expect("failed");
            gdk_rgba_parse(&mut c_background_gdk, c_string.as_ptr());
            println!("{} background: {:?}", s, c_background_gdk);
        }
        let mut c_palette_gdk: [GdkRGBA; 16] = std::mem::zeroed();
        for i in 0..16 {
            if let ConfigValue::S(s) = cfg("Colors", standard16order[i]).unwrap().v {
                c_string = CString::new(s).expect("failed");
                gdk_rgba_parse(c_palette_gdk.as_mut_ptr().add(i), c_string.as_ptr());
            }
        }
        let c_foreground_gdk_ptr: *const GdkRGBA = &c_foreground_gdk;
        let c_background_gdk_ptr: *const GdkRGBA = &c_background_gdk;
        vte_terminal_set_colors(
            (*t).term as *mut VteTerminal,
            c_foreground_gdk_ptr,
            c_background_gdk_ptr,
            c_palette_gdk.as_ptr(),
            16,
        );
        println!("{:?}", c_palette_gdk);
    }
    println!("fuck haha");
}

fn term_activate_current_font(t: *mut Terminal, win_ready: gboolean) {
    let mut font_desc: *mut PangoFontDescription = std::ptr::null_mut();
    if let Some(item) = cfg("Options", "fonts") {
        unsafe {
            if (*t).current_font >= item.l.unwrap().try_into().unwrap() {
                eprintln!(": Warning: Invalid font index");
                return;
            }
            if let ConfigValue::Sl(sl) = item.v {
                let cur_font = sl[(*t).current_font];
                println!("cur font: {}", cur_font);
                let c_string = CString::new(cur_font).unwrap();
                font_desc = pango_font_description_from_string(c_string.as_ptr());
            }
        }
    }
    unsafe {
        let width = vte_terminal_get_column_count((*t).term as *mut VteTerminal);
        let height = vte_terminal_get_row_count((*t).term as *mut VteTerminal);
        println!("width: {}, height: {}", width, height);

        vte_terminal_set_font((*t).term as *mut VteTerminal, font_desc as *const _);
        pango_font_description_free(font_desc);
        vte_terminal_set_font_scale((*t).term as *mut VteTerminal, 1.0);

        term_set_size(t, width, height, win_ready);
    }
}

fn term_change_font_scale(_: *mut Terminal, _: i64) {}

fn term_set_size(t: *mut Terminal, width: i64, height: i64, win_ready: gboolean) {
    unsafe {
        if width > 0 && height > 0 {
            vte_terminal_set_size((*t).term as *mut VteTerminal, width, height);
        }
        if win_ready > 0 {
            let natural: *mut GtkRequisition = std::ptr::null_mut();
            gtk_widget_get_preferred_size((*t).term, std::ptr::null_mut(), natural);
            println!("natural: {:?}", *natural);
            gtk_window_resize(
                (*t).win as *mut GtkWindow,
                (*natural).width,
                (*natural).height,
            );
        }
    }
}

fn main() {
    let mut t = Terminal::new();
    unsafe {
        gtk_init(std::ptr::null_mut(), std::ptr::null_mut());
    }
    term_new(&mut t);
}
