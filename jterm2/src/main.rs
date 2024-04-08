extern crate gdk;
extern crate gtk;
extern crate vte;
use glib::{ffi::gboolean, *};
use gtk::ffi::GtkWidget;
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
enum ConfigUnion {
    s(Vec<char>),
    sl(Vec<Vec<char>>),
    b(gboolean),
    i(i64),
    ui(u64),
}

struct ConfigItem {
    s: Vec<char>,
    n: Vec<char>,
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

fn cb_spawn_async(term: *mut VteTerminal, pid: Pid, err: *mut Error, data: Pointer) {}

fn cfg(s: Vec<char>, n: Vec<char>) {}

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

fn main() {
    println!("evil");
}
