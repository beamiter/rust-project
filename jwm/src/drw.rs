#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
// #![allow(unused_mut)]

use pango::{prelude::FontMapExt, FontDescription, Layout};
use std::{
    cell::RefCell, ffi::CString, i32, mem::zeroed, process::exit, ptr::null_mut, rc::Rc, u32, usize,
};

use log::info;
// use log::warn;
// For whatever reason, dmenu and other suckless tools use libXft, which does not support Unicode properly.
// If you use Pango however, Unicode will work great and this includes flag emojis.
use pango::{
    ffi::{
        pango_context_get_metrics, pango_context_new, pango_font_description_from_string,
        pango_font_metrics_get_height, pango_font_metrics_unref, pango_layout_get_extents,
        pango_layout_new, pango_layout_set_attributes, pango_layout_set_font_description,
        pango_layout_set_markup, pango_layout_set_text, PangoLayout, PangoRectangle, PANGO_SCALE,
    },
    glib::gobject_ffi::g_object_unref,
};
use pango_cairo_sys::pango_cairo_create_context;
use x11::{
    xft::{XftColor, XftColorAllocName, XftDraw, XftDrawCreate, XftDrawDestroy},
    xlib::{
        self, CapButt, Cursor, Drawable, False, JoinMiter, LineSolid, Window, XCopyArea,
        XCreateFontCursor, XCreateGC, XCreatePixmap, XDefaultColormap, XDefaultDepth,
        XDefaultVisual, XDrawRectangle, XFillRectangle, XFreeCursor, XFreeGC, XFreePixmap,
        XSetForeground, XSetLineAttributes, XSync, GC,
    },
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

#[derive(Debug, Clone, PartialEq)]
pub struct Fnt {
    pub dpy: *mut xlib::Display,
    pub h: u32,
    pub layout: Option<Layout>,
}
impl Fnt {
    pub fn new() -> Self {
        Fnt {
            dpy: null_mut(),
            h: 0,
            layout: None,
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
    pub font: Option<Rc<RefCell<Fnt>>>,
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
            font: None,
        }
    }
    pub fn textw(&mut self, X: &str) -> u32 {
        self.drw_font_getwidth(X, false) + self.lrpad as u32
    }
    pub fn textwm(&mut self, X: &str) -> u32 {
        self.drw_font_getwidth(X, true) + self.lrpad as u32
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
            self.drw_font_free(self.font.clone());
        }
    }
    pub fn xfont_create(&mut self, fontname: &str) -> Option<Rc<RefCell<Fnt>>> {
        if fontname.is_empty() {
            exit(0);
        }

        let mut font = Fnt::new();
        font.dpy = self.dpy;

        let font_map = pangocairo::FontMap::default();
        let context = font_map.create_context();
        let layout = Layout::new(&context);
        let desc = FontDescription::from_string("SauceCodePro Nerd Font Regular 12");
        let desc = Some(&desc);
        layout.set_font_description(desc);
        let metrics = context.metrics(desc, None);
        font.h = metrics.height() as u32;
        font.layout = Some(layout);

        return Some(Rc::new(RefCell::new(font)));
    }
    pub fn drw_font_create(&mut self, font: &str) -> Option<Rc<RefCell<Fnt>>> {
        if font.is_empty() {
            return None;
        }

        let fnt = self.xfont_create(font);
        self.font = fnt;
        return self.font.clone();
    }
    pub fn drw_font_free(&self, font: Option<Rc<RefCell<Fnt>>>) {
        if font.is_some() {
            Self::xfont_free(font);
        }
    }

    pub fn drw_font_getwidth(&mut self, text: &str, markup: bool) -> u32 {
        if self.font.is_none() || text.is_empty() {
            return 0;
        }
        return self.drw_text(0, 0, 0, 0, 0, text, 0, markup) as u32;
    }

    #[allow(dead_code)]
    pub fn drw_font_getwidth_clamp(&mut self, text: &str, n: u32, markup: bool) -> u32 {
        let mut tmp: u32 = 0;
        if self.font.is_some() && (n > 0) {
            tmp = self.drw_text(0, 0, 0, 0, 0, text, n as i32, markup) as u32;
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
            dest.pixel |= 0xff << 24;
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
        markup: bool,
    ) -> i32 {
        info!("[drw_text]");
        info!(
            "[drw_text] x: {}, y: {},w: {},h: {}, lpad: {}, text: {:?}, invert: {}",
            x, y, w, h, lpad, text, invert
        );
        let render = x > 0 || y > 0 || w > 0 || h > 0;
        if (render && (self.scheme.is_empty())) || text.is_empty() || self.font.is_none() {
            return 0;
        }
        let mut d: *mut XftDraw = null_mut();
        let mut ew: u32 = 0;
        let mut eh: u32 = 0;

        unsafe {
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

            let mut len = text.len();
            let max_buf_len = 1024;
            if len > 0 {
                Self::drw_font_gettexts(self.font.clone(), text, &mut ew, &mut eh, markup);
                let mut th = eh;
                // shorten text if necessary.
                len = len.min(max_buf_len);
                while len > 0 {
                    if ew <= w {
                        break;
                    }
                    Self::drw_font_gettexts(self.font.clone(), text, &mut ew, &mut eh, markup);
                    if eh > th {
                        th = eh;
                    }
                    len -= 1;
                }

                if len > 0 {
                    let mut buf = text[0..len].to_string();
                    if len < text.len() && len > 3 {
                        // drw "..."
                        buf.truncate(buf.len() - 3);
                        buf.push_str("...");
                    }

                    if render {
                        let ty = y + (h - th) as i32 / 2;
                        if markup {
                            self.font
                                .as_ref()
                                .unwrap()
                                .borrow_mut()
                                .layout
                                .as_ref()
                                .unwrap()
                                .set_markup(&buf);
                        } else {
                            self.font
                                .as_ref()
                                .unwrap()
                                .borrow_mut()
                                .layout
                                .as_ref()
                                .unwrap()
                                .set_text(&buf);
                        }
                        // (TODO) pango_xft_render_layout
                        let context = self
                            .font
                            .as_ref()
                            .unwrap()
                            .borrow_mut()
                            .layout
                            .as_ref()
                            .unwrap().context();
                        pangocairo::functions::show_layout(
                            &context,
                            &self
                                .font
                                .as_ref()
                                .unwrap()
                                .borrow_mut()
                                .layout
                                .as_ref()
                                .unwrap(),
                        );
                        if markup {
                            // clear markup attributes
                            self.font
                                .as_ref()
                                .unwrap()
                                .borrow_mut()
                                .layout
                                .as_ref()
                                .unwrap()
                                .set_attributes(None);
                        }
                    }
                    x += ew as i32;
                    w -= ew;
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
        if let Some(ref font_opt) = font {
            if font_opt.borrow_mut().layout.is_some() {
                // no need.
                // g_object_unref();
            }
        }
    }

    fn drw_font_gettexts(
        font: Option<Rc<RefCell<Fnt>>>,
        text: &str,
        w: &mut u32,
        h: &mut u32,
        markup: bool,
    ) {
        if font.is_none() || text.is_empty() {
            return;
        }
        let font = font.as_ref().unwrap().borrow_mut();

        if markup {
            font.layout.as_ref().unwrap().set_markup(text);
        } else {
            font.layout.as_ref().unwrap().set_text(text);
        }
        let (_, r) = font.layout.as_ref().unwrap().extents();
        *w = (r.width() / PANGO_SCALE) as u32;
        *h = (r.height() / PANGO_SCALE) as u32;
    }
}
