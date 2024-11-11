#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
#![allow(unused_mut)]
#![allow(unused)]

use std::rc::Rc;

use once_cell::unsync::Lazy;
use x11::{
    keysym::{
        XK_Return, XK_Tab, XK_b, XK_c, XK_comma, XK_d, XK_e, XK_f, XK_h, XK_i, XK_j, XK_k, XK_l,
        XK_m, XK_o, XK_period, XK_q, XK_r, XK_space, XK_t, XK_0, XK_1, XK_2, XK_3, XK_4, XK_5,
        XK_6, XK_7, XK_8, XK_9,
    },
    xlib::{Button1, Button2, Button3, ControlMask, Mod1Mask, ShiftMask},
};

use crate::dwm::{self, monocle, tile, Button, Key, Layout, Rule, CLICK};

// border pixel of windows
pub const borderpx: u32 = 1;
// snap pixel
pub const snap: u32 = 32;
pub const showbar: bool = true;
pub const topbar: bool = true;
pub const vertpad: i32 = 8;
pub const sidepad: i32 = 8;
// horizontal padding for statusbar
pub const horizpadbar: i32 = 0;
// vertical padding for statusbar
pub const vertpadbar: i32 = 2;
// pub const fonts: Lazy<Vec<&str>> = Lazy::new(|| vec!["SauceCodeProNerdFontRegular:size=12"]);
// pub const font: &str = "Sans Bold 12";
// pub const dmenufont: &str = "Sans Bold 11";
pub const font: &str = "SauceCodeProNerdFontRegular:size=12";
pub const dmenufont: &str = "SauceCodeProNerdFontRegular:size=11";
pub const col_gray1: &str = "#222222";
pub const col_gray2: &str = "#444444";
pub const col_gray3: &str = "#bbbbbb";
pub const col_gray4: &str = "#eeeeee";
pub const col_cyan: &str = "#005577";
pub const col_black: &str = "#000000";
pub const col_red: &str = "#ff0000";
pub const col_yellow: &str = "#ffff00";
pub const col_white: &str = "#ffffff";
pub const TRANSPARENT: u8 = 0x00u8;
pub const OPAQUE: u8 = 0xffu8;
pub const HALF_OPAQUE: u8 = 0xa0u8;
pub const baralpha: u8 = 0xd0u8;
pub const borderalpha: u8 = OPAQUE;

pub const colors: [&[&'static str; 3]; 10] = [
    // fg | bg | border
    &[col_gray3, col_gray1, col_gray2],
    &[col_gray4, col_cyan, col_cyan],
    &["#cde6c7", "#224b8f", "#000000"], // Statubar right {text, background, not used but cannot be
    // empty}
    &["#ea66a6", "#94d6da", "#000000"], // Tagbar left selected {text, background, not used but cannot be
    // empty}
    &["#c85d44", "#7bbfea", "#000000"], // Tagbar left unselected {text, background, not used but cannot be
    // empty}
    &["#ffffff", "#9b95c9", "#000000"], // infobar middle selected {text, background, not used but cannot be
    // empty}
    &["#78cdd1", "#74787c", "#000000"], // infobar middle unselected {text, background, not used but cannot be
    // empty}
    &[col_gray3, col_gray1, col_gray2],
    &[col_black, col_yellow, col_red],
    &[col_white, col_red, col_red],
];
pub const alphas: [&[u8; 3]; 10] = [
    &[OPAQUE, baralpha, borderalpha],
    &[OPAQUE, baralpha, borderalpha],
    &[OPAQUE, TRANSPARENT, borderalpha],
    &[OPAQUE, baralpha, borderalpha],
    &[OPAQUE, baralpha, borderalpha],
    &[OPAQUE, baralpha, borderalpha],
    &[OPAQUE, baralpha, borderalpha],
    &[OPAQUE, baralpha, borderalpha],
    &[OPAQUE, baralpha, borderalpha],
    &[OPAQUE, baralpha, borderalpha],
];

// No need rules.
pub const rules: Lazy<Vec<Rule>> = Lazy::new(|| {
    vec![
        // class | instance | title | tags mask | isfloating | monitor
        // Rule::new("Gimp", "", "", 0, false, -1),
        // Rule::new("Firefox", "", "", 1 << 8, false, -1),
    ]
});
// https://symbl.cc/en/
pub const tags: [&str; 9] = ["🍇", "🍵", "🎦", "🎮", "🎵", "🏖", "🐣", "🐶", "🦄"];
// pub const tags: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
pub const tagmask: u32 = (1 << tags.len()) - 1;

pub const mfact: f32 = 0.55;
pub const nmaster: u32 = 1;
pub const resizehints: bool = true;
pub const lockfullscreen: bool = true;

pub const ulinepad: u32 = 5; // horizontal padding between the underline and tag
pub const ulinestroke: u32 = 2; // thickness /height of the unerline
pub const ulinevoffset: u32 = 0; // how far above the bottom of the bar the line should appear
pub const ulineall: bool = false; // true to show underline on all tags, false for just the acitve ones

pub const layouts: Lazy<Vec<Rc<Layout>>> = Lazy::new(|| {
    vec![
        Rc::new(Layout::new("[]=", Some(tile))),
        Rc::new(Layout::new("><>", None)),
        Rc::new(Layout::new("[M]", Some(monocle))),
    ]
});

fn TAGKEYS(KEY: u32, TAG: i32) -> Vec<Key> {
    vec![
        Key::new(MODKEY, KEY.into(), Some(dwm::view), dwm::Arg::Ui(1 << TAG)),
        Key::new(
            MODKEY | ControlMask,
            KEY.into(),
            Some(dwm::toggleview),
            dwm::Arg::Ui(1 << TAG),
        ),
        Key::new(
            MODKEY | ShiftMask,
            KEY.into(),
            Some(dwm::tag),
            dwm::Arg::Ui(1 << TAG),
        ),
        Key::new(
            MODKEY | ControlMask | ShiftMask,
            KEY.into(),
            Some(dwm::toggletag),
            dwm::Arg::Ui(1 << TAG),
        ),
    ]
}

pub const MODKEY: u32 = Mod1Mask;
pub static mut dmenumon: &'static str = "0";
macro_rules! enclose_in_quotes {
    ($val:expr) => {
        concat!("\"", $val, "\"")
    };
}
// dmenu_run -m 0 -fn "monospace:size=10" -nb "#222222" -nf "#bbbbbb" -sb "#005577" -sf "#eeeeee"
pub const dmenucmd: Lazy<Vec<&'static str>> = Lazy::new(|| {
    vec![
        "dmenu_run",
        "-m",
        unsafe { dmenumon },
        "-fn",
        dmenufont,
        "-nb",
        col_gray1,
        "-nf",
        col_gray3,
        "-sb",
        col_cyan,
        "-sf",
        col_gray4,
    ]
});
pub const termcmd: Lazy<Vec<&'static str>> = Lazy::new(|| vec!["gnome-terminal", ""]);
pub const keys: Lazy<Vec<Key>> = Lazy::new(|| {
    let mut m = vec![
        // modifier | key | function | argument
        Key::new(
            MODKEY,
            XK_e.into(),
            Some(dwm::spawn),
            dwm::Arg::V(dmenucmd.clone()),
        ),
        Key::new(
            MODKEY,
            XK_r.into(),
            Some(dwm::spawn),
            dwm::Arg::V(dmenucmd.clone()),
        ),
        Key::new(
            MODKEY | ShiftMask,
            XK_Return.into(),
            Some(dwm::spawn),
            dwm::Arg::V(termcmd.clone()),
        ),
        Key::new(MODKEY, XK_b.into(), Some(dwm::togglebar), dwm::Arg::I(0)),
        Key::new(MODKEY, XK_j.into(), Some(dwm::focusstack), dwm::Arg::I(1)),
        Key::new(MODKEY, XK_k.into(), Some(dwm::focusstack), dwm::Arg::I(-1)),
        Key::new(MODKEY, XK_i.into(), Some(dwm::incnmaster), dwm::Arg::I(1)),
        Key::new(MODKEY, XK_d.into(), Some(dwm::incnmaster), dwm::Arg::I(-1)),
        Key::new(
            MODKEY,
            XK_h.into(),
            Some(dwm::setmfact),
            dwm::Arg::F(-0.025),
        ),
        Key::new(
            MODKEY | ShiftMask,
            XK_h.into(),
            Some(dwm::setcfact),
            dwm::Arg::F(0.2),
        ),
        Key::new(
            MODKEY | ShiftMask,
            XK_l.into(),
            Some(dwm::setcfact),
            dwm::Arg::F(-0.2),
        ),
        Key::new(
            MODKEY | ShiftMask,
            XK_o.into(),
            Some(dwm::setcfact),
            dwm::Arg::F(0.0),
        ),
        Key::new(MODKEY, XK_l.into(), Some(dwm::setmfact), dwm::Arg::F(0.025)),
        Key::new(MODKEY, XK_Return.into(), Some(dwm::zoom), dwm::Arg::I(0)),
        Key::new(MODKEY, XK_Tab.into(), Some(dwm::view), dwm::Arg::Ui(0)),
        Key::new(
            MODKEY | ShiftMask,
            XK_c.into(),
            Some(dwm::killclient),
            dwm::Arg::I(0),
        ),
        Key::new(
            MODKEY,
            XK_t.into(),
            Some(dwm::setlayout),
            dwm::Arg::Lt(layouts[0].clone()),
        ),
        Key::new(
            MODKEY,
            XK_f.into(),
            Some(dwm::setlayout),
            dwm::Arg::Lt(layouts[1].clone()),
        ),
        Key::new(
            MODKEY,
            XK_m.into(),
            Some(dwm::setlayout),
            dwm::Arg::Lt(layouts[2].clone()),
        ),
        Key::new(
            MODKEY,
            XK_space.into(),
            Some(dwm::setlayout),
            dwm::Arg::I(0),
        ),
        Key::new(
            MODKEY | ShiftMask,
            XK_space.into(),
            Some(dwm::togglefloating),
            dwm::Arg::I(0),
        ),
        Key::new(
            MODKEY | ShiftMask,
            XK_f.into(),
            Some(dwm::togglefullscr),
            dwm::Arg::I(0),
        ),
        Key::new(MODKEY, XK_0.into(), Some(dwm::view), dwm::Arg::Ui(!0)),
        Key::new(
            MODKEY | ShiftMask,
            XK_0.into(),
            Some(dwm::tag),
            dwm::Arg::Ui(!0),
        ),
        Key::new(
            MODKEY,
            XK_comma.into(),
            Some(dwm::focusmon),
            dwm::Arg::I(-1),
        ),
        Key::new(
            MODKEY,
            XK_period.into(),
            Some(dwm::focusmon),
            dwm::Arg::I(1),
        ),
        Key::new(
            MODKEY | ShiftMask,
            XK_comma.into(),
            Some(dwm::tagmon),
            dwm::Arg::I(-1),
        ),
        Key::new(
            MODKEY | ShiftMask,
            XK_period.into(),
            Some(dwm::tagmon),
            dwm::Arg::I(1),
        ),
    ];
    m.extend(TAGKEYS(XK_1, 0));
    m.extend(TAGKEYS(XK_2, 1));
    m.extend(TAGKEYS(XK_3, 2));
    m.extend(TAGKEYS(XK_4, 3));
    m.extend(TAGKEYS(XK_5, 4));
    m.extend(TAGKEYS(XK_6, 5));
    m.extend(TAGKEYS(XK_7, 6));
    m.extend(TAGKEYS(XK_8, 7));
    m.extend(TAGKEYS(XK_9, 8));
    m.push(Key::new(
        MODKEY | ShiftMask,
        XK_q.into(),
        Some(dwm::quit),
        dwm::Arg::I(0),
    ));
    m
});
pub const buttons: Lazy<Vec<Button>> = Lazy::new(|| {
    vec![
        Button::new(
            CLICK::ClkLtSymbol as u32,
            0,
            Button1,
            Some(dwm::setlayout),
            dwm::Arg::I(0),
        ),
        Button::new(
            CLICK::ClkLtSymbol as u32,
            0,
            Button3,
            Some(dwm::setlayout),
            dwm::Arg::Lt(layouts[2].clone()),
        ),
        Button::new(
            CLICK::ClkWinTitle as u32,
            0,
            Button2,
            Some(dwm::zoom),
            dwm::Arg::I(0),
        ),
        Button::new(
            CLICK::ClkStatusText as u32,
            0,
            Button2,
            Some(dwm::spawn),
            dwm::Arg::V(termcmd.clone()),
        ),
        Button::new(
            CLICK::ClkClientWin as u32,
            MODKEY,
            Button1,
            Some(dwm::movemouse),
            dwm::Arg::I(0),
        ),
        Button::new(
            CLICK::ClkClientWin as u32,
            MODKEY,
            Button2,
            Some(dwm::togglefloating),
            dwm::Arg::I(0),
        ),
        Button::new(
            CLICK::ClkClientWin as u32,
            MODKEY,
            Button3,
            Some(dwm::resizemouse),
            dwm::Arg::I(0),
        ),
        Button::new(
            CLICK::ClkTagBar as u32,
            0,
            Button1,
            Some(dwm::view),
            dwm::Arg::Ui(0),
        ),
        Button::new(
            CLICK::ClkTagBar as u32,
            0,
            Button3,
            Some(dwm::toggleview),
            dwm::Arg::Ui(0),
        ),
        Button::new(
            CLICK::ClkTagBar as u32,
            MODKEY,
            Button1,
            Some(dwm::tag),
            dwm::Arg::Ui(0),
        ),
        Button::new(
            CLICK::ClkTagBar as u32,
            MODKEY,
            Button3,
            Some(dwm::toggletag),
            dwm::Arg::Ui(0),
        ),
    ]
});
