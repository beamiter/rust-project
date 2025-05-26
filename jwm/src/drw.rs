#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
// #![allow(unused_mut)]

use log::info;
use std::{ffi::CString, i32, ptr::null_mut, rc::Rc, u32};

// use log::info;
// use log::warn;
// For whatever reason, dmenu and other suckless tools use libXft, which does not support Unicode properly.
// If you use Pango however, Unicode will work great and this includes flag emojis.
use x11::{
    xft::{XftColor, XftColorAllocName},
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

pub type Clr = XftColor;

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
    ) -> Vec<Option<Rc<Clr>>> {
        if clrnames.is_empty() {
            return vec![];
        }
        let mut ret: Vec<Option<Rc<Clr>>> = vec![];
        for i in 0..clrnames.len() {
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
}
