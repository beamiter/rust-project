#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
// #![allow(unused_mut)]

use std::{i32, ptr::null_mut, u32};

// use log::info;
// use log::warn;
// For whatever reason, dmenu and other suckless tools use libXft, which does not support Unicode properly.
// If you use Pango however, Unicode will work great and this includes flag emojis.
use x11::{
    xft::XftColor,
    xlib::{self, Colormap, Cursor, Visual, XCreateFontCursor, XFreeCursor},
};

#[derive(Debug, Clone, Copy)]
pub struct Cur {
    pub cursor: Cursor,
}
impl Cur {
    pub fn new() -> Self {
        Cur { cursor: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct Drw {
    pub dpy: *mut xlib::Display,
    visual: *mut Visual,
    cmap: Colormap,
}
impl Drw {
    pub fn new() -> Self {
        Drw {
            dpy: null_mut(),
            visual: null_mut(),
            cmap: 0,
        }
    }
    pub fn drw_create(dpy: *mut xlib::Display, visual: *mut Visual, cmap: Colormap) -> Self {
        let mut drw = Drw::new();
        drw.dpy = dpy;
        drw.visual = visual;
        drw.cmap = cmap;
        return drw;
    }

    pub fn drw_clr_create_direct(r: u8, g: u8, b: u8, alpha: u8) -> Option<XftColor> {
        unsafe {
            let mut xcolor: XftColor = std::mem::zeroed();
            // 手动构造像素值 (ARGB格式)
            xcolor.pixel =
                ((alpha as u64) << 24) | ((r as u64) << 16) | ((g as u64) << 8) | (b as u64);
            // 设置其他字段
            xcolor.color.red = (r as u16) << 8;
            xcolor.color.green = (g as u16) << 8;
            xcolor.color.blue = (b as u16) << 8;
            xcolor.color.alpha = (alpha as u16) << 8;
            Some(xcolor)
        }
    }

    pub fn drw_clr_create_from_hex(hex_color: &str, alpha: u8) -> Option<XftColor> {
        // 解析 "#ff0000" 格式
        if hex_color.starts_with('#') && hex_color.len() == 7 {
            let r = u8::from_str_radix(&hex_color[1..3], 16).ok()?;
            let g = u8::from_str_radix(&hex_color[3..5], 16).ok()?;
            let b = u8::from_str_radix(&hex_color[5..7], 16).ok()?;
            return Self::drw_clr_create_direct(r, g, b, alpha);
        }
        None
    }

    pub fn drw_cur_create(&mut self, shape: i32) -> Option<Box<Cur>> {
        let mut cur: Cur = Cur::new();

        unsafe {
            cur.cursor = XCreateFontCursor(self.dpy, shape as u32);
        }
        return Some(Box::new(cur));
    }

    pub fn drw_cur_free(&mut self, cursor: *mut Cur) {
        if cursor.is_null() {
            return;
        }

        unsafe {
            XFreeCursor(self.dpy, (*cursor).cursor);
        }
    }

    // Drawing functions.
}
