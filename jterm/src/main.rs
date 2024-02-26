use std::{ffi::CString, ptr::null};

use x11::xlib::{
    Colormap, Display, ExposureMask, KeyPressMask, KeyReleaseMask, ParentRelative, Window,
    XConnectionNumber, XDefaultScreen, XFontStruct, XLoadQueryFont, XOpenDisplay, XRootWindow,
    XSetWindowAttributes, XTextWidth, GC,
};

#[derive(Default)]
struct PTY {
    master: i64,
    slave: i64,
}

struct X11 {
    fd: i64,
    dpy: *mut Display,
    screen: i64,
    root: Window,

    termwin: Window,
    termgc: GC,
    col_fg: u64,
    col_bg: u64,
    w: i64,
    h: i64,

    xfont: *mut XFontStruct,
    font_width: i64,
    font_height: i64,

    buf: Vec<char>,
    buf_w: i64,
    buf_h: i64,
    buf_x: i64,
    buf_y: i64,
}

impl X11 {
    fn new() -> Self {
        X11 {
            fd: 0,
            dpy: std::ptr::null_mut(),
            screen: 0,
            root: 0,

            termwin: 0,
            termgc: std::ptr::null_mut(),
            col_fg: 0,
            col_bg: 0,
            w: 0,
            h: 0,

            xfont: std::ptr::null_mut(),
            font_width: 0,
            font_height: 0,
            buf: vec![],
            buf_w: 0,
            buf_h: 0,
            buf_x: 0,
            buf_y: 0,
        }
    }
    fn x11_setup(&mut self) -> bool {
        let wa: XSetWindowAttributes = XSetWindowAttributes {
            background_pixmap: ParentRelative as u64,
            background_pixel: 0,
            border_pixmap: 0,
            border_pixel: 0,
            bit_gravity: 0,
            win_gravity: 0,
            backing_store: 0,
            backing_planes: 0,
            backing_pixel: 0,
            save_under: 0,
            event_mask: KeyPressMask | KeyReleaseMask | ExposureMask,
            do_not_propagate_mask: 0,
            override_redirect: 0,
            colormap: 0,
            cursor: 0,
        };
        self.dpy = unsafe { XOpenDisplay(std::ptr::null()) };
        if self.dpy.is_null() {
            println!("Cannot open display");
            return false;
        }

        self.screen = unsafe { XDefaultScreen(self.dpy) as i64 };
        self.root = unsafe { XRootWindow(self.dpy, self.screen.try_into().unwrap()) };
        self.fd = unsafe { XConnectionNumber(self.dpy).into() };

        self.xfont = unsafe {
            let c_string_ptr = CString::new("fixed").expect("new failed");
            XLoadQueryFont(self.dpy, c_string_ptr.as_ptr())
        };
        if self.xfont.is_null() {
            println!("Could not load font");
            return false;
        }
        self.font_width = unsafe {
            let c_string_ptr = CString::new("fixed").expect("new failed");
            XTextWidth(self.xfont, c_string_ptr.as_ptr(), 1).into()
        };
        true
    }
}

fn main() {
    println!("Hello, world!");
    let mut x11 = X11::new();
    x11.x11_setup();
}
