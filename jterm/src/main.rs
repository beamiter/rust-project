use std::ffi::CString;
use x11::xlib::{
    CWBackPixmap, CWEventMask, CopyFromParent, Display, ExposureMask, KeyPressMask, KeyReleaseMask,
    ParentRelative, Window, XAllocNamedColor, XColor, XConnectionNumber, XCreateGC, XCreateWindow,
    XDefaultColormap, XDefaultDepth, XDefaultScreen, XDefaultVisual, XFontStruct, XLoadQueryFont,
    XMapWindow, XOpenDisplay, XRootWindow, XSetWindowAttributes, XStoreName, XSync, XTextWidth, GC,
};

#[allow(dead_code)]
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
        let mut wa: XSetWindowAttributes = XSetWindowAttributes {
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
        println!("Open display");

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
        println!("Load font");
        self.font_width = unsafe {
            let c_string_ptr = CString::new("m").expect("new failed");
            XTextWidth(self.xfont, c_string_ptr.as_ptr(), 1).into()
        };
        self.font_height = unsafe { ((*self.xfont).ascent + (*self.xfont).descent) as i64 };

        let cmap = unsafe { XDefaultColormap(self.dpy, self.screen.try_into().unwrap()) };

        let mut color: XColor = XColor {
            pixel: (0),
            red: (0),
            green: (0),
            blue: (0),
            flags: (0),
            pad: (0),
        };
        unsafe {
            let c_string_ptr = CString::new("#000000").expect("new failed");
            if XAllocNamedColor(
                self.dpy,
                cmap,
                c_string_ptr.as_ptr(),
                &mut color,
                &mut color,
            ) < 0
            {
                println!("Could not load bg color");
                return false;
            }
        }
        println!("Load bg color");
        self.col_bg = color.pixel;

        unsafe {
            let c_string_ptr = CString::new("#aaaaaa").expect("new failed");
            if XAllocNamedColor(
                self.dpy,
                cmap,
                c_string_ptr.as_ptr(),
                &mut color,
                &mut color,
            ) < 0
            {
                println!("Could not load fg color");
                return false;
            }
        }
        println!("Load fg color");
        self.col_fg = color.pixel;

        self.buf_w = 80;
        self.buf_h = 25;
        self.buf_x = 0;
        self.buf_y = 0;
        self.buf = vec!['1'; (self.buf_w * self.buf_h).try_into().unwrap()];
        if self.buf.is_empty() {
            println!("calloc");
            return false;
        }

        self.w = self.buf_w * self.font_width;
        self.h = self.buf_h * self.font_height;

        self.termwin = unsafe {
            XCreateWindow(
                self.dpy,
                self.root,
                0,
                0,
                self.w.try_into().unwrap(),
                self.h.try_into().unwrap(),
                0,
                XDefaultDepth(self.dpy, self.screen.try_into().unwrap()),
                CopyFromParent.try_into().unwrap(),
                XDefaultVisual(self.dpy, self.screen.try_into().unwrap()),
                CWBackPixmap | CWEventMask,
                &mut wa,
            )
        };
        println!("Create Window");
        unsafe {
            let c_string_ptr = CString::new("eduterm").expect("new failed");
            XStoreName(self.dpy, self.termwin, c_string_ptr.as_ptr());
        }
        println!("Store name");
        unsafe {
            XMapWindow(self.dpy, self.termwin);
        }
        println!("Map window");
        self.termgc = unsafe { XCreateGC(self.dpy, self.termwin, 0, std::ptr::null_mut()) };
        println!("Create GC");

        unsafe {
            XSync(self.dpy, 0);
        }
        println!("Sync");
        true
    }
}

fn main() {
    println!("Hello, world!");
    let mut x11 = X11::new();
    x11.x11_setup();
}
