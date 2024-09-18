#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
// #![allow(unused_mut)]

use std::ffi::{c_char, c_int, CStr, CString};
use std::mem::transmute;
use std::mem::zeroed;
use std::ptr::{addr_of, null_mut};
use std::{os::raw::c_long, usize};

use x11::keysym::XK_Num_Lock;
use x11::xlib::{
    AnyButton, AnyKey, AnyModifier, Atom, BadAccess, BadDrawable, BadMatch, BadWindow, Below,
    ButtonPressMask, ButtonReleaseMask, CWBackPixmap, CWBorderWidth, CWEventMask, CWHeight,
    CWOverrideRedirect, CWSibling, CWStackMode, CWWidth, ClientMessage, ConfigureNotify,
    ControlMask, CopyFromParent, CurrentTime, DestroyAll, Display, EnterWindowMask, ExposureMask,
    False, GrabModeSync, GrayScale, KeySym, LockMask, Mod1Mask, Mod2Mask, Mod3Mask, Mod4Mask,
    Mod5Mask, NoEventMask, NotifyInferior, NotifyNormal, PAspect, PBaseSize, PMaxSize, PMinSize,
    PResizeInc, PSize, ParentRelative, PointerMotionMask, PropModeAppend, PropModeReplace,
    ReplayPointer, RevertToPointerRoot, ShiftMask, StructureNotifyMask, SubstructureRedirectMask,
    Success, True, Window, XAllowEvents, XChangeProperty, XCheckMaskEvent, XClassHint,
    XConfigureEvent, XConfigureWindow, XCreateWindow, XDefaultDepth, XDefaultRootWindow,
    XDefaultVisual, XDefineCursor, XDeleteProperty, XDisplayKeycodes, XErrorEvent, XEvent, XFree,
    XFreeModifiermap, XFreeStringList, XGetClassHint, XGetKeyboardMapping, XGetModifierMapping,
    XGetTextProperty, XGetWMHints, XGetWMNormalHints, XGetWMProtocols, XGetWindowProperty,
    XGrabButton, XGrabKey, XGrabServer, XKeysymToKeycode, XKillClient, XMapRaised,
    XMoveResizeWindow, XMoveWindow, XQueryPointer, XRaiseWindow, XSelectInput, XSendEvent,
    XSetClassHint, XSetCloseDownMode, XSetErrorHandler, XSetInputFocus, XSetWMHints,
    XSetWindowAttributes, XSetWindowBorder, XSizeHints, XSync, XTextProperty, XUngrabButton,
    XUngrabKey, XUngrabServer, XUrgencyHint, XWindowChanges, XmbTextPropertyToTextList, CWX, CWY,
    XA_ATOM, XA_STRING, XA_WINDOW, XA_WM_NAME,
};

use std::cmp::{max, min};

use crate::config::{
    buttons, keys, layouts, mfact, nmaster, resizehints, rules, showbar, tags, topbar,
};
use crate::drw::{
    drw_fontset_getwidth, drw_map, drw_rect, drw_setscheme, drw_text, Clr, Cur, Drw, _Col,
};
use crate::xproto::{
    WithdrawnState, X_ConfigureWindow, X_CopyArea, X_GrabButton, X_GrabKey, X_PolyFillRectangle,
    X_PolySegment, X_PolyText8, X_SetInputFocus,
};

pub const BUTTONMASK: c_long = ButtonPressMask | ButtonReleaseMask;
#[inline]
fn CLEANMASK(mask: u32) -> u32 {
    return mask
        & unsafe { !(numlockmask | LockMask) }
        & (ShiftMask | ControlMask | Mod1Mask | Mod2Mask | Mod3Mask | Mod4Mask | Mod5Mask);
}
pub const MOUSEMASK: c_long = BUTTONMASK | PointerMotionMask;
pub const VERSION: &str = "6.5";

// Variables.
pub const broken: &str = "broken";
pub static mut stext: &str = "";
pub static mut screen: i32 = 0;
pub static mut sw: i32 = 0;
pub static mut sh: i32 = 0;
pub static mut bh: i32 = 0;
pub static mut lrpad: i32 = 0;
pub static mut numlockmask: u32 = 0;
pub static mut wmatom: [Atom; _WM::WMLast as usize] = unsafe { zeroed() };
pub static mut netatom: [Atom; _NET::NetLast as usize] = unsafe { zeroed() };
pub static mut running: i32 = 0;
pub static mut cursor: [*mut Cur; _CUR::CurLast as usize] = [null_mut(); _CUR::CurLast as usize];
pub static mut scheme: Vec<Vec<*mut Clr>> = vec![];
pub static mut dpy: *mut Display = null_mut();
pub static mut drw: *mut Drw = null_mut();
pub static mut mons: *mut Monitor = null_mut();
pub static mut selmon: *mut Monitor = null_mut();
pub static mut root: Window = 0;
pub static mut wmcheckwin: Window = 0;
pub static mut xerrorxlib: Option<unsafe extern "C" fn(*mut Display, *mut XErrorEvent) -> c_int> =
    None;

#[repr(C)]
#[derive(Debug, Clone)]
pub enum _CUR {
    CurNormal = 0,
    CurResize = 1,
    CurMove = 2,
    CurLast = 3,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum _SCHEME {
    SchemeNorm = 0,
    SchemeSel = 1,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum _NET {
    NetSupported = 0,
    NetWMName = 1,
    NetWMState = 2,
    NetWMCheck = 3,
    NetWMFullscreen = 4,
    NetActiveWindow = 5,
    NetWMWindowType = 6,
    NetWMWindowTypeDialog = 7,
    NetClientList = 8,
    NetLast = 9,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub enum _WM {
    WMProtocols = 0,
    WMDelete = 1,
    WMState = 2,
    WMTakeFocus = 3,
    WMLast = 4,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub enum _CLICK {
    ClkTagBar = 0,
    ClkLtSymbol = 1,
    ClkStatusText = 2,
    ClkWinTitle = 3,
    ClkClientWin = 4,
    ClkRootWin = 5,
    ClkLast = 6,
}

#[derive(Debug, Clone)]
pub enum Arg {
    i(i32),
    ui(u32),
    f(f32),
    v(Vec<&'static str>),
    lt(Layout),
}

#[derive(Debug, Clone)]
pub struct Button {
    pub click: u32,
    pub mask: u32,
    pub button: u32,
    pub func: Option<fn(*const Arg)>,
    pub arg: Arg,
}
impl Button {
    #[allow(unused)]
    pub fn new(click: u32, mask: u32, button: u32, func: Option<fn(*const Arg)>, arg: Arg) -> Self {
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
    pub func: Option<fn(*const Arg)>,
    pub arg: Arg,
}
impl Key {
    #[allow(unused)]
    pub fn new(mod0: u32, keysym: KeySym, func: Option<fn(*const Arg)>, arg: Arg) -> Self {
        Self {
            mod0,
            keysym,
            func,
            arg,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub symbol: &'static str,
    pub arrange: Option<fn(*mut Monitor)>,
}
impl Layout {
    #[allow(unused)]
    pub fn new(symbol: &'static str, arrange: Option<fn(*mut Monitor)>) -> Self {
        Self { symbol, arrange }
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    pub name: &'static str,
    pub mina: f32,
    pub maxa: f32,
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
    pub next: *mut Client,
    pub snext: *mut Client,
    pub mon: *mut Monitor,
    pub win: Window,
}
impl Client {
    #[allow(unused)]
    pub fn new() -> Self {
        Self {
            name: "",
            mina: 0.,
            maxa: 0.,
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
            next: null_mut(),
            snext: null_mut(),
            mon: null_mut(),
            win: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Monitor {
    pub ltsymbol: &'static str,
    pub mfact0: f32,
    pub nmaster0: i32,
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
    pub clients: *mut Client,
    pub sel: *mut Client,
    pub stack: *mut Client,
    pub next: *mut Monitor,
    pub barwin: Window,
    pub lt: [*mut Layout; 2],
}
impl Monitor {
    #[allow(unused)]
    pub fn new() -> Self {
        Self {
            ltsymbol: "",
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
            clients: null_mut(),
            sel: null_mut(),
            stack: null_mut(),
            next: null_mut(),
            barwin: 0,
            lt: [null_mut(); 2],
        }
    }
}

#[allow(unused)]
pub fn INTERSECT(x: i32, y: i32, w: i32, h: i32, m: *mut Monitor) -> i32 {
    unsafe {
        max(0, min(x + w, (*m).wx + (*m).ww) - max(x, (*m).wx))
            * max(0, min(y + h, (*m).wy + (*m).wh) - max(y, (*m).wy))
    }
}

pub fn ISVISIBLE(C: *const Client) -> u32 {
    unsafe { (*C).tags0 & (*(*C).mon).tagset[(*(*C).mon).seltags] }
}

pub fn WIDTH(X: *const Client) -> i32 {
    unsafe { (*X).w + 2 * (*X).bw }
}

pub fn HEIGHT(X: *const Client) -> i32 {
    unsafe { (*X).h + 2 * (*X).bw }
}

pub fn TAGMASK() -> u32 {
    (1 << tags.len()) - 1
}

pub fn TEXTW(drw0: *mut Drw, X: &str) -> u32 {
    unsafe { drw_fontset_getwidth(drw0, X) + lrpad as u32 }
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

// function declarations and implementations.
pub fn applyrules(c: *mut Client) {
    unsafe {
        (*c).isfloating = false;
        (*c).tags0 = 0;
        let mut ch: XClassHint = zeroed();
        XGetClassHint(dpy, (*c).win, &mut ch);
        let class = if !ch.res_class.is_null() {
            let c_str = CStr::from_ptr(ch.res_class);
            c_str.to_str().unwrap()
        } else {
            broken
        };
        let instance = if !ch.res_name.is_null() {
            let c_str = CStr::from_ptr(ch.res_name);
            c_str.to_str().unwrap()
        } else {
            broken
        };

        for r in &*rules {
            if (r.title.is_empty() || (*c).name.find(r.title).is_some())
                && (r.class.is_empty() || class.find(r.class).is_some())
                && (r.instance.is_empty() || instance.find(r.instance).is_some())
            {
                (*c).isfloating = r.isfloating;
                (*c).tags0 |= r.tags0 as u32;
                let mut m = mons;
                loop {
                    if m.is_null() || (*m).num == r.monitor {
                        break;
                    }
                    m = (*m).next;
                }
                if !m.is_null() {
                    (*c).mon = m;
                }
            }
        }
        if !ch.res_class.is_null() {
            XFree(ch.res_class as *mut _);
        }
        if !ch.res_name.is_null() {
            XFree(ch.res_name as *mut _);
        }
        (*c).tags0 = if ((*c).tags0 & TAGMASK()) > 0 {
            (*c).tags0 & TAGMASK()
        } else {
            (*(*c).mon).tagset[(*(*c).mon).seltags]
        }
    }
}

pub fn updatesizehints(c: *mut Client) {
    let msize: u32 = 0;
    unsafe {
        let mut size: XSizeHints = zeroed();

        if XGetWMNormalHints(dpy, (*c).win, &mut size, msize as *mut i64) <= 0 {
            size.flags = PSize;
        }
        if size.flags & PBaseSize > 0 {
            (*c).basew = size.base_width;
            (*c).baseh = size.base_height;
        } else if size.flags & PMinSize > 0 {
            (*c).basew = size.min_width;
            (*c).baseh = size.min_height;
        } else {
            (*c).basew = 0;
            (*c).baseh = 0;
        }
        if size.flags & PResizeInc > 0 {
            (*c).incw = size.width_inc;
            (*c).inch = size.height_inc;
        } else {
            (*c).incw = 0;
            (*c).inch = 0;
        }
        if size.flags & PMaxSize > 0 {
            (*c).maxw = size.max_width;
            (*c).maxh = size.max_height;
        } else {
            (*c).maxw = 0;
            (*c).maxh = 0;
        }
        if size.flags & PMinSize > 0 {
            (*c).minw = size.min_width;
            (*c).minh = size.min_height;
        } else if size.flags & PBaseSize > 0 {
            (*c).minw = size.base_width;
            (*c).minh = size.base_height;
        } else {
            (*c).minw = 0;
            (*c).minh = 0;
        }
        if size.flags & PAspect > 0 {
            (*c).mina = size.min_aspect.y as f32 / size.min_aspect.x as f32;
            (*c).maxa = size.max_aspect.x as f32 / size.max_aspect.y as f32;
        } else {
            (*c).maxa = 0.;
            (*c).mina = 0.;
        }
        (*c).isfixed =
            (*c).maxw > 0 && (*c).maxh > 0 && ((*c).maxw == (*c).minw) && ((*c).maxh == (*c).minh);
        (*c).hintsvalid = true;
    }
}

pub fn applysizehints(
    c: *mut Client,
    x: &mut i32,
    y: &mut i32,
    w: &mut i32,
    h: &mut i32,
    interact: bool,
) -> bool {
    unsafe {
        let m = (*c).mon;

        // set minimum possible.
        *w = 1.max(*w);
        *h = 1.max(*h);
        if interact {
            if *x > sw {
                *x = sw - WIDTH(c);
            }
            if *y > sh {
                *y = sh - HEIGHT(c);
            }
            if *x + *w + 2 * (*c).bw < 0 {
                *x = 0;
            }
            if *y + *h + 2 * (*c).bw < 0 {
                *y = 0;
            }
        } else {
            if *x >= (*m).wx + (*m).ww {
                *x = (*m).wx + (*m).ww - WIDTH(c);
            }
            if *y >= (*m).wy + (*m).wh {
                *x = (*m).wy + (*m).wh - HEIGHT(c);
            }
            if *x + *w + 2 * (*c).bw <= (*m).wx {
                *x = (*m).wx;
            }
            if *y + *h + 2 * (*c).bw <= (*m).wy {
                *y = (*m).wy;
            }
        }
        if *h < bh {
            *h = bh;
        }
        if *w < bh {
            *w = bh;
        }
        if resizehints || (*c).isfloating || (*(*(*c).mon).lt[(*(*c).mon).sellt]).arrange.is_none()
        {
            if !(*c).hintsvalid {
                updatesizehints(c);
            }
            // see last two sentences in ICCCM 4.1.2.3
            let baseismin = (*c).basew == (*c).minw && (*c).baseh == (*c).minh;
            if !baseismin {
                // temporarily remove base dimensions.
                (*w) -= (*c).basew;
                (*h) -= (*c).baseh;
            }
            // adjust for aspect limits.
            if (*c).mina > 0. && (*c).maxa > 0. {
                if (*c).maxa < *w as f32 / *h as f32 {
                    *w = (*h as f32 * (*c).maxa + 0.5) as i32;
                } else if (*c).mina < *h as f32 / *w as f32 {
                    *h = (*w as f32 * (*c).mina + 0.5) as i32;
                }
            }
            if baseismin {
                // increment calcalation requires this.
                *w -= (*c).basew;
                *h -= (*c).baseh;
            }
            // adjust for increment value.
            if (*c).incw > 0 {
                *w -= *w % (*c).incw;
            }
            if (*c).inch > 0 {
                *h -= *h % (*c).inch;
            }
            // restore base dimensions.
            *w = (*w + (*c).basew).max((*c).minw);
            *h = (*h + (*c).baseh).max((*c).minh);
            if (*c).maxw > 0 {
                *w = *w.min(&mut (*c).maxw);
            }
            if (*c).maxh > 0 {
                *h = *h.min(&mut (*c).maxh);
            }
        }
        return *x != (*c).x || (*y) != (*c).y || *w != (*c).w || *h != (*c).h;
    }
}

pub fn configure(c: *mut Client) {
    unsafe {
        let mut ce: XConfigureEvent = zeroed();

        ce.type_ = ConfigureNotify;
        ce.display = dpy;
        ce.event = (*c).win;
        ce.window = (*c).win;
        ce.x = (*c).x;
        ce.y = (*c).y;
        ce.width = (*c).w;
        ce.height = (*c).h;
        ce.border_width = (*c).bw;
        ce.above = 0;
        ce.override_redirect = 0;
        let mut xe = XEvent { configure: ce };
        XSendEvent(dpy, (*c).win, 0, StructureNotifyMask, &mut xe);
    }
}
pub fn setfullscreen(c: *mut Client, fullscreen: bool) {
    unsafe {
        if fullscreen && !(*c).isfullscreen {
            XChangeProperty(
                dpy,
                (*c).win,
                netatom[_NET::NetWMState as usize],
                XA_ATOM,
                32,
                PropModeReplace,
                netatom[_NET::NetWMFullscreen as usize] as *const _,
                1,
            );
            (*c).isfullscreen = true;
            (*c).oldstate = (*c).isfloating;
            (*c).oldbw = (*c).bw;
            (*c).bw = 0;
            (*c).isfloating = true;
            resizeclient(
                c,
                (*(*c).mon).mx,
                (*(*c).mon).my,
                (*(*c).mon).mw,
                (*(*c).mon).mh,
            );
            XRaiseWindow(dpy, (*c).win);
        } else if !fullscreen && (*c).isfullscreen {
            XChangeProperty(
                dpy,
                (*c).win,
                netatom[_NET::NetWMState as usize],
                XA_ATOM,
                32,
                PropModeReplace,
                0 as *const _,
                0,
            );
            (*c).isfullscreen = false;
            (*c).isfloating = (*c).oldstate;
            (*c).bw = (*c).oldbw;
            (*c).x = (*c).oldx;
            (*c).y = (*c).oldy;
            (*c).w = (*c).oldw;
            (*c).h = (*c).oldh;
            resizeclient(c, (*c).x, (*c).y, (*c).w, (*c).h);
            arrange((*c).mon);
        }
    }
}
pub fn resizeclient(c: *mut Client, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let mut wc: XWindowChanges = zeroed();
        (*c).oldx = (*c).x;
        (*c).x = x;
        wc.x = x;
        (*c).oldy = (*c).y;
        (*c).y = y;
        wc.y = y;
        (*c).oldw = (*c).w;
        (*c).w = w;
        wc.width = w;
        (*c).oldh = (*c).h;
        (*c).h = h;
        wc.height = h;
        wc.border_width = (*c).bw;
        XConfigureWindow(
            dpy,
            (*c).win,
            (CWX | CWY | CWWidth | CWHeight | CWBorderWidth).into(),
            &mut wc as *mut _,
        );
        configure(c);
        XSync(dpy, 0);
    }
}

pub fn resize(c: *mut Client, mut x: i32, mut y: i32, mut w: i32, mut h: i32, interact: bool) {
    if applysizehints(c, &mut x, &mut y, &mut w, &mut h, interact) {
        resizeclient(c, x, y, w, h);
    }
}

pub fn seturgent(c: *mut Client, urg: bool) {
    unsafe {
        (*c).isurgent = urg;
        let wmh = XGetWMHints(dpy, (*c).win);
        if wmh.is_null() {
            return;
        }
        (*wmh).flags = if urg {
            (*wmh).flags | XUrgencyHint
        } else {
            (*wmh).flags & !XUrgencyHint
        };
        XSetWMHints(dpy, (*c).win, wmh);
        XFree(wmh as *mut _);
    }
}

pub fn showhide(c: *mut Client) {
    if c.is_null() {
        return;
    }
    unsafe {
        if ISVISIBLE(c) > 0 {
            // show clients top down.
            XMoveWindow(dpy, (*c).win, (*c).x, (*c).y);
            if ((*(*(*c).mon).lt[(*(*c).mon).sellt]).arrange.is_some() || (*c).isfloating)
                && !(*c).isfullscreen
            {
                resize(c, (*c).x, (*c).y, (*c).w, (*c).h, false);
            }
            showhide((*c).snext);
        } else {
            // hide clients bottom up.
            showhide((*c).snext);
            XMoveWindow(dpy, (*c).win, WIDTH(c) * -2, (*c).y);
        }
    }
}
pub fn createmon() -> *mut Monitor {
    let mut m: Monitor = Monitor::new();
    m.tagset[0] = 1;
    m.tagset[1] = 1;
    m.mfact0 = mfact;
    m.nmaster0 = nmaster;
    m.showbar0 = showbar;
    m.topbar0 = topbar;
    // m.lt[0] = &mut layouts[0].clone();
    // m.lt[1] = &mut layouts[1 % layouts.len()].clone();
    m.ltsymbol = layouts[0].symbol;
    return &mut m;
}
pub fn arrangemon(m: *mut Monitor) {
    unsafe {
        (*m).ltsymbol = (*(*m).lt[(*m).sellt]).symbol;
        if let Some(arrange0) = (*(*m).lt[(*m).sellt]).arrange {
            arrange0(m);
        }
    }
}
pub fn detach(c: *mut Client) {
    unsafe {
        let mut tc: *mut *mut Client = &mut (*(*c).mon).clients;
        while !tc.is_null() && *tc != c {
            tc = &mut (*(*tc)).next;
        }
        *tc = (*c).next;
    }
}
pub fn detachstack(c: *mut Client) {
    unsafe {
        let mut tc: *mut *mut Client = &mut (*(*c).mon).stack;
        while !tc.is_null() && *tc != c {
            tc = &mut (*(*tc)).snext;
        }
        *tc = (*c).snext;

        if c == (*(*c).mon).sel {
            let mut t = (*(*c).mon).stack;
            while !t.is_null() && (ISVISIBLE(t) <= 0) {
                (*(*c).mon).sel = t;
                t = (*t).snext;
            }
        }
    }
}
pub fn dirtomon(dir: i32) -> *mut Monitor {
    unsafe {
        let mut m: *mut Monitor;
        if dir > 0 {
            m = (*selmon).next;
            if !m.is_null() {
                m = mons;
            }
        } else if selmon == mons {
            m = mons;
            while !m.is_null() && !(*m).next.is_null() {
                m = (*m).next;
            }
        } else {
            m = mons;
            while !m.is_null() && (*m).next != selmon {
                m = (*m).next;
            }
        }
        m
    }
}
pub fn drawbar(m: *mut Monitor) {
    let mut tw: i32 = 0;
    let mut _i: u32 = 0;
    let mut occ: u32 = 0;
    let mut urg: u32 = 0;
    unsafe {
        let boxs = (*(*drw).fonts).h / 9;
        let boxw = (*(*drw).fonts).h / 6 + 2;

        if !(*m).showbar0 {
            return;
        }

        // draw status first so it can be overdrawn by tags later.
        if m == selmon {
            // status is only drawn on selected monitor.
            drw_setscheme(drw, scheme[_SCHEME::SchemeNorm as usize].clone());
            // 2px right padding.
            tw = TEXTW(drw, stext) as i32 - lrpad + 2;
            drw_text(
                drw,
                (*m).ww - tw,
                0,
                tw.try_into().unwrap(),
                bh.try_into().unwrap(),
                0,
                stext,
                0,
            );
        }
        let mut c = (*m).clients;
        while !c.is_null() {
            occ |= (*c).tags0;
            if (*c).isurgent {
                urg |= (*c).tags0;
            }
            c = (*c).next;
        }
        let mut x = 0;
        let mut w;
        for i in 0..tags.len() {
            w = TEXTW(drw, tags[i]) as i32;
            let idx = if (*m).tagset[(*m).seltags] & 1 << i > 0 {
                _SCHEME::SchemeSel as usize
            } else {
                _SCHEME::SchemeNorm as usize
            };
            drw_setscheme(drw, scheme[idx].clone());
            drw_text(
                drw,
                x,
                0,
                w as u32,
                bh.try_into().unwrap(),
                (lrpad / 2) as u32,
                tags[i],
                (urg & 1 << i) as i32,
            );
            if (occ & 1 << i) > 0 {
                drw_rect(
                    drw,
                    x + boxs as i32,
                    0,
                    boxs,
                    boxw,
                    ((m == selmon)
                        && !(*selmon).sel.is_null()
                        && ((*(*selmon).sel).tags0 & 1 << i > 0)) as i32,
                    (urg & 1 << i).try_into().unwrap(),
                );
                x += w;
            }
        }
        w = TEXTW(drw, (*m).ltsymbol) as i32;
        drw_setscheme(drw, scheme[_SCHEME::SchemeNorm as usize].clone());
        x = drw_text(
            drw,
            x,
            0,
            w.try_into().unwrap(),
            bh.try_into().unwrap(),
            (lrpad / 2).try_into().unwrap(),
            (*m).ltsymbol,
            0,
        );

        w = (*m).ww - tw - x;
        if w > bh {
            if !(*m).sel.is_null() {
                let idx = if m == selmon {
                    _SCHEME::SchemeSel
                } else {
                    _SCHEME::SchemeNorm
                } as usize;
                drw_setscheme(drw, scheme[idx].clone());
                drw_text(
                    drw,
                    x,
                    0,
                    w.try_into().unwrap(),
                    bh.try_into().unwrap(),
                    (lrpad / 2).try_into().unwrap(),
                    (*m).ltsymbol,
                    0,
                );
                if (*(*m).sel).isfloating {
                    drw_rect(
                        drw,
                        x + boxs as i32,
                        boxs as i32,
                        boxw,
                        boxw,
                        (*(*m).sel).isfixed as i32,
                        0,
                    );
                }
            } else {
                drw_setscheme(drw, scheme[_SCHEME::SchemeNorm as usize].clone());
                drw_rect(
                    drw,
                    x,
                    0,
                    w.try_into().unwrap(),
                    bh.try_into().unwrap(),
                    1,
                    1,
                );
            }
        }
        drw_map(
            drw,
            (*m).barwin,
            0,
            0,
            (*m).ww.try_into().unwrap(),
            bh.try_into().unwrap(),
        );
    }
}

pub fn restack(m: *mut Monitor) {
    drawbar(m);

    unsafe {
        let mut wc: XWindowChanges = zeroed();
        if (*m).sel.is_null() {
            return;
        }
        if (*(*m).sel).isfloating || (*(*m).lt[(*m).sellt]).arrange.is_none() {
            XRaiseWindow(dpy, (*(*m).sel).win);
        }
        if (*(*m).lt[(*m).sellt]).arrange.is_some() {
            wc.stack_mode = Below;
            wc.sibling = (*m).barwin;
            let mut c = (*m).stack;
            while !c.is_null() {
                if !(*c).isfloating && ISVISIBLE(c) > 0 {
                    XConfigureWindow(dpy, (*c).win, (CWSibling | CWStackMode) as u32, &mut wc);
                    wc.sibling = (*c).win;
                }
                c = (*c).snext;
            }
        }
        XSync(dpy, 0);
        let mut ev: XEvent = zeroed();
        loop {
            if XCheckMaskEvent(dpy, EnterWindowMask, &mut ev) <= 0 {
                break;
            }
        }
    }
}

pub fn arrange(mut m: *mut Monitor) {
    unsafe {
        if !m.is_null() {
            showhide((*m).stack);
        } else {
            m = mons;
            loop {
                if m.is_null() {
                    break;
                }
                showhide((*m).stack);
                m = (*m).next;
            }
        }
        if !m.is_null() {
            arrangemon(m);
            restack(m);
        } else {
            m = mons;
            while !m.is_null() {
                arrangemon((*m).next);
                m = (*m).next;
            }
        }
    }
}

pub fn attach(c: *mut Client) {
    unsafe {
        (*c).next = (*(*c).mon).clients;
        (*(*c).mon).clients = c;
    }
}
pub fn attachstack(c: *mut Client) {
    unsafe {
        (*c).snext = (*(*c).mon).stack;
        (*(*c).mon).stack = c;
    }
}

pub fn getatomprop(c: *mut Client, prop: Atom) -> u64 {
    let mut di = 0;
    let mut dl: u64 = 0;
    let mut da: Atom = 0;
    let mut atom: Atom = 0;
    let mut p: *mut u8 = null_mut();
    unsafe {
        if XGetWindowProperty(
            dpy,
            (*c).win,
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

pub fn getrootptr(x: &mut i32, y: &mut i32) -> i32 {
    let mut di: i32 = 0;
    let mut dui: u32 = 0;
    unsafe {
        let mut dummy: Window = zeroed();

        return XQueryPointer(
            dpy, root, &mut dummy, &mut dummy, x, y, &mut di, &mut di, &mut dui,
        );
    }
}

pub fn getstate(w: Window) -> i64 {
    let mut format: i32 = 0;
    let mut result: i64 = -1;
    let mut p: *mut u8 = null_mut();
    let mut n: u64 = 0;
    let mut extra: u64 = 0;
    let mut real: Atom = 0;
    unsafe {
        if XGetWindowProperty(
            dpy,
            w,
            wmatom[_WM::WMState as usize],
            0,
            2,
            False,
            wmatom[_WM::WMState as usize],
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

pub fn recttomon(x: i32, y: i32, w: i32, h: i32) -> *mut Monitor {
    let mut area: i32 = 0;

    unsafe {
        let mut r: *mut Monitor = selmon;
        let mut m = mons;
        while !m.is_null() {
            let a = INTERSECT(x, y, w, h, m);
            if a > area {
                area = a;
                r = m;
            }
            m = (*m).next;
        }
        return r;
    }
}

pub fn wintoclient(w: Window) -> *mut Client {
    unsafe {
        let mut m = mons;
        while !m.is_null() {
            let mut c = (*m).clients;
            while !c.is_null() {
                if (*c).win == w {
                    return c;
                }
                c = (*c).next;
            }
            m = (*m).next;
        }
    }
    null_mut()
}

pub fn wintomon(w: Window) -> *mut Monitor {
    let mut x: i32 = 0;
    let mut y: i32 = 0;
    unsafe {
        if w == root && getrootptr(&mut x, &mut y) > 0 {
            return recttomon(x, y, 1, 1);
        }
        let mut m = mons;
        while !m.is_null() {
            if w == (*m).barwin {
                return m;
            }
            m = (*m).next;
        }
        let c = wintoclient(w);
        if !c.is_null() {
            return (*c).mon;
        }
        return selmon;
    }
}

pub fn buttonpress(e: *mut XEvent) {
    let mut i: usize = 0;
    let mut x: u32 = 0;
    let mut arg: Arg = Arg::ui(0);
    unsafe {
        let c: *mut Client;
        let ev = (*e).button;
        let mut click = _CLICK::ClkRootWin;
        // focus monitor if necessary.
        let m = wintomon(ev.window);
        if m != selmon {
            unfocus((*selmon).sel, true);
            selmon = m;
            focus(null_mut());
        }
        if ev.window == (*selmon).barwin {
            loop {
                x += TEXTW(drw, tags[i]);
                if ev.x >= x as i32
                    && ({
                        i += 1;
                        i
                    } < tags.len())
                {
                    break;
                }
            }
            if i < tags.len() {
                click = _CLICK::ClkTagBar;
                arg = Arg::ui(1 << i);
            } else if ev.x < (x + TEXTW(drw, (*selmon).ltsymbol)) as i32 {
                click = _CLICK::ClkLtSymbol;
            } else if ev.x > (*selmon).ww - TEXTW(drw, stext) as i32 {
                click = _CLICK::ClkStatusText;
            } else {
                click = _CLICK::ClkWinTitle;
            }
        } else if {
            c = wintoclient(ev.window);
            !c.is_null()
        } {
            focus(c);
            restack(selmon);
            XAllowEvents(dpy, ReplayPointer, CurrentTime);
            click = _CLICK::ClkClientWin;
        }
        for i in 0..buttons.len() {
            if click as u32 == buttons[i].click
                && buttons[i].func.is_some()
                && buttons[i].button == ev.button
                && CLEANMASK(buttons[i].mask) == CLEANMASK(ev.state)
            {
                buttons[i].func.unwrap()({
                    if click as u32 == _CLICK::ClkTagBar as u32 && {
                        if let Arg::ui(0) = arg {
                            true
                        } else {
                            false
                        }
                    } {
                        &mut arg
                    } else {
                        &mut buttons[i].arg.clone()
                    }
                });
            }
        }
    }
}

pub fn xerrordummy(_: *mut Display, _: *mut XErrorEvent) -> i32 {
    0
}
// #[no_mangle]
// pub extern "C" fn xerrorstart(dpy0: *mut Display, ee: *mut XErrorEvent) -> i32 {
//     eprintln!("jwm: another window manager is already running");
//     return -1;
// }
// Or use the method above.
pub fn xerrorstart(_: *mut Display, _: *mut XErrorEvent) -> i32 {
    eprintln!("jwm: another window manager is already running");
    return -1;
}
// There's no way to check accesses to destroyed windows, thus those cases are ignored (especially
// on UnmapNotify's). Other types of errors call xlibs default error handler, which may call exit.
pub fn xerror(_: *mut Display, ee: *mut XErrorEvent) -> i32 {
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
        {
            return 0;
        }
        println!(
            "jwm: fatal error: request code = {}, error code = {}",
            (*ee).request_code,
            (*ee).error_code
        );
        // may call exit.
        return xerrorxlib.unwrap()(dpy, ee);
    }
}
pub fn checkotherwm() {
    unsafe {
        xerrorxlib = XSetErrorHandler(Some(transmute(xerrorstart as *const ())));
        // this causes an error if some other window manager is running.
        XSelectInput(dpy, XDefaultRootWindow(dpy), SubstructureRedirectMask);
        XSync(dpy, False);
        // Attention what transmut does is great;
        XSetErrorHandler(Some(transmute(xerror as *const ())));
    }
}

pub fn spawn(arg: *const Arg) {}
pub fn updatebars() {
    unsafe {
        let mut wa: XSetWindowAttributes = zeroed();
        wa.override_redirect = True;
        wa.background_pixmap = ParentRelative as u64;
        wa.event_mask = ButtonPressMask | ExposureMask;
        let mut ch: XClassHint = zeroed();
        let c_string = CString::new("jwm").expect("fail to convert");
        ch.res_name = c_string.as_ptr() as *mut _;
        ch.res_class = c_string.as_ptr() as *mut _;
        let mut m = mons;
        while !m.is_null() {
            if (*m).barwin > 0 {
                continue;
            }
            (*m).barwin = XCreateWindow(
                dpy,
                root,
                (*m).wx,
                (*m).by,
                (*m).ww.try_into().unwrap(),
                bh.try_into().unwrap(),
                0,
                XDefaultDepth(dpy, screen),
                CopyFromParent as u32,
                XDefaultVisual(dpy, screen),
                CWOverrideRedirect | CWBackPixmap | CWEventMask,
                &mut wa,
            );
            XDefineCursor(dpy, (*m).barwin, (*cursor[_CUR::CurNormal as usize]).cursor);
            XMapRaised(dpy, (*m).barwin);
            XSetClassHint(dpy, (*m).barwin, &mut ch);
            m = (*m).next;
        }
    }
}
pub fn updatebarpos(m: *mut Monitor) {
    unsafe {
        (*m).wy = (*m).my;
        (*m).wh = (*m).mh;
        if (*m).showbar0 {
            (*m).wh -= bh;
            (*m).by = if (*m).topbar0 {
                (*m).wy
            } else {
                (*m).wy + (*m).wh
            };
        } else {
            (*m).by = -bh;
        }
    }
}
pub fn updateclientlist() {
    unsafe {
        XDeleteProperty(dpy, root, netatom[_NET::NetClientList as usize]);
        let mut m = mons;
        while !m.is_null() {
            let mut c = (*m).clients;
            while !c.is_null() {
                XChangeProperty(
                    dpy,
                    root,
                    netatom[_NET::NetClientList as usize],
                    XA_WINDOW,
                    32,
                    PropModeAppend,
                    (*c).win as *const _,
                    1,
                );
                c = (*c).next;
            }
            m = (*m).next;
        }
    }
}
pub fn togglebar(arg: *const Arg) {
    unsafe {
        (*selmon).showbar0 = !(*selmon).showbar0;
        updatebarpos(selmon);
        XMoveResizeWindow(
            dpy,
            (*selmon).barwin,
            (*selmon).wx,
            (*selmon).by,
            (*selmon).ww as u32,
            bh as u32,
        );
        arrange(selmon);
    }
}
pub fn togglefloating(arg: *const Arg) {
    unsafe {
        if (*selmon).sel.is_null() {
            return;
        }
        // no support for fullscreen windows.
        if (*(*selmon).sel).isfullscreen {
            return;
        }
        (*(*selmon).sel).isfloating = !(*(*selmon).sel).isfloating || (*(*selmon).sel).isfixed;
        if (*(*selmon).sel).isfloating {
            resize(
                (*selmon).sel,
                (*(*selmon).sel).x,
                (*(*selmon).sel).y,
                (*(*selmon).sel).w,
                (*(*selmon).sel).h,
                false,
            );
        }
        arrange(selmon);
    }
}
pub fn focusin(e: *mut XEvent) {
    unsafe {
        let ev = (*e).focus_change;
        if !(*selmon).sel.is_null() && ev.window != (*(*selmon).sel).win {
            setfocus((*selmon).sel);
        }
    }
}
pub fn focusmon(arg: *const Arg) {
    unsafe {
        if (*mons).next.is_null() {
            return;
        }
        if let Arg::i(i) = *arg {
            let m = dirtomon(i);
            if m == selmon {
                return;
            }
            unfocus((*selmon).sel, false);
            selmon = m;
            focus(null_mut());
        }
    }
}
pub fn tag(arg: *const Arg) {
    unsafe {
        if let Arg::ui(ui) = *arg {
            if !(*selmon).sel.is_null() && (ui & TAGMASK()) > 0 {
                (*(*selmon).sel).tags0 = ui & TAGMASK();
                focus(null_mut());
                arrange(selmon);
            }
        }
    }
}
pub fn tagmon(arg: *const Arg) {
    unsafe {
        if (*selmon).sel.is_null() || (*mons).next.is_null() {
            return;
        }
        if let Arg::i(i) = *arg {
            sendmon((*selmon).sel, dirtomon(i));
        }
    }
}
pub fn focusstack(arg: *const Arg) {}
pub fn incnmaster(arg: *const Arg) {
    unsafe {
        if let Arg::i(i) = *arg {
            (*selmon).nmaster0 = 0.max((*selmon).nmaster0 + i);
        }
    }
}
pub fn setmfact(arg: *const Arg) {
    unsafe {
        if arg.is_null() || (*(*selmon).lt[(*selmon).sellt]).arrange.is_none() {
            return;
        }
    }
    unsafe {
        if let Arg::f(f) = *arg {
            let f = if f < 1.0 {
                f + (*selmon).mfact0
            } else {
                f - 1.0
            };
            if f < 0.05 || f > 0.95 {
                return;
            }
            (*selmon).mfact0 = f;
        }
        arrange(selmon);
    }
}
pub fn setlayout(arg: *const Arg) {
    unsafe {
        if arg.is_null()
            || if let Arg::lt(ref lt) = *arg {
                if *lt != *(*selmon).lt[(*selmon).sellt] {
                    true
                } else {
                    false
                }
            } else {
                true
            }
        {
            (*selmon).sellt ^= 1;
        }
        if !arg.is_null() {
            if let Arg::lt(ref lt) = *arg {
                (*selmon).lt[(*selmon).sellt] = lt as *const _ as *mut _;
            }
        }
        (*selmon).ltsymbol = (*(*selmon).lt[(*selmon).sellt]).symbol;
        if !(*selmon).sel.is_null() {
            arrange(selmon);
        } else {
            drawbar(selmon);
        }
    }
}
pub fn zoom(arg: *const Arg) {}
pub fn view(arg: *const Arg) {
    unsafe {
        let ui = if let Arg::ui(ui) = *arg { ui } else { 0 };
        if (ui & TAGMASK()) == (*selmon).tagset[(*selmon).seltags] {
            return;
        }
        // toggle sel tagset.
        (*selmon).seltags ^= 1;
        if ui & TAGMASK() > 0 {
            (*selmon).tagset[(*selmon).seltags] = ui & TAGMASK();
        }
        focus(null_mut());
        arrange(selmon);
    }
}
pub fn toggleview(arg: *const Arg) {
    unsafe {
        if let Arg::ui(ui) = *arg {
            let newtagset = (*selmon).tagset[(*selmon).seltags] ^ (ui & TAGMASK());
            if newtagset > 0 {
                (*selmon).tagset[(*selmon).seltags] = newtagset;
                focus(null_mut());
                arrange(selmon);
            }
        }
    }
}
pub fn toggletag(arg: *const Arg) {
    unsafe {
        if (*selmon).sel.is_null() {
            return;
        }
        if let Arg::ui(ui) = *arg {
            let newtags = (*(*selmon).sel).tags0 ^ (ui & TAGMASK());
            if newtags > 0 {
                (*(*selmon).sel).tags0 = newtags;
                focus(null_mut());
                arrange(selmon);
            }
        }
    }
}
pub fn quit(arg: *const Arg) {
    unsafe {
        running = 0;
    }
}
pub fn killclient(arg: *const Arg) {
    unsafe {
        if (*selmon).sel.is_null() {
            return;
        }
        if !sendevent((*selmon).sel, wmatom[_WM::WMDelete as usize]) {
            XGrabServer(dpy);
            XSetErrorHandler(Some(transmute(xerrordummy as *const ())));
            XSetCloseDownMode(dpy, DestroyAll);
            XKillClient(dpy, (*(*selmon).sel).win);
            XSync(dpy, False);
            XSetErrorHandler(Some(transmute(xerror as *const ())));
            XUngrabServer(dpy);
        }
    }
}
pub fn nexttiled(mut c: *mut Client) -> *mut Client {
    unsafe {
        loop {
            if !c.is_null() && ((*c).isfloating || ISVISIBLE(c) <= 0) {
                c = (*c).next;
            } else {
                break;
            }
        }
        return c;
    }
}
pub fn pop(c: *mut Client) {
    detach(c);
    attach(c);
    focus(c);
    unsafe {
        arrange((*c).mon);
    }
}
pub fn movemouse(arg: *const Arg) {}
pub fn resizemouse(arg: *const Arg) {}
pub fn updatenumlockmask() {
    unsafe {
        numlockmask = 0;
        let modmap = XGetModifierMapping(dpy);
        for i in 0..8 {
            for j in 0..(*modmap).max_keypermod {
                if *(*modmap)
                    .modifiermap
                    .wrapping_add((i * (*modmap).max_keypermod + j) as usize)
                    == XKeysymToKeycode(dpy, XK_Num_Lock as u64)
                {
                    numlockmask = 1 << i;
                }
            }
        }
        XFreeModifiermap(modmap);
    }
}
pub fn grabbuttons(c: *mut Client, focused: bool) {
    updatenumlockmask();
    unsafe {
        let modifiers = [0, LockMask, numlockmask, numlockmask | LockMask];
        XUngrabButton(dpy, AnyButton as u32, AnyModifier, (*c).win);
        if !focused {
            XGrabButton(
                dpy,
                AnyButton as u32,
                AnyModifier,
                (*c).win,
                False,
                BUTTONMASK as u32,
                GrabModeSync,
                GrabModeSync,
                0,
                0,
            );
        }
        for i in 0..buttons.len() {
            if buttons[i].click == _CLICK::ClkClientWin as u32 {
                for j in 0..modifiers.len() {
                    XGrabButton(
                        dpy,
                        buttons[i].button,
                        buttons[i].mask | modifiers[j],
                        (*c).win,
                        False,
                        BUTTONMASK as u32,
                        GrabModeSync,
                        GrabModeSync,
                        0,
                        0,
                    );
                }
            }
        }
    }
}
pub fn grabkeys() {
    updatenumlockmask();
    unsafe {
        let modifiers = [0, LockMask, numlockmask, numlockmask | LockMask];

        XUngrabKey(dpy, AnyKey, AnyModifier, root);
        let mut start: i32 = 0;
        let mut end: i32 = 0;
        let mut skip: i32 = 0;
        XDisplayKeycodes(dpy, &mut start, &mut end);
        let syms = XGetKeyboardMapping(dpy, start as u8, end - start + 1, &mut skip);
        if syms.is_null() {
            return;
        }
        for k in start..=end {
            for i in 0..keys.len() {
                // skip modifier codes, we do that ourselves.
                if keys[i].keysym == *syms.wrapping_add(((k - start) * skip) as usize) {
                    for j in 0..modifiers.len() {
                        XGrabKey(
                            dpy,
                            k,
                            keys[i].mod0 | modifiers[j],
                            root,
                            True,
                            GrayScale,
                            GrabModeSync,
                        );
                    }
                }
            }
        }
        XFree(syms as *mut _);
    }
}
pub fn sendevent(c: *mut Client, proto: Atom) -> bool {
    let mut protocols: *mut Atom = null_mut();
    let mut n: i32 = 0;
    let mut exists: bool = false;
    unsafe {
        let mut ev: XEvent = zeroed();
        if XGetWMProtocols(dpy, (*c).win, &mut protocols, &mut n) > 0 {
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
            ev.client_message.window = (*c).win;
            ev.client_message.message_type = wmatom[_WM::WMProtocols as usize];
            ev.client_message.format = 32;
            // This data is cool!
            ev.client_message.data.as_longs_mut()[0] = proto as i64;
            ev.client_message.data.as_longs_mut()[1] = CurrentTime as i64;
            XSendEvent(dpy, (*c).win, False, NoEventMask, &mut ev);
        }
    }
    return exists;
}
pub fn setfocus(c: *mut Client) {
    unsafe {
        if !(*c).nerverfocus {
            XSetInputFocus(dpy, (*c).win, RevertToPointerRoot, CurrentTime);
            XChangeProperty(
                dpy,
                root,
                netatom[_NET::NetActiveWindow as usize],
                XA_WINDOW,
                32,
                PropModeReplace,
                (*c).win as *const _,
                1,
            );
        }
        sendevent(c, wmatom[_WM::WMTakeFocus as usize]);
    }
}
pub fn drawbars() {
    unsafe {
        let mut m = mons;
        while !m.is_null() {
            drawbar(m);
            m = (*m).next;
        }
    }
}
pub fn enternotify(e: *mut XEvent) {
    unsafe {
        let ev = (*e).crossing;
        if (ev.mode != NotifyNormal || ev.detail == NotifyInferior) && ev.window != root {
            return;
        }
        let c = wintoclient(ev.window);
        let m = if !c.is_null() {
            (*c).mon
        } else {
            wintomon(ev.window)
        };
        if m != selmon {
            unfocus((*selmon).sel, true);
            selmon = m;
        } else if c.is_null() || c == (*selmon).sel {
            return;
        }
        focus(c);
    }
}
pub fn expose(e: *mut XEvent) {
    unsafe {
        let ev = (*e).expose;
        let m = wintomon(ev.window);

        if ev.count == 0 && !m.is_null() {
            drawbar(m);
        }
    }
}
pub fn focus(mut c: *mut Client) {
    unsafe {
        if c.is_null() || ISVISIBLE(c) <= 0 {
            c = (*selmon).stack;
            while !c.is_null() && ISVISIBLE(c) <= 0 {
                c = (*c).snext;
            }
        }
        if !(*selmon).sel.is_null() && (*selmon).sel != c {
            unfocus((*selmon).sel, false);
        }
        if !c.is_null() {
            if (*c).mon != selmon {
                selmon = (*c).mon;
            }
            if (*c).isurgent {
                seturgent(c, false);
            }
            detachstack(c);
            attachstack(c);
            grabbuttons(c, true);
            XSetWindowBorder(
                dpy,
                (*c).win,
                (*scheme[_SCHEME::SchemeSel as usize][_Col::ColBorder as usize]).pixel,
            );
            setfocus(c);
        } else {
            XSetInputFocus(dpy, root, RevertToPointerRoot, CurrentTime);
            XDeleteProperty(dpy, root, netatom[_NET::NetActiveWindow as usize]);
        }
        (*selmon).sel = c;
        drawbars();
    }
}
pub fn unfocus(c: *mut Client, setfocus: bool) {
    if c.is_null() {
        return;
    }
    grabbuttons(c, false);
    unsafe {
        XSetWindowBorder(
            dpy,
            (*c).win,
            (*scheme[_SCHEME::SchemeNorm as usize][_Col::ColBorder as usize]).pixel,
        );
        if setfocus {
            XSetInputFocus(dpy, root, RevertToPointerRoot, CurrentTime);
            XDeleteProperty(dpy, root, netatom[_NET::NetActiveWindow as usize]);
        }
    }
}
pub fn sendmon(c: *mut Client, m: *mut Monitor) {
    unsafe {
        if (*c).mon == m {
            return;
        }
        unfocus(c, true);
        detach(c);
        detachstack(c);
        (*c).mon = m;
        // assign tags of target monitor.
        (*c).tags0 = (*m).tagset[(*m).seltags];
        attach(c);
        attachstack(c);
        focus(null_mut());
        arrange(null_mut());
    }
}
pub fn setclientstate(c: *mut Client, state: i64) {
    unsafe {
        XChangeProperty(
            dpy,
            (*c).win,
            wmatom[_WM::WMState as usize],
            wmatom[_WM::WMState as usize],
            32,
            PropModeReplace,
            state as *const _,
            2,
        );
    }
}
pub fn unmanage(c: *mut Client, destroyed: bool) {
    unsafe {
        let m = (*c).mon;
        let mut wc: XWindowChanges = zeroed();

        detach(c);
        detachstack(c);
        if !destroyed {
            wc.border_width = (*c).oldbw;
            // avoid race conditions.
            XGrabServer(dpy);
            XSetErrorHandler(Some(transmute(xerrordummy as *const ())));
            XSelectInput(dpy, (*c).win, NoEventMask);
            // restore border.
            XConfigureWindow(dpy, (*c).win, CWBorderWidth as u32, &mut wc);
            XUngrabButton(dpy, AnyButton as u32, AnyModifier, (*c).win);
            setclientstate(c, WithdrawnState as i64);
            XSync(dpy, False);
            XSetErrorHandler(Some(transmute(xerror as *const ())));
            XUngrabServer(dpy);
        }
        XFree(c as *mut _);
        focus(null_mut());
        updateclientlist();
        arrange(m);
    }
}
pub fn unmapnotify(e: *mut XEvent) {
    unsafe {
        let ev = (*e).unmap;
        let c = wintoclient(ev.window);
        if !c.is_null() {
            if ev.send_event > 0 {
                setclientstate(c, WithdrawnState as i64);
            } else {
                unmanage(c, false);
            }
        }
    }
}

pub fn updategeom() -> bool {
    let mut dirty: bool = false;
    unsafe {
        if mons.is_null() {
            mons = createmon();
        }
        if (*mons).mw != sw || (*mons).mh != sh {
            dirty = true;
            (*mons).mw = sw;
            (*mons).ww = sw;
            (*mons).mh = sh;
            (*mons).wh = sh;
            updatebarpos(mons);
        }
        if dirty {
            // Is it necessary?
            selmon = mons;
            selmon = wintomon(root);
        }
    }
    return dirty;
}
#[allow(unused_assignments)]
pub fn gettextprop(w: Window, atom: Atom, mut text: &str, size: usize) -> bool {
    if text.is_empty() || size == 0 {
        return false;
    }
    unsafe {
        let mut name: XTextProperty = zeroed();
        if XGetTextProperty(dpy, w, &mut name, atom) <= 0 || name.nitems <= 0 {
            return false;
        }
        text = "\0";
        let mut list: *mut *mut c_char = null_mut();
        let mut n: i32 = 0;
        if name.encoding == XA_STRING {
            let c_str = CStr::from_ptr(name.value as *const _);
            // Not same as strncpy!
            text = c_str.to_str().unwrap();
        } else if XmbTextPropertyToTextList(dpy, &mut name, &mut list, &mut n) >= Success as i32 {
            // may be buggy.
            let c_str = CStr::from_ptr(*list);
            text = c_str.to_str().unwrap();
            XFreeStringList(list);
        }
        // No need to end with '\0'
        XFree(name.value as *mut _);
    }
    true
}
pub fn updatestatus() {
    unsafe {
        if !gettextprop(root, XA_WM_NAME, *addr_of!(stext), stext.len()) {
            stext = "jwm-1.0";
        }
        drawbar(selmon);
    }
}
pub fn upodatetitle(c: *mut Client) {
    unsafe {
        if !gettextprop(
            (*c).win,
            netatom[_NET::NetWMName as usize],
            (*c).name,
            (*c).name.len(),
        ) {
            gettextprop((*c).win, XA_WM_NAME, (*c).name, (*c).name.len());
        }
        // hack to mark broken clients
        if (*c).name.chars().nth(0).unwrap() == '\0' {
            (*c).name = broken;
        }
    }
}
