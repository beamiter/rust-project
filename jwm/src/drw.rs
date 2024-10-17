#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
// #![allow(unused_mut)]

use std::{
    cell::RefCell, ffi::CString, i32, mem::zeroed, process::exit, ptr::null_mut, rc::Rc, u32, usize,
};

pub use fontconfig_sys;

use fontconfig_sys::{
    constants::{FC_CHARSET, FC_SCALABLE},
    FcChar8, FcCharSet, FcCharSetAddChar, FcCharSetCreate, FcCharSetDestroy, FcConfigSubstitute,
    FcDefaultSubstitute, FcMatchPattern, FcNameParse, FcPattern, FcPatternAddBool,
    FcPatternAddCharSet, FcPatternDestroy, FcPatternDuplicate,
};
use log::info;
use log::warn;
use x11::{
    xft::{
        FcResult, XftCharExists, XftColor, XftColorAllocName, XftDraw, XftDrawCreate,
        XftDrawDestroy, XftDrawStringUtf8, XftFont, XftFontClose, XftFontMatch, XftFontOpenName,
        XftFontOpenPattern, XftTextExtentsUtf8,
    },
    xlib::{
        self, CapButt, Cursor, Drawable, False, JoinMiter, LineSolid, True, Window, XCopyArea,
        XCreateFontCursor, XCreateGC, XCreatePixmap, XDefaultColormap, XDefaultDepth,
        XDefaultVisual, XDrawRectangle, XFillRectangle, XFreeCursor, XFreeGC, XFreePixmap,
        XSetForeground, XSetLineAttributes, XSync, GC,
    },
    xrender::XGlyphInfo,
};

const NOMATCHES_LEN: usize = 64;
pub struct NoMathes {
    codepoint: [u64; NOMATCHES_LEN],
    idx: u32,
}
#[allow(dead_code)]
impl NoMathes {
    pub fn new() -> Self {
        Self {
            codepoint: [0; NOMATCHES_LEN],
            idx: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Cur {
    pub cursor: Cursor,
}
impl Cur {
    pub fn new() -> Self {
        Cur { cursor: 0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fnt {
    pub dpy: *mut xlib::Display,
    pub h: u32,
    pub xfont: *mut XftFont,
    pub pattern: *mut FcPattern,
    pub next: Option<Rc<RefCell<Fnt>>>,
}
impl Fnt {
    pub fn new() -> Self {
        Fnt {
            dpy: null_mut(),
            h: 0,
            xfont: null_mut(),
            pattern: null_mut(),
            next: None,
        }
    }
}

#[repr(C)]
pub enum Col {
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
    // sum of left and right padding for text
    pub lrpad: i32,
    pub root: Window,
    pub drawable: Drawable,
    pub gc: GC,
    pub scheme: Vec<Option<Rc<Clr>>>,
    pub fonts: Option<Rc<RefCell<Fnt>>>,
}
impl Drw {
    pub fn new() -> Self {
        Drw {
            w: 0,
            h: 0,
            dpy: null_mut(),
            screen: 0,
            lrpad: 0,
            root: 0,
            drawable: 0,
            gc: null_mut(),
            scheme: vec![],
            fonts: None,
        }
    }
    pub fn textw(&mut self, X: &str) -> u32 {
        self.drw_fontset_getwidth(X) + self.lrpad as u32
    }
    pub fn drw_create(dpy: *mut xlib::Display, screen: i32, root: Window, w: u32, h: u32) -> Self {
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
    pub fn drw_resize(&mut self, w: u32, h: u32) {
        unsafe {
            self.w = w;
            self.h = h;
            if self.drawable > 0 {
                XFreePixmap(self.dpy, self.drawable);
            }
            self.drawable = XCreatePixmap(
                self.dpy,
                self.root,
                w,
                h,
                XDefaultDepth(self.dpy, self.screen).try_into().unwrap(),
            );
        }
    }
    pub fn drw_free(&mut self) {
        unsafe {
            XFreePixmap(self.dpy, self.drawable);
            XFreeGC(self.dpy, self.gc);
        }
    }
    pub fn xfont_create(
        &mut self,
        fontname: &str,
        fontpattern: *mut FcPattern,
    ) -> Option<Rc<RefCell<Fnt>>> {
        let mut font = Fnt::new();
        let xfont: *mut XftFont;
        let mut pattern: *mut FcPattern = null_mut();

        unsafe {
            if !fontname.is_empty() {
                let cstring = CString::new(fontname).expect("fail to convert");
                xfont = XftFontOpenName(self.dpy, self.screen, cstring.as_ptr());
                if xfont.is_null() {
                    eprintln!("error, cannot load font from name: {}", fontname);
                    return None;
                }
                pattern = FcNameParse(cstring.as_ptr() as *const FcChar8);
                if pattern.is_null() {
                    eprintln!("error, cannot parse font name to pattern: {}", fontname);
                    XftFontClose(self.dpy, xfont);
                    return None;
                }
            } else if !fontpattern.is_null() {
                xfont = XftFontOpenPattern(self.dpy, fontpattern as *mut _);
                if xfont.is_null() {
                    eprintln!("error, cannot load font from pattern.");
                    return None;
                }
            } else {
                exit(0);
            }
        }

        font.xfont = xfont;
        font.pattern = pattern;
        unsafe {
            font.h = ((*xfont).ascent + (*xfont).descent) as u32;
            font.dpy = self.dpy;
        }

        return Some(Rc::new(RefCell::new(font)));
    }
    pub fn drw_fontset_create(
        &mut self,
        fonts: &[&str],
        fontcount: u64,
    ) -> Option<Rc<RefCell<Fnt>>> {
        let mut ret: Option<Rc<RefCell<Fnt>>> = None;

        if fonts.is_empty() {
            return None;
        }

        for i in 1..=fontcount {
            let cur = self.xfont_create(fonts[(i - 1) as usize], null_mut());
            if cur.is_some() {
                cur.as_ref().unwrap().borrow_mut().next = ret;
                ret = cur;
            }
        }
        self.fonts = ret;
        return self.fonts.clone();
    }
    #[allow(dead_code)]
    pub fn drw_fontset_free(&self, font: Option<Rc<RefCell<Fnt>>>) {
        if font.is_some() {
            self.drw_fontset_free(font.as_ref().unwrap().borrow_mut().next.clone());
            Self::xfont_free(font);
        }
    }

    pub fn drw_fontset_getwidth(&mut self, text: &str) -> u32 {
        if self.fonts.is_none() || text.is_empty() {
            return 0;
        }
        return self.drw_text(0, 0, 0, 0, 0, text, 0) as u32;
    }

    #[allow(dead_code)]
    pub fn drw_fontset_getwidth_clamp(&mut self, text: &str, n: u32) -> u32 {
        let mut tmp: u32 = 0;
        if self.fonts.is_some() && (n > 0) {
            tmp = self.drw_text(0, 0, 0, 0, 0, text, n as i32) as u32;
        }
        return n.min(tmp);
    }
    pub fn drw_clr_create(&mut self, clrname: &str) -> Option<Rc<Clr>> {
        if clrname.is_empty() {
            return None;
        }

        unsafe {
            let cstring = CString::new(clrname).expect("fail to connect");
            let mut dest: Clr = std::mem::zeroed();
            let dpy = self.dpy;
            let screen = self.screen;
            if XftColorAllocName(
                dpy,
                XDefaultVisual(dpy, screen),
                XDefaultColormap(dpy, screen),
                cstring.as_ptr(),
                &mut dest,
            ) <= 0
            {
                eprintln!("error, cannot allocate color: {}", clrname);
                return None;
            }
            return Some(Rc::new(dest));
        }
    }
    pub fn drw_scm_create(&mut self, clrnames: &[&str]) -> Vec<Option<Rc<Clr>>> {
        let clrcount = clrnames.len();
        // Need at least two colors for a scheme.
        if clrnames.is_empty() || clrcount < 2 {
            return vec![];
        }
        let mut ret: Vec<Option<Rc<Clr>>> = vec![];
        for clrname in clrnames {
            let one_ret = self.drw_clr_create(clrname);
            ret.push(one_ret);
        }
        return ret;
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
    #[allow(dead_code)]
    pub fn drw_setfontset(&mut self, set: Option<Rc<RefCell<Fnt>>>) {
        self.fonts = set;
    }

    pub fn drw_setscheme(&mut self, scm: Vec<Option<Rc<Clr>>>) {
        self.scheme = scm;
    }

    // Drawing functions.
    pub fn drw_rect(&mut self, x: i32, y: i32, w: u32, h: u32, filled: i32, invert: i32) {
        info!("[drw_rect]");
        info!(
            "[drw_rect] x: {}, y: {},w: {},h: {}, filled: {}, invert: {}",
            x, y, w, h, filled, invert
        );
        unsafe {
            if self.scheme.is_empty() {
                return;
            }
            XSetForeground(
                self.dpy,
                self.gc,
                if invert > 0 {
                    self.scheme[Col::ColBg as usize].as_ref().unwrap().pixel
                } else {
                    self.scheme[Col::ColFg as usize].as_ref().unwrap().pixel
                },
            );
            if filled > 0 {
                XFillRectangle(self.dpy, self.drawable, self.gc, x, y, w, h);
            } else {
                XDrawRectangle(self.dpy, self.drawable, self.gc, x, y, w - 1, h - 1);
            }
        }
    }
    #[allow(unused_mut)]
    pub fn drw_text(
        &mut self,
        mut x: i32,
        mut y: i32,
        mut w: u32,
        mut h: u32,
        lpad: u32,
        mut text: &str,
        invert: i32,
    ) -> i32 {
        info!("[drw_text]");
        info!(
            "[drw_text] x: {}, y: {},w: {},h: {}, lpad: {}, text: {:?}, invert: {}",
            x, y, w, h, lpad, text, invert
        );
        let mut ellipsis_x: i32 = 0;

        let mut tmpw: u32 = 0;
        let mut ellipsis_w: u32 = 0;

        let mut d: *mut XftDraw = null_mut();

        let mut curfont: Option<Rc<RefCell<Fnt>>>;
        let mut nextfont: Option<Rc<RefCell<Fnt>>>;
        let mut utf8strlen: i32;
        let mut utf8charlen: i32;
        let render: bool = x > 0 || y > 0 || w > 0 || h > 0;
        let mut utf8codepoint: u64 = 0;
        let mut utf8str: &str;
        let mut fccharset: *mut FcCharSet;
        let mut fcpattern: *mut FcPattern;
        let mut match0: *mut FcPattern;
        let mut result: FcResult = FcResult::NoId;
        let mut charexists: i32 = 0;
        let mut overflow: i32 = 0;

        unsafe {
            static mut ellipsis_width: u32 = 0;
            static mut nomatches: NoMathes = unsafe { zeroed() };
            if (render && (self.scheme.is_empty() || w <= 0))
                || (text.is_empty())
                || (self.fonts.is_none())
            {
                return 0;
            }

            if !render {
                w = if invert > 0 {
                    invert as u32
                } else {
                    (!invert) as u32
                };
            } else {
                let idx = if invert > 0 { Col::ColFg } else { Col::ColBg } as usize;
                XSetForeground(self.dpy, self.gc, self.scheme[idx].as_ref().unwrap().pixel);
                XFillRectangle(self.dpy, self.drawable, self.gc, x, y, w, h);
                d = XftDrawCreate(
                    self.dpy,
                    self.drawable,
                    XDefaultVisual(self.dpy, self.screen),
                    XDefaultColormap(self.dpy, self.screen),
                );
                x += lpad as i32;
                w -= lpad;
            }

            let mut usedfont = self.fonts.clone();
            if ellipsis_width <= 0 && render {
                ellipsis_width = self.drw_fontset_getwidth("...");
                // info!("[drw_text], ellipsis_width: {}", ellipsis_width);
            }
            loop {
                let mut ew = 0;
                let mut ellipsis_len = 0;
                utf8strlen = 0;
                utf8str = text;
                nextfont = None;

                while !text.is_empty() {
                    utf8charlen = text.chars().nth(0).unwrap().len_utf8() as i32;
                    utf8codepoint = text.chars().nth(0).unwrap() as u64;
                    curfont = self.fonts.clone();
                    while let Some(ref curfont_opt) = curfont {
                        charexists |= XftCharExists(
                            self.dpy,
                            curfont_opt.borrow_mut().xfont,
                            utf8codepoint as u32,
                        );
                        if charexists > 0 {
                            Self::drw_font_gettexts(
                                &mut *curfont_opt.borrow_mut(),
                                text,
                                utf8charlen as u32,
                                &mut tmpw,
                                null_mut(),
                            );
                            if ew + ellipsis_width <= w {
                                // keep track where the ellipsis still fits
                                ellipsis_x = x + ew as i32;
                                ellipsis_w = w - ew;
                                ellipsis_len = utf8strlen as u32;
                            }

                            if ew + tmpw > w {
                                overflow = 1;
                                // called from drw_fontset_getwidth_clamp()
                                // it wants the width AFTER the overflow
                                if !render {
                                    x += tmpw as i32;
                                } else {
                                    utf8strlen = ellipsis_len as i32;
                                }
                            } else if Rc::ptr_eq(curfont_opt, usedfont.as_ref().unwrap()) {
                                utf8strlen += utf8charlen;
                                text = &text[utf8charlen as usize..];
                                ew += tmpw;
                            } else {
                                nextfont = curfont;
                            }
                            break;
                        }

                        let next = curfont_opt.borrow_mut().next.clone();
                        curfont = next;
                    }

                    // info!("[drw_text] charexists: {}, ew: {}, ellipsis_width: {}, w: {}, tmpw: {}, overflow: {}, nextfont: {}",charexists, ew, ellipsis_width, w, tmpw, overflow, nextfont.is_some());
                    if overflow > 0 || charexists <= 0 || nextfont.is_some() {
                        break;
                    } else {
                        charexists = 0;
                    }
                }

                if utf8strlen > 0 {
                    if render {
                        let usedfont_mut = usedfont.as_ref().unwrap().borrow_mut();
                        let usedfont_h = usedfont_mut.h;
                        let ascent = (*usedfont_mut.xfont).ascent;
                        let ty = y + (h - usedfont_h) as i32 / 2 + ascent;
                        let idx = if invert > 0 { Col::ColBg } else { Col::ColFg } as usize;
                        let cstring = CString::new(utf8str).expect("fail to create");
                        let clr = self.scheme[idx].as_ref().unwrap();
                        XftDrawStringUtf8(
                            d,
                            clr.as_ref(),
                            usedfont_mut.xfont,
                            x,
                            ty,
                            cstring.as_ptr() as *const _,
                            utf8strlen,
                        );
                    }
                    x += ew as i32;
                    w -= ew;
                }

                if render && overflow > 0 {
                    info!("[drw_text] render overflow, draw ...");
                    self.drw_text(ellipsis_x, y, ellipsis_w, h, 0, "...", invert);
                }

                if text.is_empty() || overflow > 0 {
                    break;
                } else if nextfont.is_some() {
                    charexists = 0;
                    usedfont = nextfont;
                } else {
                    // Regardless of whether or not a fallback font is found, the character must be
                    // drawn.
                    charexists = 1;

                    for i in 0..NOMATCHES_LEN {
                        // avoid calling XftFontMatch if we know we won't find a match.
                        if utf8codepoint == nomatches.codepoint[i] {
                            usedfont = self.fonts.clone();
                            continue;
                        }
                    }

                    fccharset = FcCharSetCreate();
                    FcCharSetAddChar(fccharset, utf8codepoint as u32);

                    let pattern = { self.fonts.as_ref().unwrap().borrow_mut().pattern };
                    if pattern.is_null() {
                        // Refer to the comment if xfont_free for more information
                        // The first font in the cache must be loaded from a font string.
                        warn!("[drw_text] pattern is null");
                        exit(0);
                    }

                    fcpattern = FcPatternDuplicate(pattern);
                    FcPatternAddCharSet(fcpattern, FC_CHARSET.as_ptr(), fccharset);
                    FcPatternAddBool(fcpattern, FC_SCALABLE.as_ptr(), True);

                    FcConfigSubstitute(null_mut(), fcpattern, FcMatchPattern);
                    FcDefaultSubstitute(fcpattern);
                    match0 = XftFontMatch(
                        self.dpy,
                        self.screen,
                        fcpattern as *mut _,
                        &mut result as *mut _,
                    ) as *mut _;

                    FcCharSetDestroy(fccharset);
                    FcPatternDestroy(fcpattern);

                    if !match0.is_null() {
                        usedfont = self.xfont_create("", match0);
                        let xfont = { usedfont.as_ref().unwrap().borrow_mut().xfont };
                        if usedfont.is_some()
                            && XftCharExists(self.dpy, xfont, utf8codepoint as u32) > 0
                        {
                            curfont = self.fonts.clone();
                            while let Some(ref curfont_opt) = curfont {
                                // NOP
                                let next = curfont_opt.borrow_mut().next.clone();
                                if next.is_none() {
                                    break;
                                }
                                curfont = next;
                            }
                            curfont.as_ref().unwrap().borrow_mut().next = usedfont.clone();
                        } else {
                            Self::xfont_free(usedfont);
                            nomatches.idx += 1;
                            let idx = nomatches.idx as usize % NOMATCHES_LEN;
                            nomatches.codepoint[idx] = utf8codepoint;
                            usedfont = self.fonts.clone();
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
    pub fn drw_map(&mut self, win: Window, x: i32, y: i32, w: u32, h: u32) {
        unsafe {
            XCopyArea(self.dpy, self.drawable, win, self.gc, x, y, w, h, x, y);
            XSync(self.dpy, False);
        }
    }
    fn xfont_free(font: Option<Rc<RefCell<Fnt>>>) {
        unsafe {
            if let Some(ref font) = font {
                if !font.borrow_mut().pattern.is_null() {
                    FcPatternDestroy(font.borrow_mut().pattern);
                }
            }
        }
    }

    fn drw_font_gettexts(font: *mut Fnt, text: &str, len: u32, w: *mut u32, h: *mut u32) {
        unsafe {
            let mut ext: XGlyphInfo = std::mem::zeroed();

            if font.is_null() || text.is_empty() {
                return;
            }

            let cstring = CString::new(text).expect("fail to convert");
            XftTextExtentsUtf8(
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
}
