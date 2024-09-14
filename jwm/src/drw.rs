use std::{
    char,
    ffi::CString,
    i32,
    process::exit,
    ptr::{null, null_mut},
    sync::atomic::{AtomicU32, Ordering},
    u32, usize,
};

pub use fontconfig_sys;

use fontconfig_sys::{
    constants::{FC_CHARSET, FC_SCALABLE},
    FcChar8, FcCharSet, FcCharSetAddChar, FcCharSetCreate, FcCharSetDestroy, FcConfigSubstitute,
    FcDefaultSubstitute, FcMatchPattern, FcNameParse, FcPattern, FcPatternAddBool,
    FcPatternAddCharSet, FcPatternDestroy, FcPatternDuplicate,
};
use x11::{
    xft::{
        FcResult, XftCharExists, XftColor, XftColorAllocName, XftDraw, XftDrawCreate,
        XftDrawDestroy, XftDrawStringUtf8, XftFont, XftFontClose, XftFontMatch, XftFontOpenName,
        XftFontOpenPattern, XftTextExtents8,
    },
    xlib::{
        self, CapButt, Cursor, Drawable, False, JoinMiter, LineSolid, Window, XCopyArea,
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

const NOMATCHES_LEN: usize = 64;
pub struct NoMathes {
    codepoint: [u64; NOMATCHES_LEN],
    idx: u32,
}
pub static mut NOMATCHES: NoMathes = NoMathes {
    codepoint: [0; NOMATCHES_LEN],
    idx: 0,
};
static ELLIPSIS_WIDTH: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone)]
pub struct Cur {
    pub cursor: Cursor,
}
impl Cur {
    fn new() -> Self {
        Cur { cursor: 0 }
    }
}

#[derive(Debug, Clone)]
pub struct Fnt {
    pub dpy: *mut xlib::Display,
    pub h: u32,
    pub xfont: *mut XftFont,
    pub pattern: *mut FcPattern,
    pub next: *mut Fnt,
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

#[repr(C)]
pub enum _Col {
    ColFg = 0,
    ColBg = 1,
    ColBorder = 2,
}

pub type Clr = XftColor;

#[derive(Debug, Clone)]
pub struct Drw {
    pub w: u32,
    pub h: u32,
    pub dpy: *mut xlib::Display,
    pub screen: i32,
    pub root: Window,
    pub drawable: Drawable,
    pub gc: GC,
    pub scheme: Vec<*mut Clr>,
    pub fonts: *mut Fnt,
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
            scheme: vec![],
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
    let xfont: *mut XftFont;
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
    let mut ret: *mut Fnt = null_mut();

    if drw.is_null() || fonts.is_empty() {
        return null_mut();
    }

    unsafe {
        for i in 1..=fontcount {
            let cur = xfont_create(drw, fonts[(i - 1) as usize], null_mut());
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

pub fn drw_scm_create(drw: *mut Drw, clrnames: &[&str], clrcount: u64) -> Vec<*mut Clr> {
    unsafe {
        // Need at least two colors for a scheme.
        if drw.is_null() || clrnames.is_empty() || clrcount < 2 {
            return vec![];
        }
        let mut ret: Vec<*mut Clr> = vec![];
        for i in 0..clrcount {
            let mut one_ret: Clr = std::mem::zeroed();
            drw_clr_create(drw, &mut one_ret, clrnames[i as usize]);
            ret.push(&mut one_ret);
        }
        return ret;
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

pub fn drw_setscheme(drw: *mut Drw, scm: Vec<*mut Clr>) {
    if !drw.is_null() {
        unsafe {
            (*drw).scheme = scm;
        }
    }
}

// Drawing functions.
pub fn drw_rect(drw: *mut Drw, x: i32, y: i32, w: u32, h: u32, filled: i32, invert: i32) {
    unsafe {
        if drw.is_null() || (*drw).scheme.is_empty() {
            return;
        }
        XSetForeground(
            (*drw).dpy,
            (*drw).gc,
            if invert > 0 {
                (*(*drw).scheme[_Col::ColBg as usize]).pixel
            } else {
                (*(*drw).scheme[_Col::ColFg as usize]).pixel
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
    mut x: i32,
    mut y: i32,
    mut w: u32,
    mut h: u32,
    lpad: u32,
    mut text: &str,
    invert: i32,
) -> i32 {
    let mut ellipsis_x: i32 = 0;

    let mut tmpw: u32 = 0;
    let mut ellipsis_w: u32 = 0;

    let d: *mut XftDraw = null_mut();

    let mut usedfont: *mut Fnt = null_mut();
    let mut curfont: *mut Fnt = null_mut();
    let mut nextfont: *mut Fnt = null_mut();
    let mut utf8strlen: i32 = 0;
    let mut utf8charlen: i32 = 0;
    let render: bool = x > 0 || y > 0 || w > 0 || h > 0;
    let mut utf8codepoint: u64 = 0;
    let mut utf8str: &str;
    let mut fccharset: *mut FcCharSet = null_mut();
    let mut fcpattern: *mut FcPattern = null_mut();
    let mut match0: *mut FcPattern = null_mut();
    let mut result: FcResult = FcResult::NoId;
    let mut charexists: i32 = 0;
    let mut overflow: i32 = 0;

    unsafe {
        if drw.is_null()
            || (render && ((*drw).scheme.is_empty() || w <= 0)
                || text.is_empty()
                || (*drw).fonts.is_null())
        {
            return 0;
        }

        if !render {
            w = if invert > 0 {
                invert.try_into().unwrap()
            } else {
                (!invert).try_into().unwrap()
            };
        } else {
            let idx = if invert > 0 { _Col::ColFg } else { _Col::ColBg } as usize;
            XSetForeground((*drw).dpy, (*drw).gc, (*(*drw).scheme[idx]).pixel);
            XFillRectangle((*drw).dpy, (*drw).drawable, (*drw).gc, x, y, w, h);
            XftDrawCreate(
                (*drw).dpy,
                (*drw).drawable,
                XDefaultVisual((*drw).dpy, (*drw).screen),
                XDefaultColormap((*drw).dpy, (*drw).screen),
            );
            x += lpad as i32;
            w -= lpad;
        }

        usedfont = (*drw).fonts;
        let ellipsis_width = ELLIPSIS_WIDTH.load(Ordering::SeqCst);
        if ellipsis_width > 0 && render {
            ELLIPSIS_WIDTH.store(drw_fontset_getwidth(drw, "..."), Ordering::SeqCst);
        }
        loop {
            let mut ew = 0;
            let mut ellipsis_len = 0;
            utf8strlen = 0;
            utf8str = text;
            nextfont = null_mut();

            while !text.is_empty() {
                utf8charlen = utf8decode(text, &mut utf8codepoint, UTF_SIZ) as i32;
                curfont = (*drw).fonts;
                while !curfont.is_null() {
                    charexists = (charexists > 0
                        || XftCharExists(
                            (*drw).dpy,
                            (*curfont).xfont,
                            utf8codepoint.try_into().unwrap(),
                        ) > 0) as i32;
                    if charexists > 0 {
                        drw_font_gettexts(curfont, text, utf8charlen as u32, &mut tmpw, null_mut());
                        if ew + ellipsis_width <= w {
                            ellipsis_x = x + ew as i32;
                            ellipsis_w = w - ew;
                            ellipsis_len = utf8strlen as u32;
                        }

                        if ew + tmpw > w {
                            if !render {
                                x += tmpw as i32;
                            } else {
                                utf8strlen = ellipsis_len as i32;
                            }
                        } else if curfont == usedfont {
                            utf8strlen += utf8charlen;
                            text = &text[utf8charlen as usize..];
                            ew += tmpw;
                        } else {
                            nextfont = curfont;
                        }
                        break;
                    }

                    curfont = (*curfont).next;
                }

                if overflow > 0 || charexists <= 0 || !nextfont.is_null() {
                    break;
                } else {
                    charexists = 0;
                }
            }

            if utf8strlen > 0 {
                if render {
                    let ty = y + ((h - (*usedfont).h) / 2) as i32 + (*(*usedfont).xfont).ascent;
                    let idx = if invert > 0 { _Col::ColBg } else { _Col::ColFg } as usize;
                    let cstring = CString::new(utf8str).expect("fail to create");
                    XftDrawStringUtf8(
                        d,
                        (*drw).scheme[idx],
                        (*usedfont).xfont,
                        x,
                        ty,
                        cstring.as_ptr() as *const _,
                        utf8strlen,
                    );
                    x += ew as i32;
                    w -= ew;
                }
            }

            if render && overflow > 0 {
                drw_text(drw, ellipsis_x, y, ellipsis_w, h, 0, "...", invert);
            }

            if text.is_empty() || overflow > 0 {
                break;
            } else if !nextfont.is_null() {
                charexists = 0;
                usedfont = nextfont;
            } else {
                // Regardless of whether or not a fallback font is found, the character must be
                // drawn.
                charexists = 1;

                for i in 0..NOMATCHES_LEN {
                    // avoid calling XftFontMatch if we know we won't find a match.
                    if utf8codepoint == NOMATCHES.codepoint[i] {
                        usedfont = (*drw).fonts;
                        continue;
                    }
                }

                fccharset = FcCharSetCreate();
                FcCharSetAddChar(fccharset, utf8codepoint as u32);

                if (*(*drw).fonts).pattern.is_null() {
                    // The first font in the cache must be loaded from a font string.
                    exit(0);
                }

                fcpattern = FcPatternDuplicate((*(*drw).fonts).pattern);
                FcPatternAddCharSet(fcpattern, FC_CHARSET.as_ptr(), fccharset);
                FcPatternAddBool(fcpattern, FC_SCALABLE.as_ptr(), 1);

                FcConfigSubstitute(null_mut(), fcpattern, FcMatchPattern);
                FcDefaultSubstitute(fcpattern);
                match0 = XftFontMatch(
                    (*drw).dpy,
                    (*drw).screen,
                    fcpattern as *mut _,
                    &mut result as *mut _,
                ) as *mut _;

                FcCharSetDestroy(fccharset);
                FcPatternDestroy(fcpattern);

                if !match0.is_null() {
                    usedfont = xfont_create(drw, "", match0);
                    if !usedfont.is_null()
                        && XftCharExists((*drw).dpy, (*usedfont).xfont, utf8codepoint as u32) > 0
                    {
                        curfont = (*drw).fonts;
                        loop {
                            if (*curfont).xfont.is_null() {
                                break;
                            }
                            curfont = (*curfont).next;
                        }
                        (*curfont).next = usedfont;
                    } else {
                        xfont_free(usedfont);
                        NOMATCHES.idx += 1;
                        let idx = NOMATCHES.idx as usize;
                        NOMATCHES.codepoint[idx % NOMATCHES_LEN] = utf8codepoint;
                        usedfont = (*drw).fonts;
                    }
                }
            }
        }
        if !d.is_null() {
            XftDrawDestroy(d);
        }
    }

    return x + if render { w as i32 } else { 0 };
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
