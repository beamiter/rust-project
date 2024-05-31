use gdk_sys::gdk_keyval_from_name;
use gdk_sys::gdk_rgba_parse;
use gdk_sys::GdkEvent;
use gdk_sys::GdkEventKey;
use gdk_sys::GdkRGBA;
use gdk_sys::GdkRectangle;
use gdk_sys::GDK_BUTTON_PRESS;
use gdk_sys::GDK_CONTROL_MASK;
use gio_sys::g_file_get_path;
use gio_sys::g_file_new_tmp;
use gio_sys::g_io_stream_close;
use gio_sys::g_io_stream_get_output_stream;
use gio_sys::GFile;
use gio_sys::GFileIOStream;
use gio_sys::GIOStream;
use gio_sys::GOutputStream;
use glib_sys::g_clear_error;
use glib_sys::g_file_test;
use glib_sys::g_free;
use glib_sys::g_get_user_config_dir;
use glib_sys::g_key_file_get_boolean;
use glib_sys::g_key_file_get_int64;
use glib_sys::g_key_file_get_string;
use glib_sys::g_key_file_get_string_list;
use glib_sys::g_key_file_get_uint64;
use glib_sys::g_key_file_load_from_file;
use glib_sys::g_key_file_new;
use glib_sys::g_set_prgname;
use glib_sys::g_spawn_async;
use glib_sys::g_strup;
use glib_sys::gboolean;
use glib_sys::gpointer;
use glib_sys::GError;
use glib_sys::GPid;
use glib_sys::GSpawnFlags;
use glib_sys::GFALSE;
use glib_sys::GTRUE;
use glib_sys::G_FILE_TEST_EXISTS;
use glib_sys::G_KEY_FILE_NONE;
use glib_sys::G_SPAWN_DEFAULT;
use glib_sys::G_SPAWN_FILE_AND_ARGV_ZERO;
use glib_sys::G_SPAWN_SEARCH_PATH;
use gobject_sys::g_object_unref;
use gobject_sys::g_signal_connect_data;
use gobject_sys::GCallback;
use gobject_sys::GObject;
use gtk_sys::gtk_container_add;
use gtk_sys::gtk_init;
use gtk_sys::gtk_main;
use gtk_sys::gtk_widget_destroy;
use gtk_sys::gtk_widget_get_preferred_size;
use gtk_sys::gtk_widget_set_has_tooltip;
use gtk_sys::gtk_widget_set_tooltip_text;
use gtk_sys::gtk_widget_show_all;
use gtk_sys::gtk_window_new;
use gtk_sys::gtk_window_resize;
use gtk_sys::gtk_window_set_title;
use gtk_sys::gtk_window_set_urgency_hint;
use gtk_sys::GtkContainer;
use gtk_sys::GtkRequisition;
use gtk_sys::GtkWidget;
use gtk_sys::GtkWindow;
use gtk_sys::GTK_WINDOW_TOPLEVEL;
use pango_sys::pango_font_description_free;
use pango_sys::pango_font_description_from_string;
use pango_sys::PangoFontDescription;
use pcre2_sys::PCRE2_CASELESS;
use pcre2_sys::PCRE2_MULTILINE;
use std::env;
use std::ffi::c_char;
use std::ffi::c_void;
use std::ffi::CStr;
use std::ffi::CString;
use std::i8;
use std::path::PathBuf;
use std::slice;
use vte_sys::vte_get_user_shell;
use vte_sys::vte_regex_new_for_match;
use vte_sys::vte_regex_unref;
use vte_sys::vte_terminal_copy_clipboard_format;
use vte_sys::vte_terminal_get_column_count;
use vte_sys::vte_terminal_get_row_count;
use vte_sys::vte_terminal_hyperlink_check_event;
use vte_sys::vte_terminal_match_add_regex;
use vte_sys::vte_terminal_match_check_event;
use vte_sys::vte_terminal_new;
use vte_sys::vte_terminal_paste_clipboard;
use vte_sys::vte_terminal_set_allow_hyperlink;
use vte_sys::vte_terminal_set_bold_is_bright;
use vte_sys::vte_terminal_set_color_bold;
use vte_sys::vte_terminal_set_color_cursor;
use vte_sys::vte_terminal_set_color_cursor_foreground;
use vte_sys::vte_terminal_set_colors;
use vte_sys::vte_terminal_set_cursor_blink_mode;
use vte_sys::vte_terminal_set_cursor_shape;
use vte_sys::vte_terminal_set_font;
use vte_sys::vte_terminal_set_font_scale;
use vte_sys::vte_terminal_set_mouse_autohide;
use vte_sys::vte_terminal_set_scrollback_lines;
use vte_sys::vte_terminal_set_size;
use vte_sys::vte_terminal_spawn_async;
use vte_sys::vte_terminal_write_contents_sync;
use vte_sys::VteCursorBlinkMode;
use vte_sys::VteCursorShape;
use vte_sys::VteRegex;
use vte_sys::VteTerminal;
use vte_sys::VteTerminalSpawnAsyncCallback;
use vte_sys::VTE_CURSOR_BLINK_OFF;
use vte_sys::VTE_CURSOR_BLINK_ON;
use vte_sys::VTE_CURSOR_BLINK_SYSTEM;
use vte_sys::VTE_CURSOR_SHAPE_BLOCK;
use vte_sys::VTE_CURSOR_SHAPE_IBEAM;
use vte_sys::VTE_CURSOR_SHAPE_UNDERLINE;
use vte_sys::VTE_FORMAT_TEXT;
use vte_sys::VTE_PTY_DEFAULT;
use vte_sys::VTE_WRITE_DEFAULT;

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

fn get_keyval(name: &str) -> u32 {
    if let ConfigValue::S(cfg_s) = cfg("Controls", name).unwrap().v {
        unsafe {
            let c_str = CString::new(cfg_s).expect("failed to convert");
            return gdk_keyval_from_name(c_str.as_ptr());
        }
    }
    0
}

fn handle_history(term: *mut VteTerminal) {
    let tmpfile: *mut GFile;
    let io_stream: *mut GFileIOStream = std::ptr::null_mut();
    let out_stream: *mut GOutputStream;
    let err: *mut GError = std::ptr::null_mut();
    let mut argv: Vec<&str> = vec![""; 3];
    let spawn_flags = G_SPAWN_DEFAULT | G_SPAWN_SEARCH_PATH;

    if let ConfigValue::S(s) = cfg("Options", "history_handler").unwrap().v {
        argv[0] = s;
    }

    unsafe {
        tmpfile = g_file_new_tmp(std::ptr::null(), io_stream as *mut _, err as *mut _);
        if tmpfile.is_null() {
            eprintln!("Could not write history: {}", safe_emsg(err));

            if !argv[1].is_empty() {
                let c_string = CString::new(argv[1]).expect("failed to convert");
                g_free(c_string.into_raw() as *mut c_void);
            }
            if !io_stream.is_null() {
                g_object_unref(io_stream as *mut GObject);
            }
            if !tmpfile.is_null() {
                g_object_unref(tmpfile as *mut GObject);
            }
            g_clear_error(err as *mut _);
        }

        out_stream = g_io_stream_get_output_stream(io_stream as *mut GIOStream);
        if vte_terminal_write_contents_sync(
            term,
            out_stream,
            VTE_WRITE_DEFAULT,
            std::ptr::null_mut(),
            err as *mut _,
        ) <= 0
        {
            eprintln!("Could not write history: {}", safe_emsg(err));
            if !argv[1].is_empty() {
                let c_string = CString::new(argv[1]).expect("failed to convert");
                g_free(c_string.into_raw() as *mut c_void);
            }
            if !io_stream.is_null() {
                g_object_unref(io_stream as *mut GObject);
            }
            if !tmpfile.is_null() {
                g_object_unref(tmpfile as *mut GObject);
            }
            g_clear_error(err as *mut _);
        }

        if g_io_stream_close(
            io_stream as *mut GIOStream,
            std::ptr::null_mut(),
            err as *mut _,
        ) <= 0
        {
            eprintln!("Could not write history: {}", safe_emsg(err));
            if !argv[1].is_empty() {
                let c_string = CString::new(argv[1]).expect("failed to convert");
                g_free(c_string.into_raw() as *mut c_void);
            }
            if !io_stream.is_null() {
                g_object_unref(io_stream as *mut GObject);
            }
            if !tmpfile.is_null() {
                g_object_unref(tmpfile as *mut GObject);
            }
            g_clear_error(err as *mut _);
        }

        let c_str = CStr::from_ptr(g_file_get_path(tmpfile));
        argv[1] = c_str.to_str().unwrap();
        let argv_ptr = convert_vec_str_to_raw(argv.clone());
        if g_spawn_async(
            std::ptr::null(),
            argv_ptr,
            std::ptr::null_mut(),
            spawn_flags,
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            err as *mut _,
        ) <= 0
        {
            eprintln!("Could not launch history handler: {}", safe_emsg(err));
        }
    }

    //free_and_out.
    unsafe {
        if !argv[1].is_empty() {
            let c_string = CString::new(argv[1]).expect("failed to convert");
            g_free(c_string.into_raw() as *mut c_void);
        }
        if !io_stream.is_null() {
            g_object_unref(io_stream as *mut GObject);
        }
        if !tmpfile.is_null() {
            g_object_unref(tmpfile as *mut GObject);
        }
        g_clear_error(err as *mut _);
    }
}

fn ini_load(config_file: *mut c_char) {
    let mut p: *const c_char;
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

    let ini = unsafe { g_key_file_new() };
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

unsafe fn safe_emsg(err: *mut GError) -> &'static str {
    if err.is_null() {
        return "<GError is NULL>";
    } else {
        let c_str = CStr::from_ptr((*err).message);
        return c_str.to_str().expect("invalid utf-8");
    }
}

fn sig_bell(_: *mut VteTerminal, data: gpointer) {
    let t = data as *mut Terminal;
    unsafe {
        gtk_window_set_urgency_hint((*t).win as *mut GtkWindow, GFALSE);
        gtk_window_set_urgency_hint((*t).win as *mut GtkWindow, GTRUE);
    }
}

fn sig_button_press(widget: *mut GtkWidget, event: *mut GdkEvent, _: gpointer) -> gboolean {
    let mut url: *mut c_char;
    let mut argv: Vec<&str> = vec![""; 4];
    let mut err: *mut GError = std::ptr::null_mut();
    let mut retval: gboolean = GFALSE;
    let spawn_flags = G_SPAWN_DEFAULT | G_SPAWN_SEARCH_PATH;
    if let ConfigValue::S(s) = cfg("Options", "link_handler").unwrap().v {
        argv[0] = s;
    }
    unsafe {
        if (*event).type_ == GDK_BUTTON_PRESS {
            if let ConfigValue::Ui(ui) = cfg("Controls", "button_link").unwrap().v {
                if (*event).button.button == ui as u32 {
                    url = vte_terminal_hyperlink_check_event(widget as *mut VteTerminal, event);
                    if !url.is_null() {
                        argv[1] = "explicit";
                    } else {
                        url = vte_terminal_match_check_event(
                            widget as *mut VteTerminal,
                            event,
                            std::ptr::null_mut(),
                        );
                        if !url.is_null() {
                            argv[1] = "match";
                        }
                    }

                    if !url.is_null() {
                        let c_str = CStr::from_ptr(url);
                        argv[2] = c_str.to_str().unwrap();
                        // This is fantastic converter.
                        let argv_ptr = convert_vec_str_to_raw(argv);
                        if g_spawn_async(
                            std::ptr::null(),
                            argv_ptr,
                            std::ptr::null_mut(),
                            spawn_flags,
                            None,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            &mut err,
                        ) <= 0
                        {
                            eprint!(": Could not spawn link handler: {}", safe_emsg(err));
                        } else {
                            retval = GTRUE;
                        }
                    }
                }
            }
        }
    }
    return retval;
}

fn sig_child_exited(term: *mut VteTerminal, status: i64, data: gpointer) {
    let t = data as *mut Terminal;
    let mut c_background_gdk: GdkRGBA = GdkRGBA {
        red: 0.,
        green: 0.,
        blue: 0.,
        alpha: 0.,
    };

    unsafe {
        (*t).has_child_exit_status = GTRUE;
        (*t).child_exit_status = status;

        if (*t).hold >= 0 {
            if let ConfigValue::S(s) = cfg("Colors", "background").unwrap().v {
                let mut c_string = CString::new(s).expect("failed");
                gdk_rgba_parse(&mut c_background_gdk, c_string.as_ptr());
                vte_terminal_set_color_cursor(term, &c_background_gdk);
                c_string = CString::new("CHILD HAS QUIT").expect("failed");
                gtk_window_set_title((*t).win as *mut GtkWindow, c_string.as_ptr());
            }
        } else {
            gtk_widget_destroy((*t).win);
        }
    }
}

fn sig_hyperlink_changed(
    term: *mut VteTerminal,
    uri: *const c_char,
    _: *mut GdkRectangle,
    _: gpointer,
) {
    unsafe {
        if uri.is_null() {
            gtk_widget_set_has_tooltip(term as *mut GtkWidget, GFALSE);
        } else {
            gtk_widget_set_tooltip_text(term as *mut GtkWidget, uri);
        }
    }
}

fn sig_key_press(widget: *mut GtkWidget, event: *mut GdkEvent, data: gpointer) -> gboolean {
    let term: *mut VteTerminal = widget as *mut VteTerminal;
    let t: *mut Terminal = data as *mut Terminal;
    let kv: u32;
    let event_key = event as *mut GdkEventKey;
    unsafe {
        if (*event_key).state & GDK_CONTROL_MASK > 0 {
            kv = (*event_key).keyval;
            if kv == get_keyval("key_copy_to_clipboard") {
                vte_terminal_copy_clipboard_format(term, VTE_FORMAT_TEXT);
                return GTRUE;
            }
            if kv == get_keyval("key_paste_from_clipboard") {
                vte_terminal_paste_clipboard(term);
                return GTRUE;
            }
            if kv == get_keyval("key_handle_history") {
                handle_history(term);
                return GTRUE;
            }
            if kv == get_keyval("key_next_font") {
                (*t).current_font += 1;
                (*t).current_font %= cfg("Options", "fonts").unwrap().l.unwrap() as usize;
                term_activate_current_font(t, GTRUE);
                return GTRUE;
            }
            if kv == get_keyval("key_previous_font") {
                if (*t).current_font == 0 {
                    (*t).current_font = cfg("Options", "fonts").unwrap().l.unwrap() as usize - 1;
                } else {
                    (*t).current_font -= 1;
                }
                term_activate_current_font(t, GTRUE);
                return GTRUE;
            }
            if kv == get_keyval("key_zoom_in") {
                term_change_font_scale(t, 1);
                return GTRUE;
            }
            if kv == get_keyval("key_zoom_out") {
                term_change_font_scale(t, -1);
                return GTRUE;
            }
            if kv == get_keyval("key_zoom_reset") {
                term_change_font_scale(t, 0);
                return GTRUE;
            }
        }
    }
    return GFALSE;
}

fn sig_window_destroy(widget: *mut GtkWidget, data: gpointer) {}

fn sig_window_resize(_: *mut VteTerminal, _: u64, _: u64, _: gpointer) {}

fn sig_window_title_changed(_: *mut VteTerminal, _: gpointer) {}

fn convert_vec_str_to_raw(vec: Vec<&str>) -> *mut *mut c_char {
    let cstrings: Vec<CString> = vec
        .into_iter()
        .map(|s| CString::new(s).expect("CString::new failed"))
        .collect();
    let mut c_char_ptrs: Vec<*mut c_char> = cstrings
        .iter()
        .map(|cstring| cstring.as_ptr() as *mut c_char)
        .collect();
    // Add a null pointer at the end if needed for a null-terminated array.
    c_char_ptrs.push(std::ptr::null_mut());
    let ptr = c_char_ptrs.as_mut_ptr();
    ptr
}

fn term_new(t: *mut Terminal) {
    let title: &str = "jterm2";
    let res_class: &str = "Jterm2";
    let res_name: &str = "jterm2";
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
    let url_vregex: *mut VteRegex;
    let mut err: *mut GError = std::ptr::null_mut();
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
    let mut argv_cmdline: &str = "";
    let args_use: Vec<&str>;
    let spawn_flags: GSpawnFlags;
    let mut args_default: Vec<String> = vec![String::from(""); 3];
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
            if let Some((_, argv_cmdline0)) = iter.next() {
                argv_cmdline = argv_cmdline0;
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
        c_string = CString::new(title).expect("failed to convert");
        gtk_window_set_title((*t).win as *mut GtkWindow, c_string.as_ptr());
        let callback: GCallback = Some(std::mem::transmute(sig_window_destroy as *const ()));
        c_string = CString::new("destroy").expect("failed to convert");
        g_signal_connect_data(
            (*t).win as *mut GObject,
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
        println!("{:?}", c_palette_gdk);
        let c_foreground_gdk_ptr: *const GdkRGBA = &c_foreground_gdk;
        let c_background_gdk_ptr: *const GdkRGBA = &c_background_gdk;
        vte_terminal_set_colors(
            (*t).term as *mut VteTerminal,
            c_foreground_gdk_ptr,
            c_background_gdk_ptr,
            c_palette_gdk.as_ptr(),
            16,
        );

        if let ConfigValue::S(s) = cfg("Colors", "bold").unwrap().v {
            if !s.is_empty() {
                c_string = CString::new(s).expect("failed");
                gdk_rgba_parse(&mut c_gdk, c_string.as_ptr());
                vte_terminal_set_color_bold((*t).term as *mut VteTerminal, &c_gdk);
                println!("{} : {:?}", s, c_gdk);
            } else {
                vte_terminal_set_color_bold((*t).term as *mut VteTerminal, std::ptr::null_mut());
            }
        }
        if let ConfigValue::S(s) = cfg("Colors", "cursor").unwrap().v {
            if !s.is_empty() {
                c_string = CString::new(s).expect("failed");
                gdk_rgba_parse(&mut c_gdk, c_string.as_ptr());
                vte_terminal_set_color_cursor((*t).term as *mut VteTerminal, &c_gdk);
                println!("{} : {:?}", s, c_gdk);
            } else {
                vte_terminal_set_color_cursor((*t).term as *mut VteTerminal, std::ptr::null_mut());
            }
        }
        if let ConfigValue::S(s) = cfg("Colors", "cursor_foreground").unwrap().v {
            if !s.is_empty() {
                c_string = CString::new(s).expect("failed");
                gdk_rgba_parse(&mut c_gdk, c_string.as_ptr());
                vte_terminal_set_color_cursor_foreground((*t).term as *mut VteTerminal, &c_gdk);
                println!("{} : {:?}", s, c_gdk);
            } else {
                vte_terminal_set_color_cursor_foreground(
                    (*t).term as *mut VteTerminal,
                    std::ptr::null_mut(),
                );
            }
        }

        if let ConfigValue::S(link_regex) = cfg("Options", "link_regex").unwrap().v {
            c_string = CString::new(link_regex).expect("failed");
            url_vregex = vte_regex_new_for_match(
                c_string.as_ptr(),
                link_regex.len().try_into().unwrap(),
                PCRE2_MULTILINE | PCRE2_CASELESS,
                &mut err,
            );
            if url_vregex.is_null() {
                println!("link regex: {}", safe_emsg(err));
                g_clear_error(&mut err);
            } else {
                vte_terminal_match_add_regex((*t).term as *mut VteTerminal, url_vregex, 0);
                vte_regex_unref(url_vregex);
            }
        }

        // Signals.
        let callback: GCallback = Some(std::mem::transmute(sig_bell as *const ()));
        c_string = CString::new("bell").expect("failed to convert");
        g_signal_connect_data(
            (*t).term as *mut GObject,
            c_string.as_ptr(),
            callback,
            t as *mut c_void,
            None,
            0,
        );
        let callback: GCallback = Some(std::mem::transmute(sig_button_press as *const ()));
        c_string = CString::new("button-press-event").expect("failed to convert");
        g_signal_connect_data(
            (*t).term as *mut GObject,
            c_string.as_ptr(),
            callback,
            t as *mut c_void,
            None,
            0,
        );
        let callback: GCallback = Some(std::mem::transmute(sig_child_exited as *const ()));
        c_string = CString::new("child-exited").expect("failed to convert");
        g_signal_connect_data(
            (*t).term as *mut GObject,
            c_string.as_ptr(),
            callback,
            t as *mut c_void,
            None,
            0,
        );
        let callback: GCallback = Some(std::mem::transmute(sig_hyperlink_changed as *const ()));
        c_string = CString::new("hyperlink-hover-uri-changed").expect("failed to convert");
        g_signal_connect_data(
            (*t).term as *mut GObject,
            c_string.as_ptr(),
            callback,
            t as *mut c_void,
            None,
            0,
        );
        let callback: GCallback = Some(std::mem::transmute(sig_key_press as *const ()));
        c_string = CString::new("key-press-event").expect("failed to convert");
        g_signal_connect_data(
            (*t).term as *mut GObject,
            c_string.as_ptr(),
            callback,
            t as *mut c_void,
            None,
            0,
        );
        let callback: GCallback = Some(std::mem::transmute(sig_window_resize as *const ()));
        c_string = CString::new("resize-window").expect("failed to convert");
        g_signal_connect_data(
            (*t).term as *mut GObject,
            c_string.as_ptr(),
            callback,
            t as *mut c_void,
            None,
            0,
        );
        let callback: GCallback = Some(std::mem::transmute(sig_window_title_changed as *const ()));
        c_string = CString::new("window-title-changed").expect("failed to convert");
        g_signal_connect_data(
            (*t).term as *mut GObject,
            c_string.as_ptr(),
            callback,
            t as *mut c_void,
            None,
            0,
        );

        // Spawn child.
        if !argv_cmdline.is_empty() {
            args_use = vec![argv_cmdline];
            spawn_flags = G_SPAWN_SEARCH_PATH;
        } else {
            if args_default[0].is_empty() {
                let c_str = CStr::from_ptr(vte_get_user_shell());
                args_default[0] = String::from(c_str.to_str().unwrap());
                if args_default[0].is_empty() {
                    args_default[0] = String::from("/bin/sh");
                }
                if let ConfigValue::B(b) = cfg("Options", "login_shell").unwrap().v {
                    if b > 0 {
                        args_default[1] = format!("-{}", args_default[0]);
                    } else {
                        args_default[1] = args_default[0].to_string();
                    }
                }
            }
            args_use = args_default.iter().map(|s| s.as_str()).collect();
            spawn_flags = G_SPAWN_SEARCH_PATH | G_SPAWN_FILE_AND_ARGV_ZERO;
        }

        let callback: VteTerminalSpawnAsyncCallback =
            Some(std::mem::transmute(cb_spawn_async as *const ()));
        let args_use_ptr = convert_vec_str_to_raw(args_use);
        vte_terminal_spawn_async(
            (*t).term as *mut VteTerminal,
            VTE_PTY_DEFAULT,
            std::ptr::null(),
            args_use_ptr,
            std::ptr::null_mut(),
            spawn_flags,
            None,
            std::ptr::null_mut(),
            None,
            -1,
            std::ptr::null_mut(),
            callback,
            t as gpointer,
        );
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

// (TODO)
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
        term_new(&mut t);
        // gtk_main();
    }
}
