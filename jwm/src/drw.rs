use std::{
    i32,
    ptr::{null, null_mut},
    u32,
};

use fontconfig::FontSet;
use x11::{
    xft::{FcPattern, XftColor, XftFont},
    xlib::{
        self, CapButt, Cursor, Drawable, Font, JoinMiter, LineSolid, Window, XCreateGC,
        XCreatePixmap, XDefaultDepth, XFreeGC, XFreePixmap, XSetLineAttributes, GC,
    },
};

pub const UTF_INVALID: u64 = 0xFFFD;
pub const UTF_SIZ: usize = 4;

pub const UTFBYTE: [u8; UTF_SIZ + 1] = [0x80, 0, 0xC0, 0xE0, 0xF0];
pub const UTFMASK: [u8; UTF_SIZ + 1] = [0xc0, 0x80, 0xE0, 0xF0, 0xF8];
pub const UTFMIN: [u64; UTF_SIZ + 1] = [0, 0, 0x80, 0x800, 0x10000];
pub const UTFMAX: [u64; UTF_SIZ + 1] = [0x10FFFF, 0x7F, 0x7FF, 0xFFF, 0x10FFFF];

macro_rules! BETWEEN {
    ($x:expr, $a:expr, $b:expr) => {
        $a <= $x && $x <= $b
    };
}

pub struct Cur {
    cursor: Cursor,
}

pub struct Fnt {
    dpy: *mut xlib::Display,
    h: u32,
    xfont: *mut XftFont,
    pattern: *mut FcPattern,
    next: *mut Fnt,
}

pub enum _Col {
    ColFg,
    ColBg,
    ColBorder,
}

pub type Clr = XftColor;

pub struct Drw {
    w: u32,
    h: u32,
    dpy: *mut xlib::Display,
    screen: i32,
    root: Window,
    drawable: Drawable,
    gc: GC,
    scheme: *mut Clr,
    fonts: *mut Fnt,
}

impl Drw {
    fn new() -> Self {
        Drw {
            w: 0,
            h: 0,
            dpy: null_mut(),
            screen: 0,
            root: 0,
            drawable: 0,
            gc: null_mut(),
            scheme: null_mut(),
            fonts: null_mut(),
        }
    }
}

// Drawable abstraction
pub fn drw_create(dpy: *mut xlib::Display, screen: i32, root: Window, w: u32, h: u32) -> Drw {
    let mut drw = Drw::new();

    drw.dpy = dpy;
    drw.screen = screen;
    drw.root = root;
    drw.w = w;
    drw.h = h;
    unsafe {
        drw.drawable = XCreatePixmap(
            dpy,
            root,
            w,
            h,
            XDefaultDepth(dpy, screen).try_into().unwrap(),
        );
        drw.gc = XCreateGC(dpy, root, 0, null_mut());
        XSetLineAttributes(dpy, drw.gc, 1, LineSolid, CapButt, JoinMiter);
    }
    return drw;
}

pub fn drw_resize(drw: *mut Drw, w: u32, h: u32) {
    if drw.is_null() {
        return;
    }

    unsafe {
        (*drw).w = w;
        (*drw).h = h;
        if (*drw).drawable > 0 {
            XFreePixmap((*drw).dpy, (*drw).drawable);
        }
        (*drw).drawable = XCreatePixmap(
            (*drw).dpy,
            (*drw).root,
            w,
            h,
            XDefaultDepth((*drw).dpy, (*drw).screen).try_into().unwrap(),
        );
    }
}

pub fn drw_free(drw: *mut Drw) {
    unsafe {
        XFreePixmap((*drw).dpy, (*drw).drawable);
        XFreeGC((*drw).dpy, (*drw).gc);
    }
}

// Fnt abstraction
pub fn drw_fontset_create(drw: *mut Drw, fonts: &[&str], fontcount: u64) {}

pub fn xfont_free(font: *mut Fnt) {
    if font.is_null() {
        return;
    }
    unsafe { if !(*font).pattern.is_null() {} }
}

pub fn drw_fontset_free(font: *mut Fnt) {
    unsafe {
        if !font.is_null() {
            drw_fontset_free((*font).next);
        }
    }
}

pub fn drw_fontset_getwidth(drw: *mut Drw, text: &str) {}

pub fn drw_fontset_getwidth_clamp(drw: *mut Drw, text: &str, n: u32) {}

pub fn drw_font_gettexts(font: *mut Font, len: u32, w: *mut u32, h: *mut u32) {}

// Colorscheme abstraction
pub fn drw_clr_create(drw: *mut Drw, dest: *mut Clr, clrname: &str) {}

pub fn drw_scm_create(drw: *mut Drw, clrnames: &[&str], clrcount: u64) -> *mut Clr {
    null_mut()
}

// Cursor abstraction
pub fn drw_cur_create(drw: *mut Drw, shape: i32) -> *mut Cur {
    null_mut()
}

pub fn drw_cur_free(drw: *mut Drw, cursor: *mut Cur) {}

// Drawing context manipulation.
pub fn drw_setfontset(drw: *mut Drw, set: *mut Fnt) {}

pub fn drw_setscheme(drw: *mut Drw, scm: *mut Clr) {}

// Drawing functions.
pub fn drw_rect(drw: *mut Drw, x: i32, y: i32, w: u32, h: u32, filled: i32, invert: i32) {}

pub fn drw_text(
    drw: *mut Drw,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    lpad: u32,
    text: &str,
    invert: i32,
) -> i32 {
    0
}

// Map functions
pub fn drw_map(drw: *mut Drw, win: Window, x: i32, y: i32, w: u32, h: u32) {}

pub fn utf8decodebyt(c: char, i: &mut usize) -> u64 {
    *i = 0;
    while *i < (UTF_SIZ + 1) {
        if (c as u8 & UTFMASK[*i]) == UTFBYTE[*i] {
            return (c as u8 & !UTFMASK[*i]) as u64;
        }
        *i += 1;
    }
    return 0;
}

pub fn utf8validate(u: &mut u64, mut i: usize) -> usize {
    if !BETWEEN!(*u, UTFMIN[i], UTFMAX[i]) || BETWEEN!(*u, 0xD800, 0xDFFF) {
        *u = UTF_INVALID;
    }
    i = 1;
    loop {
        if *u > UTFMAX[i] {
            break;
        }
        i += 1;
    }
    return i;
}

pub fn utf8decode(c: &str, u: &mut u64, clen: usize) -> usize {
    *u = UTF_INVALID;
    if clen <= 0 {
        return 0;
    }
    let mut len: usize = 0;
    let mut udecoded = utf8decodebyt(c.chars().nth(0).unwrap(), &mut len);
    if !BETWEEN!(len, 1, UTF_SIZ) {
        return 1;
    }
    let mut i = 1;
    let mut j = 1;
    let mut type0: usize = 0;
    while i < clen && j < len {
        udecoded = (udecoded << 6) | utf8decodebyt(c.chars().nth(i).unwrap(), &mut type0);
        if type0 > 0 {
            return j;
        }
        i += 1;
        j += 1;
    }
    if j < len {
        return 0;
    }
    *u = udecoded;
    utf8validate(u, len);

    return len;
}
