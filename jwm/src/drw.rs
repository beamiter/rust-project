#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
// #![allow(unused_mut)]

use cairo::ffi::{
    cairo_create, cairo_destroy, cairo_surface_destroy, cairo_xlib_surface_create,
};
use log::info;
use pango::ffi::pango_font_description_free;
use pangocairo::ffi::pango_cairo_create_layout;
use std::{cell::RefCell, ffi::CString, i32, ptr::null_mut, rc::Rc, u32, usize};

// use log::info;
// use log::warn;
// For whatever reason, dmenu and other suckless tools use libXft, which does not support Unicode properly.
// If you use Pango however, Unicode will work great and this includes flag emojis.
use cairo::ffi::cairo_t;
use pango::{
    ffi::{
        pango_font_description_from_string,
        pango_layout_set_font_description,
        PangoLayout,
    },
    glib::gobject_ffi::g_object_unref,
};
use x11::{
    xft::{XftColor, XftColorAllocName},
    xlib::{
        self, CapButt, Colormap, Cursor, Drawable, JoinMiter, LineSolid, Visual, Window,
        XCreateFontCursor, XCreateGC, XCreatePixmap, XFreeCursor, XFreeGC,
        XFreePixmap, XSetLineAttributes, GC,
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

#[derive(Debug)]
pub struct Fnt {
    pub dpy: *mut xlib::Display,
    pub h: u32,
    pub layout: *mut PangoLayout,
    pub cr: *mut cairo_t,
}
impl Fnt {
    pub fn new() -> Self {
        Fnt {
            dpy: null_mut(),
            h: 0,
            layout: null_mut(),
            cr: null_mut(),
        }
    }
}

#[repr(C)]
#[allow(dead_code)]
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
    visual: *mut Visual,
    depth: i32,
    cmap: Colormap,
    pub drawable: Drawable,
    pub gc: GC,
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
            visual: null_mut(),
            depth: 0,
            cmap: 0,
            drawable: 0,
            gc: null_mut(),
            font: None,
        }
    }
    pub fn drw_create(
        dpy: *mut xlib::Display,
        screen: i32,
        root: Window,
        w: u32,
        h: u32,
        visual: *mut Visual,
        depth: i32,
        cmap: Colormap,
    ) -> Self {
        let mut drw = Drw::new();
        drw.dpy = dpy;
        drw.screen = screen;
        drw.root = root;
        drw.w = w;
        drw.h = h;
        unsafe {
            drw.drawable = XCreatePixmap(dpy, root, w, h, depth as u32);
            drw.gc = XCreateGC(dpy, drw.drawable, 0, null_mut());
            XSetLineAttributes(dpy, drw.gc, 1, LineSolid, CapButt, JoinMiter);
            drw.visual = visual;
            drw.depth = depth;
            drw.cmap = cmap;
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
            self.drawable = XCreatePixmap(self.dpy, self.root, w, h, self.depth as u32);
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
            return None;
        }

        let mut font = Fnt::new();
        font.dpy = self.dpy;

        unsafe {
            let surface =
                cairo_xlib_surface_create(self.dpy, self.drawable, self.visual, 3000, 200);
            let cr = cairo_create(surface);
            let layout = pango_cairo_create_layout(cr);
            let cstring = CString::new(fontname);
            if let Err(e) = cstring {
                info!("[xfont_create] an error occured: {}", e);
                return None;
            }
            let cstring = cstring.expect("fail to convert");
            let desc = pango_font_description_from_string(cstring.as_ptr());
            if desc.is_null() {
                println!("fail to parse font description");
                return None;
            } else {
                println!("parse font description succeed {}", fontname);
            }
            pango_layout_set_font_description(layout, desc);

            font.layout = layout;
            font.cr = cr;
            font.h = 20;

            // g_object_unref(layout as *mut _);
            pango_font_description_free(desc);
            cairo_surface_destroy(surface);
            // cairo_destroy(cr);
        }

        return Some(Rc::new(RefCell::new(font)));
    }
    pub fn drw_font_create(&mut self, font: &str) -> bool {
        if font.is_empty() {
            return false;
        }

        let fnt = self.xfont_create(font);
        self.font = fnt;
        return self.font.is_some();
    }
    pub fn drw_font_free(&self, font: Option<Rc<RefCell<Fnt>>>) {
        if font.is_some() {
            Self::xfont_free(font);
        }
    }

    pub fn drw_clr_create(&mut self, clrname: &str, alpha: u8) -> Option<Rc<Clr>> {
        if clrname.is_empty() {
            return None;
        }

        unsafe {
            let cstring = CString::new(clrname);
            if let Err(e) = cstring {
                info!("[drw_clr_create] an error occured: {}", e);
                return None;
            }
            let cstring = cstring.expect("fail to convert");
            let mut dest: Clr = std::mem::zeroed();
            if XftColorAllocName(
                self.dpy,
                self.visual,
                self.cmap,
                cstring.as_ptr(),
                &mut dest,
            ) <= 0
            {
                eprintln!("error, cannot allocate color: {}", clrname);
                return None;
            }
            dest.pixel = (dest.pixel & 0x00ffffffu64) | ((alpha as u64) << 24);
            return Some(Rc::new(dest));
        }
    }
    pub fn drw_scm_create(
        &mut self,
        clrnames: &[&'static str; 3],
        alphas: &[u8; 3],
        clrcount: usize,
    ) -> Vec<Option<Rc<Clr>>> {
        // Need at least two colors for a scheme.
        if clrnames.is_empty() || clrcount < 2 {
            return vec![];
        }
        let mut ret: Vec<Option<Rc<Clr>>> = vec![];
        for i in 0..clrcount {
            let clrname = clrnames[i];
            let alpha = alphas[i];
            let one_ret = self.drw_clr_create(clrname, alpha);
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

    // Drawing functions.

    fn xfont_free(font: Option<Rc<RefCell<Fnt>>>) {
        if let Some(ref font_opt) = font {
            if !font_opt.borrow_mut().layout.is_null() {
                unsafe {
                    g_object_unref(font_opt.borrow_mut().layout as *mut _);
                    cairo_destroy(font_opt.borrow_mut().cr);
                }
            }
        }
    }
}
