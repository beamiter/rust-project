use std::{
    ffi::CString,
    i32,
    process::exit,
    ptr::{null, null_mut},
    u32, usize,
};

pub use fontconfig_sys;

use fontconfig_sys::{FcChar8, FcNameParse, FcPattern, FcPatternDestroy};
use x11::{
    xft::{
        XftColor, XftColorAllocName, XftFont, XftFontClose, XftFontOpenName, XftFontOpenPattern,
        XftTextExtents8,
    },
    xlib::{
        self, CapButt, Cursor, Drawable, False, Font, JoinMiter, LineSolid, Window, XCopyArea,
        XCreateFontCursor, XCreateGC, XCreatePixmap, XDefaultColormap, XDefaultDepth,
        XDefaultVisual, XDrawRectangle, XFillRectangle, XFreeCursor, XFreeGC, XFreePixmap,
        XSetForeground, XSetLineAttributes, XSync, GC,
    },
    xrender::XGlyphInfo,
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
impl Cur {
    fn new() -> Self {
        Cur { cursor: 0 }
    }
}

pub struct Fnt {
    dpy: *mut xlib::Display,
    h: u32,
    xfont: *mut XftFont,
    pattern: *mut FcPattern,
    next: *mut Fnt,
}
impl Fnt {
    fn new() -> Self {
        Fnt {
            dpy: null_mut(),
            h: 0,
            xfont: null_mut(),
            pattern: null_mut(),
            next: null_mut(),
        }
    }
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
pub fn drw_create(dpy: *mut xlib::Display, screen: i32, root: Window, w: u32, h: u32) -> *mut Drw {
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
    return &mut drw;
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

fn xfont_create(drw: *mut Drw, fontname: &str, fontpattern: *mut FcPattern) -> *mut Fnt {
    let mut font = Fnt::new();
    let mut xfont: *mut XftFont = null_mut();
    let mut pattern: *mut FcPattern = null_mut();

    unsafe {
        if !fontname.is_empty() {
            let cstring = CString::new(fontname).expect("fail to convert");
            xfont = XftFontOpenName((*drw).dpy, (*drw).screen, cstring.as_ptr());
            if xfont.is_null() {
                eprintln!("error, cannot load font from name: {}", fontname);
                return null_mut();
            }
            pattern = FcNameParse(cstring.as_ptr() as *const FcChar8);
            if pattern.is_null() {
                eprintln!("error, cannot parse font name to pattern: {}", fontname);
                XftFontClose((*drw).dpy, xfont);
                return null_mut();
            }
        } else if !fontpattern.is_null() {
            xfont = XftFontOpenPattern((*drw).dpy, fontpattern as *mut _);
            if xfont.is_null() {
                eprintln!("error, cannot load font from pattern.");
                return null_mut();
            }
        } else {
            exit(0);
        }
    }

    font.xfont = xfont;
    font.pattern = pattern;
    unsafe {
        font.h = ((*xfont).ascent + (*xfont).descent) as u32;
        font.dpy = (*drw).dpy;
    }

    return &mut font;
}

fn xfont_free(font: *mut Fnt) {
    if font.is_null() {
        return;
    }
    unsafe {
        if !(*font).pattern.is_null() {
            FcPatternDestroy((*font).pattern);
        }
    }
}

// Fnt abstraction
pub fn drw_fontset_create(drw: *mut Drw, fonts: &[&str], fontcount: u64) -> *mut Fnt {
    let mut cur: *mut Fnt = null_mut();
    let mut ret: *mut Fnt = null_mut();

    let mut i: usize = 0;

    if drw.is_null() || fonts.is_empty() {
        return null_mut();
    }

    unsafe {
        for i in 1..=fontcount {
            cur = xfont_create(drw, fonts[(i - 1) as usize], null_mut());
            if !cur.is_null() {
                (*cur).next = ret;
                ret = cur;
            }
        }
        (*drw).fonts = ret;
        return (*drw).fonts;
    }
}

pub fn drw_fontset_free(font: *mut Fnt) {
    unsafe {
        if !font.is_null() {
            drw_fontset_free((*font).next);
            xfont_free(font);
        }
    }
}

pub fn drw_fontset_getwidth(drw: *mut Drw, text: &str) -> u32 {
    unsafe {
        if drw.is_null() || (*drw).fonts.is_null() || text.is_empty() {
            return 0;
        }
        return drw_text(drw, 0, 0, 0, 0, 0, text, 0) as u32;
    }
}

pub fn drw_fontset_getwidth_clamp(drw: *mut Drw, text: &str, n: u32) -> u32 {
    let mut tmp: u32 = 0;
    unsafe {
        if !drw.is_null() && !(*drw).fonts.is_null() && (n > 0) {
            tmp = drw_text(drw, 0, 0, 0, 0, 0, text, n as i32) as u32;
        }
    }
    return n.min(tmp);
}

pub fn drw_font_gettexts(font: *mut Fnt, text: &str, len: u32, w: *mut u32, h: *mut u32) {
    unsafe {
        let mut ext: XGlyphInfo = std::mem::zeroed();

        if font.is_null() || text.is_empty() {
            return;
        }

        let cstring = CString::new(text).expect("fail to convert");
        XftTextExtents8(
            (*font).dpy,
            (*font).xfont,
            cstring.as_ptr() as *const _,
            len as i32,
            &mut ext,
        );
        if !w.is_null() {
            *w = ext.xOff as u32;
        }
        if !h.is_null() {
            *h = (*font).h;
        }
    }
}

// Colorscheme abstraction
pub fn drw_clr_create(drw: *mut Drw, dest: *mut Clr, clrname: &str) {
    if drw.is_null() || dest.is_null() || clrname.is_empty() {
        return;
    }

    unsafe {
        let cstring = CString::new(clrname).expect("fail to connect");
        if XftColorAllocName(
            (*drw).dpy,
            XDefaultVisual((*drw).dpy, (*drw).screen),
            XDefaultColormap((*drw).dpy, (*drw).screen),
            cstring.as_ptr(),
            dest,
        ) <= 0
        {
            eprintln!("error, cannot allocate color: {}", clrname);
            exit(0);
        }
    }
}

// (TODO): Be array.
pub fn drw_scm_create(drw: *mut Drw, clrnames: &[&str], clrcount: u64) -> *mut Clr {
    unsafe {
        // (TODO): is array!
        let mut ret: Clr = std::mem::zeroed();

        // Need at least two colors for a scheme.
        if drw.is_null() || clrnames.is_empty() || clrcount < 2 {
            return null_mut();
        }

        for i in 0..clrcount {
            drw_clr_create(drw, &mut ret, clrnames[i as usize]);
        }
        return &mut ret;
    }
}

// Cursor abstraction
pub fn drw_cur_create(drw: *mut Drw, shape: i32) -> *mut Cur {
    let mut cur: Cur = Cur::new();

    if drw.is_null() {
        return null_mut();
    }
    unsafe {
        cur.cursor = XCreateFontCursor((*drw).dpy, shape as u32);
    }
    return &mut cur;
}

pub fn drw_cur_free(drw: *mut Drw, cursor: *mut Cur) {
    if cursor.is_null() {
        return;
    }

    unsafe {
        XFreeCursor((*drw).dpy, (*cursor).cursor);
    }
}

// Drawing context manipulation.
pub fn drw_setfontset(drw: *mut Drw, set: *mut Fnt) {
    if !drw.is_null() {
        unsafe {
            (*drw).fonts = set;
        }
    }
}

pub fn drw_setscheme(drw: *mut Drw, scm: *mut Clr) {
    if !drw.is_null() {
        unsafe {
            (*drw).scheme = scm;
        }
    }
}

// Drawing functions.
pub fn drw_rect(drw: *mut Drw, x: i32, y: i32, w: u32, h: u32, filled: i32, invert: i32) {
    unsafe {
        if drw.is_null() || (*drw).scheme.is_null() {
            return;
        }
        // (TODO): is scheme array?
        XSetForeground(
            (*drw).dpy,
            (*drw).gc,
            if invert > 0 {
                (*(*drw).scheme).pixel
            } else {
                (*(*drw).scheme).pixel
            },
        );
        if filled > 0 {
            XFillRectangle((*drw).dpy, (*drw).drawable, (*drw).gc, x, y, w, h);
        } else {
            XDrawRectangle((*drw).dpy, (*drw).drawable, (*drw).gc, x, y, w - 1, h - 1);
        }
    }
}

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
    // (TODO)
    0
}

// Map functions
pub fn drw_map(drw: *mut Drw, win: Window, x: i32, y: i32, w: u32, h: u32) {
    if drw.is_null() {
        return;
    }

    unsafe {
        XCopyArea(
            (*drw).dpy,
            (*drw).drawable,
            win,
            (*drw).gc,
            x,
            y,
            w,
            h,
            x,
            y,
        );
        XSync((*drw).dpy, False);
    }
}

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
