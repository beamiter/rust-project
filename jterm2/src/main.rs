extern crate vte;
extern crate gtk;
use glib::{*, ffi::gboolean};
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
    b (gboolean),
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

fn cb_spawn_async() {
    
}

fn main() {
    println!("evil");
}
