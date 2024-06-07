use std::sync::Mutex;

use crate::ConfigItem;
use crate::ConfigItemType;
use crate::ConfigValue;

use once_cell::sync::Lazy;

pub static CONFIG: Lazy<Mutex<Vec<ConfigItem>>> = Lazy::new(|| {
    Mutex::new(vec![
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
            v: ConfigValue::Sl(vec!["JetBrainsMono Nerd Font Regular 12"]),
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
            n: "foreground",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#f8f7e9"),
        },
        ConfigItem {
            s: "Colors",
            n: "background",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#121616"),
        },
        ConfigItem {
            s: "Colors",
            n: "cursor",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#00FF00"),
        },
        ConfigItem {
            s: "Colors",
            n: "cursor_foreground",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#000000"),
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
            v: ConfigValue::S("#1a1818"),
        },
        ConfigItem {
            s: "Colors",
            n: "dark_red",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#c90e0e"),
        },
        ConfigItem {
            s: "Colors",
            n: "dark_green",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#0e800e"),
        },
        ConfigItem {
            s: "Colors",
            n: "dark_yellow",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#d99755"),
        },
        ConfigItem {
            s: "Colors",
            n: "dark_blue",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#6767d6"),
        },
        ConfigItem {
            s: "Colors",
            n: "dark_magenta",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#ad39ad"),
        },
        ConfigItem {
            s: "Colors",
            n: "dark_cyan",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#0abab4"),
        },
        ConfigItem {
            s: "Colors",
            n: "dark_white",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#aaaaaa"),
        },
        ConfigItem {
            s: "Colors",
            n: "bright_black",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#555555"),
        },
        ConfigItem {
            s: "Colors",
            n: "bright_red",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#ff5555"),
        },
        ConfigItem {
            s: "Colors",
            n: "bright_green",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#55ff55"),
        },
        ConfigItem {
            s: "Colors",
            n: "bright_yellow",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#ffff55"),
        },
        ConfigItem {
            s: "Colors",
            n: "bright_blue",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#8787ed"),
        },
        ConfigItem {
            s: "Colors",
            n: "bright_magenta",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#ff55ff"),
        },
        ConfigItem {
            s: "Colors",
            n: "bright_cyan",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#55ffff"),
        },
        ConfigItem {
            s: "Colors",
            n: "bright_white",
            t: ConfigItemType::String,
            l: None,
            v: ConfigValue::S("#ffffff"),
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
    ])
});

#[allow(unused_macros)]
macro_rules! wifexited {
    ($status:expr) => {
        (($status) & 0x7f) == 0
    };
}
