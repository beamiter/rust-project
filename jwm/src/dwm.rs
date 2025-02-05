#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

use libc::{
    close, exit, fork, setsid, sigaction, sigemptyset, waitpid, SA_NOCLDSTOP, SA_NOCLDWAIT,
    SA_RESTART, SIGCHLD, SIG_DFL, SIG_IGN, WNOHANG,
};
use log::info;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, CStr, CString};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::mem::transmute;
use std::mem::zeroed;
use std::process::Command;
use std::ptr::{addr_of_mut, null, null_mut};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::{os::raw::c_long, usize};
use x11::xinerama::{XineramaIsActive, XineramaQueryScreens, XineramaScreenInfo};
use x11::xrender::{PictTypeDirect, XRenderFindVisualFormat};

use x11::keysym::XK_Num_Lock;
use x11::xlib::{
    AllocNone, AnyButton, AnyKey, AnyModifier, Atom, BadAccess, BadDrawable, BadLength, BadMatch,
    BadWindow, Below, ButtonPress, ButtonPressMask, ButtonRelease, ButtonReleaseMask, CWBackPixel,
    CWBorderPixel, CWBorderWidth, CWColormap, CWCursor, CWEventMask, CWHeight, CWOverrideRedirect,
    CWSibling, CWStackMode, CWWidth, ClientMessage, Colormap, ConfigureNotify, ConfigureRequest,
    CurrentTime, DestroyAll, DestroyNotify, Display, EnterNotify, EnterWindowMask, Expose,
    ExposureMask, False, FocusChangeMask, FocusIn, GrabModeAsync, GrabModeSync, GrabSuccess,
    InputHint, InputOutput, IsViewable, KeyPress, KeySym, LeaveWindowMask, LockMask, MapRequest,
    MappingKeyboard, MappingNotify, MotionNotify, NoEventMask, NotifyInferior, NotifyNormal,
    PAspect, PBaseSize, PMaxSize, PMinSize, PResizeInc, PSize, PointerMotionMask, PointerRoot,
    PropModeAppend, PropModeReplace, PropertyChangeMask, PropertyDelete, PropertyNotify,
    ReplayPointer, RevertToPointerRoot, StructureNotifyMask, SubstructureNotifyMask,
    SubstructureRedirectMask, Success, Time, True, TrueColor, UnmapNotify, Visual, VisualClassMask,
    VisualDepthMask, VisualScreenMask, Window, XAllowEvents, XChangeProperty,
    XChangeWindowAttributes, XCheckMaskEvent, XClassHint, XConfigureEvent, XConfigureWindow,
    XConnectionNumber, XCreateColormap, XCreateSimpleWindow, XCreateWindow, XDefaultColormap,
    XDefaultDepth, XDefaultRootWindow, XDefaultScreen, XDefaultVisual, XDefineCursor,
    XDeleteProperty, XDestroyWindow, XDisplayHeight, XDisplayKeycodes, XDisplayWidth, XErrorEvent,
    XEvent, XFree, XFreeModifiermap, XGetClassHint, XGetKeyboardMapping, XGetModifierMapping,
    XGetTextProperty, XGetTransientForHint, XGetVisualInfo, XGetWMHints, XGetWMNormalHints,
    XGetWMProtocols, XGetWindowAttributes, XGetWindowProperty, XGrabButton, XGrabKey, XGrabPointer,
    XGrabServer, XInternAtom, XKeycodeToKeysym, XKeysymToKeycode, XKillClient, XMapRaised,
    XMapWindow, XMaskEvent, XMoveResizeWindow, XMoveWindow, XNextEvent, XQueryPointer, XQueryTree,
    XRaiseWindow, XRefreshKeyboardMapping, XRootWindow, XSelectInput, XSendEvent, XSetClassHint,
    XSetCloseDownMode, XSetErrorHandler, XSetInputFocus, XSetWMHints, XSetWindowAttributes,
    XSetWindowBorder, XSizeHints, XSync, XTextProperty, XUngrabButton, XUngrabKey, XUngrabPointer,
    XUngrabServer, XUnmapWindow, XUrgencyHint, XVisualInfo, XWarpPointer, XWindowAttributes,
    XWindowChanges, XmbTextPropertyToTextList, CWX, CWY, XA_ATOM, XA_CARDINAL, XA_STRING,
    XA_WINDOW, XA_WM_HINTS, XA_WM_NAME, XA_WM_NORMAL_HINTS, XA_WM_TRANSIENT_FOR,
};

use std::cmp::{max, min};

use crate::config::Config;
use crate::drw::{Clr, Col, Cur, Drw};
use crate::icon_gallery::generate_random_tags;
use crate::xproto::{
    IconicState, NormalState, WithdrawnState, XC_fleur, XC_left_ptr, XC_sizing, X_ConfigureWindow,
    X_CopyArea, X_GrabButton, X_GrabKey, X_PolyFillRectangle, X_PolySegment, X_PolyText8,
    X_SetInputFocus,
};

pub const BUTTONMASK: c_long = ButtonPressMask | ButtonReleaseMask;
pub const MOUSEMASK: c_long = BUTTONMASK | PointerMotionMask;

#[repr(C)]
#[derive(Debug, Clone)]
pub enum CUR {
    CurNormal = 0,
    CurResize = 1,
    CurMove = 2,
    CurLast = 3,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum SCHEME {
    SchemeNorm = 0,
    SchemeSel = 1,
    SchemeStatus = 2,
    SchemeTagsSel = 3,
    SchemeTagsNorm = 4,
    SchemeInfoSel = 5,
    SchemeInfoNorm = 6,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum NET {
    NetSupported = 0,
    NetWMName = 1,
    NetWMState = 2,
    NetWMCheck = 3,
    NetWMFullscreen = 4,
    NetActiveWindow = 5,
    NetWMWindowType = 6,
    NetWMWindowTypeDialog = 7,
    NetClientList = 8,
    NetClientInfo = 9,
    NetLast = 10,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum WM {
    WMProtocols = 0,
    WMDelete = 1,
    WMState = 2,
    WMTakeFocus = 3,
    WMLast = 4,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
pub enum CLICK {
    ClkTagBar = 0,
    ClkLtSymbol = 1,
    ClkStatusText = 2,
    ClkWinTitle = 3,
    ClkClientWin = 4,
    ClkRootWin = 5,
    ClkLast = 6,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    I(i32),
    Ui(u32),
    F(f32),
    V(Vec<String>),
    Lt(Rc<Layout>),
}

#[derive(Debug, Clone)]
pub struct Button {
    pub click: u32,
    pub mask: u32,
    pub button: u32,
    pub func: Option<fn(&mut Dwm, *const Arg)>,
    pub arg: Arg,
}
impl Button {
    #[allow(unused)]
    pub fn new(
        click: u32,
        mask: u32,
        button: u32,
        func: Option<fn(&mut Dwm, *const Arg)>,
        arg: Arg,
    ) -> Self {
        Self {
            click,
            mask,
            button,
            func,
            arg,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Key {
    pub mod0: u32,
    pub keysym: KeySym,
    pub func: Option<fn(&mut Dwm, *const Arg)>,
    pub arg: Arg,
}
impl Key {
    #[allow(unused)]
    pub fn new(
        mod0: u32,
        keysym: KeySym,
        func: Option<fn(&mut Dwm, *const Arg)>,
        arg: Arg,
    ) -> Self {
        Self {
            mod0,
            keysym,
            func,
            arg,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pertag {
    // current tag
    pub curtag: usize,
    // previous tag
    pub prevtag: usize,
    // number of windows in master area
    pub nmasters: [u32; Config::tags_length + 1],
    // mfacts per tag
    pub mfacts: [f32; Config::tags_length + 1],
    // selected layouts
    pub sellts: [usize; Config::tags_length + 1],
    // matrix of tags and layouts indexes
    ltidxs: [[Option<Rc<Layout>>; 2]; Config::tags_length + 1],
    // display bar for the current tag
    pub showbars: [bool; Config::tags_length + 1],
    // selected client
    pub sel: [Option<Rc<RefCell<Client>>>; Config::tags_length + 1],
}
impl Pertag {
    pub fn new() -> Self {
        Self {
            curtag: 0,
            prevtag: 0,
            nmasters: [0; Config::tags_length + 1],
            mfacts: [0.; Config::tags_length + 1],
            sellts: [0; Config::tags_length + 1],
            ltidxs: unsafe { zeroed() },
            showbars: [false; Config::tags_length + 1],
            sel: unsafe { zeroed() },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutType {
    TypeTile,
    TypeFloat,
    TypeMonocle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub symbol: &'static str,
    pub layout_type: Option<LayoutType>,
}
impl Layout {
    #[allow(unused)]
    pub fn new(symbol: &'static str, layout_type: Option<LayoutType>) -> Self {
        Self {
            symbol,
            layout_type,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub name: String,
    pub mina: f32,
    pub maxa: f32,
    pub cfact: f32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub oldx: i32,
    pub oldy: i32,
    pub oldw: i32,
    pub oldh: i32,
    pub basew: i32,
    pub baseh: i32,
    pub incw: i32,
    pub inch: i32,
    pub maxw: i32,
    pub maxh: i32,
    pub minw: i32,
    pub minh: i32,
    pub hintsvalid: bool,
    pub bw: i32,
    pub oldbw: i32,
    pub tags0: u32,
    pub isfixed: bool,
    pub isfloating: bool,
    pub isurgent: bool,
    pub nerverfocus: bool,
    pub oldstate: bool,
    pub isfullscreen: bool,
    pub next: Option<Rc<RefCell<Client>>>,
    pub snext: Option<Rc<RefCell<Client>>>,
    pub mon: Option<Rc<RefCell<Monitor>>>,
    pub win: Window,
}
impl fmt::Display for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Client {{ name: {}, mina: {}, maxa: {}, cfact: {}, x: {}, y: {}, w: {}, h: {}, oldx: {}, oldy: {}, oldw: {}, oldh: {}, basew: {}, baseh: {}, incw: {}, inch: {}, maxw: {}, maxh: {}, minw: {}, minh: {}, hintsvalid: {}, bw: {}, oldbw: {}, tags0: {}, isfixed: {}, isfloating: {}, isurgent: {}, nerverfocus: {}, oldstate: {}, isfullscreen: {} }}",
    self.name,
    self.mina,
    self.maxa,
    self.cfact,
    self.x,
    self.y,
    self.w,
    self.h,
    self.oldx,
    self.oldy,
    self.oldw,
    self.oldh,
    self.basew,
    self.baseh,
    self.incw,
    self.inch,
    self.maxw,
    self.maxh,
    self.minw,
    self.minh,
    self.hintsvalid,
    self.bw,
    self.oldbw,
    self.tags0,
    self.isfixed,
    self.isfloating,
    self.isurgent,
    self.nerverfocus,
    self.oldstate,
    self.isfullscreen
        )
    }
}
impl Client {
    #[allow(unused)]
    pub fn new() -> Self {
        Self {
            name: String::new(),
            mina: 0.,
            maxa: 0.,
            cfact: 0.,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            oldx: 0,
            oldy: 0,
            oldw: 0,
            oldh: 0,
            basew: 0,
            baseh: 0,
            incw: 0,
            inch: 0,
            maxw: 0,
            maxh: 0,
            minw: 0,
            minh: 0,
            hintsvalid: false,
            bw: 0,
            oldbw: 0,
            tags0: 0,
            isfixed: false,
            isfloating: false,
            isurgent: false,
            nerverfocus: false,
            oldstate: false,
            isfullscreen: false,
            next: None,
            snext: None,
            mon: None,
            win: 0,
        }
    }
    pub fn isvisible(&self) -> bool {
        // info!("[ISVISIBLE]");
        let b = {
            let seltags = self.mon.as_ref().unwrap().borrow_mut().seltags;
            self.tags0 & self.mon.as_ref().unwrap().borrow_mut().tagset[seltags]
        };
        b > 0
    }
    pub fn width(&self) -> i32 {
        self.w + 2 * self.bw
    }
    pub fn height(&self) -> i32 {
        self.h + 2 * self.bw
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub ltsymbol: String,
    pub mfact0: f32,
    pub nmaster0: u32,
    pub num: i32,
    pub by: i32,
    pub mx: i32,
    pub my: i32,
    pub mw: i32,
    pub mh: i32,
    pub wx: i32,
    pub wy: i32,
    pub ww: i32,
    pub wh: i32,
    pub seltags: usize,
    pub sellt: usize,
    pub tagset: [u32; 2],
    pub showbar0: bool,
    pub topbar0: bool,
    pub clients: Option<Rc<RefCell<Client>>>,
    pub sel: Option<Rc<RefCell<Client>>>,
    pub stack: Option<Rc<RefCell<Client>>>,
    pub next: Option<Rc<RefCell<Monitor>>>,
    pub barwin: Window,
    pub lt: [Rc<Layout>; 2],
    pub pertag: Option<Pertag>,
}
impl Monitor {
    #[allow(unused)]
    pub fn new() -> Self {
        Self {
            ltsymbol: String::new(),
            mfact0: 0.0,
            nmaster0: 0,
            num: 0,
            by: 0,
            mx: 0,
            my: 0,
            mw: 0,
            mh: 0,
            wx: 0,
            wy: 0,
            ww: 0,
            wh: 0,
            seltags: 0,
            sellt: 0,
            tagset: [0; 2],
            showbar0: false,
            topbar0: false,
            clients: None,
            sel: None,
            stack: None,
            next: None,
            barwin: 0,
            lt: [
                Rc::new(Layout {
                    symbol: "",
                    layout_type: None,
                }),
                Rc::new(Layout {
                    symbol: "",
                    layout_type: None,
                }),
            ],
            pertag: None,
        }
    }
    pub fn intersect(&self, x: i32, y: i32, w: i32, h: i32) -> i32 {
        max(0, min(x + w, self.wx + self.ww) - max(x, self.wx))
            * max(0, min(y + h, self.wy + self.wh) - max(y, self.wy))
    }
}
impl fmt::Display for Monitor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Monitor {{ ltsymbol: {}, mfact0: {}, nmaster0: {}, num: {}, by: {}, mx: {}, my: {}, mw: {}, mh: {}, wx: {}, wy: {}, ww: {}, wh: {}, seltags: {}, sellt: {}, tagset: [{}, {}], showbar0: {}, topbar0: {},  barwin: {}}}",
               self.ltsymbol,
               self.mfact0,
               self.nmaster0,
               self.num,
               self.by,
               self.mx,
               self.my,
               self.mw,
               self.mh,
               self.wx,
               self.wy,
               self.ww,
               self.wh,
               self.seltags,
               self.sellt,
               self.tagset[0],
               self.tagset[1],
               self.showbar0,
               self.topbar0,
               self.barwin,
        )
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub class: &'static str,
    pub instance: &'static str,
    pub title: &'static str,
    pub tags0: usize,
    pub isfloating: bool,
    pub monitor: i32,
}
impl Rule {
    #[allow(unused)]
    pub fn new(
        class: &'static str,
        instance: &'static str,
        title: &'static str,
        tags0: usize,
        isfloating: bool,
        monitor: i32,
    ) -> Self {
        Rule {
            class,
            instance,
            title,
            tags0,
            isfloating,
            monitor,
        }
    }
}

static mut xerrorxlib: Option<unsafe extern "C" fn(*mut Display, *mut XErrorEvent) -> c_int> = None;
pub fn xerrorstart(_: *mut Display, _: *mut XErrorEvent) -> i32 {
    // info!("[xerrorstart]");
    eprintln!("jwm: another window manager is already running");
    unsafe {
        exit(1);
    }
}
// There's no way to check accesses to destroyed windows, thus those cases are ignored (especially
// on UnmapNotify's). Other types of errors call xlibs default error handler, which may call exit.
pub fn xerror(_: *mut Display, ee: *mut XErrorEvent) -> i32 {
    // info!("[xerror]");
    let hack_request_code: u8 = 139;
    unsafe {
        if (*ee).error_code == BadWindow
            || ((*ee).request_code == X_SetInputFocus && (*ee).error_code == BadMatch)
            || ((*ee).request_code == X_PolyText8 && (*ee).error_code == BadDrawable)
            || ((*ee).request_code == X_PolyFillRectangle && (*ee).error_code == BadDrawable)
            || ((*ee).request_code == X_PolySegment && (*ee).error_code == BadDrawable)
            || ((*ee).request_code == X_ConfigureWindow && (*ee).error_code == BadMatch)
            || ((*ee).request_code == X_GrabButton && (*ee).error_code == BadAccess)
            || ((*ee).request_code == X_GrabKey && (*ee).error_code == BadAccess)
            || ((*ee).request_code == X_CopyArea && (*ee).error_code == BadDrawable)
            || ((*ee).request_code == hack_request_code && (*ee).error_code == BadLength)
        {
            return 0;
        }
        info!(
            "jwm: fatal error: request code = {}, error code = {}",
            (*ee).request_code,
            (*ee).error_code
        );
        // (TODO): check here, may call exit.
        return -1;
    }
}
pub fn xerrordummy(_: *mut Display, _: *mut XErrorEvent) -> i32 {
    // info!("[xerrordummy]");
    0
}

#[derive(Debug)]
pub struct Dwm {
    pub broken: String,
    pub stext_max_len: usize,
    pub stext: String,
    pub screen: i32,
    pub sw: i32,
    pub sh: i32,
    pub bh: i32,
    pub vp: i32, // vertical padding for bar
    pub sp: i32, // side padding for bar
    pub numlockmask: u32,
    pub wmatom: [Atom; WM::WMLast as usize],
    pub netatom: [Atom; NET::NetLast as usize],
    pub running: AtomicBool,
    pub cursor: [Option<Box<Cur>>; CUR::CurLast as usize],
    pub scheme: Vec<Vec<Option<Rc<Clr>>>>,
    pub dpy: *mut Display,
    pub drw: Option<Box<Drw>>,
    pub mons: Option<Rc<RefCell<Monitor>>>,
    pub motionmon: Option<Rc<RefCell<Monitor>>>,
    pub selmon: Option<Rc<RefCell<Monitor>>>,
    pub root: Window,
    pub wmcheckwin: Window,
    pub useargb: bool,
    pub visual: *mut Visual,
    pub depth: i32,
    pub cmap: Colormap,
    pub sender: Sender<u8>,
    pub tags: Vec<&'static str>,
    pub pipe: Option<File>,
}

#[derive(Debug)]
enum TextElement {
    WithCaret(String),
    WithoutCaret(String),
}

impl Dwm {
    fn handler(&mut self, key: i32, e: *mut XEvent) {
        match key {
            ButtonPress => self.buttonpress(e),
            ClientMessage => self.clientmessage(e),
            ConfigureRequest => self.configurerequest(e),
            ConfigureNotify => self.configurenotify(e),
            DestroyNotify => self.destroynotify(e),
            EnterNotify => self.enternotify(e),
            Expose => self.expose(e),
            FocusIn => self.focusin(e),
            KeyPress => self.keypress(e),
            MappingNotify => self.mappingnotify(e),
            MapRequest => self.maprequest(e),
            MotionNotify => self.motionnotify(e),
            PropertyNotify => self.propertynotify(e),
            UnmapNotify => self.unmapnotify(e),
            _ => {
                // info!("Unsupported event type: {}", key)
            }
        }
    }
    pub fn new(sender: Sender<u8>, pipe_path: String) -> Self {
        Dwm {
            broken: "broken".to_string(),
            stext_max_len: 512,
            stext: String::new(),
            screen: 0,
            sw: 0,
            sh: 0,
            bh: 0,
            vp: 0,
            sp: 0,
            numlockmask: 0,
            wmatom: [0; WM::WMLast as usize],
            netatom: [0; NET::NetLast as usize],
            running: AtomicBool::new(true),
            cursor: [const { None }; CUR::CurLast as usize],
            scheme: vec![],
            dpy: null_mut(),
            drw: None,
            mons: None,
            motionmon: None,
            selmon: None,
            root: 0,
            wmcheckwin: 0,
            useargb: false,
            visual: null_mut(),
            depth: 0,
            cmap: 0,
            sender,
            tags: generate_random_tags(Config::tags_length),
            pipe: if pipe_path.is_empty() {
                None
            } else {
                Some(File::create(pipe_path).unwrap())
            },
        }
    }

    fn are_equal_rc<T>(a: &Option<Rc<RefCell<T>>>, b: &Option<Rc<RefCell<T>>>) -> bool {
        match (a, b) {
            (Some(rc_a), Some(rc_b)) => Rc::ptr_eq(rc_a, rc_b),
            _ => false,
        }
    }

    fn CLEANMASK(&self, mask: u32) -> u32 {
        mask & !(self.numlockmask | LockMask)
            & (x11::xlib::ShiftMask
                | x11::xlib::ControlMask
                | x11::xlib::Mod1Mask
                | x11::xlib::Mod2Mask
                | x11::xlib::Mod3Mask
                | x11::xlib::Mod4Mask
                | x11::xlib::Mod5Mask)
    }
    // function declarations and implementations.
    pub fn applyrules(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[applyrules]");
        unsafe {
            // rule matching
            let mut c = c.borrow_mut();
            c.isfloating = false;
            c.tags0 = 0;
            let mut ch: XClassHint = zeroed();
            XGetClassHint(self.dpy, c.win, &mut ch);
            let class = if !ch.res_class.is_null() {
                let c_str = CStr::from_ptr(ch.res_class);
                c_str.to_str().unwrap()
            } else {
                &self.broken
            };
            let instance = if !ch.res_name.is_null() {
                let c_str = CStr::from_ptr(ch.res_name);
                c_str.to_str().unwrap()
            } else {
                &self.broken
            };
            // info!(
            //     "[applyrules] class: {}, instance: {}, name: {}",
            //     class, instance, c.name
            // );

            for r in &*Config::rules {
                if (!r.title.is_empty() && c.name.find(r.title).is_some())
                    || (!r.class.is_empty() && class.find(r.class).is_some())
                    || (!r.instance.is_empty() && instance.find(r.instance).is_some())
                {
                    c.isfloating = r.isfloating;
                    c.tags0 |= r.tags0 as u32;
                    let mut m = self.mons.clone();
                    while let Some(ref m_opt) = m {
                        if m_opt.borrow_mut().num == r.monitor {
                            break;
                        }
                        let next = m_opt.borrow_mut().next.clone();
                        m = next;
                    }
                    if m.is_some() {
                        c.mon = m.clone();
                    }
                }
            }
            if !ch.res_class.is_null() {
                XFree(ch.res_class as *mut _);
            }
            if !ch.res_name.is_null() {
                XFree(ch.res_name as *mut _);
            }
            let condition = c.tags0 & Config::tagmask;
            c.tags0 = if condition > 0 {
                condition
            } else {
                let seltags = { c.mon.as_ref().unwrap().borrow_mut().seltags };
                c.mon.as_ref().unwrap().borrow_mut().tagset[seltags]
            }
        }
    }

    pub fn updatesizehints(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[updatesizehints]");
        let mut c = c.as_ref().borrow_mut();
        unsafe {
            let mut size: XSizeHints = zeroed();

            let mut msize: i64 = 0;
            if XGetWMNormalHints(self.dpy, c.win, &mut size, &mut msize) <= 0 {
                size.flags = PSize;
            }
            if size.flags & PBaseSize > 0 {
                c.basew = size.base_width;
                c.baseh = size.base_height;
            } else if size.flags & PMinSize > 0 {
                c.basew = size.min_width;
                c.baseh = size.min_height;
            } else {
                c.basew = 0;
                c.baseh = 0;
            }
            if size.flags & PResizeInc > 0 {
                c.incw = size.width_inc;
                c.inch = size.height_inc;
            } else {
                c.incw = 0;
                c.inch = 0;
            }
            if size.flags & PMaxSize > 0 {
                c.maxw = size.max_width;
                c.maxh = size.max_height;
            } else {
                c.maxw = 0;
                c.maxh = 0;
            }
            if size.flags & PMinSize > 0 {
                c.minw = size.min_width;
                c.minh = size.min_height;
            } else if size.flags & PBaseSize > 0 {
                c.minw = size.base_width;
                c.minh = size.base_height;
            } else {
                c.minw = 0;
                c.minh = 0;
            }
            if size.flags & PAspect > 0 {
                c.mina = size.min_aspect.y as f32 / size.min_aspect.x as f32;
                c.maxa = size.max_aspect.x as f32 / size.max_aspect.y as f32;
            } else {
                c.maxa = 0.;
                c.mina = 0.;
            }
            c.isfixed = c.maxw > 0 && c.maxh > 0 && (c.maxw == c.minw) && (c.maxh == c.minh);
            c.hintsvalid = true;
        }
    }

    pub fn applysizehints(
        &mut self,
        c: &Rc<RefCell<Client>>,
        x: &mut i32,
        y: &mut i32,
        w: &mut i32,
        h: &mut i32,
        interact: bool,
    ) -> bool {
        // info!("[applysizehints]");
        // set minimum possible.
        *w = 1.max(*w);
        *h = 1.max(*h);
        if interact {
            let cc = c.as_ref().borrow_mut();
            let width = cc.width();
            let height = cc.height();
            if *x > self.sw {
                *x = self.sw - width;
            }
            if *y > self.sh {
                *y = self.sh - height;
            }
            if *x + *w + 2 * cc.bw < 0 {
                *x = 0;
            }
            if *y + *h + 2 * cc.bw < 0 {
                *y = 0;
            }
        } else {
            let cc = c.as_ref().borrow_mut();
            let wx = cc.mon.as_ref().unwrap().borrow_mut().wx;
            let wy = cc.mon.as_ref().unwrap().borrow_mut().wy;
            let ww = cc.mon.as_ref().unwrap().borrow_mut().ww;
            let wh = cc.mon.as_ref().unwrap().borrow_mut().wh;
            let width = cc.width();
            let height = cc.height();
            if *x >= wx + ww {
                *x = wx + ww - width;
            }
            if *y >= wy + wh {
                *y = wy + wh - height;
            }
            let bw = cc.bw;
            if *x + *w + 2 * bw <= wx {
                *x = wx;
            }
            if *y + *h + 2 * bw <= wy {
                *y = wy;
            }
        }
        if *h < self.bh {
            *h = self.bh;
        }
        if *w < self.bh {
            *w = self.bh;
        }
        let isfloating = { c.as_ref().borrow_mut().isfloating };
        let layout_type = {
            let mon = c.as_ref().borrow_mut().mon.clone();
            let sellt = mon.as_ref().unwrap().borrow_mut().sellt;
            let x = mon.as_ref().unwrap().borrow_mut().lt[sellt]
                .layout_type
                .clone();
            x
        };
        if Config::resizehints || isfloating || layout_type.is_none() {
            if !c.as_ref().borrow_mut().hintsvalid {
                self.updatesizehints(c);
            }
            // see last two sentences in ICCCM 4.1.2.3
            let cc = c.as_ref().borrow_mut();
            let baseismin = cc.basew == cc.minw && cc.baseh == cc.minh;
            if !baseismin {
                // temporarily remove base dimensions.
                (*w) -= cc.basew;
                (*h) -= cc.baseh;
            }
            // adjust for aspect limits.
            if cc.mina > 0. && cc.maxa > 0. {
                if cc.maxa < *w as f32 / *h as f32 {
                    *w = (*h as f32 * cc.maxa + 0.5) as i32;
                } else if cc.mina < *h as f32 / *w as f32 {
                    *h = (*w as f32 * cc.mina + 0.5) as i32;
                }
            }
            if baseismin {
                // increment calcalation requires this.
                *w -= cc.basew;
                *h -= cc.baseh;
            }
            // adjust for increment value.
            if (cc).incw > 0 {
                *w -= *w % (cc).incw;
            }
            if (cc).inch > 0 {
                *h -= *h % (cc).inch;
            }
            // restore base dimensions.
            *w = (*w + cc.basew).max(cc.minw);
            *h = (*h + cc.baseh).max(cc.minh);
            if cc.maxw > 0 {
                let mut maxw = cc.maxw;
                *w = *w.min(&mut maxw);
            }
            if cc.maxh > 0 {
                let mut maxh = cc.maxh;
                *h = *h.min(&mut maxh);
            }
        }
        {
            let cc = c.as_ref().borrow_mut();
            return *x != cc.x || (*y) != cc.y || *w != cc.w || *h != cc.h;
        }
    }
    pub fn cleanup(&mut self) {
        // info!("[cleanup]");
        // Bitwise or to get max value.
        drop(self.sender.clone());
        let mut a: Arg = Arg::Ui(!0);
        let foo: Layout = Layout::new("", None);
        unsafe {
            self.view(&mut a);
            {
                let mut selmon_mut = self.selmon.as_mut().unwrap().borrow_mut();
                let idx = selmon_mut.sellt;
                selmon_mut.lt[idx] = Rc::new(foo);
            }
            let mut m = self.mons.clone();
            while let Some(ref m_opt) = m {
                let mut stack: Option<Rc<RefCell<Client>>>;
                while {
                    stack = m_opt.borrow_mut().stack.clone();
                    stack.is_some()
                } {
                    self.unmanage(stack, false);
                }
                let next = { m_opt.borrow_mut().next.clone() };
                m = next;
            }
            XUngrabKey(self.dpy, AnyKey, AnyModifier, self.root);
            while self.mons.is_some() {
                self.cleanupmon(self.mons.clone());
            }
            for i in 0..CUR::CurLast as usize {
                self.drw
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .drw_cur_free(self.cursor[i].as_mut().unwrap().as_mut());
            }
            XDestroyWindow(self.dpy, self.wmcheckwin);
            self.drw.as_mut().unwrap().drw_free();
            XSync(self.dpy, False);
            XSetInputFocus(
                self.dpy,
                PointerRoot as u64,
                RevertToPointerRoot,
                CurrentTime,
            );
            XDeleteProperty(
                self.dpy,
                self.root,
                self.netatom[NET::NetActiveWindow as usize],
            );
        }
    }
    pub fn cleanupmon(&mut self, mon: Option<Rc<RefCell<Monitor>>>) {
        // info!("[cleanupmon]");
        unsafe {
            if Rc::ptr_eq(mon.as_ref().unwrap(), self.mons.as_ref().unwrap()) {
                let next = self.mons.as_ref().unwrap().borrow_mut().next.clone();
                self.mons = next;
            } else {
                let mut m = self.mons.clone();
                while let Some(ref m_opt) = m {
                    if Dwm::are_equal_rc(&m_opt.borrow_mut().next, &mon) {
                        break;
                    }
                    let next = m_opt.borrow_mut().next.clone();
                    m = next;
                }
                m.as_ref().unwrap().borrow_mut().next =
                    mon.as_ref().unwrap().borrow_mut().next.clone();
            }
            let barwin = mon.as_ref().unwrap().borrow_mut().barwin;
            XUnmapWindow(self.dpy, barwin);
            XDestroyWindow(self.dpy, barwin);
        }
    }
    pub fn clientmessage(&mut self, e: *mut XEvent) {
        // info!("[clientmessage]");
        unsafe {
            let cme = (*e).client_message;
            let c = self.wintoclient(cme.window);

            if c.is_none() {
                return;
            }
            if cme.message_type == self.netatom[NET::NetWMState as usize] {
                if cme.data.get_long(1) == self.netatom[NET::NetWMFullscreen as usize] as i64
                    || cme.data.get_long(2) == self.netatom[NET::NetWMFullscreen as usize] as i64
                {
                    // NET_WM_STATE_ADD
                    // NET_WM_STATE_TOGGLE
                    let isfullscreen = { c.as_ref().unwrap().borrow_mut().isfullscreen };
                    let fullscreen =
                        cme.data.get_long(0) == 1 || (cme.data.get_long(0) == 2 && !isfullscreen);
                    self.setfullscreen(c.as_ref().unwrap(), fullscreen);
                }
            } else if cme.message_type == self.netatom[NET::NetActiveWindow as usize] {
                let isurgent = { c.as_ref().unwrap().borrow_mut().isurgent };
                let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
                if !Self::are_equal_rc(&c, &sel) && !isurgent {
                    self.seturgent(c.as_ref().unwrap(), true);
                }
            }
        }
    }

    pub fn configurenotify(&mut self, e: *mut XEvent) {
        // info!("[configurenotify]");
        unsafe {
            let ev = (*e).configure;
            if ev.window == self.root {
                let dirty = self.sw != ev.width || self.sh != ev.height;
                self.sw = ev.width;
                self.sh = ev.height;
                if self.updategeom() || dirty {
                    self.drw
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .drw_resize(self.sw as u32, self.bh as u32);
                    self.updatebars();
                    let mut m = self.mons.clone();
                    while let Some(m_opt) = m {
                        let mut c = m_opt.borrow_mut().clients.clone();
                        while c.is_some() {
                            if c.as_ref().unwrap().borrow_mut().isfullscreen {
                                self.resizeclient(
                                    &mut *c.as_ref().unwrap().borrow_mut(),
                                    m_opt.borrow_mut().mx,
                                    m_opt.borrow_mut().my,
                                    m_opt.borrow_mut().mw,
                                    m_opt.borrow_mut().mh,
                                );
                            }
                            let next = c.as_ref().unwrap().borrow_mut().next.clone();
                            c = next;
                        }
                        XMoveResizeWindow(
                            self.dpy,
                            m_opt.borrow_mut().barwin,
                            m_opt.borrow_mut().wx + self.sp,
                            m_opt.borrow_mut().by + self.vp,
                            (m_opt.borrow_mut().ww - 2 * self.sp) as u32,
                            self.bh as u32,
                        );
                        let next = m_opt.borrow_mut().next.clone();
                        m = next;
                    }
                    self.focus(None);
                    self.arrange(None);
                }
            }
        }
    }

    pub fn configure(&mut self, c: &mut Client) {
        // info!("[configure]");
        unsafe {
            let mut ce: XConfigureEvent = zeroed();

            ce.type_ = ConfigureNotify;
            ce.display = self.dpy;
            ce.event = c.win;
            ce.window = c.win;
            ce.x = c.x;
            ce.y = c.y;
            ce.width = c.w;
            ce.height = c.h;
            ce.border_width = c.bw;
            ce.above = 0;
            ce.override_redirect = 0;
            let mut xe = XEvent { configure: ce };
            XSendEvent(self.dpy, c.win, 0, StructureNotifyMask, &mut xe);
        }
    }
    pub fn setfullscreen(&mut self, c: &Rc<RefCell<Client>>, fullscreen: bool) {
        // info!("[setfullscreen]");
        unsafe {
            let isfullscreen = { c.borrow_mut().isfullscreen };
            let win = { c.borrow_mut().win };
            if fullscreen && !isfullscreen {
                XChangeProperty(
                    self.dpy,
                    win,
                    self.netatom[NET::NetWMState as usize],
                    XA_ATOM,
                    32,
                    PropModeReplace,
                    self.netatom.as_ptr().add(NET::NetWMFullscreen as usize) as *const _,
                    1,
                );
                {
                    let mut c = c.borrow_mut();
                    c.isfullscreen = true;
                    c.oldstate = c.isfloating;
                    c.oldbw = c.bw;
                    c.bw = 0;
                    c.isfloating = true;
                }
                let (mx, my, mw, mh) = {
                    let c_mon = &c.borrow().mon;
                    let mon_mut = c_mon.as_ref().unwrap().borrow();
                    (mon_mut.mx, mon_mut.my, mon_mut.mw, mon_mut.mh)
                };
                self.resizeclient(&mut *c.borrow_mut(), mx, my, mw, mh);
                XRaiseWindow(self.dpy, win);
            } else if !fullscreen && isfullscreen {
                XChangeProperty(
                    self.dpy,
                    win,
                    self.netatom[NET::NetWMState as usize],
                    XA_ATOM,
                    32,
                    PropModeReplace,
                    null(),
                    0,
                );
                {
                    let mut c = c.borrow_mut();
                    c.isfullscreen = false;
                    c.isfloating = c.oldstate;
                    c.bw = c.oldbw;
                    c.x = c.oldx;
                    c.y = c.oldy;
                    c.w = c.oldw;
                    c.h = c.oldh;
                }
                {
                    let mut c = c.borrow_mut();
                    let x = c.x;
                    let y = c.y;
                    let w = c.w;
                    let h = c.h;
                    self.resizeclient(&mut *c, x, y, w, h);
                }
                let mon = { c.borrow_mut().mon.clone() };
                self.arrange(mon);
            }
        }
    }
    pub fn resizeclient(&mut self, c: &mut Client, x: i32, y: i32, w: i32, h: i32) {
        // info!("[resizeclient]");
        unsafe {
            let mut wc: XWindowChanges = zeroed();
            c.oldx = c.x;
            c.x = x;
            wc.x = x;
            c.oldy = c.y;
            c.y = y;
            wc.y = y;
            c.oldw = c.w;
            c.w = w;
            wc.width = w;
            c.oldh = c.h;
            c.h = h;
            wc.height = h;
            wc.border_width = c.bw;
            XConfigureWindow(
                self.dpy,
                c.win,
                (CWX | CWY | CWWidth | CWHeight | CWBorderWidth) as u32,
                &mut wc as *mut _,
            );
            self.configure(c);
            XSync(self.dpy, 0);
        }
    }

    pub fn resize(
        &mut self,
        c: &Rc<RefCell<Client>>,
        mut x: i32,
        mut y: i32,
        mut w: i32,
        mut h: i32,
        interact: bool,
    ) {
        // info!("[resize]");
        if self.applysizehints(c, &mut x, &mut y, &mut w, &mut h, interact) {
            self.resizeclient(&mut *c.borrow_mut(), x, y, w, h);
        }
    }

    pub fn seturgent(&mut self, c: &Rc<RefCell<Client>>, urg: bool) {
        // info!("[seturgent]");
        unsafe {
            c.borrow_mut().isurgent = urg;
            let win = c.borrow_mut().win;
            let wmh = XGetWMHints(self.dpy, win);
            if wmh.is_null() {
                return;
            }
            (*wmh).flags = if urg {
                (*wmh).flags | XUrgencyHint
            } else {
                (*wmh).flags & !XUrgencyHint
            };
            XSetWMHints(self.dpy, win, wmh);
            XFree(wmh as *mut _);
        }
    }

    pub fn showhide(&mut self, c: Option<Rc<RefCell<Client>>>) {
        // info!("[showhide]");
        if c.is_none() {
            return;
        }
        unsafe {
            let isvisible = { c.as_ref().unwrap().borrow_mut().isvisible() };
            if isvisible {
                // show clients top down.
                let win = c.as_ref().unwrap().borrow_mut().win;
                let x = c.as_ref().unwrap().borrow_mut().x;
                let y = c.as_ref().unwrap().borrow_mut().y;
                XMoveWindow(self.dpy, win, x, y);
                let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
                let sellt = mon.as_ref().unwrap().borrow_mut().sellt;
                let isfloating = c.as_ref().unwrap().borrow_mut().isfloating;
                let isfullscreen = c.as_ref().unwrap().borrow_mut().isfullscreen;
                if (mon.as_ref().unwrap().borrow_mut().lt[sellt]
                    .layout_type
                    .is_none()
                    || isfloating)
                    && !isfullscreen
                {
                    let x;
                    let y;
                    let w;
                    let h;
                    {
                        let cc = c.as_ref().unwrap().borrow_mut();
                        x = cc.x;
                        y = cc.y;
                        w = cc.w;
                        h = cc.h;
                    }
                    self.resize(c.as_ref().unwrap(), x, y, w, h, false);
                }
                let snext = c.as_ref().unwrap().borrow_mut().snext.clone();
                self.showhide(snext);
            } else {
                // hide clients bottom up.
                let snext = c.as_ref().unwrap().borrow_mut().snext.clone();
                self.showhide(snext);
                let y;
                let win;
                {
                    let cc = c.as_ref().unwrap().borrow_mut();
                    y = cc.y;
                    win = cc.win;
                }
                XMoveWindow(
                    self.dpy,
                    win,
                    c.as_ref().unwrap().borrow_mut().width() * -2,
                    y,
                );
            }
        }
    }
    pub fn configurerequest(&mut self, e: *mut XEvent) {
        // info!("[configurerequest]");
        unsafe {
            let ev = (*e).configure_request;
            let c = self.wintoclient(ev.window);
            if let Some(c_opt) = c {
                let layout_type = {
                    let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                    let sellt = selmon_mut.sellt;
                    selmon_mut.lt[sellt].layout_type.clone()
                };
                let isfloating = { c_opt.borrow_mut().isfloating };
                if ev.value_mask & CWBorderWidth as u64 > 0 {
                    let mut c_mut = c_opt.borrow_mut();
                    c_mut.bw = ev.border_width;
                } else if isfloating || layout_type.is_none() {
                    let mx;
                    let my;
                    let mw;
                    let mh;
                    {
                        let c_mut = c_opt.borrow_mut();
                        let m = c_mut.mon.as_ref().unwrap().borrow_mut();
                        mx = m.mx;
                        my = m.my;
                        mw = m.mw;
                        mh = m.mh;
                    }
                    {
                        let mut c_mut = c_opt.borrow_mut();
                        if ev.value_mask & CWX as u64 > 0 {
                            c_mut.oldx = c_mut.x;
                            c_mut.x = mx + ev.x;
                        }
                        if ev.value_mask & CWY as u64 > 0 {
                            c_mut.oldy = c_mut.y;
                            c_mut.y = my + ev.y;
                        }
                        if ev.value_mask & CWWidth as u64 > 0 {
                            c_mut.oldw = c_mut.w;
                            c_mut.w = ev.width;
                        }
                        if ev.value_mask & CWHeight as u64 > 0 {
                            c_mut.oldh = c_mut.h;
                            c_mut.h = ev.height;
                        }
                        if (c_mut.x + c_mut.w) > mx + mw && c_mut.isfloating {
                            // center in x direction
                            // c_mut.x = mx + (mw / 2 - c_mut.width() / 2);
                            c_mut.x = 0;
                        }
                        if (c_mut.y + c_mut.h) > my + mh && c_mut.isfloating {
                            // center in y direction
                            c_mut.y = my + (mh / 2 - c_mut.height() / 2);
                        }
                    }
                    if (ev.value_mask & (CWX | CWY) as u64) > 0
                        && (ev.value_mask & (CWWidth | CWHeight) as u64) <= 0
                    {
                        self.configure(&mut *c_opt.borrow_mut());
                    }
                    let isvisible = { c_opt.borrow_mut().isvisible() };
                    if isvisible {
                        let c_mut = c_opt.borrow();
                        XMoveResizeWindow(
                            self.dpy,
                            c_mut.win,
                            c_mut.x,
                            c_mut.y,
                            c_mut.w as u32,
                            c_mut.h as u32,
                        );
                    }
                } else {
                    self.configure(&mut *c_opt.borrow_mut());
                }
            } else {
                let mut wc: XWindowChanges = zeroed();
                wc.x = ev.x;
                wc.y = ev.y;
                wc.width = ev.width;
                wc.height = ev.height;
                wc.border_width = ev.border_width;
                wc.sibling = ev.above;
                wc.stack_mode = ev.detail;
                XConfigureWindow(self.dpy, ev.window, ev.value_mask as u32, &mut wc);
            }
            XSync(self.dpy, False);
        }
    }
    pub fn createmon(&mut self) -> Monitor {
        // info!("[createmon]");
        let mut m: Monitor = Monitor::new();
        m.tagset[0] = 1;
        m.tagset[1] = 1;
        m.mfact0 = Config::mfact;
        m.nmaster0 = Config::nmaster;
        m.showbar0 = Config::showbar;
        m.topbar0 = Config::topbar;
        m.lt[0] = Config::layouts[0].clone();
        m.lt[1] = Config::layouts[1 % Config::layouts.len()].clone();
        m.ltsymbol = Config::layouts[0].symbol.to_string();
        info!(
            "[createmon]: ltsymbol: {:?}, mfact0: {}, nmaster0: {}, showbar0: {}, topbar0: {}",
            m.ltsymbol, m.mfact0, m.nmaster0, m.showbar0, m.topbar0
        );
        m.pertag = Some(Pertag::new());
        let ref_pertag = m.pertag.as_mut().unwrap();
        ref_pertag.curtag = 1;
        ref_pertag.prevtag = 1;
        for i in 0..=Config::tags_length {
            ref_pertag.nmasters[i] = m.nmaster0;
            ref_pertag.mfacts[i] = m.mfact0;

            ref_pertag.ltidxs[i][0] = Some(m.lt[0].clone());
            ref_pertag.ltidxs[i][1] = Some(m.lt[1].clone());
            ref_pertag.sellts[i] = m.sellt;

            ref_pertag.showbars[i] = m.showbar0;
        }

        return m;
    }
    pub fn destroynotify(&mut self, e: *mut XEvent) {
        // info!("[destroynotify]");
        unsafe {
            let ev = (*e).destroy_window;
            let c = self.wintoclient(ev.window);
            if c.is_some() {
                self.unmanage(c, true);
            }
        }
    }
    pub fn applylayout(&mut self, layout_type: &LayoutType, m: *mut Monitor) {
        match layout_type {
            LayoutType::TypeTile => {
                self.tile(m);
            }
            LayoutType::TypeFloat => {}
            LayoutType::TypeMonocle => {
                self.monocle(m);
            }
        }
    }
    pub fn arrangemon(&mut self, m: &Rc<RefCell<Monitor>>) {
        // info!("[arrangemon]");
        let sellt;
        {
            let mut mm = m.borrow_mut();
            sellt = (mm).sellt;
            mm.ltsymbol = (mm).lt[sellt].symbol.to_string();
            info!("[arrangemon] sellt: {}, ltsymbol: {:?}", sellt, mm.ltsymbol);
        }
        let layout_type = { m.borrow_mut().lt[sellt].layout_type.clone() };
        if let Some(ref layout_type) = layout_type {
            let m_ptr: *mut Monitor;
            {
                m_ptr = &mut *m.borrow_mut();
            }
            self.applylayout(layout_type, m_ptr);
        }
    }
    // This is cool!
    pub fn detach(&mut self, c: Option<Rc<RefCell<Client>>>) {
        // info!("[detach]");
        let mut current = {
            c.as_ref()
                .unwrap()
                .borrow_mut()
                .mon
                .as_ref()
                .unwrap()
                .borrow_mut()
                .clients
                .clone()
        };
        let mut prev: Option<Rc<RefCell<Client>>> = None;
        while let Some(ref current_opt) = current {
            if Self::are_equal_rc(&current, &c) {
                let next = { current_opt.borrow_mut().next.clone() };
                if let Some(ref prev_opt) = prev {
                    prev_opt.borrow_mut().next = next;
                } else {
                    c.as_ref()
                        .unwrap()
                        .borrow_mut()
                        .mon
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .clients = next;
                }
                break;
            }
            prev = current.clone();
            let next = current_opt.borrow_mut().next.clone();
            current = next;
        }
    }
    pub fn detachstack(&mut self, c: Option<Rc<RefCell<Client>>>) {
        // info!("[detachstack]");
        let mut current = {
            c.as_ref()
                .unwrap()
                .borrow_mut()
                .mon
                .as_ref()
                .unwrap()
                .borrow_mut()
                .stack
                .clone()
        };
        let mut prev: Option<Rc<RefCell<Client>>> = None;
        while let Some(ref current_opt) = current {
            if Self::are_equal_rc(&current, &c) {
                let snext = { current_opt.borrow_mut().snext.clone() };
                if let Some(ref prev_opt) = prev {
                    prev_opt.borrow_mut().snext = snext;
                } else {
                    c.as_ref()
                        .unwrap()
                        .borrow_mut()
                        .mon
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .stack = snext;
                }
                break;
            }
            prev = current.clone();
            let snext = current_opt.borrow_mut().snext.clone();
            current = snext;
        }

        let mut condition = false;
        if let Some(ref mon_opt) = c.as_ref().unwrap().borrow_mut().mon {
            if Self::are_equal_rc(&c, &mon_opt.borrow_mut().sel) {
                condition = true;
            }
        }
        if condition {
            let mut t = {
                c.as_ref()
                    .unwrap()
                    .borrow_mut()
                    .mon
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .stack
                    .clone()
            };
            while let Some(ref t_opt) = t {
                let isvisible = { t_opt.borrow_mut().isvisible() };
                if isvisible {
                    break;
                }
                let snext = { t_opt.borrow_mut().snext.clone() };
                t = snext;
            }
            {
                c.as_ref()
                    .unwrap()
                    .borrow_mut()
                    .mon
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .sel = t.clone()
            };
        }
    }
    pub fn dirtomon(&mut self, dir: i32) -> Option<Rc<RefCell<Monitor>>> {
        // info!("[dirtomon]");
        let mut m: Option<Rc<RefCell<Monitor>>>;
        if dir > 0 {
            // info!("[dirtomon] dir: {}", dir);
            m = self.selmon.as_ref().unwrap().borrow_mut().next.clone();
            if m.is_none() {
                m = self.mons.clone();
            }
        } else if Rc::ptr_eq(self.selmon.as_ref().unwrap(), self.mons.as_ref().unwrap()) {
            // info!("[dirtomon] selmon equal mons");
            m = self.mons.clone();
            while let Some(ref m_opt) = m {
                let next = m_opt.borrow_mut().next.clone();
                if next.is_none() {
                    break;
                }
                m = next;
            }
        } else {
            // info!("[dirtomon] other dir: {}", dir);
            m = self.mons.clone();
            while let Some(ref m_opt) = m {
                let next = m_opt.borrow_mut().next.clone();
                if Rc::ptr_eq(next.as_ref().unwrap(), self.selmon.as_ref().unwrap()) {
                    break;
                }
                m = next;
            }
        }
        m
    }

    fn parse_string(input: &str) -> Vec<TextElement> {
        let mut elements = Vec::new();
        let mut current_segment = String::new();
        let mut inside_caret = false;

        for c in input.chars() {
            match c {
                '^' => {
                    if !current_segment.is_empty() {
                        // Push the current segment into the appropriate category.
                        if inside_caret {
                            elements.push(TextElement::WithCaret(current_segment));
                        } else {
                            elements.push(TextElement::WithoutCaret(current_segment));
                        }
                        current_segment = String::new();
                    }
                    inside_caret = !inside_caret;
                }
                _ => {
                    // Add the current character to the current segment.
                    current_segment.push(c);
                }
            }
        }

        // Add any remaining segment after the last caret or at the end of the string.
        if !current_segment.is_empty() {
            if inside_caret {
                elements.push(TextElement::WithCaret(current_segment));
            } else {
                elements.push(TextElement::WithoutCaret(current_segment));
            }
        }

        elements
    }

    pub fn drawstatusbar(&mut self, m: Option<Rc<RefCell<Monitor>>>, text: &str) -> i32 {
        // compute width of the status text
        let mut w: u32 = 0;
        let parsed_elements = Self::parse_string(text);
        // info!("[drawstatusbar] parsed_elements: {:?}", parsed_elements);
        let drw_mut = self.drw.as_mut().unwrap();
        for element in &parsed_elements {
            match element {
                TextElement::WithoutCaret(val) => {
                    w += drw_mut.textw(&val) - drw_mut.lrpad as u32;
                    if val.starts_with('f') {
                        match val[1..].parse::<u32>() {
                            Ok(num) => w += num,
                            Err(e) => eprintln!("Failed to parse the number: {}", e),
                        }
                    }
                }
                _ => {}
            }
        }

        w += Config::horizpadbar as u32;
        let ww = { m.as_ref().unwrap().borrow_mut().ww };
        let ret = ww - w as i32;
        let mut x = ret - 2 * self.sp;
        drw_mut.drw_setscheme(self.scheme[SCHEME::SchemeStatus as usize].clone());
        drw_mut.drw_rect(x, 0, w, self.bh as u32, 1, 0);
        x += Config::horizpadbar / 2;
        for element in &parsed_elements {
            // info!("[drawstatusbar] element {:?}", element);
            match element {
                TextElement::WithoutCaret(val) => {
                    w = drw_mut.textw(val) - drw_mut.lrpad as u32;
                    drw_mut.drw_text(
                        x,
                        Config::vertpadbar / 2,
                        w,
                        self.bh as u32 - Config::vertpadbar as u32,
                        0,
                        &val,
                        0,
                        false,
                    );
                    x += w as i32;
                }
                TextElement::WithCaret(val) => {
                    if val.starts_with('c') {
                        let color = &val[1..];
                        drw_mut.scheme[Col::ColFg as usize] =
                            drw_mut.drw_clr_create(color, Config::OPAQUE);
                    } else if val.starts_with('b') {
                        let color = &val[1..];
                        drw_mut.scheme[Col::ColBg as usize] =
                            drw_mut.drw_clr_create(color, Config::baralpha);
                    } else if val.starts_with('d') {
                        drw_mut.scheme[Col::ColFg as usize] =
                            self.scheme[SCHEME::SchemeNorm as usize][Col::ColFg as usize].clone();
                        drw_mut.scheme[Col::ColBg as usize] =
                            self.scheme[SCHEME::SchemeNorm as usize][Col::ColBg as usize].clone();
                    } else if val.starts_with('r') {
                        let numbers: Result<Vec<i32>, _> =
                            (&val[1..]).split(',').map(|s| s.parse::<i32>()).collect();
                        if let Ok(numbers) = numbers {
                            println!("numbers: {:?}", numbers);
                            let rx = numbers[0];
                            let ry = numbers[1];
                            let rw = numbers[2];
                            let rh = numbers[3];
                            drw_mut.drw_rect(
                                rx + x,
                                ry + Config::vertpadbar / 2,
                                rw as u32,
                                rh as u32,
                                0,
                                0,
                            );
                        }
                    } else if val.starts_with('f') {
                        match val[1..].parse::<u32>() {
                            Ok(num) => x += num as i32,
                            Err(e) => eprintln!("Failed to parse the number: {}", e),
                        }
                    }
                }
            }
        }
        return ret;
    }
    pub fn drawbar(&mut self, m: Option<Rc<RefCell<Monitor>>>) {
        // info!("[drawbar]");
        let mut tw: i32 = 0;
        let mut occ: u32 = 0;
        let mut urg: u32 = 0;
        {
            // info!("[drawbar] {}", m.as_ref().unwrap().borrow_mut());
            let formatted_string = format!("[drawbar] {}", m.as_ref().unwrap().borrow_mut());
            if self.pipe.is_some() {
                self.pipe
                    .as_mut()
                    .unwrap()
                    .write_all(formatted_string.as_bytes())
                    .unwrap();
            }
        }
        let boxs;
        let boxw;
        let lrpad;
        {
            let h = self
                .drw
                .as_ref()
                .unwrap()
                .font
                .as_ref()
                .unwrap()
                .borrow_mut()
                .h;
            lrpad = self.drw.as_ref().unwrap().lrpad;
            boxs = h / 9;
            boxw = h / 6 + 2;
            // info!("[drawbar] boxs: {}, boxw: {}, lrpad: {}", boxs, boxw, lrpad);
        }
        let showbar0 = { m.as_ref().unwrap().borrow_mut().showbar0 };
        if !showbar0 {
            return;
        }

        let ww = { m.as_ref().unwrap().borrow_mut().ww };
        // draw status first so it can be overdrawn by tags later.
        if Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap()) {
            // status is only drawn on selected monitor.
            // draw status bar here
            let stext = self.stext.clone();
            tw = ww - self.drawstatusbar(m.clone(), &stext);
        }
        {
            let mut c = m.as_ref().unwrap().borrow_mut().clients.clone();
            while let Some(ref c_opt) = c {
                let tags0 = c_opt.borrow_mut().tags0;
                occ |= tags0;
                if c_opt.borrow_mut().isurgent {
                    urg |= tags0;
                }
                let next = c_opt.borrow_mut().next.clone();
                c = next;
            }
        }
        let mut x = 0;
        let mut w;
        for i in 0..Config::tags_length {
            w = self.drw.as_mut().unwrap().textw(self.tags[i]) as i32;
            let seltags = { m.as_ref().unwrap().borrow_mut().seltags };
            let tagset = { m.as_ref().unwrap().borrow_mut().tagset };
            let is_selected_tag = tagset[seltags] & 1 << i > 0;
            let idx = if is_selected_tag {
                SCHEME::SchemeTagsSel as usize
            } else {
                SCHEME::SchemeTagsNorm as usize
            };
            // info!(
            //     "[drawbar] seltags: {}, tagset: {:?}, i: {}: idx: {}, w: {}",
            //     seltags, tagset, i, idx, w
            // );
            self.drw
                .as_mut()
                .unwrap()
                .as_mut()
                .drw_setscheme(self.scheme[idx].clone());
            self.drw.as_mut().unwrap().drw_text(
                x,
                0,
                w as u32,
                self.bh as u32,
                (lrpad / 2) as u32,
                self.tags[i],
                (urg & 1 << i) as i32,
                false,
            );
            if Config::ulineall || is_selected_tag {
                self.drw.as_mut().unwrap().drw_rect(
                    x + Config::ulinepad as i32,
                    self.bh - Config::ulinestroke as i32 - Config::ulinevoffset as i32,
                    w as u32 - (Config::ulinepad * 2),
                    Config::ulinestroke,
                    1,
                    0,
                );
            }
            if (occ & 1 << i) > 0 {
                let selmon_mut = { self.selmon.as_ref().unwrap().borrow_mut() };
                let filled = (Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap())
                    && selmon_mut.sel.is_some()
                    && (selmon_mut.sel.as_ref().unwrap().borrow_mut().tags0 & 1 << i > 0))
                    as i32;
                self.drw.as_mut().unwrap().drw_rect(
                    x + boxs as i32,
                    boxs as i32,
                    boxw,
                    boxw,
                    filled,
                    (urg & 1 << i) as i32,
                );
            }
            x += w;
        }
        w = self
            .drw
            .as_mut()
            .unwrap()
            .as_mut()
            .textw(&m.as_ref().unwrap().borrow_mut().ltsymbol) as i32;
        self.drw
            .as_mut()
            .unwrap()
            .as_mut()
            .drw_setscheme(self.scheme[SCHEME::SchemeTagsNorm as usize].clone());
        x = self.drw.as_mut().unwrap().drw_text(
            x,
            0,
            w as u32,
            self.bh as u32,
            (lrpad / 2) as u32,
            &m.as_ref().unwrap().borrow_mut().ltsymbol,
            0,
            false,
        );

        w = ww - tw - x;
        // info!("[drawbar] tw: {}, x: {}, w: {}, bh: {}", tw, x, w, bh);
        if w > self.bh {
            if let Some(ref sel_opt) = m.as_ref().unwrap().borrow_mut().sel {
                let idx = if Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap()) {
                    SCHEME::SchemeInfoSel
                } else {
                    SCHEME::SchemeInfoNorm
                } as usize;
                self.drw
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .drw_setscheme(self.scheme[idx].clone());
                self.drw.as_mut().unwrap().drw_text(
                    x,
                    0,
                    (w - 2 * self.sp) as u32,
                    self.bh as u32,
                    (lrpad / 2) as u32,
                    &sel_opt.borrow_mut().name,
                    0,
                    false,
                );
                if sel_opt.borrow_mut().isfloating {
                    // Useless, drw rectangle.
                }
            } else {
                self.drw
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .drw_setscheme(self.scheme[SCHEME::SchemeInfoNorm as usize].clone());
                self.drw.as_mut().unwrap().drw_rect(
                    x,
                    0,
                    (w - 2 * self.sp) as u32,
                    self.bh as u32,
                    1,
                    0,
                );
            }
        }
        let barwin = { m.as_ref().unwrap().borrow_mut().barwin };
        let ww: u32 = { m.as_ref().unwrap().borrow_mut().ww } as u32;
        // info!("[drawbar] drw_map");
        self.drw
            .as_mut()
            .unwrap()
            .as_mut()
            .drw_map(barwin, 0, 0, ww, self.bh as u32);
        // info!("[drawbar] finish");
    }

    pub fn restack(&mut self, m: Option<Rc<RefCell<Monitor>>>) {
        // info!("[restack]");
        self.drawbar(m.clone());

        unsafe {
            let mut wc: XWindowChanges = zeroed();
            let sel = m.as_ref().unwrap().borrow_mut().sel.clone();
            if sel.is_none() {
                return;
            }
            let isfloating = sel.as_ref().unwrap().borrow_mut().isfloating;
            let sellt = m.as_ref().unwrap().borrow_mut().sellt;
            let layout_type = {
                m.as_ref().unwrap().borrow_mut().lt[sellt]
                    .layout_type
                    .clone()
            };
            if isfloating || layout_type.is_none() {
                let win = sel.as_ref().unwrap().borrow_mut().win;
                XRaiseWindow(self.dpy, win);
            }
            if layout_type.is_some() {
                wc.stack_mode = Below;
                wc.sibling = m.as_ref().unwrap().borrow_mut().barwin;
                let mut c = m.as_ref().unwrap().borrow_mut().stack.clone();
                while let Some(ref c_opt) = c {
                    let isfloating = { c_opt.borrow_mut().isfloating };
                    let isvisible = { c_opt.borrow_mut().isvisible() };
                    if !isfloating && isvisible {
                        let win = c_opt.borrow_mut().win;
                        XConfigureWindow(self.dpy, win, (CWSibling | CWStackMode) as u32, &mut wc);
                        wc.sibling = win;
                    }
                    let next = c_opt.borrow_mut().snext.clone();
                    c = next;
                }
            }
            XSync(self.dpy, 0);
            let mut ev: XEvent = zeroed();
            while XCheckMaskEvent(self.dpy, EnterWindowMask, &mut ev) > 0 {}
        }
    }

    pub fn run(&mut self) {
        // info!("[run]");
        // main event loop
        unsafe {
            let mut ev: XEvent = zeroed();
            XSync(self.dpy, False);
            let mut i: u64 = 0;
            while self.running.load(Ordering::SeqCst) && XNextEvent(self.dpy, &mut ev) <= 0 {
                // info!("running frame: {}, handler type: {}", i, ev.type_);
                i = (i + 1) % std::u64::MAX;
                self.handler(ev.type_, &mut ev);
            }
        }
    }

    pub fn scan(&mut self) {
        // info!("[scan]");
        let mut num: u32 = 0;
        let mut d1: Window = 0;
        let mut d2: Window = 0;
        let mut wins: *mut Window = null_mut();
        unsafe {
            let mut wa: XWindowAttributes = zeroed();
            if XQueryTree(self.dpy, self.root, &mut d1, &mut d2, &mut wins, &mut num) > 0 {
                for i in 0..num as usize {
                    if XGetWindowAttributes(self.dpy, *wins.wrapping_add(i), &mut wa) <= 0
                        || wa.override_redirect > 0
                        || XGetTransientForHint(self.dpy, *wins.wrapping_add(i), &mut d1) > 0
                    {
                        continue;
                    }
                    if wa.map_state == IsViewable
                        || self.getstate(*wins.wrapping_add(i)) == IconicState as i64
                    {
                        self.manage(*wins.wrapping_add(i), &mut wa);
                    }
                }
                for i in 0..num as usize {
                    // now the transients
                    if XGetWindowAttributes(self.dpy, *wins.wrapping_add(i), &mut wa) <= 0 {
                        continue;
                    }
                    if XGetTransientForHint(self.dpy, *wins.wrapping_add(i), &mut d1) > 0
                        && (wa.map_state == IsViewable
                            || self.getstate(*wins.wrapping_add(i)) == IconicState as i64)
                    {
                        self.manage(*wins.wrapping_add(i), &mut wa);
                    }
                }
            }
            if !wins.is_null() {
                XFree(wins as *mut _);
            }
        }
    }

    pub fn arrange(&mut self, mut m: Option<Rc<RefCell<Monitor>>>) {
        // info!("[arrange]");
        if let Some(ref m_opt) = m {
            {
                let stack = { m_opt.borrow_mut().stack.clone() };
                self.showhide(stack);
            }
        } else {
            m = self.mons.clone();
            while let Some(ref m_opt) = m {
                let stack = { m_opt.borrow_mut().stack.clone() };
                self.showhide(stack);
                let next = { m_opt.borrow_mut().next.clone() };
                m = next;
            }
        }
        if let Some(ref m_opt) = m {
            self.arrangemon(m_opt);
            self.restack(m);
        } else {
            m = self.mons.clone();
            while let Some(ref m_opt) = m {
                self.arrangemon(m_opt);
                let next = { m_opt.borrow_mut().next.clone() };
                m = next;
            }
        }
    }

    pub fn attach(&mut self, c: Option<Rc<RefCell<Client>>>) {
        // info!("[attach]");
        let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
        c.as_ref().unwrap().borrow_mut().next = mon.as_ref().unwrap().borrow_mut().clients.clone();
        mon.as_ref().unwrap().borrow_mut().clients = c.clone();
    }
    pub fn attachstack(&mut self, c: Option<Rc<RefCell<Client>>>) {
        // info!("[attachstack]");
        let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
        c.as_ref().unwrap().borrow_mut().snext = mon.as_ref().unwrap().borrow_mut().stack.clone();
        mon.as_ref().unwrap().borrow_mut().stack = c.clone();
    }

    pub fn getatomprop(&mut self, c: &mut Client, prop: Atom) -> u64 {
        // info!("[getatomprop]");
        let mut di = 0;
        let mut dl: u64 = 0;
        let mut da: Atom = 0;
        let mut atom: Atom = 0;
        let mut p: *mut u8 = null_mut();
        unsafe {
            if XGetWindowProperty(
                self.dpy,
                c.win,
                prop,
                0,
                size_of::<Atom>() as i64,
                False,
                XA_ATOM,
                &mut da,
                &mut di,
                &mut dl,
                &mut dl,
                &mut p,
            ) == Success as i32
                && !p.is_null()
            {
                atom = *p as u64;
                XFree(p as *mut _);
            }
        }
        return atom;
    }

    pub fn getrootptr(&mut self, x: &mut i32, y: &mut i32) -> i32 {
        // info!("[getrootptr]");
        let mut di: i32 = 0;
        let mut dui: u32 = 0;
        unsafe {
            let mut dummy: Window = zeroed();

            return XQueryPointer(
                self.dpy, self.root, &mut dummy, &mut dummy, x, y, &mut di, &mut di, &mut dui,
            );
        }
    }

    pub fn getstate(&mut self, w: Window) -> i64 {
        // info!("[getstate]");
        let mut format: i32 = 0;
        let mut result: i64 = -1;
        let mut p: *mut u8 = null_mut();
        let mut n: u64 = 0;
        let mut extra: u64 = 0;
        let mut real: Atom = 0;
        unsafe {
            if XGetWindowProperty(
                self.dpy,
                w,
                self.wmatom[WM::WMState as usize],
                0,
                2,
                False,
                self.wmatom[WM::WMState as usize],
                &mut real,
                &mut format,
                &mut n,
                &mut extra,
                &mut p,
            ) != Success as i32
            {
                return -1;
            }
            if n != 0 {
                result = *p as i64;
            }
            XFree(p as *mut _);
        }
        return result;
    }

    pub fn recttomon(&mut self, x: i32, y: i32, w: i32, h: i32) -> Option<Rc<RefCell<Monitor>>> {
        // info!("[recttomon]");
        let mut area: i32 = 0;

        let mut r = self.selmon.clone();
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
            let a = m_opt.borrow_mut().intersect(x, y, w, h);
            if a > area {
                area = a;
                r = m.clone();
            }
            let next = m_opt.borrow_mut().next.clone();
            m = next;
        }
        return r;
    }

    pub fn wintoclient(&mut self, w: Window) -> Option<Rc<RefCell<Client>>> {
        // info!("[wintoclient]");
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
            let mut c = { m_opt.borrow_mut().clients.clone() };
            while let Some(ref c_opt) = c {
                let win = { c_opt.borrow_mut().win };
                if win == w {
                    return c;
                }
                let next = { c_opt.borrow_mut().next.clone() };
                c = next;
            }
            let next = { m_opt.borrow_mut().next.clone() };
            m = next;
        }
        None
    }

    pub fn wintomon(&mut self, w: Window) -> Option<Rc<RefCell<Monitor>>> {
        // info!("[wintomon]");
        let mut x: i32 = 0;
        let mut y: i32 = 0;
        if w == self.root && self.getrootptr(&mut x, &mut y) > 0 {
            return self.recttomon(x, y, 1, 1);
        }
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
            if w == m_opt.borrow_mut().barwin {
                return m;
            }
            let next = m_opt.borrow_mut().next.clone();
            m = next;
        }
        let c = self.wintoclient(w);
        if let Some(ref c_opt) = c {
            return c_opt.borrow_mut().mon.clone();
        }
        return self.selmon.clone();
    }

    pub fn buttonpress(&mut self, e: *mut XEvent) {
        // info!("[buttonpress]");
        let mut arg: Arg = Arg::Ui(0);
        unsafe {
            let c: Option<Rc<RefCell<Client>>>;
            let ev = (*e).button;
            let mut click = CLICK::ClkRootWin;
            // focus monitor if necessary.
            let m = self.wintomon(ev.window);
            if m.is_some() && !Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap()) {
                let sel = self.selmon.as_ref().unwrap().borrow_mut().sel.clone();
                self.unfocus(sel, true);
                self.selmon = m;
                self.focus(None);
            }
            let barwin = { self.selmon.as_ref().unwrap().borrow_mut().barwin };
            if ev.window == barwin {
                info!("[buttonpress] barwin: {}, ev.x: {}", barwin, ev.x);
                let mut i: usize = 0;
                let mut x: u32 = 0;
                for tag_i in 0..Config::tags_length {
                    x += self.drw.as_mut().unwrap().textw(self.tags[tag_i]);
                    if ev.x < x as i32 {
                        break;
                    }
                    i = tag_i + 1;
                    info!("[buttonpress] x: {}, i: {}", x, i);
                }
                let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                if i < Config::tags_length {
                    click = CLICK::ClkTagBar;
                    arg = Arg::Ui(1 << i);
                    info!("[buttonpress] ClkTagBar");
                } else if ev.x < (x + self.drw.as_mut().unwrap().textw(&selmon_mut.ltsymbol)) as i32
                {
                    click = CLICK::ClkLtSymbol;
                    info!("[buttonpress] ClkLtSymbol");
                } else if ev.x
                    > selmon_mut.ww - self.drw.as_mut().unwrap().textwm(&self.stext) as i32
                {
                    click = CLICK::ClkStatusText;
                    info!("[buttonpress] ClkStatusText");
                } else {
                    click = CLICK::ClkWinTitle;
                    info!("[buttonpress] ClkWinTitle");
                }
            } else if {
                c = self.wintoclient(ev.window);
                c.is_some()
            } {
                self.focus(c);
                self.restack(self.selmon.clone());
                XAllowEvents(self.dpy, ReplayPointer, CurrentTime);
                click = CLICK::ClkClientWin;
            }
            for i in 0..Config::buttons.len() {
                if click as u32 == Config::buttons[i].click
                    && Config::buttons[i].func.is_some()
                    && Config::buttons[i].button == ev.button
                    && self.CLEANMASK(Config::buttons[i].mask) == self.CLEANMASK(ev.state)
                {
                    if let Some(ref func) = Config::buttons[i].func {
                        info!(
                            "[buttonpress] click: {}, button: {}, mask: {}",
                            Config::buttons[i].click,
                            Config::buttons[i].button,
                            Config::buttons[i].mask
                        );
                        if let Arg::Ui(0) = Config::buttons[i].arg {
                            if click as u32 == CLICK::ClkTagBar as u32 {
                                info!("[buttonpress] use fresh arg");
                                func(self, &arg);
                                break;
                            }
                        }
                        info!("[buttonpress] use button arg");
                        func(self, &Config::buttons[i].arg);
                        break;
                    }
                }
            }
        }
    }

    pub fn checkotherwm(&mut self) {
        info!("[checkotherwm]");
        unsafe {
            xerrorxlib = XSetErrorHandler(Some(transmute(xerrorstart as *const ())));
            // this causes an error if some other window manager is running.
            XSelectInput(
                self.dpy,
                XDefaultRootWindow(self.dpy),
                SubstructureRedirectMask,
            );
            XSync(self.dpy, False);
            // Attention what transmut does is great;
            XSetErrorHandler(Some(transmute(xerror as *const ())));
            XSync(self.dpy, False);
        }
    }

    pub fn spawn(&mut self, arg: *const Arg) {
        info!("[spawn]");
        unsafe {
            let mut sa: sigaction = zeroed();

            let mut mut_arg: Arg = (*arg).clone();
            if let Arg::V(ref mut v) = mut_arg {
                if *v == *Config::dmenucmd {
                    let tmp = (b'0' + self.selmon.as_ref().unwrap().borrow_mut().num as u8) as char;
                    let tmp = tmp.to_string();
                    info!(
                        "[spawn] dmenumon tmp: {}, num: {}",
                        tmp,
                        self.selmon.as_ref().unwrap().borrow_mut().num
                    );
                    (*v)[2] = tmp;
                }
                if fork() == 0 {
                    if !self.dpy.is_null() {
                        close(XConnectionNumber(self.dpy));
                    }
                    setsid();

                    sigemptyset(&mut sa.sa_mask);
                    sa.sa_flags = 0;
                    sa.sa_sigaction = SIG_DFL;
                    sigaction(SIGCHLD, &sa, null_mut());

                    info!("[spawn] arg v: {:?}", v);
                    if let Err(val) = Command::new(&v[0]).args(&v[1..]).spawn() {
                        info!("[spawn] Command exited with error {:?}", val);
                    }
                }
            }
        }
    }
    pub fn updatebars(&mut self) {
        // info!("[updatebars]");
        unsafe {
            let mut wa: XSetWindowAttributes = zeroed();
            wa.override_redirect = True;
            wa.background_pixel = 0;
            wa.border_pixel = 0;
            wa.colormap = self.cmap;
            wa.event_mask = ButtonPressMask | ExposureMask;
            let mut ch: XClassHint = zeroed();
            let c_string = CString::new("jwm").expect("fail to convert");
            ch.res_name = c_string.as_ptr() as *mut _;
            ch.res_class = c_string.as_ptr() as *mut _;
            let mut m = self.mons.clone();
            while let Some(ref m_opt) = m {
                if m_opt.borrow_mut().barwin > 0 {
                    continue;
                }
                let wx = m_opt.borrow_mut().wx;
                let by = m_opt.borrow_mut().by;
                let ww = m_opt.borrow_mut().ww as u32;
                m_opt.borrow_mut().barwin = XCreateWindow(
                    self.dpy,
                    self.root,
                    wx + self.sp,
                    by + self.vp,
                    ww - 2 * self.sp as u32,
                    self.bh as u32,
                    0,
                    self.depth as i32,
                    InputOutput as u32,
                    self.visual,
                    CWOverrideRedirect | CWBackPixel | CWBorderPixel | CWColormap | CWEventMask,
                    &mut wa,
                );
                let barwin = m_opt.borrow_mut().barwin;
                XDefineCursor(
                    self.dpy,
                    barwin,
                    self.cursor[CUR::CurNormal as usize]
                        .as_ref()
                        .unwrap()
                        .cursor,
                );
                XMapRaised(self.dpy, barwin);
                XSetClassHint(self.dpy, barwin, &mut ch);
                let next = m_opt.borrow_mut().next.clone();
                m = next;
            }
        }
    }
    pub fn xinitvisual(&mut self) {
        unsafe {
            let mut tpl: XVisualInfo = zeroed();
            tpl.screen = self.screen;
            tpl.depth = 32;
            tpl.class = TrueColor;
            let masks = VisualScreenMask | VisualDepthMask | VisualClassMask;

            let mut nitems: i32 = 0;
            let infos = XGetVisualInfo(self.dpy, masks, &mut tpl, &mut nitems);
            self.visual = null_mut();
            for i in 0..nitems {
                let fmt =
                    XRenderFindVisualFormat(self.dpy, (*infos.wrapping_add(i as usize)).visual);
                if (*fmt).type_ == PictTypeDirect && (*fmt).direct.alphaMask > 0 {
                    self.visual = (*infos.wrapping_add(i as usize)).visual;
                    self.depth = (*infos.wrapping_add(i as usize)).depth;
                    self.cmap = XCreateColormap(self.dpy, self.root, self.visual, AllocNone);
                    self.useargb = true;
                    break;
                }
            }

            XFree(infos as *mut _);

            if self.visual.is_null() {
                self.visual = XDefaultVisual(self.dpy, self.screen);
                self.depth = XDefaultDepth(self.dpy, self.screen);
                self.cmap = XDefaultColormap(self.dpy, self.screen);
            }
        }
    }
    pub fn updatebarpos(&mut self, m: &mut Monitor) {
        // info!("[updatebarpos]");
        m.wy = m.my;
        m.wh = m.mh;
        if m.showbar0 {
            m.wh = m.wh - 3 * self.vp / 2 - self.bh;
            m.by = if m.topbar0 {
                m.wy
            } else {
                m.wy + m.wh + 3 * self.vp / 2
            };
            m.wy = if m.topbar0 {
                m.wy + self.bh + 3 * self.vp / 2
            } else {
                m.wy
            };
        } else {
            m.by = -self.bh - self.vp;
        }
    }
    pub fn updateclientlist(&mut self) {
        // info!("[updateclientlist]");
        unsafe {
            XDeleteProperty(
                self.dpy,
                self.root,
                self.netatom[NET::NetClientList as usize],
            );
            let mut m = self.mons.clone();
            while let Some(ref m_opt) = m {
                let mut c = m_opt.borrow_mut().clients.clone();
                while let Some(c_opt) = c {
                    XChangeProperty(
                        self.dpy,
                        self.root,
                        self.netatom[NET::NetClientList as usize],
                        XA_WINDOW,
                        32,
                        PropModeAppend,
                        &mut c_opt.borrow_mut().win as *const u64 as *const _,
                        1,
                    );
                    let next = c_opt.borrow_mut().next.clone();
                    c = next;
                }
                let next = m_opt.borrow_mut().next.clone();
                m = next;
            }
        }
    }
    pub fn tile(&mut self, m: *mut Monitor) {
        // info!("[tile]");
        let mut n: u32 = 0;
        let mut mfacts: f32 = 0.;
        let mut sfacts: f32 = 0.;
        unsafe {
            let mut c = self.nexttiled((*m).clients.clone());
            while c.is_some() {
                if n < (*m).nmaster0 {
                    mfacts += c.as_ref().unwrap().borrow_mut().cfact;
                } else {
                    sfacts += c.as_ref().unwrap().borrow_mut().cfact;
                }
                let next = self.nexttiled(c.as_ref().unwrap().borrow_mut().next.clone());
                c = next;
                n += 1;
            }
            if n == 0 {
                return;
            }

            let mw: u32;
            if n > (*m).nmaster0 {
                mw = if (*m).nmaster0 > 0 {
                    ((*m).ww as f32 * (*m).mfact0) as u32
                } else {
                    0
                };
            } else {
                mw = (*m).ww as u32;
            }
            let mut my: u32 = 0;
            let mut ty: u32 = 0;
            let mut i: u32 = 0;
            let mut h: u32;
            c = self.nexttiled((*m).clients.clone());
            while c.is_some() {
                if i < (*m).nmaster0 {
                    // h = ((*m).wh as u32 - my) / (n.min((*m).nmaster0) - i);
                    let cfact = c.as_ref().unwrap().borrow_mut().cfact;
                    h = (((*m).wh as u32 - my) as f32 * (cfact / mfacts)) as u32;
                    let bw = c.as_ref().unwrap().borrow_mut().bw;
                    self.resize(
                        c.as_ref().unwrap(),
                        (*m).wx,
                        (*m).wy + my as i32,
                        mw as i32 - (2 * bw),
                        h as i32 - (2 * bw),
                        false,
                    );
                    let height = c.as_ref().unwrap().borrow_mut().height() as u32;
                    if my + height < (*m).wh as u32 {
                        my += height;
                    }
                    mfacts -= cfact;
                } else {
                    // h = ((*m).wh as u32 - ty) / (n - i);
                    let cfact = c.as_ref().unwrap().borrow_mut().cfact;
                    h = (((*m).wh as u32 - ty) as f32 * (cfact / sfacts)) as u32;
                    let bw = c.as_ref().unwrap().borrow_mut().bw;
                    self.resize(
                        c.as_ref().unwrap(),
                        (*m).wx + mw as i32,
                        (*m).wy + ty as i32,
                        (*m).ww - mw as i32 - (2 * bw),
                        h as i32 - (2 * bw),
                        false,
                    );
                    let height = c.as_ref().unwrap().borrow_mut().height();
                    if ty as i32 + height < (*m).wh {
                        ty += height as u32;
                    }
                    sfacts -= cfact;
                }

                let next = self.nexttiled(c.as_ref().unwrap().borrow_mut().next.clone());
                c = next;
                i += 1;
            }
        }
    }
    pub fn togglebar(&mut self, _arg: *const Arg) {
        // info!("[togglebar]");
        unsafe {
            {
                let mut selmon_clone = self.selmon.clone();
                let mut selmon_mut = selmon_clone.as_mut().unwrap().borrow_mut();
                let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                selmon_mut.pertag.as_mut().unwrap().showbars[curtag] = !selmon_mut.showbar0;
                selmon_mut.showbar0 = selmon_mut.pertag.as_mut().unwrap().showbars[curtag];
                if !selmon_mut.showbar0 {
                    let _ = self.sender.send(1);
                    self.tags = generate_random_tags(Config::tags_length);
                }
                self.updatebarpos(&mut selmon_mut);
                XMoveResizeWindow(
                    self.dpy,
                    selmon_mut.barwin,
                    selmon_mut.wx + self.sp,
                    selmon_mut.by + self.vp,
                    (selmon_mut.ww - 2 * self.sp) as u32,
                    self.bh as u32,
                );
            }
            self.arrange(self.selmon.clone());
        }
    }
    pub fn togglefloating(&mut self, _arg: *const Arg) {
        // info!("[togglefloating]");
        if self.selmon.is_none() {
            return;
        }
        let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
        if let Some(ref sel_opt) = sel {
            // no support for fullscreen windows.
            let isfullscreen = { sel_opt.borrow_mut().isfullscreen };
            if isfullscreen {
                return;
            }
            {
                let isfloating = sel_opt.borrow_mut().isfloating;
                let isfixed = sel_opt.borrow_mut().isfixed;
                sel_opt.borrow_mut().isfloating = !isfloating || isfixed;
            }
            let isfloating = { sel_opt.borrow_mut().isfloating };
            if isfloating {
                let (x, y, w, h) = {
                    let sel_opt_mut = sel_opt.borrow_mut();
                    (sel_opt_mut.x, sel_opt_mut.y, sel_opt_mut.w, sel_opt_mut.h)
                };
                self.resize(sel_opt, x, y, w, h, false);
            }
            self.arrange(self.selmon.clone());
        } else {
            return;
        }
    }
    pub fn focusin(&mut self, e: *mut XEvent) {
        // info!("[focusin]");
        unsafe {
            let mut selmon_clone = self.selmon.clone();
            let selmon_mut = selmon_clone.as_mut().unwrap().borrow_mut();
            let ev = (*e).focus_change;
            if selmon_mut.sel.is_some()
                && ev.window != (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).win
            {
                self.setfocus(selmon_mut.sel.as_ref().unwrap());
            }
        }
    }
    pub fn focusmon(&mut self, arg: *const Arg) {
        // info!("[focusmon]");
        unsafe {
            if let Some(ref mons_opt) = self.mons {
                if mons_opt.borrow_mut().next.is_none() {
                    return;
                }
            }
            if let Arg::I(i) = *arg {
                let m = self.dirtomon(i);
                if Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap()) {
                    return;
                }
                let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
                self.unfocus(sel, false);
                self.selmon = m;
                self.focus(None);
            }
        }
    }
    pub fn tag(&mut self, arg: *const Arg) {
        // info!("[tag]");
        unsafe {
            if let Arg::Ui(ui) = *arg {
                let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
                if sel.is_some() && (ui & Config::tagmask) > 0 {
                    sel.as_ref().unwrap().borrow_mut().tags0 = ui & Config::tagmask;
                    self.setclienttagprop(sel.as_ref().unwrap());
                    self.focus(None);
                    self.arrange(self.selmon.clone());
                }
            }
        }
    }
    pub fn tagmon(&mut self, arg: *const Arg) {
        // info!("[tagmon]");
        unsafe {
            if let Some(ref selmon_opt) = self.selmon {
                if selmon_opt.borrow_mut().sel.is_none() {
                    return;
                }
            }
            if let Some(ref mons_opt) = self.mons {
                if mons_opt.borrow_mut().next.is_none() {
                    return;
                }
            }
            if let Arg::I(i) = *arg {
                let selmon_clone = self.selmon.clone();
                if let Some(ref selmon_opt) = selmon_clone {
                    let dir_i_mon = self.dirtomon(i);
                    let sel = { selmon_opt.borrow_mut().sel.clone() };
                    self.sendmon(sel, dir_i_mon);
                }
            }
        }
    }
    pub fn focusstack(&mut self, arg: *const Arg) {
        // info!("[focusstack]");
        unsafe {
            {
                let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                if selmon_mut.sel.is_none()
                    || (selmon_mut.sel.as_ref().unwrap().borrow_mut().isfullscreen
                        && Config::lockfullscreen)
                {
                    return;
                }
            }
            let mut c: Option<Rc<RefCell<Client>>> = None;
            let i = if let Arg::I(i) = *arg { i } else { 0 };
            if i > 0 {
                c = {
                    self.selmon
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .sel
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .next
                        .clone()
                };
                while c.is_some() {
                    if c.as_ref().unwrap().borrow_mut().isvisible() {
                        break;
                    }
                    let next = c.as_ref().unwrap().borrow_mut().next.clone();
                    c = next;
                }
                if c.is_none() {
                    c = {
                        let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                        selmon_mut.clients.clone()
                    };
                    while let Some(ref c_opt) = c {
                        if c_opt.borrow_mut().isvisible() {
                            break;
                        }
                        let next = c_opt.borrow_mut().next.clone();
                        c = next;
                    }
                }
            } else {
                let mut cl;
                let sel;
                {
                    let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                    cl = selmon_mut.clients.clone();
                    sel = selmon_mut.sel.clone();
                }
                while !Self::are_equal_rc(&cl, &sel) {
                    if cl.as_ref().unwrap().borrow_mut().isvisible() {
                        c = cl.clone();
                    }
                    let next = cl.as_ref().unwrap().borrow_mut().next.clone();
                    cl = next;
                }
                if c.is_none() {
                    while let Some(ref cl_opt) = cl {
                        if cl_opt.borrow_mut().isvisible() {
                            c = cl.clone();
                        }
                        let next = cl_opt.borrow_mut().next.clone();
                        cl = next;
                    }
                }
            }
            if c.is_some() {
                self.focus(c);
                self.restack(self.selmon.clone());
            }
        }
    }
    pub fn incnmaster(&mut self, arg: *const Arg) {
        // info!("[incnmaster]");
        unsafe {
            if let Arg::I(i) = *arg {
                let mut selmon_mut = self.selmon.as_mut().unwrap().borrow_mut();
                let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                selmon_mut.pertag.as_mut().unwrap().nmasters[curtag] =
                    0.max(selmon_mut.nmaster0 as i32 + i) as u32;

                selmon_mut.nmaster0 = selmon_mut.pertag.as_ref().unwrap().nmasters[curtag];
            }
            self.arrange(self.selmon.clone());
        }
    }
    pub fn setcfact(&mut self, arg: *const Arg) {
        // info!("[setcfact]");
        if arg.is_null() {
            return;
        }
        unsafe {
            let c = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
            if c.is_none() {
                return;
            }
            let lt_layout_type = {
                let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                selmon_mut.lt[selmon_mut.sellt].layout_type.clone()
            };
            if lt_layout_type.is_none() {
                return;
            }
            if let Arg::F(f0) = *arg {
                let mut f = f0 + c.as_ref().unwrap().borrow_mut().cfact;
                if f0.abs() < 0.0001 {
                    f = 1.0;
                } else if f < 0.25 || f > 4.0 {
                    return;
                }
                c.as_ref().unwrap().borrow_mut().cfact = f;
                self.arrange(self.selmon.clone());
            }
        }
    }
    pub fn movestack(&mut self, arg: *const Arg) {
        unsafe {
            let mut c: Option<Rc<RefCell<Client>>> = None;
            let mut i: Option<Rc<RefCell<Client>>>;
            let mut p: Option<Rc<RefCell<Client>>> = None;
            let mut pc: Option<Rc<RefCell<Client>>> = None;
            if let Arg::I(arg_i) = *arg {
                if arg_i > 0 {
                    // Find the client after selmon->sel
                    c = self
                        .selmon
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .sel
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .next
                        .clone();
                    while c.is_some() {
                        let isvisible = c.as_ref().unwrap().borrow_mut().isvisible();
                        let isfloating = c.as_ref().unwrap().borrow_mut().isfloating;
                        let condition = !isvisible || isfloating;
                        if !condition {
                            break;
                        }
                        let next = c.as_ref().unwrap().borrow_mut().next.clone();
                        c = next;
                    }
                    if c.is_none() {
                        c = self.selmon.as_ref().unwrap().borrow_mut().clients.clone();
                    }
                    while c.is_some() {
                        let isvisible = c.as_ref().unwrap().borrow_mut().isvisible();
                        let isfloating = c.as_ref().unwrap().borrow_mut().isfloating;
                        let condition = !isvisible || isfloating;
                        if !condition {
                            break;
                        }
                        let next = c.as_ref().unwrap().borrow_mut().next.clone();
                        c = next;
                    }
                } else {
                    // Find the client before selmon->sel
                    i = self.selmon.as_ref().unwrap().borrow_mut().clients.clone();
                    let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
                    while !Self::are_equal_rc(&i, &sel) {
                        let isvisible = i.as_ref().unwrap().borrow_mut().isvisible();
                        let isfloating = i.as_ref().unwrap().borrow_mut().isfloating;
                        if isvisible && !isfloating {
                            c = i.clone();
                        }
                        let next = i.as_ref().unwrap().borrow_mut().next.clone();
                        i = next;
                    }
                    if c.is_none() {
                        while i.is_some() {
                            let isvisible = i.as_ref().unwrap().borrow_mut().isvisible();
                            let isfloating = i.as_ref().unwrap().borrow_mut().isfloating;
                            if isvisible && !isfloating {
                                c = i.clone();
                            }
                            let next = i.as_ref().unwrap().borrow_mut().next.clone();
                            i = next;
                        }
                    }
                }
                // Find the client before selmon->sel and c
                i = self.selmon.as_ref().unwrap().borrow_mut().clients.clone();
                let sel = self.selmon.as_ref().unwrap().borrow_mut().sel.clone();
                while i.is_some() && (p.is_none() || pc.is_none()) {
                    let next = i.as_ref().unwrap().borrow_mut().next.clone();
                    if next.is_some() && sel.is_some() && Self::are_equal_rc(&next, &sel) {
                        p = i.clone();
                    }
                    if next.is_some() && c.is_some() && Self::are_equal_rc(&next, &c) {
                        pc = i.clone();
                    }
                    i = next;
                }
                // Swap c and selmon->sel selmon->clietns in the selmon->clients list
                if c.is_some() && sel.is_some() && !Self::are_equal_rc(&c, &sel) {
                    let sel_next = self
                        .selmon
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .sel
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .next
                        .clone();
                    let temp =
                        if sel_next.is_some() && c.is_some() && Self::are_equal_rc(&sel_next, &c) {
                            self.selmon.as_ref().unwrap().borrow_mut().sel.clone()
                        } else {
                            sel_next
                        };
                    let sel = self.selmon.as_ref().unwrap().borrow_mut().sel.clone();
                    let c_next = c.as_ref().unwrap().borrow_mut().next.clone();
                    self.selmon
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .sel
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .next =
                        if c_next.is_some() && sel.is_some() && Self::are_equal_rc(&c_next, &sel) {
                            c.clone()
                        } else {
                            c_next
                        };
                    c.as_ref().unwrap().borrow_mut().next = temp;

                    if p.is_some() && !Self::are_equal_rc(&p, &c) {
                        p.as_ref().unwrap().borrow_mut().next = c.clone();
                    }
                    if pc.is_some() {
                        let sel = self.selmon.as_ref().unwrap().borrow_mut().sel.clone();
                        if !Self::are_equal_rc(&pc, &sel) {
                            pc.as_ref().unwrap().borrow_mut().next = sel;
                        }
                    }

                    let sel = self.selmon.as_ref().unwrap().borrow_mut().sel.clone();
                    let clients = self.selmon.as_ref().unwrap().borrow_mut().clients.clone();
                    if Self::are_equal_rc(&sel, &clients) {
                        self.selmon.as_ref().unwrap().borrow_mut().clients = c;
                    } else if Self::are_equal_rc(&c, &clients) {
                        self.selmon.as_ref().unwrap().borrow_mut().clients = sel;
                    }

                    self.arrange(self.selmon.clone());
                }
            } else {
                return;
            }
        }
    }
    pub fn setmfact(&mut self, arg: *const Arg) {
        // info!("[setmfact]");
        unsafe {
            let lt_layout_type = {
                let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                selmon_mut.lt[selmon_mut.sellt].layout_type.clone()
            };
            if arg.is_null() || lt_layout_type.is_none() {
                return;
            }
            if let Arg::F(f) = *arg {
                let mut selmon_mut = self.selmon.as_mut().unwrap().borrow_mut();
                let f = if f < 1.0 {
                    f + selmon_mut.mfact0
                } else {
                    f - 1.0
                };
                if f < 0.05 || f > 0.95 {
                    return;
                }
                let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                selmon_mut.pertag.as_mut().unwrap().mfacts[curtag] = f;
                selmon_mut.mfact0 = selmon_mut.pertag.as_mut().unwrap().mfacts[curtag];
            }
            self.arrange(self.selmon.clone());
        }
    }
    pub fn setlayout(&mut self, arg: *const Arg) {
        // info!("[setlayout]");
        unsafe {
            let sel;
            {
                let mut selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                let sellt = selmon_mut.sellt;
                if arg.is_null()
                    || !if let Arg::Lt(ref lt) = *arg {
                        Rc::ptr_eq(lt, &selmon_mut.lt[sellt])
                    } else {
                        false
                    }
                {
                    let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                    selmon_mut.pertag.as_mut().unwrap().sellts[curtag] ^= 1;
                    selmon_mut.sellt = selmon_mut.pertag.as_ref().unwrap().sellts[curtag];
                }
                if !arg.is_null() {
                    if let Arg::Lt(ref lt) = *arg {
                        let sellt = selmon_mut.sellt;
                        let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                        selmon_mut.pertag.as_mut().unwrap().ltidxs[curtag][sellt] =
                            Some(lt.clone());
                        selmon_mut.lt[sellt] = selmon_mut.pertag.as_mut().unwrap().ltidxs[curtag]
                            [sellt]
                            .clone()
                            .expect("None unwrap");
                    }
                }
                selmon_mut.ltsymbol = selmon_mut.lt[selmon_mut.sellt].symbol.to_string();
                sel = selmon_mut.sel.clone();
            }
            if sel.is_some() {
                self.arrange(self.selmon.clone());
            } else {
                self.drawbar(self.selmon.clone());
            }
        }
    }
    pub fn zoom(&mut self, _arg: *const Arg) {
        // info!("[zoom]");
        let mut c;
        let sel_c;
        let nexttiled_c;
        {
            let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
            c = selmon_mut.sel.clone();
            let sellt = selmon_mut.sellt;
            if selmon_mut.lt[sellt].layout_type.is_none()
                || c.is_none()
                || c.as_ref().unwrap().borrow_mut().isfloating
            {
                return;
            }
            sel_c = selmon_mut.clients.clone();
        }
        {
            nexttiled_c = self.nexttiled(sel_c);
        }
        if Self::are_equal_rc(&c, &nexttiled_c) {
            let next = self.nexttiled(c.as_ref().unwrap().borrow_mut().next.clone());
            c = next;
            if c.is_none() {
                return;
            }
        }
        self.pop(c);
    }
    pub fn view(&mut self, arg: *const Arg) {
        // info!("[view]");
        unsafe {
            if let Arg::Ui(ui) = *arg {
                info!("[view] ui: {}", ui);
                let mut selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                if (ui & Config::tagmask) == selmon_mut.tagset[selmon_mut.seltags] {
                    return;
                }
                // toggle sel tagset.
                selmon_mut.seltags ^= 1;
                if ui & Config::tagmask > 0 {
                    let seltags = selmon_mut.seltags;
                    selmon_mut.tagset[seltags] = ui & Config::tagmask;

                    let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                    selmon_mut.pertag.as_mut().unwrap().prevtag = curtag;

                    if ui == !0 {
                        selmon_mut.pertag.as_mut().unwrap().curtag = 0;
                    } else {
                        let mut i = 0;
                        loop {
                            let condition = ui & 1 << i;
                            if condition > 0 {
                                break;
                            }
                            i += 1;
                        }
                        selmon_mut.pertag.as_mut().unwrap().curtag = i + 1;
                    }
                } else {
                    let tmptag = selmon_mut.pertag.as_mut().unwrap().prevtag;
                    selmon_mut.pertag.as_mut().unwrap().prevtag =
                        selmon_mut.pertag.as_ref().unwrap().curtag;
                    selmon_mut.pertag.as_mut().unwrap().curtag = tmptag;
                }
            } else {
                return;
            }
            let sel = {
                let condition;
                let curtag;
                {
                    let mut selmon_clone = self.selmon.clone();
                    let mut selmon_mut = selmon_clone.as_mut().unwrap().borrow_mut();
                    curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                    selmon_mut.nmaster0 = selmon_mut.pertag.as_ref().unwrap().nmasters[curtag];
                    selmon_mut.mfact0 = selmon_mut.pertag.as_ref().unwrap().mfacts[curtag];
                    selmon_mut.sellt = selmon_mut.pertag.as_ref().unwrap().sellts[curtag];
                    let sellt = selmon_mut.sellt;
                    selmon_mut.lt[sellt] = selmon_mut.pertag.as_ref().unwrap().ltidxs[curtag]
                        [sellt]
                        .clone()
                        .expect("None unwrap");
                    selmon_mut.lt[sellt ^ 1] = selmon_mut.pertag.as_ref().unwrap().ltidxs[curtag]
                        [sellt ^ 1]
                        .clone()
                        .expect("None unwrap");

                    condition =
                        selmon_mut.showbar0 != selmon_mut.pertag.as_ref().unwrap().showbars[curtag];
                }
                if condition {
                    self.togglebar(null_mut());
                }
                self.selmon
                    .as_mut()
                    .unwrap()
                    .borrow_mut()
                    .pertag
                    .as_ref()
                    .unwrap()
                    .sel[curtag]
                    .clone()
            };
            self.focus(sel);
            self.arrange(self.selmon.clone());
        }
    }
    pub fn toggleview(&mut self, arg: *const Arg) {
        info!("[toggleview]");
        unsafe {
            if let Arg::Ui(ui) = *arg {
                if self.selmon.is_none() {
                    return;
                }
                let seltags;
                let newtagset;
                {
                    let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                    seltags = selmon_mut.seltags;
                    newtagset = selmon_mut.tagset[seltags] ^ (ui & Config::tagmask);
                }
                if newtagset > 0 {
                    {
                        let mut selmon_clone = self.selmon.clone();
                        let mut selmon_mut = selmon_clone.as_mut().unwrap().borrow_mut();
                        selmon_mut.tagset[seltags] = newtagset;

                        if newtagset == !0 {
                            selmon_mut.pertag.as_mut().unwrap().prevtag =
                                selmon_mut.pertag.as_ref().unwrap().curtag;
                            selmon_mut.pertag.as_mut().unwrap().curtag = 0;
                        }

                        // test if the user did not select the same tag
                        let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                        if newtagset & 1 << (curtag - 1) <= 0 {
                            selmon_mut.pertag.as_mut().unwrap().prevtag = curtag;
                            let mut i = 0;
                            loop {
                                let condition = newtagset & 1 << i;
                                if condition > 0 {
                                    break;
                                }
                                i += 1;
                            }
                            selmon_mut.pertag.as_mut().unwrap().curtag = i + 1;
                        }

                        // apply settings for this view
                        let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                        selmon_mut.nmaster0 = selmon_mut.pertag.as_ref().unwrap().nmasters[curtag];
                        selmon_mut.mfact0 = selmon_mut.pertag.as_ref().unwrap().mfacts[curtag];
                        selmon_mut.sellt = selmon_mut.pertag.as_ref().unwrap().sellts[curtag];
                        let sellt = selmon_mut.sellt;
                        selmon_mut.lt[sellt] = selmon_mut.pertag.as_ref().unwrap().ltidxs[curtag]
                            [sellt]
                            .clone()
                            .expect("None unwrap");
                        selmon_mut.lt[sellt ^ 1] = selmon_mut.pertag.as_ref().unwrap().ltidxs
                            [curtag][sellt ^ 1]
                            .clone()
                            .expect("None unwrap");

                        if selmon_mut.showbar0
                            != selmon_mut.pertag.as_ref().unwrap().showbars[curtag]
                        {
                            self.togglebar(null_mut());
                        }
                    }
                    self.focus(None);
                    self.arrange(self.selmon.clone());
                }
            }
        }
    }
    pub fn togglefullscr(&mut self, _: *const Arg) {
        info!("[togglefullscr]");
        if let Some(ref selmon_opt) = self.selmon {
            let sel = { selmon_opt.borrow_mut().sel.clone() };
            if sel.is_none() {
                return;
            }
            let isfullscreen = { sel.as_ref().unwrap().borrow_mut().isfullscreen };
            self.setfullscreen(sel.as_ref().unwrap(), !isfullscreen);
        }
    }
    pub fn toggletag(&mut self, arg: *const Arg) {
        info!("[toggletag]");
        unsafe {
            let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
            if sel.is_none() {
                return;
            }
            if let Arg::Ui(ui) = *arg {
                let newtags = sel.as_ref().unwrap().borrow_mut().tags0 ^ (ui & Config::tagmask);
                if newtags > 0 {
                    sel.as_ref().unwrap().borrow_mut().tags0 = newtags;
                    self.setclienttagprop(sel.as_ref().unwrap());
                    self.focus(None);
                    self.arrange(self.selmon.clone());
                }
            }
        }
    }
    pub fn quit(&mut self, _arg: *const Arg) {
        // info!("[quit]");
        self.running.store(false, Ordering::SeqCst);
        let _ = self.sender.send(0);
    }
    pub fn setup(&mut self) {
        // info!("[setup]");
        unsafe {
            let mut wa: XSetWindowAttributes = zeroed();
            let mut sa: sigaction = zeroed();
            //do not transform children into zombies whien they terminate
            sigemptyset(&mut sa.sa_mask);
            sa.sa_flags = SA_NOCLDSTOP | SA_NOCLDWAIT | SA_RESTART;
            sa.sa_sigaction = SIG_IGN;
            sigaction(SIGCHLD, &sa, null_mut());

            // clean up any zombies (inherited from .xinitrc etc) immediately
            while waitpid(-1, null_mut(), WNOHANG) > 0 {}

            // init screen
            self.screen = XDefaultScreen(self.dpy);
            self.sw = XDisplayWidth(self.dpy, self.screen);
            self.sh = XDisplayHeight(self.dpy, self.screen);
            self.root = XRootWindow(self.dpy, self.screen);
            self.xinitvisual();
            self.drw = Some(Box::new(Drw::drw_create(
                self.dpy,
                self.screen,
                self.root,
                self.sw as u32,
                self.sh as u32,
                self.visual,
                self.depth,
                self.cmap,
            )));
            // info!("[setup] drw_fontset_create");
            if !self
                .drw
                .as_mut()
                .unwrap()
                .as_mut()
                .drw_font_create(Config::font)
            {
                eprintln!("no fonts could be loaded");
                exit(0);
            }
            {
                let h = self
                    .drw
                    .as_ref()
                    .unwrap()
                    .font
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .h as i32;
                self.drw.as_mut().unwrap().lrpad = h;
                self.bh = h + Config::vertpadbar;
            }
            self.sp = Config::sidepad;
            self.vp = if Config::topbar {
                Config::vertpad
            } else {
                -Config::vertpad
            };
            // info!("[setup] updategeom");
            self.updategeom();
            // init atoms
            let mut c_string = CString::new("UTF8_STRING").expect("fail to convert");
            let utf8string = XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("WM_PROTOCOLS").expect("fail to convert");
            self.wmatom[WM::WMProtocols as usize] = XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("WM_DELETE_WINDOW").expect("fail to convert");
            self.wmatom[WM::WMDelete as usize] = XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("WM_STATE").expect("fail to convert");
            self.wmatom[WM::WMState as usize] = XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("WM_TAKE_FOCUS").expect("fail to convert");
            self.wmatom[WM::WMTakeFocus as usize] = XInternAtom(self.dpy, c_string.as_ptr(), False);

            c_string = CString::new("_NET_ACTIVE_WINDOW").expect("fail to convert");
            self.netatom[NET::NetActiveWindow as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_SUPPORTED").expect("fail to convert");
            self.netatom[NET::NetSupported as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_NAME").expect("fail to convert");
            self.netatom[NET::NetWMName as usize] = XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_STATE").expect("fail to convert");
            self.netatom[NET::NetWMState as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_SUPPORTING_WM_CHECK").expect("fail to convert");
            self.netatom[NET::NetWMCheck as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_STATE_FULLSCREEN").expect("fail to convert");
            self.netatom[NET::NetWMFullscreen as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_WINDOW_TYPE").expect("fail to convert");
            self.netatom[NET::NetWMWindowType as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_WINDOW_TYPE_DIALOG").expect("fail to convert");
            self.netatom[NET::NetWMWindowTypeDialog as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_CLIENT_LIST").expect("fail to convert");
            self.netatom[NET::NetClientList as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_CLIENT_INFO").expect("fail to convert");
            self.netatom[NET::NetClientInfo as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);

            // init cursors
            self.cursor[CUR::CurNormal as usize] = self
                .drw
                .as_mut()
                .unwrap()
                .as_mut()
                .drw_cur_create(XC_left_ptr as i32);
            self.cursor[CUR::CurResize as usize] = self
                .drw
                .as_mut()
                .unwrap()
                .as_mut()
                .drw_cur_create(XC_sizing as i32);
            self.cursor[CUR::CurMove as usize] = self
                .drw
                .as_mut()
                .unwrap()
                .as_mut()
                .drw_cur_create(XC_fleur as i32);
            // init appearance
            self.scheme = vec![vec![]; Config::colors.len()];
            for i in 0..Config::colors.len() {
                self.scheme[i] = self.drw.as_mut().unwrap().drw_scm_create(
                    Config::colors[i],
                    &Config::alphas[i],
                    3,
                );
            }
            // init bars
            // info!("[setup] updatebars");
            self.updatebars();
            // info!("[setup] updatestatus");
            self.updatestatus();
            // supporting window fot NetWMCheck
            self.wmcheckwin = XCreateSimpleWindow(self.dpy, self.root, 0, 0, 1, 1, 0, 0, 0);
            XChangeProperty(
                self.dpy,
                self.wmcheckwin,
                self.netatom[NET::NetWMCheck as usize],
                XA_WINDOW,
                32,
                PropModeReplace,
                &mut self.wmcheckwin as *mut u64 as *const _,
                1,
            );
            c_string = CString::new("jwm").unwrap();
            XChangeProperty(
                self.dpy,
                self.wmcheckwin,
                self.netatom[NET::NetWMName as usize],
                utf8string,
                8,
                PropModeReplace,
                c_string.as_ptr() as *const _,
                1,
            );
            XChangeProperty(
                self.dpy,
                self.root,
                self.netatom[NET::NetWMCheck as usize],
                XA_WINDOW,
                32,
                PropModeReplace,
                &mut self.wmcheckwin as *mut u64 as *const _,
                1,
            );
            // EWMH support per view
            XChangeProperty(
                self.dpy,
                self.root,
                self.netatom[NET::NetSupported as usize],
                XA_ATOM,
                32,
                PropModeReplace,
                self.netatom.as_ptr() as *const _,
                NET::NetLast as i32,
            );
            XDeleteProperty(
                self.dpy,
                self.root,
                self.netatom[NET::NetClientList as usize],
            );
            XDeleteProperty(
                self.dpy,
                self.root,
                self.netatom[NET::NetClientInfo as usize],
            );
            // select events
            wa.cursor = self.cursor[CUR::CurNormal as usize]
                .as_ref()
                .unwrap()
                .cursor;
            wa.event_mask = SubstructureRedirectMask
                | SubstructureNotifyMask
                | ButtonPressMask
                | PointerMotionMask
                | EnterWindowMask
                | LeaveWindowMask
                | StructureNotifyMask
                | PropertyChangeMask;
            XChangeWindowAttributes(self.dpy, self.root, CWEventMask | CWCursor, &mut wa);
            XSelectInput(self.dpy, self.root, wa.event_mask);
            // info!("[setup] grabkeys");
            self.grabkeys();
            // info!("[setup] focus");
            self.focus(None);
        }
    }
    pub fn killclient(&mut self, _arg: *const Arg) {
        info!("[killclient]");
        unsafe {
            let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
            if sel.is_none() {
                return;
            }
            info!("[killclient] {}", sel.as_ref().unwrap().borrow_mut());
            if !self.sendevent(
                &mut sel.as_ref().unwrap().borrow_mut(),
                self.wmatom[WM::WMDelete as usize],
            ) {
                XGrabServer(self.dpy);
                XSetErrorHandler(Some(transmute(xerrordummy as *const ())));
                XSetCloseDownMode(self.dpy, DestroyAll);
                XKillClient(self.dpy, sel.as_ref().unwrap().borrow_mut().win);
                XSync(self.dpy, False);
                XSetErrorHandler(Some(transmute(xerror as *const ())));
                XUngrabServer(self.dpy);
            }
        }
    }
    pub fn nexttiled(&mut self, mut c: Option<Rc<RefCell<Client>>>) -> Option<Rc<RefCell<Client>>> {
        // info!("[nexttiled]");
        while let Some(ref c_opt) = c {
            let isfloating = c_opt.borrow_mut().isfloating;
            let isvisible = c_opt.borrow_mut().isvisible();
            if isfloating || !isvisible {
                let next = c_opt.borrow_mut().next.clone();
                c = next;
            } else {
                break;
            }
        }
        return c;
    }
    pub fn pop(&mut self, c: Option<Rc<RefCell<Client>>>) {
        // info!("[pop]");
        self.detach(c.clone());
        self.attach(c.clone());
        self.focus(c.clone());
        let mon = { c.as_ref().unwrap().borrow_mut().mon.clone() };
        self.arrange(mon);
    }
    pub fn propertynotify(&mut self, e: *mut XEvent) {
        // info!("[propertynotify]");
        unsafe {
            let c: Option<Rc<RefCell<Client>>>;
            let ev = (*e).property;
            let mut trans: Window = 0;
            if ev.window == self.root && ev.atom == XA_WM_NAME {
                self.updatestatus();
            } else if ev.state == PropertyDelete {
                // ignore
                return;
            } else if {
                c = self.wintoclient(ev.window);
                c.is_some()
            } {
                match ev.atom {
                    XA_WM_TRANSIENT_FOR => {
                        if !c.as_ref().unwrap().borrow_mut().isfloating
                            && XGetTransientForHint(
                                self.dpy,
                                c.as_ref().unwrap().borrow_mut().win,
                                &mut trans,
                            ) > 0
                            && {
                                c.as_ref().unwrap().borrow_mut().isfloating =
                                    self.wintoclient(trans).is_some();
                                c.as_ref().unwrap().borrow_mut().isfloating
                            }
                        {
                            self.arrange(c.as_ref().unwrap().borrow_mut().mon.clone());
                        }
                    }
                    XA_WM_NORMAL_HINTS => {
                        c.as_ref().unwrap().borrow_mut().hintsvalid = false;
                    }
                    XA_WM_HINTS => {
                        self.updatewmhints(c.as_ref().unwrap());
                        self.drawbars();
                    }
                    _ => {}
                }
                if ev.atom == XA_WM_NAME || ev.atom == self.netatom[NET::NetWMName as usize] {
                    self.updatetitle(c.as_ref().unwrap());
                    let sel = {
                        c.as_ref()
                            .unwrap()
                            .borrow_mut()
                            .mon
                            .as_ref()
                            .unwrap()
                            .borrow_mut()
                            .sel
                            .clone()
                    };
                    if c.is_some() && sel.is_some() && Self::are_equal_rc(&c, &sel) {
                        let mon = { c.as_ref().unwrap().borrow_mut().mon.clone() };
                        self.drawbar(mon);
                    }
                }
                if ev.atom == self.netatom[NET::NetWMWindowType as usize] {
                    self.updatewindowtype(c.as_ref().unwrap());
                }
            }
        }
    }
    pub fn movemouse(&mut self, _arg: *const Arg) {
        info!("[movemouse]");
        unsafe {
            let c = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
            if c.is_none() {
                return;
            }
            {
                info!("[movemouse] {}", c.as_ref().unwrap().borrow_mut().name);
            }
            if c.as_ref().unwrap().borrow_mut().isfullscreen {
                // no support moving fullscreen windows by mouse
                return;
            }
            self.restack(self.selmon.clone());
            let ocx = c.as_ref().unwrap().borrow_mut().x;
            let ocy = c.as_ref().unwrap().borrow_mut().y;
            if XGrabPointer(
                self.dpy,
                self.root,
                False,
                MOUSEMASK as u32,
                GrabModeAsync,
                GrabModeAsync,
                0,
                self.cursor[CUR::CurMove as usize].as_ref().unwrap().cursor,
                CurrentTime,
            ) != GrabSuccess
            {
                return;
            }
            let mut x: i32 = 0;
            let mut y: i32 = 0;
            let mut lasttime: Time = 0;
            let root_ptr = self.getrootptr(&mut x, &mut y);
            info!("[movemouse] root_ptr: {}", root_ptr);
            if root_ptr <= 0 {
                return;
            }
            let mut ev: XEvent = zeroed();
            loop {
                XMaskEvent(
                    self.dpy,
                    MOUSEMASK | ExposureMask | SubstructureRedirectMask,
                    &mut ev,
                );
                match ev.type_ {
                    ConfigureRequest | Expose | MapRequest => {
                        self.handler(ev.type_, &mut ev);
                    }
                    MotionNotify => {
                        // info!("[movemouse] MotionNotify");
                        if ev.motion.time - lasttime <= (1000 / 60) {
                            continue;
                        }
                        lasttime = ev.motion.time;

                        let wx;
                        let wy;
                        let ww;
                        let wh;
                        {
                            let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                            wx = selmon_mut.wx;
                            wy = selmon_mut.wy;
                            ww = selmon_mut.ww;
                            wh = selmon_mut.wh;
                        }
                        let mut nx = ocx + ev.motion.x - x;
                        let mut ny = ocy + ev.motion.y - y;
                        let width = { c.as_ref().unwrap().borrow_mut().width() };
                        let height = { c.as_ref().unwrap().borrow_mut().height() };
                        if (wx - nx).abs() < Config::snap as i32 {
                            nx = wx;
                        } else if ((wx + ww) - (nx + width)).abs() < Config::snap as i32 {
                            nx = wx + ww - width;
                        }
                        if (wy - ny).abs() < Config::snap as i32 {
                            ny = wy;
                        } else if ((wy + wh) - (ny + height)).abs() < Config::snap as i32 {
                            ny = wy + wh - height;
                        }
                        let isfloating = c.as_ref().unwrap().borrow_mut().isfloating;
                        let x = c.as_ref().unwrap().borrow_mut().x;
                        let y = c.as_ref().unwrap().borrow_mut().y;
                        let arrange = {
                            let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                            selmon_mut.lt[selmon_mut.sellt].layout_type.clone()
                        };
                        if !isfloating
                            && arrange.is_some()
                            && ((nx - x).abs() > Config::snap as i32
                                || (ny - y).abs() > Config::snap as i32)
                        {
                            self.togglefloating(null_mut());
                        }
                        let w = c.as_ref().unwrap().borrow_mut().w;
                        let h = c.as_ref().unwrap().borrow_mut().h;
                        if arrange.is_none() || c.as_ref().unwrap().borrow_mut().isfloating {
                            self.resize(c.as_ref().unwrap(), nx, ny, w, h, true);
                        }
                    }
                    _ => {}
                }
                if ev.type_ == ButtonRelease {
                    break;
                }
            }
            XUngrabPointer(self.dpy, CurrentTime);
            let x;
            let y;
            let w;
            let h;
            {
                x = c.as_ref().unwrap().borrow_mut().x;
                y = c.as_ref().unwrap().borrow_mut().y;
                w = c.as_ref().unwrap().borrow_mut().w;
                h = c.as_ref().unwrap().borrow_mut().h;
            }
            let m = self.recttomon(x, y, w, h);
            if !Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap()) {
                self.sendmon(c, m.clone());
                self.selmon = m;
                self.focus(None);
            }
        }
    }
    pub fn resizemouse(&mut self, _arg: *const Arg) {
        info!("[resizemouse]");
        unsafe {
            let c = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
            if c.is_none() {
                return;
            }
            if c.as_ref().unwrap().borrow_mut().isfullscreen {
                // no support mmoving fullscreen windows by mouse
                return;
            }
            self.restack(self.selmon.clone());
            let ocx = c.as_ref().unwrap().borrow_mut().x;
            let ocy = c.as_ref().unwrap().borrow_mut().y;
            if XGrabPointer(
                self.dpy,
                self.root,
                False,
                MOUSEMASK as u32,
                GrabModeAsync,
                GrabModeAsync,
                0,
                self.cursor[CUR::CurResize as usize]
                    .as_ref()
                    .unwrap()
                    .cursor,
                CurrentTime,
            ) != GrabSuccess
            {
                return;
            }
            let win;
            let w;
            let h;
            let bw;
            {
                win = c.as_ref().unwrap().borrow_mut().win;
                w = c.as_ref().unwrap().borrow_mut().w;
                bw = c.as_ref().unwrap().borrow_mut().bw;
                h = c.as_ref().unwrap().borrow_mut().h;
            }
            XWarpPointer(self.dpy, 0, win, 0, 0, 0, 0, w + bw - 1, h + bw - 1);
            let mut lasttime: Time = 0;
            let mut ev: XEvent = zeroed();
            loop {
                XMaskEvent(
                    self.dpy,
                    MOUSEMASK | ExposureMask | SubstructureRedirectMask,
                    &mut ev,
                );
                match ev.type_ {
                    ConfigureRequest | Expose | MapRequest => {
                        self.handler(ev.type_, &mut ev);
                    }
                    MotionNotify => {
                        if ev.motion.time - lasttime <= (1000 / 60) {
                            continue;
                        }
                        lasttime = ev.motion.time;
                        let nw = (ev.motion.x - ocx - 2 * bw + 1).max(1);
                        let nh = (ev.motion.y - ocy - 2 * bw + 1).max(1);
                        let wx;
                        let wy;
                        let ww;
                        let wh;
                        {
                            let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                            wx = selmon_mut.wx;
                            wy = selmon_mut.wy;
                            ww = selmon_mut.ww;
                            wh = selmon_mut.wh;
                        }
                        let mon_wx;
                        let mon_wy;
                        {
                            mon_wx = c
                                .as_ref()
                                .unwrap()
                                .borrow_mut()
                                .mon
                                .as_ref()
                                .unwrap()
                                .borrow_mut()
                                .wx;
                            mon_wy = c
                                .as_ref()
                                .unwrap()
                                .borrow_mut()
                                .mon
                                .as_ref()
                                .unwrap()
                                .borrow_mut()
                                .wy;
                        }
                        let layout_type = {
                            let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                            selmon_mut.lt[selmon_mut.sellt].layout_type.clone()
                        };
                        if mon_wx + nw >= wx
                            && mon_wx + nw <= wx + ww
                            && mon_wy + nh >= wy
                            && mon_wy + nh <= wy + wh
                        {
                            let isfloating = { c.as_ref().unwrap().borrow_mut().isfloating };
                            if !isfloating
                                && layout_type.is_some()
                                && ((nw - (*c.as_ref().unwrap().borrow_mut()).w).abs()
                                    > Config::snap as i32
                                    || (nh - (*c.as_ref().unwrap().borrow_mut()).h).abs()
                                        > Config::snap as i32)
                            {
                                self.togglefloating(null_mut());
                            }
                        }
                        let isfloating = c.as_ref().unwrap().borrow_mut().isfloating;
                        let x = c.as_ref().unwrap().borrow_mut().x;
                        let y = c.as_ref().unwrap().borrow_mut().y;
                        if layout_type.is_none() || isfloating {
                            self.resize(c.as_ref().unwrap(), x, y, nw, nh, true);
                        }
                    }
                    _ => {}
                }
                if ev.type_ == ButtonRelease {
                    break;
                }
            }
            let win;
            let w;
            let h;
            let x;
            let y;
            let bw;
            {
                win = c.as_ref().unwrap().borrow_mut().win;
                w = c.as_ref().unwrap().borrow_mut().w;
                h = c.as_ref().unwrap().borrow_mut().h;
                x = c.as_ref().unwrap().borrow_mut().x;
                y = c.as_ref().unwrap().borrow_mut().y;
                bw = c.as_ref().unwrap().borrow_mut().bw;
            }
            XWarpPointer(self.dpy, 0, win, 0, 0, 0, 0, w + bw - 1, h + bw - 1);
            XUngrabPointer(self.dpy, CurrentTime);
            while XCheckMaskEvent(self.dpy, EnterWindowMask, &mut ev) > 0 {}
            let m = self.recttomon(x, y, w, h);
            if !Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap()) {
                self.sendmon(c, m.clone());
                self.selmon = m;
                self.focus(None);
            }
        }
    }
    pub fn updatenumlockmask(&mut self) {
        // info!("[updatenumlockmask]");
        unsafe {
            self.numlockmask = 0;
            let modmap = XGetModifierMapping(self.dpy);
            for i in 0..8 {
                for j in 0..(*modmap).max_keypermod {
                    if *(*modmap)
                        .modifiermap
                        .wrapping_add((i * (*modmap).max_keypermod + j) as usize)
                        == XKeysymToKeycode(self.dpy, XK_Num_Lock as u64)
                    {
                        self.numlockmask = 1 << i;
                    }
                }
            }
            XFreeModifiermap(modmap);
        }
    }
    pub fn setclienttagprop(&mut self, c: &Rc<RefCell<Client>>) {
        let c_mut = c.borrow_mut();
        let data: [u8; 2] = [
            c_mut.tags0 as u8,
            c_mut.mon.as_ref().unwrap().borrow_mut().num as u8,
        ];
        unsafe {
            XChangeProperty(
                self.dpy,
                c_mut.win,
                self.netatom[NET::NetClientInfo as usize],
                XA_CARDINAL,
                32,
                PropModeReplace,
                data.as_ptr(),
                2,
            );
        }
    }
    pub fn grabbuttons(&mut self, c: Option<Rc<RefCell<Client>>>, focused: bool) {
        // info!("[grabbuttons]");
        self.updatenumlockmask();
        unsafe {
            let modifiers = [0, LockMask, self.numlockmask, self.numlockmask | LockMask];
            let c = c.as_ref().unwrap().borrow_mut();
            XUngrabButton(self.dpy, AnyButton as u32, AnyModifier, c.win);
            if !focused {
                XGrabButton(
                    self.dpy,
                    AnyButton as u32,
                    AnyModifier,
                    c.win,
                    False,
                    BUTTONMASK as u32,
                    GrabModeSync,
                    GrabModeSync,
                    0,
                    0,
                );
            }
            for i in 0..Config::buttons.len() {
                if Config::buttons[i].click == CLICK::ClkClientWin as u32 {
                    for j in 0..modifiers.len() {
                        XGrabButton(
                            self.dpy,
                            Config::buttons[i].button,
                            Config::buttons[i].mask | modifiers[j],
                            c.win,
                            False,
                            BUTTONMASK as u32,
                            GrabModeAsync,
                            GrabModeSync,
                            0,
                            0,
                        );
                    }
                }
            }
        }
    }
    pub fn grabkeys(&mut self) {
        // info!("[grabkeys]");
        self.updatenumlockmask();
        unsafe {
            let modifiers = [0, LockMask, self.numlockmask, self.numlockmask | LockMask];

            XUngrabKey(self.dpy, AnyKey, AnyModifier, self.root);
            let mut start: i32 = 0;
            let mut end: i32 = 0;
            let mut skip: i32 = 0;
            XDisplayKeycodes(self.dpy, &mut start, &mut end);
            let syms = XGetKeyboardMapping(self.dpy, start as u8, end - start + 1, &mut skip);
            if syms.is_null() {
                return;
            }
            for k in start..=end {
                for i in 0..Config::keys.len() {
                    // skip modifier codes, we do that ourselves.
                    if Config::keys[i].keysym == *syms.wrapping_add(((k - start) * skip) as usize) {
                        for j in 0..modifiers.len() {
                            XGrabKey(
                                self.dpy,
                                k,
                                Config::keys[i].mod0 | modifiers[j],
                                self.root,
                                True,
                                GrabModeAsync,
                                GrabModeAsync,
                            );
                        }
                    }
                }
            }
            XFree(syms as *mut _);
        }
    }
    pub fn sendevent(&mut self, c: &mut Client, proto: Atom) -> bool {
        info!("[sendevent] {}", c);
        let mut protocols: *mut Atom = null_mut();
        let mut n: i32 = 0;
        let mut exists: bool = false;
        unsafe {
            let mut ev: XEvent = zeroed();
            if XGetWMProtocols(self.dpy, c.win, &mut protocols, &mut n) > 0 {
                while !exists && {
                    let tmp = n;
                    n -= 1;
                    tmp
                } > 0
                {
                    exists = *protocols.wrapping_add(n as usize) == proto;
                }
                XFree(protocols as *mut _);
            }
            if exists {
                ev.type_ = ClientMessage;
                ev.client_message.window = c.win;
                ev.client_message.message_type = self.wmatom[WM::WMProtocols as usize];
                ev.client_message.format = 32;
                // This data is cool!
                ev.client_message.data.as_longs_mut()[0] = proto as i64;
                ev.client_message.data.as_longs_mut()[1] = CurrentTime as i64;
                XSendEvent(self.dpy, c.win, False, NoEventMask, &mut ev);
            }
        }
        return exists;
    }
    pub fn setfocus(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[setfocus]");
        unsafe {
            let mut c = c.borrow_mut();
            if !c.nerverfocus {
                XSetInputFocus(self.dpy, c.win, RevertToPointerRoot, CurrentTime);
                XChangeProperty(
                    self.dpy,
                    self.root,
                    self.netatom[NET::NetActiveWindow as usize],
                    XA_WINDOW,
                    32,
                    PropModeReplace,
                    &mut c.win as *const u64 as *const _,
                    1,
                );
            }
            self.sendevent(&mut c, self.wmatom[WM::WMTakeFocus as usize]);
        }
    }
    pub fn drawbars(&mut self) {
        // info!("[drawbars]");
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
            info!("[drawbars] barwin: {}", m_opt.borrow_mut().barwin);
            self.drawbar(m.clone());
            let next = m_opt.borrow_mut().next.clone();
            m = next;
        }
    }
    pub fn enternotify(&mut self, e: *mut XEvent) {
        // info!("[enternotify]");
        unsafe {
            let ev = (*e).crossing;
            if (ev.mode != NotifyNormal || ev.detail == NotifyInferior) && ev.window != self.root {
                return;
            }
            let c = self.wintoclient(ev.window);
            let m = if let Some(ref c_opt) = c {
                c_opt.borrow_mut().mon.clone()
            } else {
                self.wintomon(ev.window)
            };
            let mon_eq = { Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap()) };
            if !mon_eq {
                let mut selmon_clone = self.selmon.clone();
                let selmon_mut = selmon_clone.as_mut().unwrap().borrow_mut();
                self.unfocus(selmon_mut.sel.clone(), true);
                self.selmon = m;
            } else if c.is_none()
                || Self::are_equal_rc(&c, &self.selmon.as_ref().unwrap().borrow_mut().sel)
            {
                return;
            }
            self.focus(c);
        }
    }
    pub fn expose(&mut self, e: *mut XEvent) {
        // info!("[expose]");
        unsafe {
            let ev = (*e).expose;
            let m = self.wintomon(ev.window);

            if ev.count == 0 && m.is_some() {
                self.drawbar(m);
            }
        }
    }
    pub fn focus(&mut self, mut c: Option<Rc<RefCell<Client>>>) {
        // info!("[focus]");
        unsafe {
            {
                let isvisible = { c.is_some() && c.as_ref().unwrap().borrow_mut().isvisible() };
                if !isvisible {
                    if let Some(ref sel_mon_opt) = self.selmon {
                        c = sel_mon_opt.borrow_mut().stack.clone();
                    }
                    while c.is_some() {
                        if c.as_ref().unwrap().borrow_mut().isvisible() {
                            break;
                        }
                        let next = { c.as_ref().unwrap().borrow_mut().snext.clone() };
                        c = next;
                    }
                }
                let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
                if sel.is_some() && !Self::are_equal_rc(&sel, &c) {
                    self.unfocus(sel.clone(), false);
                }
            }
            if c.is_some() {
                if !Rc::ptr_eq(
                    c.as_ref().unwrap().borrow_mut().mon.as_ref().unwrap(),
                    self.selmon.as_ref().unwrap(),
                ) {
                    self.selmon = c.as_ref().unwrap().borrow_mut().mon.clone();
                }
                if c.as_ref().unwrap().borrow_mut().isurgent {
                    self.seturgent(c.as_ref().unwrap(), false);
                }
                self.detachstack(c.clone());
                self.attachstack(c.clone());
                self.grabbuttons(c.clone(), true);
                XSetWindowBorder(
                    self.dpy,
                    c.as_ref().unwrap().borrow_mut().win,
                    self.scheme[SCHEME::SchemeSel as usize][Col::ColBorder as usize]
                        .as_ref()
                        .unwrap()
                        .pixel,
                );
                self.setfocus(c.as_ref().unwrap());
            } else {
                XSetInputFocus(self.dpy, self.root, RevertToPointerRoot, CurrentTime);
                XDeleteProperty(
                    self.dpy,
                    self.root,
                    self.netatom[NET::NetActiveWindow as usize],
                );
            }
            {
                let mut selmon_mut = self.selmon.as_mut().unwrap().borrow_mut();
                selmon_mut.sel = c.clone();
                let curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                selmon_mut.pertag.as_mut().unwrap().sel[curtag] = c.clone();
            }
            self.drawbars();
        }
    }
    pub fn unfocus(&mut self, c: Option<Rc<RefCell<Client>>>, setfocus: bool) {
        // info!("[unfocus]");
        if c.is_none() {
            return;
        }
        self.grabbuttons(c.clone(), false);
        unsafe {
            XSetWindowBorder(
                self.dpy,
                c.as_ref().unwrap().borrow_mut().win,
                self.scheme[SCHEME::SchemeNorm as usize][Col::ColBorder as usize]
                    .as_ref()
                    .unwrap()
                    .pixel,
            );
            if setfocus {
                XSetInputFocus(self.dpy, self.root, RevertToPointerRoot, CurrentTime);
                XDeleteProperty(
                    self.dpy,
                    self.root,
                    self.netatom[NET::NetActiveWindow as usize],
                );
            }
        }
    }
    pub fn sendmon(&mut self, c: Option<Rc<RefCell<Client>>>, m: Option<Rc<RefCell<Monitor>>>) {
        // info!("[sendmon]");
        if Self::are_equal_rc(&c.as_ref().unwrap().borrow_mut().mon, &m) {
            return;
        }
        self.unfocus(c.clone(), true);
        self.detach(c.clone());
        self.detachstack(c.clone());
        {
            c.as_ref().unwrap().borrow_mut().mon = m.clone()
        };
        // assign tags of target monitor.
        let seltags = { m.as_ref().unwrap().borrow_mut().seltags };
        {
            c.as_ref().unwrap().borrow_mut().tags0 =
                m.as_ref().unwrap().borrow_mut().tagset[seltags]
        };
        self.attach(c.clone());
        self.attachstack(c.clone());
        self.setclienttagprop(c.as_ref().unwrap());
        self.focus(None);
        self.arrange(None);
    }
    pub fn setclientstate(&mut self, c: &Rc<RefCell<Client>>, mut state: i64) {
        // info!("[setclientstate]");
        unsafe {
            let win = c.borrow_mut().win;
            XChangeProperty(
                self.dpy,
                win,
                self.wmatom[WM::WMState as usize],
                self.wmatom[WM::WMState as usize],
                32,
                PropModeReplace,
                &mut state as *const i64 as *const _,
                2,
            );
        }
    }
    pub fn keypress(&mut self, e: *mut XEvent) {
        info!("[keypress]");
        unsafe {
            let ev = (*e).key;
            let keysym = XKeycodeToKeysym(self.dpy, ev.keycode as u8, 0);
            info!(
                "[keypress] keysym: {}, mask: {}",
                keysym,
                self.CLEANMASK(ev.state)
            );
            for i in 0..Config::keys.len() {
                if keysym == Config::keys[i].keysym
                    && self.CLEANMASK(Config::keys[i].mod0) == self.CLEANMASK(ev.state)
                    && Config::keys[i].func.is_some()
                {
                    info!("[keypress] i: {}, arg: {:?}", i, Config::keys[i].arg);
                    Config::keys[i].func.unwrap()(self, &Config::keys[i].arg);
                }
            }
        }
    }
    pub fn manage(&mut self, w: Window, wa: *mut XWindowAttributes) {
        // info!("[manage]");
        let c: Option<Rc<RefCell<Client>>> = Some(Rc::new(RefCell::new(Client::new())));
        let t: Option<Rc<RefCell<Client>>>;
        let mut trans: Window = 0;
        unsafe {
            let mut wc: XWindowChanges = zeroed();
            {
                c.as_ref().unwrap().borrow_mut().win = w;
                c.as_ref().unwrap().borrow_mut().x = (*wa).x;
                c.as_ref().unwrap().borrow_mut().oldx = (*wa).x;
                c.as_ref().unwrap().borrow_mut().y = (*wa).y;
                c.as_ref().unwrap().borrow_mut().oldy = (*wa).y;
                c.as_ref().unwrap().borrow_mut().w = (*wa).width;
                c.as_ref().unwrap().borrow_mut().oldw = (*wa).width;
                c.as_ref().unwrap().borrow_mut().h = (*wa).height;
                c.as_ref().unwrap().borrow_mut().oldh = (*wa).height;
                c.as_ref().unwrap().borrow_mut().oldbw = (*wa).border_width;
                c.as_ref().unwrap().borrow_mut().cfact = 1.;
            }

            self.updatetitle(c.as_ref().unwrap());
            if XGetTransientForHint(self.dpy, w, &mut trans) > 0 && {
                t = self.wintoclient(trans);
                t.is_some()
            } {
                c.as_ref().unwrap().borrow_mut().mon = t.as_ref().unwrap().borrow_mut().mon.clone();
                c.as_ref().unwrap().borrow_mut().tags0 = t.as_ref().unwrap().borrow_mut().tags0;
            } else {
                c.as_ref().unwrap().borrow_mut().mon = self.selmon.clone();
                self.applyrules(c.as_ref().unwrap());
            }

            let width;
            let ww;
            let wh;
            let wx;
            let wy;
            {
                width = c.as_ref().unwrap().borrow_mut().width();
                let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
                ww = mon.as_ref().unwrap().borrow_mut().ww;
                wh = mon.as_ref().unwrap().borrow_mut().wh;
                wx = mon.as_ref().unwrap().borrow_mut().wx;
                wy = mon.as_ref().unwrap().borrow_mut().wy;
            }
            {
                if c.as_ref().unwrap().borrow_mut().x + width > wx + ww {
                    c.as_ref().unwrap().borrow_mut().x = wx + ww - width;
                }
                let height = c.as_ref().unwrap().borrow_mut().height();
                if c.as_ref().unwrap().borrow_mut().y + height > wy + wh {
                    c.as_ref().unwrap().borrow_mut().y = wy + wh - height;
                }
                let x = c.as_ref().unwrap().borrow_mut().x;
                c.as_ref().unwrap().borrow_mut().x = x.max(wx);
                let y = c.as_ref().unwrap().borrow_mut().y;
                c.as_ref().unwrap().borrow_mut().y = y.max(wy);
                c.as_ref().unwrap().borrow_mut().bw = Config::borderpx as i32;
                wc.border_width = c.as_ref().unwrap().borrow_mut().bw;
                XConfigureWindow(self.dpy, w, CWBorderWidth as u32, &mut wc);
                XSetWindowBorder(
                    self.dpy,
                    w,
                    self.scheme[SCHEME::SchemeNorm as usize][Col::ColBorder as usize]
                        .as_ref()
                        .unwrap()
                        .pixel,
                );
                self.configure(&mut *c.as_ref().unwrap().borrow_mut());
            }
            self.updatewindowtype(c.as_ref().unwrap());
            self.updatesizehints(c.as_ref().unwrap());
            self.updatewmhints(c.as_ref().unwrap());
            {
                let mut format: i32 = 0;
                let mut n: u64 = 0;
                let mut extra: u64 = 0;
                let mut atom: Atom = 0;
                let mut data: *mut u8 = null_mut();
                if XGetWindowProperty(
                    self.dpy,
                    c.as_ref().unwrap().borrow_mut().win,
                    self.netatom[NET::NetClientInfo as usize],
                    0,
                    2,
                    False,
                    XA_CARDINAL,
                    &mut atom,
                    &mut format,
                    &mut n,
                    &mut extra,
                    &mut data,
                ) == Success as i32
                    && n == 2
                {
                    c.as_ref().unwrap().borrow_mut().tags0 = *data.wrapping_add(0) as u32;
                    let mut m = self.mons.clone();
                    while let Some(ref m_opt) = m {
                        if m_opt.borrow_mut().num == *data.wrapping_add(1) as i32 {
                            c.as_ref().unwrap().borrow_mut().mon = m;
                            break;
                        }
                        let next = m_opt.borrow_mut().next.clone();
                        m = next;
                    }
                }
                if n > 0 {
                    XFree(data as *mut _);
                }
                self.setclienttagprop(c.as_ref().unwrap());
            }
            XSelectInput(
                self.dpy,
                w,
                EnterWindowMask | FocusChangeMask | PropertyChangeMask | StructureNotifyMask,
            );
            self.grabbuttons(c.clone(), false);
            {
                if !c.as_ref().unwrap().borrow_mut().isfloating {
                    let isfixed = c.as_ref().unwrap().borrow_mut().isfixed;
                    c.as_ref().unwrap().borrow_mut().oldstate = trans != 0 || isfixed;
                    let oldstate = c.as_ref().unwrap().borrow_mut().oldstate;
                    c.as_ref().unwrap().borrow_mut().isfloating = oldstate;
                }
                if c.as_ref().unwrap().borrow_mut().isfloating {
                    XRaiseWindow(self.dpy, c.as_ref().unwrap().borrow_mut().win);
                }
            }
            self.attach(c.clone());
            self.attachstack(c.clone());
            {
                XChangeProperty(
                    self.dpy,
                    self.root,
                    self.netatom[NET::NetClientList as usize],
                    XA_WINDOW,
                    32,
                    PropModeAppend,
                    &mut c.as_ref().unwrap().borrow_mut().win as *const u64 as *const _,
                    1,
                );
                let win = c.as_ref().unwrap().borrow_mut().win;
                let x = c.as_ref().unwrap().borrow_mut().x;
                let y = c.as_ref().unwrap().borrow_mut().y;
                let w = c.as_ref().unwrap().borrow_mut().w;
                let h = c.as_ref().unwrap().borrow_mut().h;
                XMoveResizeWindow(self.dpy, win, x + 2 * self.sw, y, w as u32, h as u32);
            }
            self.setclientstate(c.as_ref().unwrap(), NormalState as i64);
            let mon_eq_selmon;
            {
                mon_eq_selmon = Rc::ptr_eq(
                    c.as_ref().unwrap().borrow_mut().mon.as_ref().unwrap(),
                    self.selmon.as_ref().unwrap(),
                );
            }
            if mon_eq_selmon {
                let mut selmon_clone = self.selmon.clone();
                let selmon_mut = selmon_clone.as_mut().unwrap().borrow_mut();
                self.unfocus(selmon_mut.sel.clone(), false);
            }
            {
                let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
                mon.as_ref().unwrap().borrow_mut().sel = c.clone();
                self.arrange(mon);
            }
            {
                XMapWindow(self.dpy, c.as_ref().unwrap().borrow_mut().win);
            }
            self.focus(None);
        }
    }
    pub fn mappingnotify(&mut self, e: *mut XEvent) {
        // info!("[mappingnotify]");
        unsafe {
            let mut ev = (*e).mapping;
            XRefreshKeyboardMapping(&mut ev);
            if ev.request == MappingKeyboard {
                self.grabkeys();
            }
        }
    }
    pub fn maprequest(&mut self, e: *mut XEvent) {
        // info!("[maprequest]");
        unsafe {
            let ev = (*e).map_request;
            static mut wa: XWindowAttributes = unsafe { zeroed() };
            if XGetWindowAttributes(self.dpy, ev.window, addr_of_mut!(wa)) <= 0
                || wa.override_redirect > 0
            {
                return;
            }
            if self.wintoclient(ev.window).is_none() {
                self.manage(ev.window, addr_of_mut!(wa));
            }
        }
    }
    pub fn monocle(&mut self, m: *mut Monitor) {
        // info!("[monocle]");
        unsafe {
            // This idea is cool!.
            let mut n: u32 = 0;
            let mut c = (*m).clients.clone();
            while let Some(ref c_opt) = c {
                if c_opt.borrow_mut().isvisible() {
                    n += 1;
                }
                let next = c_opt.borrow_mut().next.clone();
                c = next;
            }
            if n > 0 {
                // override layout symbol
                let formatted_string = format!("[{}]", n);
                info!("[monocle] formatted_string: {}", formatted_string);
                (*m).ltsymbol = formatted_string;
            }
            c = self.nexttiled((*m).clients.clone());
            while let Some(ref c_opt) = c {
                let bw = c_opt.borrow_mut().bw;
                self.resize(
                    c_opt,
                    (*m).wx,
                    (*m).wy,
                    (*m).ww - 2 * bw,
                    (*m).wh - 2 * bw,
                    false,
                );
                let next = self.nexttiled(c_opt.borrow_mut().next.clone());
                c = next;
            }
        }
    }
    pub fn motionnotify(&mut self, e: *mut XEvent) {
        // info!("[motionnotify]");
        unsafe {
            let ev = (*e).motion;
            if ev.window != self.root {
                return;
            }
            let m = self.recttomon(ev.x_root, ev.y_root, 1, 1);
            if !Self::are_equal_rc(&m, &self.motionmon) {
                let selmon_mut_sel = self.selmon.as_ref().unwrap().borrow_mut().sel.clone();
                self.unfocus(selmon_mut_sel, true);
                self.selmon = m.clone();
                self.focus(None);
            }
            self.motionmon = m;
        }
    }
    pub fn unmanage(&mut self, c: Option<Rc<RefCell<Client>>>, destroyed: bool) {
        // info!("[unmanage]");
        unsafe {
            let mut wc: XWindowChanges = zeroed();

            for i in 0..=Config::tags_length {
                let cel_i = c
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .mon
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .pertag
                    .as_ref()
                    .unwrap()
                    .sel[i]
                    .clone();
                if Self::are_equal_rc(&cel_i, &c) {
                    c.as_ref()
                        .unwrap()
                        .borrow_mut()
                        .mon
                        .as_mut()
                        .unwrap()
                        .borrow_mut()
                        .pertag
                        .as_mut()
                        .unwrap()
                        .sel[i] = None;
                }
            }

            self.detach(c.clone());
            self.detachstack(c.clone());
            if !destroyed {
                let oldbw = c.as_ref().unwrap().borrow_mut().oldbw;
                let win = c.as_ref().unwrap().borrow_mut().win;
                wc.border_width = oldbw;
                // avoid race conditions.
                XGrabServer(self.dpy);
                XSetErrorHandler(Some(transmute(xerrordummy as *const ())));
                XSelectInput(self.dpy, win, NoEventMask);
                // restore border.
                XConfigureWindow(self.dpy, win, CWBorderWidth as u32, &mut wc);
                XUngrabButton(self.dpy, AnyButton as u32, AnyModifier, win);
                self.setclientstate(c.as_ref().unwrap(), WithdrawnState as i64);
                XSync(self.dpy, False);
                XSetErrorHandler(Some(transmute(xerror as *const ())));
                XUngrabServer(self.dpy);
            }
            self.focus(None);
            self.updateclientlist();
            self.arrange(c.as_ref().unwrap().borrow_mut().mon.clone());
        }
    }
    pub fn unmapnotify(&mut self, e: *mut XEvent) {
        // info!("[unmapnotify]");
        unsafe {
            let ev = (*e).unmap;
            let c = self.wintoclient(ev.window);
            if c.is_some() {
                if ev.send_event > 0 {
                    self.setclientstate(c.as_ref().unwrap(), WithdrawnState as i64);
                } else {
                    self.unmanage(c, false);
                }
            }
        }
    }

    pub fn isuniquegeom(
        &mut self,
        unique: &mut Vec<XineramaScreenInfo>,
        info: *mut XineramaScreenInfo,
    ) -> bool {
        // info!("[isuniquegeom]");
        unsafe {
            for val in unique.iter().rev() {
                if val.x_org == (*info).x_org
                    && val.y_org == (*info).y_org
                    && val.width == (*info).width
                    && val.height == (*info).height
                {
                    return false;
                }
            }
        }
        return true;
    }

    pub fn updategeom(&mut self) -> bool {
        // info!("[updategeom]");
        let mut dirty: bool = false;
        unsafe {
            let mut nn: i32 = 0;
            if XineramaIsActive(self.dpy) > 0 {
                // info!("[updategeom] XineramaIsActive");
                let info = XineramaQueryScreens(self.dpy, &mut nn);
                let mut unique: Vec<XineramaScreenInfo> = vec![];
                unique.reserve(nn as usize);
                let mut n = 0;
                let mut m = self.mons.clone();
                while let Some(ref m_opt) = m {
                    n += 1;
                    let next = m_opt.borrow_mut().next.clone();
                    m = next;
                }
                // Only consider unique geometries as separate screens
                for i in 0..nn as usize {
                    if self.isuniquegeom(&mut unique, info.wrapping_add(i)) {
                        unique.push(*info.wrapping_add(i));
                        // info!("[updategeom] set info i {} as unique {}", i, unique.len());
                    }
                }
                XFree(info as *mut _);
                nn = unique.len() as i32;

                // new monitors if nn > n
                // info!("[updategeom] n: {}, nn: {}", n, nn);
                for _ in n..nn as usize {
                    m = self.mons.clone();
                    while let Some(ref m_opt) = m {
                        let next = m_opt.borrow_mut().next.clone();
                        if next.is_none() {
                            break;
                        }
                        m = next;
                    }
                    if let Some(ref m_opt) = m {
                        m_opt.borrow_mut().next = Some(Rc::new(RefCell::new(self.createmon())));
                    } else {
                        self.mons = Some(Rc::new(RefCell::new(self.createmon())));
                    }
                }
                m = self.mons.clone();
                for i in 0..nn as usize {
                    if m.is_none() {
                        break;
                    }
                    let mx;
                    let my;
                    let mw;
                    let mh;
                    {
                        let m_mut = m.as_ref().unwrap().borrow_mut();
                        mx = m_mut.mx;
                        my = m_mut.my;
                        mw = m_mut.mw;
                        mh = m_mut.mh;
                    }
                    if i >= n
                        || unique[i].x_org as i32 != mx
                        || unique[i].y_org as i32 != my
                        || unique[i].width as i32 != mw
                        || unique[i].height as i32 != mh
                    {
                        dirty = true;
                        let mut m_mut = m.as_ref().unwrap().borrow_mut();
                        m_mut.num = i as i32;
                        m_mut.mx = unique[i].x_org as i32;
                        m_mut.wx = unique[i].x_org as i32;
                        m_mut.my = unique[i].y_org as i32;
                        m_mut.wy = unique[i].y_org as i32;
                        m_mut.mw = unique[i].width as i32;
                        m_mut.ww = unique[i].width as i32;
                        m_mut.mh = unique[i].height as i32;
                        m_mut.wh = unique[i].height as i32;
                        self.updatebarpos(&mut *m_mut);
                    }
                    let next = { m.as_ref().unwrap().borrow_mut().next.clone() };
                    m = next;
                }
                // remove monitors if n > nn
                for _ in nn..n as i32 {
                    m = self.mons.clone();
                    while let Some(ref m_opt) = m {
                        let next = m_opt.borrow_mut().next.clone();
                        if next.is_none() {
                            break;
                        }
                        m = next;
                    }

                    let mut c: Option<Rc<RefCell<Client>>>;
                    while {
                        c = m.as_ref().unwrap().borrow_mut().clients.clone();
                        c.is_some()
                    } {
                        dirty = true;
                        {
                            m.as_ref().unwrap().borrow_mut().clients =
                                c.as_ref().unwrap().borrow_mut().next.clone();
                        }
                        self.detachstack(c.clone());
                        {
                            c.as_ref().unwrap().borrow_mut().mon = self.mons.clone();
                        }
                        self.attach(c.clone());
                        self.attachstack(c.clone());
                    }
                    if Rc::ptr_eq(m.as_ref().unwrap(), self.selmon.as_ref().unwrap()) {
                        self.selmon = self.mons.clone();
                    }
                    self.cleanupmon(m);
                }
            } else {
                // default monitor setup
                if self.mons.is_none() {
                    self.mons = Some(Rc::new(RefCell::new(self.createmon())));
                }
                {
                    let mons_clone = self.mons.clone();
                    let mut mons_mut = mons_clone.as_ref().unwrap().borrow_mut();
                    if mons_mut.mw != self.sw || mons_mut.mh != self.sh {
                        dirty = true;
                        mons_mut.mw = self.sw;
                        mons_mut.ww = self.sw;
                        mons_mut.mh = self.sh;
                        mons_mut.wh = self.sh;
                        self.updatebarpos(&mut *mons_mut);
                    }
                }
            }
            if dirty {
                self.selmon = self.mons.clone();
                self.selmon = self.wintomon(self.root);
            }
        }
        return dirty;
    }

    pub fn gettextprop(&mut self, w: Window, atom: Atom, text: &mut String) -> bool {
        // info!("[gettextprop]");
        unsafe {
            let mut name: XTextProperty = zeroed();
            if XGetTextProperty(self.dpy, w, &mut name, atom) <= 0 || name.nitems <= 0 {
                return false;
            }
            *text = "".to_string();
            let mut list: *mut *mut c_char = null_mut();
            let mut n: i32 = 0;
            if name.encoding == XA_STRING {
                let c_str = CStr::from_ptr(name.value as *const _);
                match c_str.to_str() {
                    Ok(val) => {
                        let mut tmp = val.to_string();
                        while tmp.as_bytes().len() > self.stext_max_len {
                            tmp.pop();
                        }
                        *text = tmp;
                        // info!(
                        //     "[gettextprop]text from string, len: {}, text: {:?}",
                        //     text.len(),
                        //     *text
                        // );
                    }
                    Err(val) => {
                        info!("[gettextprop]text from string error: {:?}", val);
                        println!("[gettextprop]text from string error: {:?}", val);
                        return false;
                    }
                }
            } else if XmbTextPropertyToTextList(self.dpy, &mut name, &mut list, &mut n)
                >= Success as i32
                && n > 0
                && !list.is_null()
            {
                let c_str = CStr::from_ptr(*list);
                match c_str.to_str() {
                    Ok(val) => {
                        let mut tmp = val.to_string();
                        while tmp.as_bytes().len() > self.stext_max_len {
                            tmp.pop();
                        }
                        *text = tmp;
                        // info!(
                        //     "[gettextprop]text from string list, len: {},  text: {:?}",
                        //     text.len(),
                        //     *text
                        // );
                    }
                    Err(val) => {
                        info!("[gettextprop]text from string list error: {:?}", val);
                        println!("[gettextprop]text from string list error: {:?}", val);
                        return false;
                    }
                }
            }
        }
        true
    }
    pub fn updatestatus(&mut self) {
        // info!("[updatestatus]");
        let mut stext = self.stext.clone();
        if !self.gettextprop(self.root, XA_WM_NAME, &mut stext) {
            self.stext = "jwm-1.0".to_string();
        } else {
            self.stext = stext;
        }
        self.drawbar(self.selmon.clone());
    }
    pub fn updatewindowtype(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[updatewindowtype]");
        let state;
        let wtype;
        {
            let c = &mut *c.borrow_mut();
            state = self.getatomprop(c, self.netatom[NET::NetWMState as usize]);
            wtype = self.getatomprop(c, self.netatom[NET::NetWMWindowType as usize]);
        }

        if state == self.netatom[NET::NetWMFullscreen as usize] {
            self.setfullscreen(c, true);
        }
        if wtype == self.netatom[NET::NetWMWindowTypeDialog as usize] {
            let c = &mut *c.borrow_mut();
            c.isfloating = true;
        }
    }
    pub fn updatewmhints(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[updatewmhints]");
        unsafe {
            let mut cc = c.borrow_mut();
            let wmh = XGetWMHints(self.dpy, cc.win);
            let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
            if !wmh.is_null() {
                if selmon_mut.sel.is_some()
                    && Rc::ptr_eq(c, selmon_mut.sel.as_ref().unwrap())
                    && ((*wmh).flags & XUrgencyHint) > 0
                {
                    (*wmh).flags &= !XUrgencyHint;
                    XSetWMHints(self.dpy, cc.win, wmh);
                } else {
                    cc.isurgent = if (*wmh).flags & XUrgencyHint > 0 {
                        true
                    } else {
                        false
                    };
                }
                if (*wmh).flags & InputHint > 0 {
                    cc.nerverfocus = (*wmh).input <= 0;
                } else {
                    cc.nerverfocus = false;
                }
                XFree(wmh as *mut _);
            }
        }
    }
    pub fn updatetitle(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[updatetitle]");
        let mut c = c.borrow_mut();
        if !self.gettextprop(c.win, self.netatom[NET::NetWMName as usize], &mut c.name) {
            self.gettextprop(c.win, XA_WM_NAME, &mut c.name);
        }
        if c.name.is_empty() {
            c.name = self.broken.to_string();
        }
    }
}
