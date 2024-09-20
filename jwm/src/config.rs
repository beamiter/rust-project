#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
#![allow(unused_mut)]
#![allow(unused)]

use lazy_static::lazy_static;
use x11::{
    keysym::{
        XK_Return, XK_Tab, XK_b, XK_c, XK_comma, XK_d, XK_f, XK_h, XK_i, XK_j, XK_k, XK_l, XK_m,
        XK_p, XK_period, XK_q, XK_space, XK_t, XK_0, XK_1, XK_2, XK_3, XK_4, XK_5, XK_6, XK_7,
        XK_8, XK_9,
    },
    xlib::{Button1, Button2, Button3, ControlMask, Mod1Mask, ShiftMask},
};

use crate::dwm::{self, Button, Key, Layout, Rule, CLICK};

pub const borderpx: u32 = 1;
pub const snap: u32 = 32;
pub const showbar: bool = true;
pub const topbar: bool = true;
lazy_static! {
    pub static ref fonts: [&'static str; 1] = ["monospace:size=10"];
}
pub const dmenufont: &str = "monospace:size=10";
pub const col_gray1: &str = "#222222";
pub const col_gray2: &str = "#444444";
pub const col_gray3: &str = "#bbbbbb";
pub const col_gray4: &str = "#eeeeee";
pub const col_cyan: &str = "#005577";

pub const colors: [&[&'static str; 3]; 2] = [
    &[col_gray3, col_gray1, col_gray2],
    &[col_gray4, col_cyan, col_cyan],
];

lazy_static! {
    pub static ref rules: Vec<Rule> = vec![
        Rule::new("Gimp", "", "", 0, true, -1),
        Rule::new("Firefox", "", "", 1 << 8, false, -1)
    ];
}
pub const tags: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];

pub const mfact: f32 = 0.55;
pub const nmaster: i32 = 1;
pub const resizehints: bool = true;
pub const lockfullscreen: bool = true;

lazy_static! {
    pub static ref layouts: Vec<Layout> = vec![
        Layout::new("[]=", None),
        Layout::new("><>", None),
        Layout::new("[M]", None),
    ];
}

fn TAGKEYS(KEY: u32, TAG: i32) -> Vec<Key> {
    vec![
        Key::new(MODKEY, KEY.into(), Some(dwm::view), dwm::Arg::ui(1 << TAG)),
        Key::new(
            MODKEY | ControlMask,
            KEY.into(),
            Some(dwm::toggleview),
            dwm::Arg::ui(1 << TAG),
        ),
        Key::new(
            MODKEY | ShiftMask,
            KEY.into(),
            Some(dwm::tag),
            dwm::Arg::ui(1 << TAG),
        ),
        Key::new(
            MODKEY | ControlMask | ShiftMask,
            KEY.into(),
            Some(dwm::toggletag),
            dwm::Arg::ui(1 << TAG),
        ),
    ]
}

pub const MODKEY: u32 = Mod1Mask;
pub static mut dmenumon: &'static str = "0";
lazy_static! {
   pub  static ref dmenucmd: Vec<&'static str> = vec![
        "dmenu_run",
        "-m",
        unsafe {dmenumon},
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
        "",
    ];
    pub static ref termcmd: Vec<&'static str> = vec!["st", ""];

    pub static ref keys: Vec<Key> =  {let mut m = vec![
        // modifier     key           function          argument
        Key::new(MODKEY, XK_p.into(), Some(dwm::spawn), dwm::Arg::v(dmenucmd.clone())),
        Key::new(MODKEY | ShiftMask, XK_Return.into(), Some(dwm::spawn), dwm::Arg::v(termcmd.clone())),
        Key::new(MODKEY, XK_b.into(), Some(dwm::togglebar), dwm::Arg::i(0)),
        Key::new(MODKEY, XK_j.into(), Some(dwm::focusstack), dwm::Arg::i(1)),
        Key::new(MODKEY, XK_k.into(), Some(dwm::focusstack), dwm::Arg::i(-1)),
        Key::new(MODKEY, XK_i.into(), Some(dwm::incnmaster), dwm::Arg::i(1)),
        Key::new(MODKEY, XK_d.into(), Some(dwm::incnmaster), dwm::Arg::i(-1)),
        Key::new(MODKEY, XK_h.into(), Some(dwm::setmfact), dwm::Arg::f(-0.05)),
        Key::new(MODKEY, XK_l.into(), Some(dwm::setmfact), dwm::Arg::f(0.05)),
        Key::new(MODKEY, XK_Return.into(), Some(dwm::zoom), dwm::Arg::i(0)),
        Key::new(MODKEY, XK_Tab.into(), Some(dwm::view), dwm::Arg::i(0)),
        Key::new(MODKEY | ShiftMask, XK_c.into(), Some(dwm::killclient), dwm::Arg::i(0)),
        Key::new(MODKEY , XK_t.into(), Some(dwm::setlayout), dwm::Arg::lt(layouts[0].clone())),
        Key::new(MODKEY , XK_f.into(), Some(dwm::setlayout), dwm::Arg::lt(layouts[1].clone())),
        Key::new(MODKEY , XK_m.into(), Some(dwm::setlayout), dwm::Arg::lt(layouts[2].clone())),
        Key::new(MODKEY , XK_space.into(), Some(dwm::setlayout), dwm::Arg::i(0)),
        Key::new(MODKEY | ShiftMask, XK_space.into(), Some(dwm::togglefloating), dwm::Arg::i(0)),
        Key::new(MODKEY, XK_0.into(), Some(dwm::view), dwm::Arg::ui(0)),
        Key::new(MODKEY | ShiftMask, XK_0.into(), Some(dwm::tag), dwm::Arg::ui(0)),
        Key::new(MODKEY, XK_comma.into(), Some(dwm::focusmon), dwm::Arg::i(-1)),
        Key::new(MODKEY, XK_period.into(), Some(dwm::focusmon), dwm::Arg::i(1)),
        Key::new(MODKEY|ShiftMask, XK_comma.into(), Some(dwm::tagmon), dwm::Arg::i(-1)),
        Key::new(MODKEY|ShiftMask, XK_period.into(), Some(dwm::tagmon), dwm::Arg::i(1)),
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
        m.push(Key::new(MODKEY | ShiftMask, XK_q.into(), Some(dwm::quit), dwm::Arg::i(0)));
        m
    };

    pub static ref buttons: Vec<Button> = vec![
        Button::new(CLICK::ClkLtSymbol as u32, 0, Button1, Some(dwm::setlayout), dwm::Arg::i(0)),
        Button::new(CLICK::ClkLtSymbol as u32, 0, Button3, Some(dwm::setlayout), dwm::Arg::lt(layouts[2].clone())),
        Button::new(CLICK::ClkWinTitle as u32, 0, Button2, Some(dwm::zoom), dwm::Arg::i(0)),
        Button::new(CLICK::ClkStatusText as u32, 0, Button2, Some(dwm::spawn), dwm::Arg::v(termcmd.clone())),
        Button::new(CLICK::ClkClientWin as u32, MODKEY, Button1, Some(dwm::movemouse), dwm::Arg::i(0)),
        Button::new(CLICK::ClkClientWin as u32, MODKEY, Button2, Some(dwm::togglefloating), dwm::Arg::i(0)),
        Button::new(CLICK::ClkClientWin as u32, MODKEY, Button2, Some(dwm::resizemouse), dwm::Arg::i(0)),
        Button::new(CLICK::ClkTagBar as u32, 0, Button1, Some(dwm::view), dwm::Arg::i(0)),
        Button::new(CLICK::ClkTagBar as u32, 0, Button3, Some(dwm::toggleview), dwm::Arg::i(0)),
        Button::new(CLICK::ClkTagBar as u32, MODKEY, Button1, Some(dwm::tag), dwm::Arg::i(0)),
        Button::new(CLICK::ClkTagBar as u32, MODKEY, Button3, Some(dwm::toggletag), dwm::Arg::i(0)),
    ];
}
