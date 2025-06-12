#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
#![allow(unused_mut)]
#![allow(unused)]

use rand::Rng;
use std::{rc::Rc, sync::RwLock};

use once_cell::sync::Lazy;
use x11::{
    keysym::{
        XK_Return, XK_Tab, XK_b, XK_c, XK_comma, XK_d, XK_e, XK_f, XK_h, XK_i, XK_j, XK_k, XK_l,
        XK_m, XK_o, XK_period, XK_q, XK_r, XK_space, XK_t, XK_0, XK_1, XK_2, XK_3, XK_4, XK_5,
        XK_6, XK_7, XK_8, XK_9,
    },
    xlib::{Button1, Button2, Button3, ControlMask, Mod1Mask, ShiftMask},
};

use crate::{
    dwm::{self, Button, Dwm, Key, Layout, Rule, CLICK},
    icon_gallery::{generate_random_tags, ICON_GALLERY},
    terminal_prober::ADVANCED_TERMINAL_PROBER,
};

pub struct Config {}
impl Config {
    // border pixel of windows
    pub const border_px: u32 = 5;
    // snap pixel
    pub const snap: u32 = 32;
    pub const egui_bar_name: &str = "egui_bar";
    pub const egui_bar_0: &str = "egui_bar_0";
    pub const egui_bar_1: &str = "egui_bar_1";
    pub const broken: &str = "broken";
    pub const dmenu_font: &str = "SauceCodePro Nerd Font Regular 11";
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
    pub const bar_alpha: u8 = 0xd0u8;
    pub const border_alpha: u8 = Self::OPAQUE;
    pub const bar_height: Option<i32> = Some(30); // 默认状态栏高度
    pub const egui_bar_pad: i32 = 5; // 状态栏下方的间距
    pub const focus_follows_new_window: bool = false;
    pub const center_small_windows: bool = true;
    pub const small_window_threshold: (i32, i32) = (400, 300); // (width, height)

    pub const colors: [&[&'static str; 3]; 10] = [
        // fg | bg | border
        &[Self::col_gray3, Self::col_gray1, Self::col_gray2],
        &[Self::col_gray4, Self::col_cyan, Self::col_cyan],
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
        &[Self::col_gray3, Self::col_gray1, Self::col_gray2],
        &[Self::col_black, Self::col_yellow, Self::col_red],
        &[Self::col_white, Self::col_red, Self::col_red],
    ];
    pub const alphas: [&[u8; 3]; 10] = [
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
        &[Self::OPAQUE, Self::TRANSPARENT, Self::border_alpha],
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
        &[Self::OPAQUE, Self::bar_alpha, Self::border_alpha],
    ];

    pub const m_fact: f32 = 0.55;
    pub const n_master: u32 = 1;
    pub const resize_hints: bool = true;
    pub const lock_fullscreen: bool = true;

    pub const rules: Lazy<Vec<Rule>> = Lazy::new(|| {
        vec![
            // class | instance | name | tags mask | isfloating | monitor
            Rule::new(Config::broken, Config::broken, Config::broken, 0, true, -1),
            // Rule::new("Firefox", "", "", 1 << 8, false, -1),
            Rule::new(
                Config::egui_bar_0,
                Config::egui_bar_0,
                Config::egui_bar_name,
                Self::tagmask as usize, // 设置为全标签掩码
                true,
                0,
            ),
            Rule::new(
                Config::egui_bar_1,
                Config::egui_bar_1,
                Config::egui_bar_name,
                Self::tagmask as usize, // 设置为全标签掩码
                true,
                1,
            ),
            // Rule::new("", "", "jterm4", 0, false, 1),
            // Rule::new("egui_bar", "egui_bar", "egui_bar", 0, false, 1),
        ]
    });

    // https://symbl.cc/en/
    pub const tags_length: usize = 9;
    pub const tagmask: u32 = (1 << Self::tags_length) - 1;

    fn TAGKEYS(KEY: u32, TAG: i32) -> Vec<Key> {
        vec![
            Key::new(
                Self::MODKEY,
                KEY.into(),
                Some(Dwm::view),
                dwm::Arg::Ui(1 << TAG),
            ),
            Key::new(
                Self::MODKEY | ControlMask,
                KEY.into(),
                Some(Dwm::toggleview),
                dwm::Arg::Ui(1 << TAG),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                KEY.into(),
                Some(Dwm::tag),
                dwm::Arg::Ui(1 << TAG),
            ),
            Key::new(
                Self::MODKEY | ControlMask | ShiftMask,
                KEY.into(),
                Some(Dwm::toggletag),
                dwm::Arg::Ui(1 << TAG),
            ),
        ]
    }

    pub const MODKEY: u32 = Mod1Mask;
    // dmenu_run -m 0 -fn "monospace:size=10" -nb "#222222" -nf "#bbbbbb" -sb "#005577" -sf "#eeeeee"
    pub const dmenucmd: Lazy<Vec<String>> = Lazy::new(|| {
        vec![
            "dmenu_run".to_string(),
            "-m".to_string(),
            "0".to_string(),
            "-fn".to_string(),
            Self::dmenu_font.to_string(),
            "-nb".to_string(),
            Self::col_gray1.to_string(),
            "-nf".to_string(),
            Self::col_gray3.to_string(),
            "-sb".to_string(),
            Self::col_cyan.to_string(),
            "-sf".to_string(),
            Self::col_gray4.to_string(),
        ]
    });
    // pub const termcmd: Lazy<Vec<String>> = Lazy::new(|| vec!["terminator".to_string()]);
    // 兼容原有接口
    pub const termcmd: Lazy<Vec<String>> = Lazy::new(|| {
        ADVANCED_TERMINAL_PROBER
            .get_available_terminal()
            .map(|config| vec![config.command.clone()])
            .unwrap_or_else(|| vec!["x-terminal-emulator".to_string()])
    });

    pub const keys: Lazy<Vec<Key>> = Lazy::new(|| {
        let mut m = vec![
            // modifier | key | function | argument
            Key::new(
                Self::MODKEY,
                XK_e.into(),
                Some(Dwm::spawn),
                dwm::Arg::V(Self::dmenucmd.to_vec()),
            ),
            Key::new(
                Self::MODKEY,
                XK_r.into(),
                Some(Dwm::spawn),
                dwm::Arg::V(Self::dmenucmd.clone()),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_Return.into(),
                Some(Dwm::spawn),
                dwm::Arg::V(Self::termcmd.clone()),
            ),
            Key::new(
                Self::MODKEY,
                XK_j.into(),
                Some(Dwm::focusstack),
                dwm::Arg::I(1),
            ),
            Key::new(
                Self::MODKEY,
                XK_k.into(),
                Some(Dwm::focusstack),
                dwm::Arg::I(-1),
            ),
            Key::new(
                Self::MODKEY,
                XK_i.into(),
                Some(Dwm::incnmaster),
                dwm::Arg::I(1),
            ),
            Key::new(
                Self::MODKEY,
                XK_d.into(),
                Some(Dwm::incnmaster),
                dwm::Arg::I(-1),
            ),
            Key::new(
                Self::MODKEY,
                XK_h.into(),
                Some(Dwm::setmfact),
                dwm::Arg::F(-0.025),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_h.into(),
                Some(Dwm::setcfact),
                dwm::Arg::F(0.2),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_j.into(),
                Some(Dwm::movestack),
                dwm::Arg::I(1),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_k.into(),
                Some(Dwm::movestack),
                dwm::Arg::I(-1),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_l.into(),
                Some(Dwm::setcfact),
                dwm::Arg::F(-0.2),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_o.into(),
                Some(Dwm::setcfact),
                dwm::Arg::F(0.0),
            ),
            Key::new(
                Self::MODKEY,
                XK_l.into(),
                Some(Dwm::setmfact),
                dwm::Arg::F(0.025),
            ),
            Key::new(
                Self::MODKEY,
                XK_Return.into(),
                Some(Dwm::zoom),
                dwm::Arg::I(0),
            ),
            Key::new(
                Self::MODKEY,
                XK_Tab.into(),
                Some(Dwm::view),
                dwm::Arg::Ui(0),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_c.into(),
                Some(Dwm::killclient),
                dwm::Arg::I(0),
            ),
            Key::new(
                Self::MODKEY,
                XK_t.into(),
                Some(Dwm::setlayout),
                dwm::Arg::Lt(Rc::new(Layout::try_from(0).unwrap())),
            ),
            Key::new(
                Self::MODKEY,
                XK_f.into(),
                Some(Dwm::setlayout),
                dwm::Arg::Lt(Rc::new(Layout::try_from(1).unwrap())),
            ),
            Key::new(
                Self::MODKEY,
                XK_m.into(),
                Some(Dwm::setlayout),
                dwm::Arg::Lt(Rc::new(Layout::try_from(2).unwrap())),
            ),
            // For toggle layout.
            Key::new(
                Self::MODKEY,
                XK_space.into(),
                Some(Dwm::setlayout),
                dwm::Arg::Ui(0),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_space.into(),
                Some(Dwm::togglefloating),
                dwm::Arg::I(0),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_f.into(),
                Some(Dwm::togglefullscr),
                dwm::Arg::I(0),
            ),
            Key::new(Self::MODKEY, XK_0.into(), Some(Dwm::view), dwm::Arg::Ui(!0)),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_0.into(),
                Some(Dwm::tag),
                dwm::Arg::Ui(!0),
            ),
            Key::new(
                Self::MODKEY,
                XK_comma.into(),
                Some(Dwm::focusmon),
                dwm::Arg::I(-1),
            ),
            Key::new(
                Self::MODKEY,
                XK_period.into(),
                Some(Dwm::focusmon),
                dwm::Arg::I(1),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_comma.into(),
                Some(Dwm::tagmon),
                dwm::Arg::I(-1),
            ),
            Key::new(
                Self::MODKEY | ShiftMask,
                XK_period.into(),
                Some(Dwm::tagmon),
                dwm::Arg::I(1),
            ),
        ];
        m.extend(Self::TAGKEYS(XK_1, 0));
        m.extend(Self::TAGKEYS(XK_2, 1));
        m.extend(Self::TAGKEYS(XK_3, 2));
        m.extend(Self::TAGKEYS(XK_4, 3));
        m.extend(Self::TAGKEYS(XK_5, 4));
        m.extend(Self::TAGKEYS(XK_6, 5));
        m.extend(Self::TAGKEYS(XK_7, 6));
        m.extend(Self::TAGKEYS(XK_8, 7));
        m.extend(Self::TAGKEYS(XK_9, 8));
        m.push(Key::new(
            Self::MODKEY | ShiftMask,
            XK_q.into(),
            Some(Dwm::quit),
            dwm::Arg::I(0),
        ));
        m
    });

    // Button1: 鼠标左键
    // Button2: 鼠标中键（通常是滚轮按下的动作）
    // Button3: 鼠标右键
    // Button4: 向上滚动滚轮
    // Button5: 向下滚动滚轮
    pub const buttons: Lazy<Vec<Button>> = Lazy::new(|| {
        vec![
            Button::new(
                CLICK::ClkLtSymbol as u32,
                0,
                Button1,
                Some(Dwm::setlayout),
                dwm::Arg::Ui(0),
            ),
            Button::new(
                CLICK::ClkLtSymbol as u32,
                0,
                Button3,
                Some(Dwm::setlayout),
                dwm::Arg::Lt(Rc::new(Layout::try_from(2).unwrap())),
            ),
            Button::new(
                CLICK::ClkWinTitle as u32,
                0,
                Button2,
                Some(Dwm::zoom),
                dwm::Arg::I(0),
            ),
            Button::new(
                CLICK::ClkStatusText as u32,
                0,
                Button2,
                Some(Dwm::spawn),
                dwm::Arg::V(Self::termcmd.clone()),
            ),
            Button::new(
                CLICK::ClkClientWin as u32,
                Self::MODKEY,
                Button1,
                Some(Dwm::movemouse),
                dwm::Arg::I(0),
            ),
            Button::new(
                CLICK::ClkClientWin as u32,
                Self::MODKEY,
                Button2,
                Some(Dwm::togglefloating),
                dwm::Arg::I(0),
            ),
            Button::new(
                CLICK::ClkClientWin as u32,
                Self::MODKEY,
                Button3,
                Some(Dwm::resizemouse),
                dwm::Arg::I(0),
            ),
            Button::new(
                CLICK::ClkTagBar as u32,
                0,
                Button1,
                Some(Dwm::view),
                dwm::Arg::Ui(0),
            ),
            Button::new(
                CLICK::ClkTagBar as u32,
                0,
                Button3,
                Some(Dwm::toggleview),
                dwm::Arg::Ui(0),
            ),
            Button::new(
                CLICK::ClkTagBar as u32,
                Self::MODKEY,
                Button1,
                Some(Dwm::tag),
                dwm::Arg::Ui(0),
            ),
            Button::new(
                CLICK::ClkTagBar as u32,
                Self::MODKEY,
                Button3,
                Some(Dwm::toggletag),
                dwm::Arg::Ui(0),
            ),
        ]
    });
}
