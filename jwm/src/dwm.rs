#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

use libc::{
    exit, sigaction, sigemptyset, waitpid, SA_NOCLDSTOP, SA_NOCLDWAIT, SA_RESTART, SIGCHLD,
    SIG_IGN, WNOHANG,
};
use log::error;
use log::info;
use shared_structures::{MonitorInfo, SharedMessage, SharedRingBuffer, TagStatus};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_char;
use std::ffi::{CStr, CString};
use std::fmt;
use std::mem::transmute;
use std::mem::zeroed;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::process::{Child, Command};
use std::ptr::{addr_of_mut, null, null_mut};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{os::raw::c_long, usize};
use x11::xinerama::{XineramaIsActive, XineramaQueryScreens, XineramaScreenInfo};
use x11::xlib::{XGetTextProperty, XTextProperty, XmbTextPropertyToTextList, XA_STRING};
use x11::xrender::{PictTypeDirect, XRenderFindVisualFormat};

use x11::keysym::XK_Num_Lock;
use x11::xlib::{
    AllocNone, AnyButton, AnyKey, AnyModifier, Atom, BadAccess, BadDrawable, BadLength, BadMatch,
    BadWindow, Below, ButtonPress, ButtonPressMask, ButtonRelease, ButtonReleaseMask,
    CWBorderWidth, CWCursor, CWEventMask, CWHeight, CWSibling, CWStackMode, CWWidth, ClientMessage,
    Colormap, ConfigureNotify, ConfigureRequest, CurrentTime, DestroyAll, DestroyNotify, Display,
    EnterNotify, EnterWindowMask, Expose, ExposureMask, False, FocusChangeMask, FocusIn,
    GrabModeAsync, GrabModeSync, GrabSuccess, InputHint, IsViewable, KeyPress, KeySym,
    LeaveWindowMask, LockMask, MapRequest, MappingKeyboard, MappingNotify, MotionNotify,
    NoEventMask, NotifyInferior, NotifyNormal, PAspect, PBaseSize, PMaxSize, PMinSize, PResizeInc,
    PSize, PointerMotionMask, PointerRoot, PropModeAppend, PropModeReplace, PropertyChangeMask,
    PropertyDelete, PropertyNotify, ReplayPointer, RevertToPointerRoot, StructureNotifyMask,
    SubstructureNotifyMask, SubstructureRedirectMask, Success, Time, True, TrueColor, UnmapNotify,
    Visual, VisualClassMask, VisualDepthMask, VisualScreenMask, Window, XAllowEvents,
    XChangeProperty, XChangeWindowAttributes, XCheckMaskEvent, XClassHint, XConfigureEvent,
    XConfigureWindow, XCreateColormap, XCreateSimpleWindow, XDefaultColormap, XDefaultDepth,
    XDefaultRootWindow, XDefaultScreen, XDefaultVisual, XDeleteProperty, XDestroyWindow,
    XDisplayHeight, XDisplayKeycodes, XDisplayWidth, XErrorEvent, XEvent, XFree, XFreeModifiermap,
    XGetClassHint, XGetKeyboardMapping, XGetModifierMapping, XGetTransientForHint, XGetVisualInfo,
    XGetWMHints, XGetWMNormalHints, XGetWMProtocols, XGetWindowAttributes, XGetWindowProperty,
    XGrabButton, XGrabKey, XGrabPointer, XGrabServer, XInternAtom, XKeycodeToKeysym,
    XKeysymToKeycode, XKillClient, XMapWindow, XMaskEvent, XMoveResizeWindow, XMoveWindow,
    XNextEvent, XQueryPointer, XQueryTree, XRaiseWindow, XRefreshKeyboardMapping, XRootWindow,
    XSelectInput, XSendEvent, XSetCloseDownMode, XSetErrorHandler, XSetInputFocus, XSetWMHints,
    XSetWindowAttributes, XSetWindowBorder, XSizeHints, XSync, XUngrabButton, XUngrabKey,
    XUngrabPointer, XUngrabServer, XUrgencyHint, XVisualInfo, XWarpPointer, XWindowAttributes,
    XWindowChanges, CWX, CWY, XA_ATOM, XA_CARDINAL, XA_WINDOW, XA_WM_HINTS, XA_WM_NAME,
    XA_WM_NORMAL_HINTS, XA_WM_TRANSIENT_FOR,
};

use std::cmp::{max, min};

use crate::config::Config;
use crate::drw::{Clr, Cur, Drw};
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
pub enum CLICK {
    ClkTagBar = 0,
    ClkLtSymbol = 1,
    ClkStatusText = 2,
    ClkWinTitle = 3,
    ClkClientWin = 4,
    ClkRootWin = 5,
    _ClkLast = 6,
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
    pub class: String,
    pub instance: String,
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
    pub neverfocus: bool,
    pub oldstate: bool,
    pub isfullscreen: bool,
    pub next: Option<Rc<RefCell<Client>>>,
    pub snext: Option<Rc<RefCell<Client>>>,
    pub mon: Option<Rc<RefCell<Monitor>>>,
    pub win: Window,
}
impl fmt::Display for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Client {{ name: {}, class: {}, instance: {} mina: {}, maxa: {}, cfact: {}, x: {}, y: {}, w: {}, h: {}, oldx: {}, oldy: {}, oldw: {}, oldh: {}, basew: {}, baseh: {}, incw: {}, inch: {}, maxw: {}, maxh: {}, minw: {}, minh: {}, hintsvalid: {}, bw: {}, oldbw: {}, tags0: {}, isfixed: {}, isfloating: {}, isurgent: {}, neverfocus: {}, oldstate: {}, isfullscreen: {}, win: {} }}",
    self.name,
    self.class,
    self.instance,
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
    self.neverfocus,
    self.oldstate,
    self.isfullscreen,
    self.win,
        )
    }
}
impl Client {
    #[allow(unused)]
    pub fn new() -> Self {
        Self {
            name: String::new(),
            class: String::new(),
            instance: String::new(),
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
            neverfocus: false,
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
            let seltags = self.mon.as_ref().unwrap().borrow().seltags;
            self.tags0 & self.mon.as_ref().unwrap().borrow().tagset[seltags]
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
    pub topbar0: bool,
    pub clients: Option<Rc<RefCell<Client>>>,
    pub sel: Option<Rc<RefCell<Client>>>,
    pub stack: Option<Rc<RefCell<Client>>>,
    pub next: Option<Rc<RefCell<Monitor>>>,
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
            topbar0: false,
            clients: None,
            sel: None,
            stack: None,
            next: None,
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
        write!(f, "Monitor {{ ltsymbol: {}, mfact0: {}, nmaster0: {}, num: {}, by: {}, mx: {}, my: {}, mw: {}, mh: {}, wx: {}, wy: {}, ww: {}, wh: {}, seltags: {}, sellt: {}, tagset: [{}, {}], topbar0: {},  }}",
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
               self.topbar0,
        )
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub class: &'static str,
    pub instance: &'static str,
    pub name: &'static str,
    pub tags0: usize,
    pub isfloating: bool,
    pub monitor: i32,
}
impl Rule {
    #[allow(unused)]
    pub fn new(
        class: &'static str,
        instance: &'static str,
        name: &'static str,
        tags0: usize,
        isfloating: bool,
        monitor: i32,
    ) -> Self {
        Rule {
            class,
            instance,
            name,
            tags0,
            isfloating,
            monitor,
        }
    }
}

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
    let X_FixesChangeSaveSet: u8 = 139;
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
            || ((*ee).request_code == X_FixesChangeSaveSet && (*ee).error_code == BadLength)
        {
            return 0;
        }
        info!(
            "jwm: fatal error: request code = {}, error code = {}",
            (*ee).request_code,
            (*ee).error_code
        );
        return -1;
    }
}
pub fn xerrordummy(_: *mut Display, _: *mut XErrorEvent) -> i32 {
    // info!("[xerrordummy]");
    0
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct BarShape {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub struct Dwm {
    pub stext_max_len: usize,
    pub screen: i32,
    pub sw: i32,
    pub sh: i32,
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
    pub egui_bar_shmem: HashMap<i32, SharedRingBuffer>,
    pub egui_bar_child: HashMap<i32, Child>,
    pub egui_bar_shape: HashMap<i32, BarShape>,
    pub message: SharedMessage,
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
    pub fn new(sender: Sender<u8>) -> Self {
        Dwm {
            stext_max_len: 512,
            screen: 0,
            sw: 0,
            sh: 0,
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
            egui_bar_shmem: HashMap::new(),
            egui_bar_child: HashMap::new(),
            egui_bar_shape: HashMap::new(),
            message: SharedMessage::default(),
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
            c.class = if !ch.res_class.is_null() {
                let c_str = CStr::from_ptr(ch.res_class);
                c_str.to_str().unwrap_or(Config::broken).to_string()
            } else {
                Config::broken.to_string()
            };
            c.instance = if !ch.res_name.is_null() {
                let c_str = CStr::from_ptr(ch.res_name);
                c_str.to_str().unwrap_or(Config::broken).to_string()
            } else {
                Config::broken.to_string()
            };

            info!(
                "[applyrules] class: {}, instance: {}, name: {}",
                c.class, c.instance, c.name
            );
            for r in &*Config::rules {
                if r.name.is_empty() && r.class.is_empty() && r.instance.is_empty() {
                    continue;
                }
                if (r.name.is_empty() || c.name.find(&r.name).is_some())
                    && (r.class.is_empty() || c.class.find(&r.class).is_some())
                    && (r.instance.is_empty() || c.instance.find(&r.instance).is_some())
                {
                    info!(
                        "[############################### applyrules] class: {}, instance: {}, name: {}",
                        c.class, c.instance, c.name
                    );
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
            c.isfixed = (c.maxw > 0) && (c.maxh > 0) && (c.maxw == c.minw) && (c.maxh == c.minh);
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
        // info!("[applysizehints] {x}, {y}, {w}, {h}");
        // set minimum possible client area size.
        *w = 1.max(*w);
        *h = 1.max(*h);

        // Boundary checks
        if interact {
            let cc = c.as_ref().borrow(); // Borrow immutably for reading
            let client_total_width = *w + 2 * cc.bw; // Use desired w for this check
            let client_total_height = *h + 2 * cc.bw; // Use desired h for this check

            if *x > self.sw {
                // Off right edge
                *x = self.sw - client_total_width;
            }
            if *y > self.sh {
                // Off bottom edge
                *y = self.sh - client_total_height;
            }
            if *x + client_total_width < 0 {
                // Off left edge
                *x = 0;
            }
            if *y + client_total_height < 0 {
                // Off top edge
                *y = 0;
            }
        } else {
            let cc = c.as_ref().borrow(); // Borrow immutably for reading
            let mon_borrow = cc.mon.as_ref().unwrap().borrow();
            let wx = mon_borrow.wx;
            let wy = mon_borrow.wy;
            let ww = mon_borrow.ww;
            let wh = mon_borrow.wh;
            let client_total_width = *w + 2 * cc.bw; // Use desired w
            let client_total_height = *h + 2 * cc.bw; // Use desired h

            if *x >= wx + ww {
                // Client's left edge past monitor's right edge
                *x = wx + ww - client_total_width;
            }
            if *y >= wy + wh {
                // Client's top edge past monitor's bottom edge
                *y = wy + wh - client_total_height;
            }
            if *x + client_total_width <= wx {
                // Client's right edge before monitor's left edge
                *x = wx;
            }
            if *y + client_total_height <= wy {
                // Client's bottom edge before monitor's top edge
                *y = wy;
            }
        }

        let (isfloating, layout_type_is_none) = {
            let cc_borrow = c.as_ref().borrow();
            let mon_borrow = cc_borrow.mon.as_ref().unwrap().borrow();
            let sellt = mon_borrow.sellt;
            (
                cc_borrow.isfloating,
                mon_borrow.lt[sellt].layout_type.is_none(),
            )
        };

        if Config::resizehints || isfloating || layout_type_is_none {
            if !c.as_ref().borrow().hintsvalid {
                // Check immutable borrow first
                self.updatesizehints(c); // This will mutably borrow internally
            }

            let cc = c.as_ref().borrow(); // Re-borrow (immutable) after potential updatesizehints

            // Adjust w and h for base dimensions and increments
            // These are client area dimensions (without border)
            let mut current_w = *w;
            let mut current_h = *h;

            // 1. Subtract base size to get the dimensions that increments apply to.
            current_w -= cc.basew;
            current_h -= cc.baseh;

            // 2. Apply resize increments.
            if cc.incw > 0 {
                current_w -= current_w % cc.incw;
            }
            if cc.inch > 0 {
                current_h -= current_h % cc.inch;
            }

            // 3. Add base size back before aspect ratio and min/max checks.
            current_w += cc.basew;
            current_h += cc.baseh;

            // 4. Apply aspect ratio limits.
            // cc.mina is min_aspect.y / min_aspect.x (target H/W)
            // cc.maxa is max_aspect.x / max_aspect.y (target W/H)
            if cc.mina > 0.0 && cc.maxa > 0.0 {
                if cc.maxa < current_w as f32 / current_h as f32 {
                    // Too wide (current W/H > max W/H) -> Adjust W
                    current_w = (current_h as f32 * cc.maxa + 0.5) as i32;
                } else if current_h as f32 / current_w as f32 > cc.mina {
                    // Too tall (current H/W > min H/W) -> Adjust H
                    current_h = (current_w as f32 * cc.mina + 0.5) as i32;
                }
            }

            // 5. Enforce min and max dimensions.
            // Ensure client area is not smaller than min_width/height.
            current_w = current_w.max(cc.minw);
            current_h = current_h.max(cc.minh);

            // Ensure client area is not larger than max_width/height if specified.
            if cc.maxw > 0 {
                current_w = current_w.min(cc.maxw);
            }
            if cc.maxh > 0 {
                current_h = current_h.min(cc.maxh);
            }

            *w = current_w;
            *h = current_h;
        }

        // Check if final geometry is different from the client's current geometry
        let client_now = c.as_ref().borrow();
        return *x != client_now.x
            || *y != client_now.y
            || *w != client_now.w
            || *h != client_now.h;
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
                let mut stack_iter: Option<Rc<RefCell<Client>>>;
                loop {
                    stack_iter = m_opt.borrow_mut().stack.clone();
                    if let Some(client_rc) = stack_iter {
                        self.unmanage(Some(client_rc), false);
                    } else {
                        break;
                    }
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
            m.as_ref().unwrap().borrow_mut().next = mon.as_ref().unwrap().borrow_mut().next.clone();
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
            let c = c.as_ref().unwrap();
            if cme.message_type == self.netatom[NET::NetWMState as usize] {
                if cme.data.get_long(1) == self.netatom[NET::NetWMFullscreen as usize] as i64
                    || cme.data.get_long(2) == self.netatom[NET::NetWMFullscreen as usize] as i64
                {
                    // NET_WM_STATE_ADD
                    // NET_WM_STATE_TOGGLE
                    let isfullscreen = { c.borrow_mut().isfullscreen };
                    let fullscreen =
                        cme.data.get_long(0) == 1 || (cme.data.get_long(0) == 2 && !isfullscreen);
                    self.setfullscreen(c, fullscreen);
                }
            } else if cme.message_type == self.netatom[NET::NetActiveWindow as usize] {
                let isurgent = { c.borrow_mut().isurgent };
                let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
                if !Self::are_equal_rc(&Some(c.clone()), &sel) && !isurgent {
                    self.seturgent(c, true);
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
                    let mut m = self.mons.clone();
                    while let Some(ref m_opt) = m {
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
        info!("[setfullscreen]");
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
                // Raise the window to the top of the stacking order
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
                    // println!("line: {}, {}", line!(), c.y);
                    c.w = c.oldw;
                    c.h = c.oldh;
                }
                {
                    let mut c = c.borrow_mut();
                    let (x, y, w, h) = (c.x, c.y, c.w, c.h);
                    self.resizeclient(&mut *c, x, y, w, h);
                }
                let mon = { c.borrow_mut().mon.clone() };
                self.arrange(mon);
            }
        }
    }

    pub fn resizeclient(&mut self, c: &mut Client, x: i32, y: i32, w: i32, h: i32) {
        // info!("[resizeclient] {x}, {y}, {w}, {h}");
        unsafe {
            let mut wc: XWindowChanges = zeroed();
            c.oldx = c.x;
            c.x = x;
            wc.x = x;
            c.oldy = c.y;
            c.y = y;
            // println!("line: {}, {}", line!(), c.y);
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
        // info!("[resize] {x}, {y}, {w}, {h}");
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
            let c = c.as_ref().unwrap();
            let cc = c.borrow();
            let isvisible = cc.isvisible();
            if isvisible {
                // show clients top down.
                // let name = c.as_ref().unwrap().borrow_mut().name.clone();
                // info!("[showhide] show clients top down: {name}");
                let win = cc.win;
                let x = cc.x;
                let y = cc.y;
                XMoveWindow(self.dpy, win, x, y);
                {
                    let mon = cc.mon.clone().unwrap();
                    let mon = mon.borrow();
                    let isfloating = cc.isfloating;
                    let isfullscreen = cc.isfullscreen;
                    if (mon.lt[mon.sellt].layout_type.is_none() || isfloating) && !isfullscreen {
                        let (x, y, w, h) = (cc.x, cc.y, cc.w, cc.h);
                        self.resize(c, x, y, w, h, false);
                    }
                }
                let snext = cc.snext.clone();
                self.showhide(snext);
            } else {
                // hide clients bottom up.
                // let name = c.as_ref().unwrap().borrow_mut().name.clone();
                // info!("[showhide] show clients bottom up: {name}");
                let snext = cc.snext.clone();
                self.showhide(snext);
                let y;
                let win;
                {
                    y = cc.y;
                    win = cc.win;
                }
                XMoveWindow(self.dpy, win, cc.width() * -2, y);
            }
        }
    }

    pub fn configurerequest(&mut self, e: *mut XEvent) {
        // info!("[configurerequest]");
        unsafe {
            let ev = (*e).configure_request;
            let c = self.wintoclient(ev.window);
            if let Some(ref c_opt) = c {
                let layout_type = {
                    let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                    let sellt = selmon_mut.sellt;
                    selmon_mut.lt[sellt].layout_type.clone()
                };
                let mut c_mut = c_opt.borrow_mut();
                let isfloating = c_mut.isfloating;
                if ev.value_mask & CWBorderWidth as u64 > 0 {
                    c_mut.bw = ev.border_width;
                } else if isfloating || layout_type.is_none() {
                    let mx;
                    let my;
                    let mw;
                    let mh;
                    {
                        let m = c_mut.mon.as_ref().unwrap().borrow_mut();
                        mx = m.mx;
                        my = m.my;
                        mw = m.mw;
                        mh = m.mh;
                    }
                    {
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
                            if c_mut.name == Config::egui_bar_name {
                                println!("[configurerequest] egui bar x: {}", c_mut.x);
                                // c_mut.x = 0;
                            } else {
                                c_mut.x = mx + (mw / 2 - c_mut.width() / 2);
                            }
                        }
                        if (c_mut.y + c_mut.h) > my + mh && c_mut.isfloating {
                            // center in y direction
                            c_mut.y = my + (mh / 2 - c_mut.height() / 2);
                        }
                    }
                    if (ev.value_mask & (CWX | CWY) as u64) > 0
                        && (ev.value_mask & (CWWidth | CWHeight) as u64) <= 0
                    {
                        self.configure(&mut c_mut);
                    }
                    let isvisible = c_mut.isvisible();
                    if isvisible {
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
                    self.configure(&mut c_mut);
                }
            } else {
                let mut wc: XWindowChanges = zeroed();
                wc.x = ev.x;
                wc.y = ev.y;
                // println!("line: {}, {}", line!(), wc.y);
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
        m.topbar0 = Config::topbar;
        m.lt[0] = Config::layouts[0].clone();
        m.lt[1] = Config::layouts[1 % Config::layouts.len()].clone();
        m.ltsymbol = Config::layouts[0].symbol.to_string();
        info!(
            "[createmon]: ltsymbol: {:?}, mfact0: {}, nmaster0: {},  topbar0: {}",
            m.ltsymbol, m.mfact0, m.nmaster0, m.topbar0
        );
        m.pertag = Some(Pertag::new());
        let ref_pertag = m.pertag.as_mut().unwrap();
        ref_pertag.curtag = 1;
        ref_pertag.prevtag = 1;
        let default_layout_0 = m.lt[0].clone();
        let default_layout_1 = m.lt[1].clone();
        for i in 0..=Config::tags_length {
            ref_pertag.nmasters[i] = m.nmaster0;
            ref_pertag.mfacts[i] = m.mfact0;

            ref_pertag.ltidxs[i][0] = Some(default_layout_0.clone());
            ref_pertag.ltidxs[i][1] = Some(default_layout_1.clone());
            ref_pertag.sellts[i] = m.sellt;
        }

        return m;
    }

    pub fn destroynotify(&mut self, e: *mut XEvent) {
        // info!("[destroynotify]");
        unsafe {
            let ev = (*e).destroy_window;
            let c = self.wintoclient(ev.window);
            if let Some(c_opt) = c {
                self.unmanage(Some(c_opt), true);
            }
        }
    }

    pub fn applylayout(&mut self, layout_type: &LayoutType, m: &Rc<RefCell<Monitor>>) {
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
        info!("[arrangemon]");
        let layout_type;
        {
            let mut mm = m.borrow_mut();
            let sellt = (mm).sellt;
            mm.ltsymbol = (mm).lt[sellt].symbol.to_string();
            info!("[arrangemon] sellt: {}, ltsymbol: {:?}", sellt, mm.ltsymbol);
            layout_type = mm.lt[sellt].layout_type.clone();
        }
        if let Some(ref layout_type) = layout_type {
            self.applylayout(layout_type, m);
        }
    }

    // This is cool!
    // 一个更实际的提取方式可能涉及到传递闭包来访问和修改特定的字段：
    fn detach_node_from_list<FGetHead, FSetHead, FGetNext, FSetNext>(
        mon: &Rc<RefCell<Monitor>>,
        node_to_detach: &Option<Rc<RefCell<Client>>>,
        get_head: FGetHead,
        set_head: FSetHead,
        get_next: FGetNext, // Assuming this returns Option<Rc<RefCell<Client>>>
        set_next: FSetNext,
    ) where
        FGetHead: Fn(&mut Monitor) -> &mut Option<Rc<RefCell<Client>>>,
        FSetHead: Fn(&mut Monitor, Option<Rc<RefCell<Client>>>),
        FGetNext: Fn(&mut Client) -> Option<Rc<RefCell<Client>>>, // Changed to &mut Client
        FSetNext: Fn(&mut Client, Option<Rc<RefCell<Client>>>),
    {
        if node_to_detach.is_none() {
            return;
        }

        let mut mon_borrow_for_head = mon.borrow_mut();
        let mut current_node_opt = (get_head)(&mut *mon_borrow_for_head).clone();
        drop(mon_borrow_for_head);

        let mut prev_node_opt: Option<Rc<RefCell<Client>>> = None;

        while let Some(current_rc) = current_node_opt.clone() {
            // Clone current_rc for this iteration's ownership
            // current_rc is now an owned Rc<RefCell<Client>> for this iteration

            // Check if current_rc is the one to detach
            // We need an Option for are_equal_rc, so wrap current_rc
            if Self::are_equal_rc(&Some(current_rc.clone()), node_to_detach) {
                // Clone for comparison
                let next_node_to_link = (get_next)(&mut current_rc.borrow_mut()); // Get next

                if let Some(ref prev_rc_strong) = prev_node_opt {
                    (set_next)(&mut prev_rc_strong.borrow_mut(), next_node_to_link);
                } else {
                    // Detaching the head node
                    let mut mon_borrow_for_set_head = mon.borrow_mut();
                    (set_head)(&mut *mon_borrow_for_set_head, next_node_to_link);
                }
                break; // Node detached, exit loop
            }

            // Not the node to detach, advance
            let next_for_iteration = (get_next)(&mut current_rc.borrow_mut());
            prev_node_opt = Some(current_rc); // current_rc (owned for this iteration) becomes prev
            current_node_opt = next_for_iteration; // Update for the next iteration of the while loop
        }
    }

    pub fn detach(&mut self, c: Option<Rc<RefCell<Client>>>) {
        // info!("[detach]");
        let c = match c {
            Some(val) => val,
            None => return,
        };
        let m = match c.borrow().mon {
            Some(ref mon_val) => mon_val.clone(),
            None => return,
        };
        Self::detach_node_from_list(
            &m,
            &Some(c),
            |m| &mut m.clients,
            |m, next| m.clients = next,
            |cli| cli.next.clone(),
            |cli, next| cli.next = next,
        );
    }

    pub fn detachstack(&mut self, c: Option<Rc<RefCell<Client>>>) {
        // info!("[detachstack]");
        let c = match c {
            Some(val) => val,
            None => return,
        };
        let m = match c.borrow().mon {
            Some(ref mon_val) => mon_val.clone(),
            None => return,
        };
        Self::detach_node_from_list(
            &m,
            &Some(c.clone()),
            |m| &mut m.stack,
            |m, next| m.stack = next,
            |cli| cli.snext.clone(),
            |cli, next| cli.snext = next,
        );

        if Self::are_equal_rc(&Some(c), &m.borrow().sel) {
            let mut t = { m.borrow().stack.clone() };
            while let Some(ref t_opt) = t {
                let isvisible = { t_opt.borrow_mut().isvisible() };
                if isvisible {
                    break;
                }
                let snext = { t_opt.borrow_mut().snext.clone() };
                t = snext;
            }
            m.borrow_mut().sel = t.clone();
        }
    }

    pub fn dirtomon(&mut self, dir: i32) -> Option<Rc<RefCell<Monitor>>> {
        let selected_monitor = self.selmon.as_ref()?; // Return None if selmon is None
        let monitors_head = self.mons.as_ref()?; // Return None if mons is None
        if dir > 0 {
            // Next monitor
            let next_mon = selected_monitor.borrow().next.clone();
            return next_mon.or_else(|| self.mons.clone()); // If next is None, loop to head
        } else {
            // Previous monitor
            if Rc::ptr_eq(selected_monitor, monitors_head) {
                // Selected is head, find the tail
                let mut current = self.mons.clone();
                let mut tail = self.mons.clone(); // Initialize tail to head in case of single monitor
                while let Some(current_rc) = current {
                    tail = Some(current_rc.clone()); // current_rc is the potential tail
                    current = current_rc.borrow().next.clone();
                    if current.is_none() {
                        // Reached the actual tail
                        break;
                    }
                }
                return tail;
            } else {
                // Selected is not head, find p such that p.next == selected_monitor
                let mut current = self.mons.clone();
                let mut prev = None;
                while let Some(current_rc) = current {
                    if Rc::ptr_eq(&current_rc, selected_monitor) {
                        return prev; // Found selected, prev is the one before it
                    }
                    prev = Some(current_rc.clone());
                    current = current_rc.borrow().next.clone();
                    if current.is_none() && prev.is_some() {
                        // Should not happen if selected_monitor is in the list and not head,
                        // unless list structure is broken or selected_monitor is not in mons.
                        // This indicates an issue if selected_monitor was supposed to be found.
                        return None; // Or some error, or loop to tail if selmon wasn't found
                    }
                }
                // If loop finishes, selected_monitor was not found in the list after the head
                // This implies an inconsistent state.
                return None;
            }
        }
    }

    fn write_message(&mut self, num: i32, message: &SharedMessage) -> std::io::Result<()> {
        if let Some(ring_buffer) = self.egui_bar_shmem.get_mut(&num) {
            // Assuming get_mut
            match ring_buffer.try_write_message(&message) {
                Ok(true) => {
                    info!("[write_message] {:?}", message);
                    Ok(()) // Message written successfully
                }
                Ok(false) => {
                    println!("缓冲区已满，等待空间...");
                    // Consider returning a specific error type or just Ok(()) if this is not critical
                    // For example: Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "Ring buffer full"))
                    Ok(()) // Or keep as Ok, depending on desired error propagation
                }
                Err(e) => {
                    eprintln!("写入错误: {}", e);
                    Err(e) // Propagate the I/O error
                }
            }
        } else {
            // Ring buffer for this monitor number not found
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Ring buffer for monitor {} not found", num),
            ))
        }
    }

    pub fn drawbar(&mut self, m: Option<Rc<RefCell<Monitor>>>) {
        self.update_bar_message_for_monitor(m);
        let num = self.message.monitor_info.monitor_num;
        info!("[drawbar] num: {}", num);
        let shared_path = format!("/dev/shm/monitor_{}", num);
        if !self.egui_bar_shmem.contains_key(&num) {
            let ring_buffer = match SharedRingBuffer::open(&shared_path) {
                Ok(rb) => rb,
                Err(_) => {
                    println!("创建新的共享环形缓冲区");
                    SharedRingBuffer::create(&shared_path, None, None).unwrap()
                }
            };
            self.egui_bar_shmem.insert(num, ring_buffer);
        }
        // info!("[drawbar] message: {:?}", self.message);
        let _ = self.write_message(num, &self.message.clone());
        if !self.egui_bar_child.contains_key(&num) {
            let process_name = format!("{}_{}", Config::egui_bar_name, num);
            let child = Command::new(Config::egui_bar_name)
                .arg0(process_name) // This change the class and instance
                .arg(shared_path)
                .spawn()
                .expect("Failled to start egui app");
            self.egui_bar_child.insert(num, child);
        }
    }

    pub fn restack(&mut self, m: Option<Rc<RefCell<Monitor>>>) {
        info!("[restack]");
        let m = match m {
            Some(monitor) => monitor,
            None => return,
        };
        self.drawbar(Some(m.clone()));

        unsafe {
            let m = m.borrow();
            let mut wc: XWindowChanges = zeroed();
            let sel = m.sel.clone();
            if sel.is_none() {
                return;
            }
            let isfloating = sel.as_ref().unwrap().borrow_mut().isfloating;
            let sellt = m.sellt;
            let layout_type = { m.lt[sellt].layout_type.clone() };
            if isfloating || layout_type.is_none() {
                let win = sel.as_ref().unwrap().borrow_mut().win;
                XRaiseWindow(self.dpy, win);
            }
            if layout_type.is_some() {
                wc.stack_mode = Below;
                let mut c = m.stack.clone();
                while let Some(ref c_opt) = c.clone() {
                    let c_opt = c_opt.borrow_mut();
                    let isfloating = c_opt.isfloating;
                    let isvisible = c_opt.isvisible();
                    if !isfloating && isvisible {
                        let win = c_opt.win;
                        XConfigureWindow(self.dpy, win, (CWSibling | CWStackMode) as u32, &mut wc);
                        wc.sibling = win;
                    }
                    let next = c_opt.snext.clone();
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
                if ev.type_ == PropertyNotify {
                    info!("running frame: {}, handler type: {}", i, ev.type_);
                }
                i = i.wrapping_add(1);
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
                    if XGetWindowAttributes(self.dpy, *wins.add(i), &mut wa) <= 0
                        || wa.override_redirect > 0
                        || XGetTransientForHint(self.dpy, *wins.add(i), &mut d1) > 0
                    {
                        continue;
                    }
                    if wa.map_state == IsViewable
                        || self.getstate(*wins.add(i)) == IconicState as i64
                    {
                        self.manage(*wins.add(i), &mut wa);
                    }
                }
                for i in 0..num as usize {
                    // now the transients
                    // 目的是确保在管理一个瞬态窗口（如对话框）之前，它的主窗口（WM_TRANSIENT_FOR 指向的窗口）已经被窗口管理器处理了
                    if XGetWindowAttributes(self.dpy, *wins.add(i), &mut wa) <= 0 {
                        continue;
                    }
                    if XGetTransientForHint(self.dpy, *wins.add(i), &mut d1) > 0
                        && (wa.map_state == IsViewable
                            || self.getstate(*wins.add(i)) == IconicState as i64)
                    {
                        self.manage(*wins.add(i), &mut wa);
                    }
                }
            }
            if !wins.is_null() {
                XFree(wins as *mut _);
            }
        }
    }

    pub fn arrange(&mut self, m_target: Option<Rc<RefCell<Monitor>>>) {
        info!("[arrange]");
        // Determine which monitors to operate on
        let monitors_to_process: Vec<Rc<RefCell<Monitor>>> = match m_target {
            Some(monitor_rc) => vec![monitor_rc], // Operate on a single monitor
            None => {
                // Operate on all monitors
                let mut all_mons = Vec::new();
                let mut current_mon_opt = self.mons.clone();
                while let Some(current_mon_rc) = current_mon_opt {
                    all_mons.push(current_mon_rc.clone());
                    current_mon_opt = current_mon_rc.borrow().next.clone();
                }
                all_mons
            }
        };

        // Phase 1: Show/Hide windows for each targeted monitor
        for mon_rc in &monitors_to_process {
            let stack = mon_rc.borrow().stack.clone(); // Borrow immutably if stack is just read
            self.showhide(stack);
        }

        // Phase 2: Arrange layout and restack for each targeted monitor
        for mon_rc in monitors_to_process {
            // Consume Vec or iterate by ref again
            self.arrangemon(&mon_rc);
            self.restack(Some(mon_rc)); // Pass Some(mon_rc) to restack
        }
    }

    fn attach_to_list_head_internal(
        client_rc: &Rc<RefCell<Client>>,
        mon_rc: &Rc<RefCell<Monitor>>,
        // FnMut because it modifies `cli`
        mut set_client_next: impl FnMut(&mut Client, Option<Rc<RefCell<Client>>>),
        // FnMut because it modifies `mon` (by returning a mutable reference to its field)
        mut access_mon_list_head: impl FnMut(&mut Monitor) -> &mut Option<Rc<RefCell<Client>>>,
    ) {
        // Borrow client mutably once
        let mut client_borrow = client_rc.borrow_mut();
        // Borrow monitor mutably once
        let mut mon_borrow = mon_rc.borrow_mut();
        // Get a mutable reference to the monitor's list head field
        let list_head_field_ref = (access_mon_list_head)(&mut *mon_borrow);
        // 1. Client's next should point to the current head (before modification)
        //    We clone the Option<Rc<...>> from the field reference.
        let current_head_clone = (*list_head_field_ref).clone();
        set_client_next(&mut *client_borrow, current_head_clone);
        // 2. Monitor's list head should now be the new client
        //    Assign directly to the mutable reference we got.
        *list_head_field_ref = Some(client_rc.clone());
    }

    pub fn attach(&mut self, c_opt: Option<Rc<RefCell<Client>>>) {
        let client_rc = match c_opt {
            Some(c) => c,
            None => return,
        };
        let mon_rc = match client_rc.borrow().mon.as_ref() {
            Some(m) => m.clone(),
            None => return,
        };

        Self::attach_to_list_head_internal(
            &client_rc,
            &mon_rc,
            |cli, next_node| cli.next = next_node,
            |mon| &mut mon.clients,
        );
    }

    pub fn attachstack(&mut self, c_opt: Option<Rc<RefCell<Client>>>) {
        let client_rc = match c_opt {
            Some(c) => c,
            None => return,
        };
        let mon_rc = match client_rc.borrow().mon.as_ref() {
            Some(m) => m.clone(),
            None => return,
        };

        Self::attach_to_list_head_internal(
            &client_rc,
            &mon_rc,
            |cli, next_node| cli.snext = next_node,
            |mon| &mut mon.stack,
        );
    }

    pub fn getatomprop(&mut self, c: &mut Client, prop: Atom) -> u64 {
        // info!("[getatomprop]");
        let mut di = 0;
        let mut dl0: u64 = 0;
        let mut dl1: u64 = 0;
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
                &mut dl0,
                &mut dl1,
                &mut p,
            ) == Success as i32
                && !p.is_null()
            {
                atom = *(p as *const Atom);
                XFree(p as *mut _);
            }
        }
        return atom;
    }

    pub fn getrootptr(&mut self, x: &mut i32, y: &mut i32) -> i32 {
        // info!("[getrootptr]");
        let mut di0: i32 = 0;
        let mut di1: i32 = 0;
        let mut dui: u32 = 0;
        unsafe {
            let mut dummy: Window = zeroed();
            return XQueryPointer(
                self.dpy, self.root, &mut dummy, &mut dummy, x, y, &mut di0, &mut di1, &mut dui,
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
                result = *(p as *const i32) as i64;
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
            let a = m_opt.borrow().intersect(x, y, w, h);
            if a > area {
                area = a;
                r = m.clone();
            }
            let next = m_opt.borrow().next.clone();
            m = next;
        }
        return r;
    }

    pub fn wintoclient(&mut self, w: Window) -> Option<Rc<RefCell<Client>>> {
        // info!("[wintoclient]");
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
            let mut c = { m_opt.borrow().clients.clone() };
            while let Some(ref c_opt) = c {
                let win = { c_opt.borrow().win };
                if win == w {
                    return c;
                }
                let next = { c_opt.borrow().next.clone() };
                c = next;
            }
            let next = { m_opt.borrow().next.clone() };
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
        let c = self.wintoclient(w);
        if let Some(ref c_opt) = c {
            return c_opt.borrow().mon.clone();
        }
        return self.selmon.clone();
    }

    pub fn buttonpress(&mut self, e: *mut XEvent) {
        // info!("[buttonpress]");
        let _arg: Arg = Arg::Ui(0);
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
            if {
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
                    // 清理（移除NumLock, CapsLock等）后的修饰键掩码与事件中的修饰键状态匹配
                    && self.CLEANMASK(Config::buttons[i].mask) == self.CLEANMASK(ev.state)
                {
                    if let Some(ref func) = Config::buttons[i].func {
                        info!(
                            "[buttonpress] click: {}, button: {}, mask: {}",
                            Config::buttons[i].click,
                            Config::buttons[i].button,
                            Config::buttons[i].mask
                        );
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
            _ = XSetErrorHandler(Some(transmute(xerrorstart as *const ())));
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

    pub fn spawn(&mut self, arg_ptr: *const Arg) {
        info!("[spawn]");

        let arg = match unsafe { arg_ptr.as_ref() } {
            Some(a) => a.clone(),
            None => {
                error!("[spawn] Argument pointer was null");
                return;
            }
        };

        if let Arg::V(cmd_vec) = arg {
            let mut processed_cmd_vec = cmd_vec.clone();

            if processed_cmd_vec == *Config::dmenucmd {
                if let Some(selmon_rc) = self.selmon.as_ref() {
                    let monitor_num_str = {
                        let selmon_borrow = selmon_rc.borrow();
                        (b'0' + selmon_borrow.num as u8) as char
                    }
                    .to_string();

                    info!(
                        "[spawn] dmenumon: {}, num: {}",
                        monitor_num_str,
                        selmon_rc.borrow().num
                    );
                    if let Some(m_idx) = processed_cmd_vec.iter().position(|s| s == "-m") {
                        if m_idx + 1 < processed_cmd_vec.len() {
                            processed_cmd_vec[m_idx + 1] = monitor_num_str;
                        } else {
                            error!("[spawn] dmenu command has -m but no subsequent argument.");
                        }
                    } else {
                        error!("[spawn] dmenu command format in Config::dmenucmd does not contain '-m'.");
                    }
                } else {
                    error!("[spawn] No selected monitor for dmenu.");
                }
            }

            if processed_cmd_vec.is_empty() {
                error!("[spawn] Command vector is empty.");
                return;
            }

            info!("[spawn] Spawning command: {:?}", processed_cmd_vec);
            match Command::new(&processed_cmd_vec[0])
                .args(&processed_cmd_vec[1..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child_process) => {
                    info!(
                        "[spawn] Command {:?} spawned successfully with PID: {}",
                        processed_cmd_vec,
                        child_process.id()
                    );
                }
                Err(e) => {
                    error!(
                        "[spawn] Failed to spawn command '{:?}': {}",
                        processed_cmd_vec, e
                    );
                }
            }
        } else {
            error!("[spawn] Argument was not Arg::V type");
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
                while let Some(ref c_opt) = c {
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

    pub fn client_y_offset(&self, m: &Monitor) -> i32 {
        let num = m.num;
        if let Some(bar_shape) = self.egui_bar_shape.get(&num) {
            return bar_shape.height + Config::egui_bar_pad + bar_shape.y;
        }
        return 0;
    }

    pub fn tile(&mut self, m_rc: &Rc<RefCell<Monitor>>) {
        info!("[tile]"); // 日志记录，进入 tile 布局函数

        // 初始化变量
        let mut n: u32 = 0; // 可见且平铺的客户端总数
        let mut mfacts: f32 = 0.0; // 主区域 (master area) 客户端的 cfact 总和
        let mut sfacts: f32 = 0.0; // 堆叠区域 (stack area) 客户端的 cfact 总和

        // --- 第一遍遍历：计算客户端数量和 cfact 总和 ---
        {
            // 创建一个新的作用域来限制 m_borrow 的生命周期
            let m_borrow = m_rc.borrow(); // 不可变借用 Monitor
            let mut c = self.nexttiled(m_borrow.clients.clone()); // 获取第一个可见且平铺的客户端
                                                                  // nexttiled 会跳过浮动和不可见的客户端

            while let Some(c_opt) = c {
                // 遍历所有可见且平铺的客户端
                let c_borrow = c_opt.borrow(); // 可变借用 Client 来读取和修改 cfact (虽然这里只读取)
                if n < m_borrow.nmaster0 {
                    // 如果当前客户端在主区域
                    mfacts += c_borrow.cfact; // 累加到主区域的 cfact 总和
                } else {
                    // 如果当前客户端在堆叠区域
                    sfacts += c_borrow.cfact; // 累加到堆叠区域的 cfact 总和
                }
                let next_c = self.nexttiled(c_borrow.next.clone()); // 获取下一个可见平铺客户端
                drop(c_borrow); // 显式 drop 可变借用，以便 nexttiled 中的借用不会冲突
                c = next_c;
                n += 1; // 客户端总数加一
            }
        } // m_borrow 在这里被 drop

        if n == 0 {
            // 如果没有可见且平铺的客户端，则直接返回
            return;
        }

        // --- 计算主区域的宽度 (mw) ---
        let (ww, mfact0_val, nmaster0_val, wx_val, wy_val, wh_val) = {
            // 再次借用 Monitor 获取其属性
            let m_borrow = m_rc.borrow();
            (
                m_borrow.ww,
                m_borrow.mfact0,
                m_borrow.nmaster0,
                m_borrow.wx,
                m_borrow.wy,
                m_borrow.wh,
            )
        };

        let mw: u32;
        if n > nmaster0_val {
            // 如果客户端总数大于主区域配置的窗口数
            mw = if nmaster0_val > 0 {
                // 如果主区域至少有一个窗口
                (ww as f32 * mfact0_val) as u32 // 主区域宽度 = 显示器工作区宽度 * mfact 比例
            } else {
                0 // 否则主区域宽度为 0 (所有窗口都在堆叠区)
            };
        } else {
            // 如果客户端总数小于等于主区域配置的窗口数 (所有窗口都在主区域)
            mw = ww as u32; // 主区域宽度 =整个显示器工作区宽度
        }

        // --- 第二遍遍历：调整客户端大小和位置 ---
        let mut my: u32 = 0; // 主区域当前窗口的 Y 轴累积高度
        let mut ty: u32 = 0; // 堆叠区域当前窗口的 Y 轴累积高度
        let mut i: u32 = 0; // 当前处理到的客户端索引
        let mut h: u32; // 当前客户端将要设置的高度

        let client_y_offset = {
            // 获取 Y 轴偏移（考虑状态栏）
            let m_borrow = m_rc.borrow();
            self.client_y_offset(&m_borrow)
        };

        let mut c_iter = {
            // 重新从头开始获取可见平铺客户端
            let m_borrow = m_rc.borrow();
            self.nexttiled(m_borrow.clients.clone())
        };

        while let Some(ref c_opt_rc) = c_iter {
            let next_client_in_list_opt; // 用于存储下一个迭代的客户端
            let bw;
            {
                // 创建一个新的作用域来限制 c_borrow 的生命周期
                let c_borrow = c_opt_rc.borrow(); // 不可变借用开始
                bw = c_borrow.bw;
                let current_cfact = c_borrow.cfact;
                next_client_in_list_opt = c_borrow.next.clone(); // 在释放借用前获取 next

                // 在这个作用域内完成所有对 c_borrow 的只读操作
                if i < nmaster0_val {
                    h = if mfacts > 0.001 {
                        ((wh_val as u32 - my) as f32 * (current_cfact / mfacts)) as u32
                    } else if nmaster0_val - i > 0 {
                        (wh_val as u32 - my) / (nmaster0_val - i)
                    } else {
                        wh_val as u32 - my
                    };
                    // drop(c_borrow) 会在这个作用域结束时自动发生
                } else {
                    h = if sfacts > 0.001 {
                        ((wh_val as u32 - ty) as f32 * (current_cfact / sfacts)) as u32
                    } else if n - i > 0 {
                        (wh_val as u32 - ty) / (n - i)
                    } else {
                        wh_val as u32 - ty
                    };
                    // drop(c_borrow) 会在这个作用域结束时自动发生
                }
            } // c_borrow (不可变借用) 在这里被 drop

            // 现在可以安全地调用 resize，它内部可以对 c_opt_rc 进行 borrow_mut()
            if i < nmaster0_val {
                self.resize(
                    c_opt_rc,
                    wx_val,
                    wy_val + my as i32 + client_y_offset,
                    mw as i32 - (2 * bw), // bw 之前已获取
                    h as i32 - (2 * bw) - client_y_offset,
                    false,
                );
                // resize 之后，如果需要读取更新后的 height，需要重新 borrow
                let client_actual_height = c_opt_rc.borrow().height() as u32;
                if my + client_actual_height < wh_val as u32 {
                    my += client_actual_height;
                }
                mfacts -= c_opt_rc.borrow().cfact; // 重新 borrow 读取 cfact (如果 cfact 不变，可以提前读取)
                                                   // 或者确保 cfact 在 resize 中不会改变，则可以使用之前读取的 current_cfact
            } else {
                self.resize(
                    c_opt_rc,
                    wx_val + mw as i32,
                    wy_val + ty as i32 + client_y_offset,
                    ww as i32 - mw as i32 - (2 * bw),
                    h as i32 - (2 * bw) - client_y_offset,
                    false,
                );
                let client_actual_height = c_opt_rc.borrow().height() as u32;
                if ty + client_actual_height < wh_val as u32 {
                    ty += client_actual_height;
                }
                sfacts -= c_opt_rc.borrow().cfact; // 同上
            }

            c_iter = self.nexttiled(next_client_in_list_opt); // 使用之前获取的 next
            i += 1;
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
                let mut sel_borrow = sel_opt.borrow_mut();
                sel_borrow.isfloating = !sel_borrow.isfloating || sel_borrow.isfixed;
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
            let sel = { self.selmon.as_mut().unwrap().borrow().sel.clone() };
            let ev = (*e).focus_change;
            if let Some(sel) = sel.as_ref() {
                if ev.window != sel.borrow().win {
                    self.setfocus(sel);
                }
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

    pub fn tag_egui_bar(&mut self, curtag: u32) {
        let mut sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
        let target_tag = if curtag == 0 {
            !curtag
        } else {
            1 << (curtag - 1)
        } & Config::tagmask;
        if sel.is_none() || target_tag <= 0 {
            return;
        }
        // Find egui_bar client.
        while let Some(ref sel_opt) = sel {
            let name = { sel_opt.borrow_mut().name.clone() };
            if name == Config::egui_bar_name {
                sel_opt.borrow_mut().tags0 = target_tag;
                self.setclienttagprop(&sel_opt);
                // self.focus(None); // (TODO): CHECK
                self.arrange(self.selmon.clone());
                break;
            }

            let next = sel_opt.borrow_mut().next.clone();
            sel = next;
        }
    }

    pub fn tag(&mut self, arg: *const Arg) {
        // info!("[tag]");
        unsafe {
            if let Arg::Ui(ui) = *arg {
                let sel = { self.selmon.as_ref().unwrap().borrow_mut().sel.clone() };
                // Don't tag neverfocus.
                if sel.as_ref().unwrap().borrow_mut().neverfocus {
                    return;
                }
                let target_tag = ui & Config::tagmask;
                if let Some(ref sel_opt) = sel {
                    if target_tag > 0 {
                        sel_opt.borrow_mut().tags0 = target_tag;
                        self.setclienttagprop(sel_opt);
                        self.focus(None);
                        self.arrange(self.selmon.clone());
                    }
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
                // Don't send neverfocus.
                if selmon_opt
                    .borrow_mut()
                    .sel
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .neverfocus
                {
                    return;
                }
            } else {
                return;
            }
            if let Some(ref mons_opt) = self.mons {
                if mons_opt.borrow_mut().next.is_none() {
                    return;
                }
            } else {
                return;
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
            if i == 0 {
                return;
            }
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
                while let Some(ref c_opt) = c {
                    if !c_opt.borrow_mut().neverfocus && c_opt.borrow_mut().isvisible() {
                        break;
                    }
                    let next = c_opt.borrow_mut().next.clone();
                    c = next;
                }
                if c.is_none() {
                    c = {
                        let selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                        selmon_mut.clients.clone()
                    };
                    while let Some(ref c_opt) = c {
                        if !c_opt.borrow_mut().neverfocus && c_opt.borrow_mut().isvisible() {
                            break;
                        }
                        let next = c_opt.borrow_mut().next.clone();
                        c = next;
                    }
                }
            } else {
                if let Some(ref selmon_opt) = self.selmon {
                    let (mut cl, sel) = {
                        let selmon_mut = selmon_opt.borrow_mut();
                        (selmon_mut.clients.clone(), selmon_mut.sel.clone())
                    };
                    while !Self::are_equal_rc(&cl, &sel) {
                        if let Some(ref cl_opt) = cl {
                            if !cl_opt.borrow_mut().neverfocus && cl_opt.borrow_mut().isvisible() {
                                c = cl.clone();
                            }
                            let next = cl_opt.borrow_mut().next.clone();
                            cl = next;
                        }
                    }
                    if c.is_none() {
                        while let Some(ref cl_opt) = cl {
                            if !cl_opt.borrow_mut().neverfocus && cl_opt.borrow_mut().isvisible() {
                                c = cl.clone();
                            }
                            let next = cl_opt.borrow_mut().next.clone();
                            cl = next;
                        }
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
            let c = { self.selmon.as_ref().unwrap().borrow().sel.clone() };
            if c.is_none() {
                return;
            }
            let lt_layout_type = {
                let selmon_mut = self.selmon.as_ref().unwrap().borrow();
                selmon_mut.lt[selmon_mut.sellt].layout_type.clone()
            };
            if lt_layout_type.is_none() {
                return;
            }
            if let Arg::F(f0) = *arg {
                let mut f = f0 + c.as_ref().unwrap().borrow().cfact;
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
        info!("[setlayout]");
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
            let selmon_mut = self.selmon.as_ref().unwrap().borrow();
            c = selmon_mut.sel.clone();
            let sellt = selmon_mut.sellt;
            if selmon_mut.lt[sellt].layout_type.is_none()
                || c.is_none()
                || c.as_ref().unwrap().borrow().isfloating
            {
                return;
            }
            sel_c = selmon_mut.clients.clone();
        }
        {
            nexttiled_c = self.nexttiled(sel_c);
        }
        if Self::are_equal_rc(&c, &nexttiled_c) {
            let next = self.nexttiled(c.as_ref().unwrap().borrow().next.clone());
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
                let target_tag = ui & Config::tagmask;
                let mut selmon_mut = self.selmon.as_ref().unwrap().borrow_mut();
                info!("[view] ui: {ui}, {target_tag}, {:?}", selmon_mut.tagset);
                if target_tag == selmon_mut.tagset[selmon_mut.seltags] {
                    return;
                }
                // toggle sel tagset.
                info!("[view] seltags: {}", selmon_mut.seltags);
                selmon_mut.seltags ^= 1;
                info!("[view] seltags: {}", selmon_mut.seltags);
                if target_tag > 0 {
                    let seltags = selmon_mut.seltags;
                    selmon_mut.tagset[seltags] = target_tag;
                    if let Some(pertag) = selmon_mut.pertag.as_mut() {
                        pertag.prevtag = pertag.curtag;
                    }
                    if ui == !0 {
                        selmon_mut.pertag.as_mut().unwrap().curtag = 0;
                    } else {
                        // cool
                        let i = ui.trailing_zeros() as usize;
                        selmon_mut.pertag.as_mut().unwrap().curtag = i + 1;
                    }
                } else {
                    if let Some(pertag) = selmon_mut.pertag.as_mut() {
                        std::mem::swap(&mut pertag.prevtag, &mut pertag.curtag);
                    }
                }
                if let Some(pertag) = selmon_mut.pertag.as_mut() {
                    info!(
                        "[view] prevtag: {}, curtag: {}",
                        pertag.prevtag, pertag.curtag
                    );
                }
            } else {
                return;
            }
            let sel;
            let curtag;
            {
                let mut selmon_mut = self.selmon.as_mut().unwrap().borrow_mut();
                curtag = selmon_mut.pertag.as_ref().unwrap().curtag;
                selmon_mut.nmaster0 = selmon_mut.pertag.as_ref().unwrap().nmasters[curtag];
                selmon_mut.mfact0 = selmon_mut.pertag.as_ref().unwrap().mfacts[curtag];
                selmon_mut.sellt = selmon_mut.pertag.as_ref().unwrap().sellts[curtag];
                let sellt = selmon_mut.sellt;
                selmon_mut.lt[sellt] = selmon_mut.pertag.as_ref().unwrap().ltidxs[curtag][sellt]
                    .clone()
                    .expect("None unwrap");
                selmon_mut.lt[sellt ^ 1] = selmon_mut.pertag.as_ref().unwrap().ltidxs[curtag]
                    [sellt ^ 1]
                    .clone()
                    .expect("None unwrap");
                sel = selmon_mut.pertag.as_ref().unwrap().sel[curtag].clone()
            };
            // for egui bar
            self.tag_egui_bar(curtag as u32);
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
            // Don't toggletag neverfocus.
            if sel.as_ref().unwrap().borrow_mut().neverfocus {
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
            self.drw = Some(Box::new(Drw::drw_create(self.dpy, self.visual, self.cmap)));
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
                self.scheme[i] = self
                    .drw
                    .as_mut()
                    .unwrap()
                    .drw_scm_create(Config::colors[i], &Config::alphas[i]);
            }
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
            let sel = { self.selmon.as_ref().unwrap().borrow().sel.clone() };
            if sel.is_none() {
                return;
            }
            // Don't kill neverfocus.
            if sel.as_ref().unwrap().borrow().neverfocus {
                return;
            }
            info!("[killclient] {}", sel.as_ref().unwrap().borrow());
            if !self.sendevent(
                &mut sel.as_ref().unwrap().borrow_mut(),
                self.wmatom[WM::WMDelete as usize],
            ) {
                XGrabServer(self.dpy);
                XSetErrorHandler(Some(transmute(xerrordummy as *const ())));
                XSetCloseDownMode(self.dpy, DestroyAll);
                XKillClient(self.dpy, sel.as_ref().unwrap().borrow().win);
                XSync(self.dpy, False);
                XSetErrorHandler(Some(transmute(xerror as *const ())));
                XUngrabServer(self.dpy);
            }
        }
    }

    pub fn nexttiled(&mut self, mut c: Option<Rc<RefCell<Client>>>) -> Option<Rc<RefCell<Client>>> {
        // info!("[nexttiled]");
        while let Some(ref c_opt) = c {
            let isfloating = c_opt.borrow().isfloating;
            let isvisible = c_opt.borrow().isvisible();
            if isfloating || !isvisible {
                let next = c_opt.borrow().next.clone();
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

    pub fn gettextprop(&mut self, w: Window, atom: Atom, text: &mut String) -> bool {
        // info!("[gettextprop]");
        unsafe {
            let mut name: XTextProperty = std::mem::zeroed();
            if XGetTextProperty(self.dpy, w, &mut name, atom) <= 0 || name.nitems <= 0 {
                return false;
            }
            *text = "".to_string();
            let mut list: *mut *mut c_char = std::ptr::null_mut();
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

    pub fn propertynotify(&mut self, e: *mut XEvent) {
        // info!("[propertynotify]");
        unsafe {
            let ev = (*e).property;
            let mut trans: Window = 0;
            if ev.window == self.root && ev.atom == XA_WM_NAME {
                // Hack to use this to react to signal from egui_bar
                info!("revoke by egui_bar");
                self.focus(None);
                self.arrange(None);
            } else if ev.state == PropertyDelete {
                // ignore
                return;
            } else if let Some(client_rc) = self.wintoclient(ev.window) {
                match ev.atom {
                    XA_WM_TRANSIENT_FOR => {
                        let mut client_borrowd = client_rc.borrow_mut();
                        if !client_borrowd.isfloating
                            && XGetTransientForHint(self.dpy, client_borrowd.win, &mut trans) > 0
                            && {
                                client_borrowd.isfloating = self.wintoclient(trans).is_some();
                                client_borrowd.isfloating
                            }
                        {
                            self.arrange(client_borrowd.mon.clone());
                        }
                    }
                    XA_WM_NORMAL_HINTS => {
                        let mut client_borrowd = client_rc.borrow_mut();
                        client_borrowd.hintsvalid = false;
                    }
                    XA_WM_HINTS => {
                        self.updatewmhints(&client_rc);
                        self.drawbars();
                    }
                    _ => {}
                }
                if ev.atom == XA_WM_NAME || ev.atom == self.netatom[NET::NetWMName as usize] {
                    self.updatetitle(&mut client_rc.borrow_mut());
                    let sel = {
                        client_rc
                            .borrow_mut()
                            .mon
                            .as_ref()
                            .unwrap()
                            .borrow_mut()
                            .sel
                            .clone()
                    };
                    if let Some(sel_opt) = sel {
                        if Rc::ptr_eq(&sel_opt, &client_rc) {
                            let mon = { client_rc.borrow_mut().mon.clone() };
                            self.drawbar(mon);
                        }
                    }
                }
                if ev.atom == self.netatom[NET::NetWMWindowType as usize] {
                    self.updatewindowtype(&client_rc);
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
        if c.name == Config::egui_bar_name
            && ((c.class == Config::egui_bar_0 && c.instance == Config::egui_bar_0)
                || (c.class == Config::egui_bar_1 && c.instance == Config::egui_bar_1))
        {
            c.neverfocus = true;
            if let Some(mon) = &c.mon {
                let num = mon.borrow_mut().num;
                let bar_shape = BarShape {
                    x: c.x,
                    y: c.y,
                    width: c.w,
                    height: c.h,
                };
                info!("[sendevent] num: {}, bar_shape: {:?}", num, bar_shape);
                self.egui_bar_shape.insert(num, bar_shape);
            }
        }
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
                ev.client_message.data.as_longs_mut()[0] = proto as i64;
                ev.client_message.data.as_longs_mut()[1] = CurrentTime as i64;
                XSendEvent(self.dpy, c.win, False, NoEventMask, &mut ev);
            }
        }
        return exists;
    }
    pub fn setfocus(&mut self, c: &Rc<RefCell<Client>>) {
        info!("[setfocus]");
        unsafe {
            let mut c = c.borrow_mut();
            if !c.neverfocus {
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
        info!("[drawbars]");
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
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
        info!("[focus]");
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
                    self.scheme[SCHEME::SchemeSel as usize][2]
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
                self.scheme[SCHEME::SchemeNorm as usize][2]
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
        // info!("[keypress]");
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
                self.updatetitle(&mut c.as_ref().unwrap().borrow_mut());
            }

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
                    self.scheme[SCHEME::SchemeNorm as usize][2]
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

    pub fn monocle(&mut self, m: &Rc<RefCell<Monitor>>) {
        info!("[monocle]");
        let mut m = m.borrow_mut();
        // This idea is cool!.
        let mut n: u32 = 0;
        let mut c = m.clients.clone();
        while let Some(ref c_opt) = c {
            if c_opt.borrow_mut().isvisible() && !c_opt.borrow_mut().neverfocus {
                n += 1;
            }
            let next = c_opt.borrow_mut().next.clone();
            c = next;
        }
        if n > 0 {
            // override layout symbol
            let formatted_string = format!("[{}]", n);
            info!("[monocle] formatted_string: {}", formatted_string);
            m.ltsymbol = formatted_string;
        }
        c = self.nexttiled((*m).clients.clone());
        let client_y_offset = self.client_y_offset(&m);
        while let Some(ref c_opt) = c {
            let bw = c_opt.borrow_mut().bw;
            self.resize(
                c_opt,
                m.wx,
                m.wy + client_y_offset,
                m.ww - 2 * bw,
                m.wh - 2 * bw - client_y_offset,
                false,
            );
            let next = self.nexttiled(c_opt.borrow_mut().next.clone());
            c = next;
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
                    cc.neverfocus = (*wmh).input <= 0;
                } else {
                    cc.neverfocus = false;
                }
                XFree(wmh as *mut _);
            }
        }
    }

    pub fn updatetitle(&mut self, c: &mut Client) {
        // info!("[updatetitle]");
        if !self.gettextprop(c.win, self.netatom[NET::NetWMName as usize], &mut c.name) {
            self.gettextprop(c.win, XA_WM_NAME, &mut c.name);
        }
        if c.name.is_empty() {
            c.name = Config::broken.to_string();
        }
    }

    pub fn update_bar_message_for_monitor(&mut self, m: Option<Rc<RefCell<Monitor>>>) {
        info!("[update_bar_message_for_monitor]");
        {
            info!(
                "[update_bar_message_for_monitor] {}, timestamp: {}",
                m.as_ref().unwrap().borrow_mut(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            );
        }
        self.message = SharedMessage::default();
        let mut monitor_info = MonitorInfo::default();
        let mut occ: u32 = 0;
        let mut urg: u32 = 0;
        {
            let m_mut = m.as_ref().unwrap().borrow_mut();
            monitor_info.monitor_x = m_mut.wx;
            monitor_info.monitor_y = m_mut.wy;
            monitor_info.monitor_width = m_mut.ww;
            monitor_info.monitor_height = m_mut.wh;
            monitor_info.monitor_num = m_mut.num;
            monitor_info.ltsymbol = m_mut.ltsymbol.clone();
            let mut c = m_mut.clients.clone();
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
        for i in 0..Config::tags_length {
            let seltags = { m.as_ref().unwrap().borrow_mut().seltags };
            let tagset = { m.as_ref().unwrap().borrow_mut().tagset };
            let tag_i = 1 << i;
            let is_selected_tag = tagset[seltags] & tag_i != 0;
            let is_urg_tag = urg & tag_i != 0;
            let is_occ_tag = occ & tag_i != 0;
            let mut is_filled_tag: bool = false;
            if is_occ_tag {
                if let Some(selmon_opt) = self.selmon.as_mut() {
                    is_filled_tag = Rc::ptr_eq(m.as_ref().unwrap(), &selmon_opt)
                        && selmon_opt
                            .borrow_mut()
                            .sel
                            .as_mut()
                            .map_or(false, |sel| sel.borrow_mut().tags0 & tag_i != 0);
                }
            }
            let tag_status = TagStatus::new(is_selected_tag, is_urg_tag, is_filled_tag, is_occ_tag);
            monitor_info.tag_status_vec.push(tag_status);
        }
        let mut sel_client_name = String::new();
        if let Some(ref sel_opt) = m.as_ref().unwrap().borrow_mut().sel {
            // draw client name
            sel_client_name = sel_opt.borrow_mut().name.clone();
        }
        monitor_info.client_name = sel_client_name;
        self.message.monitor_info = monitor_info;
    }
}
