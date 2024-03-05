use nix::libc::{iscntrl, memmove};
use nix::unistd::read;
use nix::{
    libc::{
        close, dup2, execle, fd_set, fork, grantpt, ioctl, open, pid_t, posix_openpt, ptsname,
        select, setsid, unlockpt, FD_ISSET, FD_SET, FD_ZERO, O_NOCTTY, O_RDWR, TIOCSCTTY,
        TIOCSWINSZ,
    },
    pty::Winsize,
};
use std::ffi::{c_void, CString};
use std::i64;
use x11::xlib::{
    CWBackPixmap, CWEventMask, CopyFromParent, Display, ExposureMask, KeyPressMask, KeyReleaseMask,
    ParentRelative, Window, XAllocNamedColor, XColor, XConnectionNumber, XCreateGC, XCreateWindow,
    XDefaultColormap, XDefaultDepth, XDefaultScreen, XDefaultVisual, XDrawString, XEvent,
    XFillRectangle, XFontStruct, XLoadQueryFont, XMapWindow, XNextEvent, XOpenDisplay, XPending,
    XRootWindow, XSetForeground, XSetWindowAttributes, XStoreName, XSync, XTextWidth, GC,
};

const SHELL: &str = "/bin/dash";

#[allow(dead_code)]
#[derive(Default)]
struct PTY {
    master: i64,
    slave: i64,
}

#[allow(dead_code)]
impl PTY {
    fn new() -> Self {
        PTY {
            master: 0,
            slave: 0,
        }
    }
    unsafe fn pt_pair(&mut self) -> bool {
        self.master = posix_openpt(O_RDWR | O_NOCTTY) as i64;
        if self.master == -1 {
            println!("posix_openpt");
            return false;
        }
        if grantpt(self.master.try_into().unwrap()) == -1 {
            println!("grantpt");
            return false;
        }
        if unlockpt(self.master.try_into().unwrap()) == -1 {
            println!("grantpt");
            return false;
        }
        let slave_name = ptsname(self.master.try_into().unwrap());
        if slave_name.is_null() {
            println!("ptsname");
            return false;
        }

        self.slave = open(slave_name, O_RDWR | O_NOCTTY) as i64;
        if self.slave == -1 {
            println!("opne(slave_name)");
            return false;
        }
        true
    }

    unsafe fn spawn(&mut self) -> bool {
        let p: pid_t = fork();
        if p == 0 {
            close(self.master.try_into().unwrap());

            setsid();
            if ioctl(self.slave.try_into().unwrap(), TIOCSCTTY) == -1 {
                println!("ioctl(TIOCSCTTY)");
                return false;
            }

            dup2(self.slave.try_into().unwrap(), 0);
            dup2(self.slave.try_into().unwrap(), 1);
            dup2(self.slave.try_into().unwrap(), 2);
            close(self.slave.try_into().unwrap());

            let shell = CString::new(SHELL).unwrap();
            let arg0 = shell.clone();
            let term = CString::new("TERM=dumb").unwrap();
            let env = [term.as_ptr(), std::ptr::null()];

            // (TODO): May change the arg0.
            execle(
                shell.as_ptr(),
                arg0.as_ptr(),
                std::ptr::null::<c_void>(),
                env.as_ptr(),
            );
            return false;
        }
        if p > 0 {
            close(self.slave.try_into().unwrap());
        }

        println!("fork");
        false
    }
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

    buf: Vec<u8>,
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
        self.buf = vec![b'1'; (self.buf_w * self.buf_h).try_into().unwrap()];
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

    fn x11_redraw(&mut self) {
        let mut x: i64 = 0;
        let mut y: i64 = 0;
        let mut buf: [u8; 1] = [0];

        unsafe {
            XSetForeground(self.dpy, self.termgc, self.col_bg);
            XFillRectangle(
                self.dpy,
                self.termwin,
                self.termgc,
                0,
                0,
                self.w.try_into().unwrap(),
                self.h.try_into().unwrap(),
            );

            XSetForeground(self.dpy, self.termgc, self.col_fg);
        }
        for y in 0..self.buf_h {
            for x in 0..self.buf_w {
                buf[0] = self.buf[(y * self.buf_w + x) as usize];
                unsafe {
                    if iscntrl(buf[0].into()) > 0 {
                        XDrawString(
                            self.dpy,
                            self.termwin,
                            self.termgc,
                            (x * self.font_width).try_into().unwrap(),
                            (y * self.font_height + (*self.xfont).ascent as i64)
                                .try_into()
                                .unwrap(),
                            buf.as_mut_ptr() as *mut i8,
                            1,
                        );
                    }
                }
            }
        }

        unsafe {
            XSetForeground(self.dpy, self.termgc, self.col_fg);
            XFillRectangle(
                self.dpy,
                self.termwin,
                self.termgc,
                (self.buf_x * self.font_width).try_into().unwrap(),
                (self.buf_y * self.font_height).try_into().unwrap(),
                self.font_width.try_into().unwrap(),
                self.font_height.try_into().unwrap(),
            );
            XSync(self.dpy, 0);
        }
    }
}

fn term_set_size(pty: &mut PTY, x11: &mut X11) -> bool {
    let ws: Winsize = Winsize {
        ws_col: x11.buf_w as u16,
        ws_row: x11.buf_h as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe {
        if ioctl(pty.master.try_into().unwrap(), TIOCSWINSZ, ws) == -1 {
            println!("ioctl(TIOCSWINSZ)");
            return false;
        }
    }
    false
}

fn run(pty: &mut PTY, x11: &mut X11) -> i32 {
    let maxfd = if pty.master > x11.fd {
        pty.master
    } else {
        x11.fd
    };
    let mut buf: [u8; 1] = [0];
    let mut readable: fd_set = unsafe { std::mem::zeroed() };
    let mut just_wrapped: bool = false;
    let mut ev: XEvent;
    loop {
        unsafe {
            FD_ZERO(&mut readable);
            FD_SET(pty.master.try_into().unwrap(), &mut readable);
            FD_SET(x11.fd.try_into().unwrap(), &mut readable);

            if select(
                (maxfd + 1).try_into().unwrap(),
                &mut readable,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == -1
            {
                println!("select");
                return 1;
            }

            if FD_ISSET(pty.master.try_into().unwrap(), &mut readable) {
                match read(pty.master.try_into().unwrap(), &mut buf) {
                    Ok(bytes_read) => {
                        println!("Read {} byres", bytes_read);
                    }
                    Err(e) => {
                        eprintln!("Failed to read: {}", e);
                        return 1;
                    }
                }

                if buf[0] == b'\r' {
                    x11.buf_x = 0;
                } else {
                    if buf[0] != b'\n' {
                        x11.buf[(x11.buf_y * x11.buf_w + x11.buf_x) as usize] = buf[0];
                        x11.buf_x += 1;

                        if x11.buf_x >= x11.buf_w {
                            x11.buf_x = 0;
                            x11.buf_y += 1;
                            just_wrapped = true;
                        } else {
                            just_wrapped = false;
                        }
                    } else if !just_wrapped {
                        x11.buf_y += 1;
                        just_wrapped = false;
                    }

                    if x11.buf_y >= x11.buf_h {
                        memmove(
                            x11.buf.as_mut_ptr() as *mut c_void,
                            x11.buf[x11.buf_w as usize] as *const u8 as *const c_void,
                            (x11.buf_w * (x11.buf_h - 1)).try_into().unwrap(),
                        );

                        for i in 0..x11.buf_w {
                            x11.buf[(x11.buf_y * x11.buf_w + i) as usize] = 0;
                        }
                    }
                }

                x11.x11_redraw();
            }

            if FD_ISSET(x11.fd.try_into().unwrap(), &mut readable) {
                while XPending(x11.dpy) > 0 {
                    XNextEvent(x11.dpy, &mut ev);
                }
            }

            break;
        }
    }

    0
}

fn main() {
    println!("Hello, world!");
    let mut x11 = X11::new();
    x11.x11_setup();
    let mut pty = PTY::new();
    unsafe {
        pty.pt_pair();
    }

    if !term_set_size(&mut pty, &mut x11) {}
    unsafe { if !pty.spawn() {} }
}
