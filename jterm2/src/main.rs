extern crate gdk;
extern crate gtk;
extern crate vte;
use glib::{ffi::gboolean, *};
use gtk::ffi::GtkWidget;
use std::env;
use std::ffi::c_char;
use std::ffi::CStr;
use std::ffi::CString;
use vte_sys::VteTerminal;

const PCRE2_CODE_UNIT_WIDTH: i8 = 8;

enum ConfigItemType {
    STRING,
    STRINGLIST,
    BOOLEAN,
    INT64,
    UINT64,
}

// With tagged variants.
union ConfigUnion {
    s: *const c_char,
    sl: *const *const c_char,
    b: gboolean,
    i: i64,
    ui: u64,
}

struct ConfigItem {
    s: *const c_char,
    n: *const c_char,
    t: ConfigItemType,
    l: u64,
    v: ConfigUnion,
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

fn cb_spawn_async(term: *mut VteTerminal, pid: Pid, err: *mut Error, data: Pointer) {}

fn cfg(s: *const c_char, n: *const c_char) {}

fn get_cursor_blink_mode() {}

fn get_cursor_shape() {}

fn get_keyval() -> u64 {
    0
}

fn handle_history(term: *mut VteTerminal) {}

fn ini_load() {}

fn safe_emsg(_: *mut Error) {}

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

fn term_new(_: *mut Terminal) {
    let args: Vec<String> = env::args().collect();
    println!(
        "Number of arguments (excluding program name): {}",
        args.len() - 1
    );
    for (index, argument) in args.iter().enumerate().skip(1) {
        println!("argument {}: {}", index, argument);
    }
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
    println!("evil");
}
