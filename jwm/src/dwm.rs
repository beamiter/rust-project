#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]
// #![allow(unused_mut)]

use libc::{
    close, execvp, exit, fork, free, setsid, sigaction, sigemptyset, waitpid, SA_NOCLDSTOP,
    SA_NOCLDWAIT, SA_RESTART, SIGCHLD, SIG_DFL, SIG_IGN, WNOHANG,
};
use once_cell::sync::Lazy;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, CStr, CString};
use std::mem::transmute;
use std::mem::zeroed;
use std::process::Command;
use std::ptr::{addr_of, addr_of_mut, null, null_mut};
use std::rc::Rc;
use std::{os::raw::c_long, usize};

use x11::keysym::XK_Num_Lock;
use x11::xlib::{
    AnyButton, AnyKey, AnyModifier, Atom, BadAccess, BadDrawable, BadMatch, BadWindow, Below,
    ButtonPress, ButtonPressMask, ButtonRelease, ButtonReleaseMask, CWBackPixmap, CWBorderWidth,
    CWCursor, CWEventMask, CWHeight, CWOverrideRedirect, CWSibling, CWStackMode, CWWidth,
    ClientMessage, ConfigureNotify, ConfigureRequest, ControlMask, CopyFromParent, CurrentTime,
    DestroyAll, DestroyNotify, Display, EnterNotify, EnterWindowMask, Expose, ExposureMask, False,
    FocusChangeMask, FocusIn, GrabModeAsync, GrabModeSync, GrabSuccess, GrayScale, InputHint,
    IsViewable, KeyPress, KeySym, LASTEvent, LeaveWindowMask, LockMask, MapRequest,
    MappingKeyboard, MappingNotify, Mod1Mask, Mod2Mask, Mod3Mask, Mod4Mask, Mod5Mask, MotionNotify,
    NoEventMask, NotifyInferior, NotifyNormal, PAspect, PBaseSize, PMaxSize, PMinSize, PResizeInc,
    PSize, ParentRelative, PointerMotionMask, PointerRoot, PropModeAppend, PropModeReplace,
    PropertyChangeMask, PropertyDelete, PropertyNotify, ReplayPointer, RevertToPointerRoot,
    ShiftMask, StructureNotifyMask, SubstructureNotifyMask, SubstructureRedirectMask, Success,
    Time, True, UnmapNotify, Window, XAllowEvents, XChangeProperty, XChangeWindowAttributes,
    XCheckMaskEvent, XClassHint, XConfigureEvent, XConfigureWindow, XConnectionNumber,
    XCreateSimpleWindow, XCreateWindow, XDefaultDepth, XDefaultRootWindow, XDefaultScreen,
    XDefaultVisual, XDefineCursor, XDeleteProperty, XDestroyWindow, XDisplayHeight,
    XDisplayKeycodes, XDisplayWidth, XErrorEvent, XEvent, XFree, XFreeModifiermap, XFreeStringList,
    XGetClassHint, XGetKeyboardMapping, XGetModifierMapping, XGetTextProperty,
    XGetTransientForHint, XGetWMHints, XGetWMNormalHints, XGetWMProtocols, XGetWindowAttributes,
    XGetWindowProperty, XGrabButton, XGrabKey, XGrabPointer, XGrabServer, XInternAtom,
    XKeycodeToKeysym, XKeysymToKeycode, XKillClient, XMapRaised, XMapWindow, XMaskEvent,
    XMoveResizeWindow, XMoveWindow, XNextEvent, XQueryPointer, XQueryTree, XRaiseWindow,
    XRefreshKeyboardMapping, XRootWindow, XSelectInput, XSendEvent, XSetClassHint,
    XSetCloseDownMode, XSetErrorHandler, XSetInputFocus, XSetWMHints, XSetWindowAttributes,
    XSetWindowBorder, XSizeHints, XSync, XTextProperty, XUngrabButton, XUngrabKey, XUngrabPointer,
    XUngrabServer, XUnmapWindow, XUrgencyHint, XWarpPointer, XWindowAttributes, XWindowChanges,
    XmbTextPropertyToTextList, CWX, CWY, XA_ATOM, XA_STRING, XA_WINDOW, XA_WM_HINTS, XA_WM_NAME,
    XA_WM_NORMAL_HINTS, XA_WM_TRANSIENT_FOR,
};

use std::cmp::{max, min};

use crate::config::{
    borderpx, buttons, colors, dmenucmd, dmenumon, fonts, keys, layouts, lockfullscreen, mfact,
    nmaster, resizehints, rules, showbar, snap, tags, topbar,
};
use crate::drw::{
    drw_create, drw_cur_create, drw_cur_free, drw_fontset_create, drw_fontset_getwidth, drw_free,
    drw_map, drw_rect, drw_resize, drw_scm_create, drw_setscheme, drw_text, Clr, Col, Cur, Drw,
};
use crate::xproto::{
    IconicState, NormalState, WithdrawnState, XC_fleur, XC_left_ptr, XC_sizing, X_ConfigureWindow,
    X_CopyArea, X_GrabButton, X_GrabKey, X_PolyFillRectangle, X_PolySegment, X_PolyText8,
    X_SetInputFocus,
};

pub const BUTTONMASK: c_long = ButtonPressMask | ButtonReleaseMask;
#[inline]
fn CLEANMASK(mask: u32) -> u32 {
    return mask
        & unsafe { !(numlockmask | LockMask) }
        & (ShiftMask | ControlMask | Mod1Mask | Mod2Mask | Mod3Mask | Mod4Mask | Mod5Mask);
}
pub const MOUSEMASK: c_long = BUTTONMASK | PointerMotionMask;

// Variables.
pub const broken: &str = "broken";
pub static mut stext: &str = "";
pub static mut screen: i32 = 0;
pub static mut sw: i32 = 0;
pub static mut sh: i32 = 0;
pub static mut bh: i32 = 0;
pub static mut lrpad: i32 = 0;
pub static mut numlockmask: u32 = 0;
pub static mut wmatom: [Atom; WM::WMLast as usize] = unsafe { zeroed() };
pub static mut netatom: [Atom; NET::NetLast as usize] = unsafe { zeroed() };
pub static mut running: bool = true;
pub static mut cursor: [Option<Box<Cur>>; CUR::CurLast as usize] = [None, None, None];
pub static mut scheme: Vec<Vec<Option<Rc<Clr>>>> = vec![];
pub static mut dpy: *mut Display = null_mut();
pub static mut drw: Option<Box<Drw>> = None;
pub static mut mons: Option<Rc<RefCell<Monitor>>> = None;
pub static mut selmon: Option<Rc<RefCell<Monitor>>> = None;
pub static mut root: Window = 0;
pub static mut wmcheckwin: Window = 0;
pub static mut xerrorxlib: Option<unsafe extern "C" fn(*mut Display, *mut XErrorEvent) -> c_int> =
    None;

pub static mut handler: Lazy<[Option<fn(*mut XEvent)>; LASTEvent as usize]> = Lazy::new(|| {
    let mut res: [Option<fn(*mut XEvent)>; LASTEvent as usize] = [None; LASTEvent as usize];
    res[ButtonPress as usize] = Some(buttonpress);
    res[ClientMessage as usize] = Some(clientmessage);
    res[ConfigureRequest as usize] = Some(configurerequest);
    res[ConfigureNotify as usize] = Some(configurenotify);
    res[DestroyNotify as usize] = Some(destroynotify);
    res[EnterNotify as usize] = Some(enternotify);
    res[Expose as usize] = Some(expose);
    res[FocusIn as usize] = Some(focusin);
    res[KeyPress as usize] = Some(keypress);
    res[MappingNotify as usize] = Some(mappingnotify);
    res[MapRequest as usize] = Some(maprequest);
    res[MotionNotify as usize] = Some(motionnotify);
    res[PropertyNotify as usize] = Some(propertynotify);
    res[UnmapNotify as usize] = Some(unmapnotify);
    return res;
});

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
    NetLast = 9,
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
    V(Vec<&'static str>),
    Lt(Rc<Layout>),
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

#[derive(Debug, Clone, PartialEq)]
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
    pub next: Option<Rc<RefCell<Client>>>,
    pub snext: Option<Rc<RefCell<Client>>>,
    pub mon: Option<Rc<RefCell<Monitor>>>,
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
            next: None,
            snext: None,
            mon: None,
            win: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    pub clients: Option<Rc<RefCell<Client>>>,
    pub sel: Option<Rc<RefCell<Client>>>,
    pub stack: Option<Rc<RefCell<Client>>>,
    pub next: Option<Rc<RefCell<Monitor>>>,
    pub barwin: Window,
    pub lt: [Rc<Layout>; 2],
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
            clients: None,
            sel: None,
            stack: None,
            next: None,
            barwin: 0,
            lt: [
                Rc::new(Layout {
                    symbol: "",
                    arrange: None,
                }),
                Rc::new(Layout {
                    symbol: "",
                    arrange: None,
                }),
            ],
        }
    }
}

#[allow(unused)]
pub fn INTERSECT(x: i32, y: i32, w: i32, h: i32, m: &Monitor) -> i32 {
    unsafe {
        max(0, min(x + w, (*m).wx + (*m).ww) - max(x, (*m).wx))
            * max(0, min(y + h, (*m).wy + (*m).wh) - max(y, (*m).wy))
    }
}

pub fn ISVISIBLE(X: &Rc<RefCell<Client>>) -> u32 {
    let X = X.borrow_mut();
    let tags0 = X.tags0;
    let seltags = X.mon.as_ref().unwrap().borrow_mut().seltags;
    let x = tags0 & X.mon.as_ref().unwrap().borrow_mut().tagset[seltags];
    x
}

pub fn WIDTH(X: &mut Client) -> i32 {
    X.w + 2 * X.bw
}

pub fn HEIGHT(X: &mut Client) -> i32 {
    X.h + 2 * X.bw
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
pub fn applyrules(c: &Rc<RefCell<Client>>) {
    unsafe {
        let mut c = c.borrow_mut();
        c.isfloating = false;
        c.tags0 = 0;
        let mut ch: XClassHint = zeroed();
        XGetClassHint(dpy, c.win, &mut ch);
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
            if (r.title.is_empty() || c.name.find(r.title).is_some())
                && (r.class.is_empty() || class.find(r.class).is_some())
                && (r.instance.is_empty() || instance.find(r.instance).is_some())
            {
                c.isfloating = r.isfloating;
                c.tags0 |= r.tags0 as u32;
                let mut m = mons.clone();
                // or here: let mut m = Some(mons.as_ref().unwrap().clone());
                loop {
                    if m.is_none() || m.as_ref().unwrap().borrow_mut().num == r.monitor {
                        break;
                    }
                    let next = m.as_ref().unwrap().borrow_mut().next.clone();
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
        c.tags0 = if (c.tags0 & TAGMASK()) > 0 {
            c.tags0 & TAGMASK()
        } else {
            let seltags = (*c.mon.as_ref().unwrap().borrow_mut()).seltags;
            (c.mon.as_ref().unwrap().borrow_mut()).tagset[seltags]
        }
    }
}

pub fn updatesizehints(c: &Rc<RefCell<Client>>) {
    let mut c = c.as_ref().borrow_mut();
    unsafe {
        let mut size: XSizeHints = zeroed();

        let mut msize: i64 = 0;
        if XGetWMNormalHints(dpy, c.win, &mut size, &mut msize) <= 0 {
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
    c: &Rc<RefCell<Client>>,
    x: &mut i32,
    y: &mut i32,
    w: &mut i32,
    h: &mut i32,
    interact: bool,
) -> bool {
    unsafe {
        // set minimum possible.
        *w = 1.max(*w);
        *h = 1.max(*h);
        if interact {
            let mut cc = c.as_ref().borrow_mut();
            if *x > sw {
                *x = sw - WIDTH(&mut *cc);
            }
            if *y > sh {
                *y = sh - HEIGHT(&mut *cc);
            }
            if *x + *w + 2 * cc.bw < 0 {
                *x = 0;
            }
            if *y + *h + 2 * cc.bw < 0 {
                *y = 0;
            }
        } else {
            let wx;
            let wy;
            let ww;
            let wh;
            {
                let cc = c.as_ref().borrow_mut();
                wx = cc.mon.as_ref().unwrap().borrow_mut().wx;
                wy = cc.mon.as_ref().unwrap().borrow_mut().wy;
                ww = cc.mon.as_ref().unwrap().borrow_mut().ww;
                wh = cc.mon.as_ref().unwrap().borrow_mut().wh;
            }
            {
                let mut cc = c.as_ref().borrow_mut();
                if *x >= wx + ww {
                    *x = wx + ww - WIDTH(&mut *cc);
                }
                if *y >= wy + wh {
                    *x = wy + wh - HEIGHT(&mut *cc);
                }
                if *x + *w + 2 * cc.bw <= wx {
                    *x = wx;
                }
                if *y + *h + 2 * cc.bw <= wy {
                    *y = wy;
                }
            }
        }
        if *h < bh {
            *h = bh;
        }
        if *w < bh {
            *w = bh;
        }
        let isfloating = { c.as_ref().borrow_mut().isfloating };
        let arrange = {
            let mon = c.as_ref().borrow_mut().mon.clone();
            let sellt = mon.as_ref().unwrap().borrow_mut().sellt;
            let x = mon.as_ref().unwrap().borrow_mut().lt[sellt].arrange;
            x
        };
        if resizehints || isfloating || arrange.is_none() {
            if !c.as_ref().borrow_mut().hintsvalid {
                updatesizehints(c);
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
}
pub fn cleanup() {
    // Bitwise or to get max value.
    let mut a: Arg = Arg::Ui(!0);
    let foo: Layout = Layout::new("", None);
    unsafe {
        view(&mut a);
        {
            let mut selmon_mut = selmon.as_mut().unwrap().borrow_mut();
            let idx = selmon_mut.sellt;
            selmon_mut.lt[idx] = Rc::new(foo);
        }
        let mut m = mons.clone();
        while m.is_some() {
            while m.as_ref().unwrap().borrow_mut().stack.is_some() {
                unmanage(m.as_ref().unwrap().borrow_mut().stack.clone(), false);
            }
            let next = m.as_ref().unwrap().borrow_mut().next.clone();
            m = next;
        }
        XUngrabKey(dpy, AnyKey, AnyModifier, root);
        while mons.is_some() {
            cleanupmon(mons.clone());
        }
        for i in 0..CUR::CurLast as usize {
            drw_cur_free(
                drw.as_mut().unwrap().as_mut(),
                cursor[i].as_mut().unwrap().as_mut(),
            );
        }
        for i in 0..colors.len() {
            free(scheme[i].as_mut_ptr() as *mut _);
        }
        free(scheme.as_mut_ptr() as *mut _);
        XDestroyWindow(dpy, wmcheckwin);
        drw_free(drw.as_mut().unwrap().as_mut());
        XSync(dpy, False);
        XSetInputFocus(dpy, PointerRoot as u64, RevertToPointerRoot, CurrentTime);
        XDeleteProperty(dpy, root, netatom[NET::NetActiveWindow as usize]);
    }
}
pub fn cleanupmon(mon: Option<Rc<RefCell<Monitor>>>) {
    unsafe {
        if Rc::ptr_eq(mon.as_ref().unwrap(), mons.as_ref().unwrap()) {
            let next = mons.as_ref().unwrap().borrow_mut().next.clone();
            mons = next;
        } else {
            let mut m = mons.clone();
            while m.is_some()
                && !Rc::ptr_eq(
                    m.as_ref().unwrap().borrow_mut().next.as_ref().unwrap(),
                    mon.as_ref().unwrap(),
                )
            {
                let next = m.as_ref().unwrap().borrow_mut().next.clone();
                m = next;
            }
            m.as_ref().unwrap().borrow_mut().next = mon.as_ref().unwrap().borrow_mut().next.clone();
        }
        let barwin = mon.as_ref().unwrap().borrow_mut().barwin;
        XUnmapWindow(dpy, barwin);
        XDestroyWindow(dpy, barwin);
    }
}
pub fn clientmessage(e: *mut XEvent) {
    unsafe {
        let cme = (*e).client_message;
        let c = wintoclient(cme.window);

        if c.is_none() {
            return;
        }
        if cme.message_type == netatom[NET::NetWMState as usize] {
            if cme.data.get_long(1) == netatom[NET::NetWMFullscreen as usize] as i64
                || cme.data.get_long(2) == netatom[NET::NetWMFullscreen as usize] as i64
            {
                // NET_WM_STATE_ADD
                // NET_WM_STATE_TOGGLE
                setfullscreen(
                    &mut *c.as_ref().unwrap().borrow_mut(),
                    cme.data.get_long(0) == 1
                        || cme.data.get_long(0) == 2
                            && !c.as_ref().unwrap().borrow_mut().isfullscreen,
                );
            }
        } else if cme.message_type == netatom[NET::NetActiveWindow as usize] {
            if c != selmon.as_ref().unwrap().borrow_mut().sel
                && !c.as_ref().unwrap().borrow_mut().isurgent
            {
                seturgent(&mut *c.as_ref().unwrap().borrow_mut(), true);
            }
        }
    }
}

pub fn configurenotify(e: *mut XEvent) {
    unsafe {
        let ev = (*e).configure;
        if ev.window == root {
            let dirty = sw != ev.width || sh != ev.height;
            sw = ev.width;
            sh = ev.height;
            if updategeom() || dirty {
                drw_resize(drw.as_mut().unwrap().as_mut(), sw as u32, bh as u32);
                updatebars();
                let mut m = mons.clone();
                while m.is_some() {
                    let mut c = m.as_ref().unwrap().borrow_mut().clients.clone();
                    while c.is_some() {
                        if c.as_ref().unwrap().borrow_mut().isfullscreen {
                            resizeclient(
                                &mut *c.as_ref().unwrap().borrow_mut(),
                                m.as_ref().unwrap().borrow_mut().mx,
                                m.as_ref().unwrap().borrow_mut().my,
                                m.as_ref().unwrap().borrow_mut().mw,
                                m.as_ref().unwrap().borrow_mut().mh,
                            );
                        }
                        let next = c.as_ref().unwrap().borrow_mut().next.clone();
                        c = next;
                    }
                    XMoveResizeWindow(
                        dpy,
                        m.as_ref().unwrap().borrow_mut().barwin,
                        m.as_ref().unwrap().borrow_mut().wx,
                        m.as_ref().unwrap().borrow_mut().by,
                        m.as_ref().unwrap().borrow_mut().ww as u32,
                        bh as u32,
                    );
                    let next = m.as_ref().unwrap().borrow_mut().next.clone();
                    m = next;
                }
                focus(None);
                arrange(None);
            }
        }
    }
}

pub fn configure(c: &mut Client) {
    unsafe {
        let mut ce: XConfigureEvent = zeroed();

        ce.type_ = ConfigureNotify;
        ce.display = dpy;
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
        XSendEvent(dpy, c.win, 0, StructureNotifyMask, &mut xe);
    }
}
pub fn setfullscreen(c: &mut Client, fullscreen: bool) {
    unsafe {
        if fullscreen && !c.isfullscreen {
            XChangeProperty(
                dpy,
                c.win,
                netatom[NET::NetWMState as usize],
                XA_ATOM,
                32,
                PropModeReplace,
                netatom[NET::NetWMFullscreen as usize] as *const _,
                1,
            );
            c.isfullscreen = true;
            c.oldstate = c.isfloating;
            c.oldbw = c.bw;
            c.bw = 0;
            c.isfloating = true;
            let mx;
            let my;
            let mw;
            let mh;
            {
                let mon_mut = c.mon.as_ref().unwrap().borrow_mut();
                mx = mon_mut.mx;
                my = mon_mut.my;
                mw = mon_mut.mw;
                mh = mon_mut.mh;
            }
            resizeclient(c, mx, my, mw, mh);
            XRaiseWindow(dpy, c.win);
        } else if !fullscreen && c.isfullscreen {
            XChangeProperty(
                dpy,
                c.win,
                netatom[NET::NetWMState as usize],
                XA_ATOM,
                32,
                PropModeReplace,
                0 as *const _,
                0,
            );
            c.isfullscreen = false;
            c.isfloating = c.oldstate;
            c.bw = c.oldbw;
            c.x = c.oldx;
            c.y = c.oldy;
            c.w = c.oldw;
            c.h = c.oldh;
            resizeclient(c, c.x, c.y, c.w, c.h);
            arrange(c.mon.clone());
        }
    }
}
pub fn resizeclient(c: &mut Client, x: i32, y: i32, w: i32, h: i32) {
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
            dpy,
            c.win,
            (CWX | CWY | CWWidth | CWHeight | CWBorderWidth).into(),
            &mut wc as *mut _,
        );
        configure(c);
        XSync(dpy, 0);
    }
}

pub fn resize(
    c: &Rc<RefCell<Client>>,
    mut x: i32,
    mut y: i32,
    mut w: i32,
    mut h: i32,
    interact: bool,
) {
    if applysizehints(c, &mut x, &mut y, &mut w, &mut h, interact) {
        resizeclient(&mut *c.borrow_mut(), x, y, w, h);
    }
}

pub fn seturgent(c: &mut Client, urg: bool) {
    unsafe {
        c.isurgent = urg;
        let wmh = XGetWMHints(dpy, c.win);
        if wmh.is_null() {
            return;
        }
        (*wmh).flags = if urg {
            (*wmh).flags | XUrgencyHint
        } else {
            (*wmh).flags & !XUrgencyHint
        };
        XSetWMHints(dpy, c.win, wmh);
        XFree(wmh as *mut _);
    }
}

pub fn showhide(c: Option<Rc<RefCell<Client>>>) {
    if c.is_none() {
        return;
    }
    unsafe {
        if ISVISIBLE(c.as_ref().unwrap()) > 0 {
            // show clients top down.
            let win = c.as_ref().unwrap().borrow_mut().win;
            let x = c.as_ref().unwrap().borrow_mut().x;
            let y = c.as_ref().unwrap().borrow_mut().y;
            XMoveWindow(dpy, win, x, y);
            let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
            let sellt = mon.as_ref().unwrap().borrow_mut().sellt;
            let isfloating = c.as_ref().unwrap().borrow_mut().isfloating;
            let isfullscreen = c.as_ref().unwrap().borrow_mut().isfullscreen;
            if (mon.as_ref().unwrap().borrow_mut().lt[sellt]
                .arrange
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
                resize(c.as_ref().unwrap(), x, y, w, h, false);
            }
            let snext = c.as_ref().unwrap().borrow_mut().snext.clone();
            showhide(snext);
        } else {
            // hide clients bottom up.
            let snext = c.as_ref().unwrap().borrow_mut().snext.clone();
            showhide(snext);
            let y;
            let win;
            {
                let cc = c.as_ref().unwrap().borrow_mut();
                y = cc.y;
                win = cc.win;
            }
            XMoveWindow(
                dpy,
                win,
                WIDTH(&mut *c.as_ref().unwrap().borrow_mut()) * -2,
                y,
            );
        }
    }
}
pub fn configurerequest(e: *mut XEvent) {
    unsafe {
        let ev = (*e).configure_request;
        let cc = wintoclient(ev.window);
        if cc.is_some() {
            let mut c = cc.as_ref().unwrap().borrow_mut();
            let selmon_borrow = selmon.as_ref().unwrap().borrow_mut();
            if ev.value_mask & CWBorderWidth as u64 > 0 {
                c.bw = ev.border_width;
            } else if c.isfloating || (*(selmon_borrow.lt[selmon_borrow.sellt])).arrange.is_none() {
                let mx;
                let my;
                let mw;
                let mh;
                {
                    let m = c.mon.as_ref().unwrap().borrow_mut();
                    mx = m.mx;
                    my = m.my;
                    mw = m.mw;
                    mh = m.mh;
                }
                if ev.value_mask & CWX as u64 > 0 {
                    c.oldx = c.x;
                    c.x = mx + ev.x;
                }
                if ev.value_mask & CWY as u64 > 0 {
                    c.oldy = c.y;
                    c.y = my + ev.y;
                }
                if ev.value_mask & CWWidth as u64 > 0 {
                    c.oldw = c.w;
                    c.w = ev.width;
                }
                if ev.value_mask & CWHeight as u64 > 0 {
                    c.oldh = c.h;
                    c.h = ev.height;
                }
                if (c.x + c.w) > mx + mw && c.isfloating {
                    // center in x direction
                    c.x = mx + (mw / 2 - WIDTH(&mut *c) / 2);
                }
                if (c.y + c.h) > my + mh && c.isfloating {
                    // center in y direction
                    c.y = my + (mh / 2 - HEIGHT(&mut *c) / 2);
                }
                if (ev.value_mask & (CWX | CWY) as u64) > 0
                    && (ev.value_mask & (CWWidth | CWHeight) as u64) <= 0
                {
                    configure(&mut *c);
                }
                if ISVISIBLE(cc.as_ref().unwrap()) > 0 {
                    XMoveResizeWindow(dpy, c.win, c.x, c.y, c.w as u32, c.h as u32);
                }
            } else {
                configure(&mut *c);
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
            XConfigureWindow(dpy, ev.window, ev.value_mask as u32, &mut wc);
        }
        XSync(dpy, False);
    }
}
pub fn createmon() -> Monitor {
    let mut m: Monitor = Monitor::new();
    m.tagset[0] = 1;
    m.tagset[1] = 1;
    m.mfact0 = mfact;
    m.nmaster0 = nmaster;
    m.showbar0 = showbar;
    m.topbar0 = topbar;
    m.lt[0] = layouts[0].clone();
    m.lt[1] = layouts[1 % layouts.len()].clone();
    m.ltsymbol = layouts[0].symbol;
    println!("[createmon]: ltsymbol: {:?}", m.ltsymbol);
    return m;
}
pub fn destroynotify(e: *mut XEvent) {
    unsafe {
        let ev = (*e).destroy_window;
        let c = wintoclient(ev.window);
        if c.is_some() {
            unmanage(c, true);
        }
    }
}
pub fn arrangemon(m: &Rc<RefCell<Monitor>>) {
    let sellt;
    {
        let mut mm = m.borrow_mut();
        sellt = (mm).sellt;
        mm.ltsymbol = (*(mm).lt[sellt]).symbol;
        println!("arrangemon {}, {:?}", sellt, mm.ltsymbol);
    }
    let arrange;
    {
        arrange = m.borrow_mut().lt[sellt].arrange;
    }
    if let Some(arrange0) = arrange {
        let m_ptr: *mut Monitor;
        {
            m_ptr = &mut *m.borrow_mut();
        }
        (arrange0)(m_ptr);
    }
}
// This is cool!
pub fn detach(c: Option<Rc<RefCell<Client>>>) {
    let cc = c.as_ref().unwrap();
    let mut current_opt = cc
        .borrow_mut()
        .mon
        .as_ref()
        .unwrap()
        .borrow_mut()
        .clients
        .as_ref()
        .map(Rc::clone);
    let mut prev_opt: Option<Rc<RefCell<Client>>> = None;
    while let Some(current) = current_opt {
        if Rc::ptr_eq(&current, cc) {
            let next_opt = { current.borrow_mut().next.clone() };
            if let Some(prev) = prev_opt {
                prev.borrow_mut().next = next_opt;
            } else {
                cc.borrow_mut().mon.as_ref().unwrap().borrow_mut().clients = next_opt;
            }
            break;
        }
        prev_opt = Some(Rc::clone(&current));
        current_opt = current.borrow().next.as_ref().map(Rc::clone);
    }
}
pub fn detachstack(c: Option<Rc<RefCell<Client>>>) {
    let cc = c.as_ref().unwrap();
    let mut current_opt = cc
        .borrow_mut()
        .mon
        .as_ref()
        .unwrap()
        .borrow_mut()
        .stack
        .as_ref()
        .map(Rc::clone);
    let mut prev_opt: Option<Rc<RefCell<Client>>> = None;
    while let Some(current) = current_opt {
        if Rc::ptr_eq(&current, cc) {
            let next_opt = { current.borrow_mut().snext.clone() };
            if let Some(prev) = prev_opt {
                prev.borrow_mut().snext = next_opt;
            } else {
                cc.borrow_mut().mon.as_ref().unwrap().borrow_mut().stack = next_opt;
            }
            break;
        }
        prev_opt = Some(Rc::clone(&current));
        current_opt = current.borrow().snext.as_ref().map(Rc::clone);
    }
    if Rc::ptr_eq(
        cc,
        cc.borrow_mut()
            .mon
            .as_ref()
            .unwrap()
            .borrow_mut()
            .sel
            .as_ref()
            .unwrap(),
    ) {
        let mut t = cc
            .borrow_mut()
            .mon
            .as_ref()
            .unwrap()
            .borrow_mut()
            .stack
            .as_ref()
            .map(Rc::clone);
        while t.is_some() && ISVISIBLE(t.as_ref().unwrap()) <= 0 {
            let snext = t.as_ref().unwrap().borrow_mut().snext.clone();
            t = snext;
        }
        cc.borrow_mut().mon.as_ref().unwrap().borrow_mut().sel = t.clone();
    }
}
pub fn dirtomon(dir: i32) -> Option<Rc<RefCell<Monitor>>> {
    unsafe {
        let mut m: Option<Rc<RefCell<Monitor>>>;
        if dir > 0 {
            m = selmon.as_ref().unwrap().borrow_mut().next.clone();
            if m.is_some() {
                m = mons.clone();
            }
        } else if selmon == mons {
            m = mons.clone();
            while m.is_some() && m.as_ref().unwrap().borrow_mut().next.is_some() {
                let next = m.as_ref().unwrap().borrow_mut().next.clone();
                m = next;
            }
        } else {
            m = mons.clone();
            while m.is_some() && m.as_ref().unwrap().borrow_mut().next != selmon {
                let next = m.as_ref().unwrap().borrow_mut().next.clone();
                m = next;
            }
        }
        m
    }
}
pub fn drawbar(m: Option<Rc<RefCell<Monitor>>>) {
    let mut tw: i32 = 0;
    let mut occ: u32 = 0;
    let mut urg: u32 = 0;
    unsafe {
        let boxs;
        let boxw;
        {
            let h = drw.as_ref().unwrap().fonts.as_ref().unwrap().borrow_mut().h;
            boxs = h / 9;
            boxw = h / 6 + 2;
        }

        if !m.as_ref().unwrap().borrow_mut().showbar0 {
            return;
        }

        // draw status first so it can be overdrawn by tags later.
        if Rc::ptr_eq(m.as_ref().unwrap(), selmon.as_ref().unwrap()) {
            // status is only drawn on selected monitor.
            drw_setscheme(
                drw.as_mut().unwrap().as_mut(),
                scheme[SCHEME::SchemeNorm as usize].clone(),
            );
            // 2px right padding.
            tw = TEXTW(drw.as_mut().unwrap().as_mut(), stext) as i32 - lrpad + 2;
            let ww = m.as_ref().unwrap().borrow_mut().ww;
            drw_text(
                drw.as_mut().unwrap().as_mut(),
                ww - tw,
                0,
                tw as u32,
                bh as u32,
                0,
                stext,
                0,
            );
            println!("here0: {},{},{},{},{}", ww - tw, 0, tw, bh, stext);
        }
        {
            let mut c = m.as_ref().unwrap().borrow_mut().clients.clone();
            while c.is_some() {
                let tags0 = c.as_ref().unwrap().borrow_mut().tags0;
                occ |= tags0;
                if c.as_ref().unwrap().borrow_mut().isurgent {
                    urg |= tags0;
                }
                let next = c.as_ref().unwrap().borrow_mut().next.clone();
                c = next;
            }
        }
        let mut x = 0;
        let mut w;
        for i in 0..tags.len() {
            w = TEXTW(drw.as_mut().unwrap().as_mut(), tags[i]) as i32;
            let st = m.as_ref().unwrap().borrow_mut().seltags;
            let idx = if m.as_ref().unwrap().borrow_mut().tagset[st] & 1 << i > 0 {
                SCHEME::SchemeSel as usize
            } else {
                SCHEME::SchemeNorm as usize
            };
            drw_setscheme(drw.as_mut().unwrap().as_mut(), scheme[idx].clone());
            drw_text(
                drw.as_mut().unwrap().as_mut(),
                x,
                0,
                w as u32,
                bh as u32,
                (lrpad / 2) as u32,
                tags[i],
                (urg & 1 << i) as i32,
            );
            println!("here1: {},{},{},{},{}", x, 0, w, bh, tags[i]);
            if (occ & 1 << i) > 0 {
                let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
                let tags0 = { selmon_mut.sel.as_ref().unwrap().borrow_mut().tags0 };
                drw_rect(
                    drw.as_mut().unwrap().as_mut(),
                    x + boxs as i32,
                    boxs as i32,
                    boxw,
                    boxw,
                    (Rc::ptr_eq(m.as_ref().unwrap(), selmon.as_ref().unwrap())
                        && selmon_mut.sel.is_some()
                        && (tags0 & 1 << i > 0)) as i32,
                    (urg & 1 << i) as i32,
                );
            }
            x += w;
        }
        w = TEXTW(
            drw.as_mut().unwrap().as_mut(),
            m.as_ref().unwrap().borrow_mut().ltsymbol,
        ) as i32;
        drw_setscheme(
            drw.as_mut().unwrap().as_mut(),
            scheme[SCHEME::SchemeNorm as usize].clone(),
        );
        x = drw_text(
            drw.as_mut().unwrap().as_mut(),
            x,
            0,
            w as u32,
            bh as u32,
            (lrpad / 2) as u32,
            m.as_ref().unwrap().borrow_mut().ltsymbol,
            0,
        );
        println!(
            "here2: {},{},{},{},{}",
            x,
            0,
            w,
            bh,
            m.as_ref().unwrap().borrow_mut().ltsymbol
        );

        w = m.as_ref().unwrap().borrow_mut().ww - tw - x;
        if w > bh {
            if let Some(ref sel_opt) = m.as_ref().unwrap().borrow_mut().sel {
                let idx = if Rc::ptr_eq(m.as_ref().unwrap(), selmon.as_ref().unwrap()) {
                    SCHEME::SchemeSel
                } else {
                    SCHEME::SchemeNorm
                } as usize;
                drw_setscheme(drw.as_mut().unwrap().as_mut(), scheme[idx].clone());
                drw_text(
                    drw.as_mut().unwrap().as_mut(),
                    x,
                    0,
                    w as u32,
                    bh as u32,
                    (lrpad / 2) as u32,
                    sel_opt.borrow_mut().name,
                    0,
                );
                println!(
                    "here3: {},{},{},{},{}",
                    x,
                    0,
                    w,
                    bh,
                    m.as_ref().unwrap().borrow_mut().ltsymbol
                );
                if sel_opt.borrow_mut().isfloating {
                    drw_rect(
                        drw.as_mut().unwrap().as_mut(),
                        x + boxs as i32,
                        boxs as i32,
                        boxw,
                        boxw,
                        sel_opt.borrow_mut().isfixed as i32,
                        0,
                    );
                }
            } else {
                drw_setscheme(
                    drw.as_mut().unwrap().as_mut(),
                    scheme[SCHEME::SchemeNorm as usize].clone(),
                );
                drw_rect(
                    drw.as_mut().unwrap().as_mut(),
                    x,
                    0,
                    w.try_into().unwrap(),
                    bh.try_into().unwrap(),
                    1,
                    1,
                );
            }
        }
        let barwin = m.as_ref().unwrap().borrow_mut().barwin;
        let ww: u32 = m.as_ref().unwrap().borrow_mut().ww as u32;
        drw_map(drw.as_mut().unwrap().as_mut(), barwin, 0, 0, ww, bh as u32);
    }
}

pub fn restack(m: Option<Rc<RefCell<Monitor>>>) {
    println!("restack");
    drawbar(m.clone());

    unsafe {
        let mut wc: XWindowChanges = zeroed();
        if m.as_ref().unwrap().borrow_mut().sel.is_none() {
            return;
        }
        let isfloating = m
            .as_ref()
            .unwrap()
            .borrow_mut()
            .sel
            .as_ref()
            .unwrap()
            .borrow_mut()
            .isfloating;
        let sellt = m.as_ref().unwrap().borrow_mut().sellt;
        if isfloating || m.as_ref().unwrap().borrow_mut().lt[sellt].arrange.is_none() {
            let win = m
                .as_ref()
                .unwrap()
                .borrow_mut()
                .sel
                .as_ref()
                .unwrap()
                .borrow_mut()
                .win;
            XRaiseWindow(dpy, win);
        }
        if m.as_ref().unwrap().borrow_mut().lt[sellt].arrange.is_some() {
            wc.stack_mode = Below;
            wc.sibling = m.as_ref().unwrap().borrow_mut().barwin;
            let mut c = m.as_ref().unwrap().borrow_mut().stack.clone();
            while c.is_none() {
                if !c.as_ref().unwrap().borrow_mut().isfloating
                    && ISVISIBLE(c.as_ref().unwrap()) > 0
                {
                    let win = c.as_ref().unwrap().borrow_mut().win;
                    XConfigureWindow(dpy, win, (CWSibling | CWStackMode) as u32, &mut wc);
                    wc.sibling = win;
                }
                let next = c.as_ref().unwrap().borrow_mut().snext.clone();
                c = next;
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

pub fn run() {
    // main event loop
    unsafe {
        let mut ev: XEvent = zeroed();
        XSync(dpy, False);
        let mut i: u64 = 0;
        while running && XNextEvent(dpy, &mut ev) <= 0 {
            println!("running frame: {}, handler type: {}", i, ev.type_);
            i = (i + 1) % std::u64::MAX;
            if let Some(hd) = handler[ev.type_ as usize] {
                // call handler
                println!("*********** handler type: {} valid", ev.type_);
                hd(&mut ev);
            }
        }
    }
}

pub fn scan() {
    let mut num: u32 = 0;
    let mut d1: Window = 0;
    let mut d2: Window = 0;
    let mut wins: *mut Window = null_mut();
    unsafe {
        let mut wa: XWindowAttributes = zeroed();
        if XQueryTree(dpy, root, &mut d1, &mut d2, &mut wins, &mut num) > 0 {
            for i in 0..num as usize {
                if XGetWindowAttributes(dpy, *wins.wrapping_add(i), &mut wa) <= 0
                    || wa.override_redirect > 0
                    || XGetTransientForHint(dpy, *wins.wrapping_add(i), &mut d1) > 0
                {
                    continue;
                }
                if wa.map_state == IsViewable
                    || getstate(*wins.wrapping_add(i)) == IconicState as i64
                {
                    manage(*wins.wrapping_add(i), &mut wa);
                }
            }
            for i in 0..num as usize {
                // now the transients
                if XGetWindowAttributes(dpy, *wins.wrapping_add(i), &mut wa) <= 0 {
                    continue;
                }
                if XGetTransientForHint(dpy, *wins.wrapping_add(i), &mut d1) > 0
                    && (wa.map_state == IsViewable
                        || getstate(*wins.wrapping_add(i)) == IconicState as i64)
                {
                    manage(*wins.wrapping_add(i), &mut wa);
                }
            }
        }
        if !wins.is_null() {
            XFree(wins as *mut _);
        }
    }
}

pub fn arrange(mut m: Option<Rc<RefCell<Monitor>>>) {
    unsafe {
        if m.is_some() {
            {
                let stack = m.as_ref().unwrap().borrow_mut().stack.clone();
                showhide(stack);
            }
        } else {
            m = mons.clone();
            while m.is_some() {
                let stack = m.as_ref().unwrap().borrow_mut().stack.clone();
                showhide(stack);
                let next = m.as_ref().unwrap().borrow_mut().next.clone();
                m = next;
            }
        }
        if m.is_some() {
            arrangemon(m.as_ref().unwrap());
            restack(m);
        } else {
            m = mons.clone();
            while m.is_some() {
                arrangemon(m.as_ref().unwrap());
                let next = m.as_ref().unwrap().borrow_mut().next.clone();
                m = next;
            }
        }
    }
}

pub fn attach(c: Option<Rc<RefCell<Client>>>) {
    let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
    c.as_ref().unwrap().borrow_mut().next = mon.as_ref().unwrap().borrow_mut().clients.clone();
    mon.as_ref().unwrap().borrow_mut().clients = c.clone();
}
pub fn attachstack(c: Option<Rc<RefCell<Client>>>) {
    let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
    c.as_ref().unwrap().borrow_mut().snext = mon.as_ref().unwrap().borrow_mut().stack.clone();
    mon.as_ref().unwrap().borrow_mut().stack = c.clone();
}

pub fn getatomprop(c: &mut Client, prop: Atom) -> u64 {
    let mut di = 0;
    let mut dl: u64 = 0;
    let mut da: Atom = 0;
    let mut atom: Atom = 0;
    let mut p: *mut u8 = null_mut();
    unsafe {
        if XGetWindowProperty(
            dpy,
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
            wmatom[WM::WMState as usize],
            0,
            2,
            False,
            wmatom[WM::WMState as usize],
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

pub fn recttomon(x: i32, y: i32, w: i32, h: i32) -> Option<Rc<RefCell<Monitor>>> {
    let mut area: i32 = 0;

    unsafe {
        let mut r = selmon.clone();
        let mut m = mons.clone();
        while let Some(ref m_opt) = m {
            let a = INTERSECT(x, y, w, h, &m_opt.borrow_mut());
            if a > area {
                area = a;
                r = m.clone();
            }
            let next = m_opt.borrow_mut().next.clone();
            m = next;
        }
        return r;
    }
}

pub fn wintoclient(w: Window) -> Option<Rc<RefCell<Client>>> {
    unsafe {
        let mut m = mons.clone();
        while let Some(ref m_opt) = m {
            let mut c = m_opt.borrow_mut().clients.clone();
            while let Some(ref c_opt) = c {
                if c_opt.borrow_mut().win == w {
                    return c;
                }
                let next = c_opt.borrow_mut().next.clone();
                c = next;
            }
            let next = m_opt.borrow_mut().next.clone();
            m = next;
        }
    }
    None
}

pub fn wintomon(w: Window) -> Option<Rc<RefCell<Monitor>>> {
    let mut x: i32 = 0;
    let mut y: i32 = 0;
    unsafe {
        if w == root && getrootptr(&mut x, &mut y) > 0 {
            return recttomon(x, y, 1, 1);
        }
        let mut m = mons.clone();
        while let Some(ref m_opt) = m {
            if w == m_opt.borrow_mut().barwin {
                return m;
            }
            let next = m_opt.borrow_mut().next.clone();
            m = next;
        }
        let c = wintoclient(w);
        if let Some(ref c_opt) = c {
            return c_opt.borrow_mut().mon.clone();
        }
        return selmon.clone();
    }
}

pub fn buttonpress(e: *mut XEvent) {
    let mut i: usize = 0;
    let mut x: u32 = 0;
    let mut arg: Arg = Arg::Ui(0);
    unsafe {
        let c: Option<Rc<RefCell<Client>>>;
        let ev = (*e).button;
        let mut click = CLICK::ClkRootWin;
        // focus monitor if necessary.
        let m = wintomon(ev.window);
        if m != selmon {
            unfocus(selmon.as_ref().unwrap().borrow_mut().sel.clone(), true);
            selmon = m;
            focus(None);
        }
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if ev.window == selmon_mut.barwin {
            loop {
                x += TEXTW(drw.as_mut().unwrap().as_mut(), tags[i]);
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
                click = CLICK::ClkTagBar;
                arg = Arg::Ui(1 << i);
            } else if ev.x < (x + TEXTW(drw.as_mut().unwrap().as_mut(), selmon_mut.ltsymbol)) as i32
            {
                click = CLICK::ClkLtSymbol;
            } else if ev.x > selmon_mut.ww - TEXTW(drw.as_mut().unwrap().as_mut(), stext) as i32 {
                click = CLICK::ClkStatusText;
            } else {
                click = CLICK::ClkWinTitle;
            }
        } else if {
            c = wintoclient(ev.window);
            c.is_some()
        } {
            focus(c);
            restack(selmon.clone());
            XAllowEvents(dpy, ReplayPointer, CurrentTime);
            click = CLICK::ClkClientWin;
        }
        for i in 0..buttons.len() {
            if click as u32 == buttons[i].click
                && buttons[i].func.is_some()
                && buttons[i].button == ev.button
                && CLEANMASK(buttons[i].mask) == CLEANMASK(ev.state)
            {
                buttons[i].func.unwrap()({
                    if click as u32 == CLICK::ClkTagBar as u32 && {
                        if let Arg::Ui(0) = arg {
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
#[allow(dead_code)]
pub fn xerrorstart(_: *mut Display, _: *mut XErrorEvent) -> i32 {
    eprintln!("jwm: another window manager is already running");
    unsafe {
        exit(1);
    }
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
#[allow(dead_code)]
pub fn checkotherwm() {
    unsafe {
        xerrorxlib = XSetErrorHandler(Some(transmute(xerrorstart as *const ())));
        // this causes an error if some other window manager is running.
        XSelectInput(dpy, XDefaultRootWindow(dpy), SubstructureRedirectMask);
        XSync(dpy, False);
        // Attention what transmut does is great;
        XSetErrorHandler(Some(transmute(xerror as *const ())));
        XSync(dpy, False);
    }
}

pub fn spawn(arg: *const Arg) {
    unsafe {
        let mut sa: sigaction = zeroed();
        static mut tmp: String = String::new();

        println!("spawn");
        if let Arg::V(ref v) = *arg {
            if *v == *dmenucmd {
                // Comment for test
                // tmp =
                //     ((b'0' + selmon.as_ref().unwrap().borrow_mut().num as u8) as char).to_string();
                // dmenumon = tmp.as_str();
            }
            if fork() == 0 {
                if !dpy.is_null() {
                    close(XConnectionNumber(dpy));
                }
                setsid();

                sigemptyset(&mut sa.sa_mask);
                sa.sa_flags = 0;
                sa.sa_sigaction = SIG_DFL;
                sigaction(SIGCHLD, &sa, null_mut());

                println!("arg v: {:?}", v);
                let status = Command::new(v[0])
                    .args(&v[1..])
                    .status()
                    .expect("Failed to execute command");
                if !status.success() {
                    println!("Command exited with non-zero status code");
                }
                // Deprecated.
                // let c_args: Vec<CString> = v
                //     .iter()
                //     .map(|&arg| CString::new(arg).expect("fail to create"))
                //     .collect();
                // let arg_ptrs: Vec<*const i8> = c_args
                //     .iter()
                //     .map(|arg| arg.as_ptr())
                //     .chain(Some(null()))
                //     .collect();
                // execvp(arg_ptrs[0], arg_ptrs[1..].as_ptr());
            }
        }
    }
}
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
        let mut m = mons.clone();
        while m.is_some() {
            if m.as_ref().unwrap().borrow_mut().barwin > 0 {
                continue;
            }
            let wx = m.as_ref().unwrap().borrow_mut().wx;
            let by = m.as_ref().unwrap().borrow_mut().by;
            let ww = m.as_ref().unwrap().borrow_mut().ww as u32;
            m.as_ref().unwrap().borrow_mut().barwin = XCreateWindow(
                dpy,
                root,
                wx,
                by,
                ww,
                bh as u32,
                0,
                XDefaultDepth(dpy, screen),
                CopyFromParent as u32,
                XDefaultVisual(dpy, screen),
                CWOverrideRedirect | CWBackPixmap | CWEventMask,
                &mut wa,
            );
            let barwin = m.as_ref().unwrap().borrow_mut().barwin;
            XDefineCursor(
                dpy,
                barwin,
                cursor[CUR::CurNormal as usize].as_ref().unwrap().cursor,
            );
            XMapRaised(dpy, barwin);
            XSetClassHint(dpy, barwin, &mut ch);
            let next = m.as_ref().unwrap().borrow_mut().next.clone();
            m = next;
        }
    }
}
pub fn updatebarpos(m: &mut Monitor) {
    unsafe {
        m.wy = m.my;
        m.wh = m.mh;
        if m.showbar0 {
            m.wh -= bh;
            m.by = if m.topbar0 { m.wy } else { m.wy + m.wh };
            m.wy = if m.topbar0 { m.wy + bh } else { m.wy };
        } else {
            m.by = -bh;
        }
    }
}
pub fn updateclientlist() {
    unsafe {
        XDeleteProperty(dpy, root, netatom[NET::NetClientList as usize]);
        let mut m = mons.clone();
        while m.is_some() {
            let mut c = m.as_ref().unwrap().borrow_mut().clients.clone();
            while c.is_some() {
                XChangeProperty(
                    dpy,
                    root,
                    netatom[NET::NetClientList as usize],
                    XA_WINDOW,
                    32,
                    PropModeAppend,
                    c.as_ref().unwrap().borrow_mut().win as *const _,
                    1,
                );
                let next = c.as_ref().unwrap().borrow_mut().next.clone();
                c = next;
            }
            let next = m.as_ref().unwrap().borrow_mut().next.clone();
            m = next;
        }
    }
}
pub fn tile(m: *mut Monitor) {
    let mut n: u32 = 0;
    unsafe {
        let mut c = nexttiled((*m).clients.clone());
        while c.is_some() {
            println!("{} tile", line!());
            let next = nexttiled(c.as_ref().unwrap().borrow_mut().next.clone());
            c = next;
            n += 1;
        }
        if n == 0 {
            return;
        }

        let mw: u32;
        if n > (*m).nmaster0 as u32 {
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
        c = nexttiled((*m).clients.clone());
        while c.is_some() {
            println!("{} tile", line!());
            if i < (*m).nmaster0 as u32 {
                h = ((*m).wh as u32 - my) / (n.min((*m).nmaster0 as u32) - i);
                let bw = c.as_ref().unwrap().borrow_mut().bw;
                resize(
                    c.as_ref().unwrap(),
                    (*m).wx,
                    (*m).wy + my as i32,
                    mw as i32 - (2 * bw),
                    h as i32 - (2 * bw),
                    false,
                );
                let height = HEIGHT(&mut *c.as_ref().unwrap().borrow_mut()) as u32;
                if my + height < (*m).wh as u32 {
                    my += height;
                }
            } else {
                h = ((*m).wh as u32 - ty) / (n - i);
                let bw = c.as_ref().unwrap().borrow_mut().bw;
                resize(
                    c.as_ref().unwrap(),
                    (*m).wx + mw as i32,
                    (*m).wy + ty as i32,
                    (*m).ww - mw as i32 - (2 * bw),
                    h as i32 - (2 * bw),
                    false,
                );
                let height = HEIGHT(&mut *c.as_ref().unwrap().borrow_mut());
                if ty as i32 + height < (*m).wh {
                    ty += height as u32;
                }
            }

            let next = nexttiled(c.as_ref().unwrap().borrow_mut().next.clone());
            c = next;
            i += 1;
        }
    }
}
pub fn togglebar(_arg: *const Arg) {
    unsafe {
        let mut selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        selmon_mut.showbar0 = !selmon_mut.showbar0;
        updatebarpos(&mut *selmon_mut);
        XMoveResizeWindow(
            dpy,
            selmon_mut.barwin,
            selmon_mut.wx,
            selmon_mut.by,
            selmon_mut.ww as u32,
            bh as u32,
        );
        arrange(selmon.clone());
    }
}
pub fn togglefloating(_arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if selmon_mut.sel.is_none() {
            return;
        }
        // no support for fullscreen windows.
        if selmon_mut.sel.as_ref().unwrap().borrow_mut().isfullscreen {
            return;
        }
        (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).isfloating =
            !(*selmon_mut.sel.as_ref().unwrap().borrow_mut()).isfloating
                || (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).isfixed;
        if (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).isfloating {
            resize(
                selmon_mut.sel.as_ref().unwrap(),
                (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).x,
                (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).y,
                (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).w,
                (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).h,
                false,
            );
        }
        arrange(selmon.clone());
    }
}
pub fn focusin(e: *mut XEvent) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        let ev = (*e).focus_change;
        if selmon_mut.sel.is_some()
            && ev.window != (*selmon_mut.sel.as_ref().unwrap().borrow_mut()).win
        {
            setfocus(selmon_mut.sel.as_ref().unwrap());
        }
    }
}
pub fn focusmon(arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if mons.as_ref().unwrap().borrow_mut().next.is_none() {
            return;
        }
        if let Arg::I(i) = *arg {
            let m = dirtomon(i);
            if m == selmon {
                return;
            }
            unfocus(selmon_mut.sel.clone(), false);
            selmon = m;
            focus(None);
        }
    }
}
pub fn tag(arg: *const Arg) {
    unsafe {
        if let Arg::Ui(ui) = *arg {
            let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
            if selmon_mut.sel.is_some() && (ui & TAGMASK()) > 0 {
                selmon_mut.sel.as_ref().unwrap().borrow_mut().tags0 = ui & TAGMASK();
                focus(None);
                arrange(selmon.clone());
            }
        }
    }
}
pub fn tagmon(arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if selmon_mut.sel.is_none() || (mons.as_ref().unwrap().borrow_mut()).next.is_none() {
            return;
        }
        if let Arg::I(i) = *arg {
            sendmon(selmon_mut.sel.clone(), &dirtomon(i));
        }
    }
}
pub fn focusstack(arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if selmon_mut.sel.is_none()
            || (selmon_mut.sel.as_ref().unwrap().borrow_mut().isfullscreen && lockfullscreen)
        {
            return;
        }
        let mut c: Option<Rc<RefCell<Client>>> = None;
        let i = if let Arg::I(i) = *arg { i } else { -1 };
        if i > 0 {
            c = (*selmon_mut.sel.as_ref().unwrap().borrow_mut())
                .next
                .clone();
            while c.is_some() && ISVISIBLE(c.as_ref().unwrap()) <= 0 {
                let next = c.as_ref().unwrap().borrow_mut().next.clone();
                c = next;
            }
            if c.is_none() {
                c = selmon_mut.clients.clone();
                while c.is_some() && ISVISIBLE(c.as_ref().unwrap()) <= 0 {
                    let next = c.as_ref().unwrap().borrow_mut().next.clone();
                    c = next;
                }
            }
        } else {
            let mut cl = selmon_mut.clients.clone();
            while !Rc::ptr_eq(cl.as_ref().unwrap(), selmon_mut.sel.as_ref().unwrap()) {
                let next = cl.as_ref().unwrap().borrow_mut().next.clone();
                cl = next;
                if ISVISIBLE(cl.as_ref().unwrap()) > 0 {
                    c = cl.clone();
                }
            }
            if c.is_none() {
                while cl.is_some() {
                    if ISVISIBLE(cl.as_ref().unwrap()) > 0 {
                        c = cl.clone();
                    }
                    let next = cl.as_ref().unwrap().borrow_mut().next.clone();
                    cl = next;
                }
            }
        }
        if c.is_some() {
            focus(c);
            restack(selmon.clone());
        }
    }
}
pub fn incnmaster(arg: *const Arg) {
    unsafe {
        if let Arg::I(i) = *arg {
            let mut selmon_mut = selmon.as_ref().unwrap().borrow_mut();
            selmon_mut.nmaster0 = 0.max(selmon_mut.nmaster0 + i);
        }
    }
}
pub fn setmfact(arg: *const Arg) {
    unsafe {
        let mut selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if arg.is_null() || (*selmon_mut.lt[selmon_mut.sellt]).arrange.is_none() {
            return;
        }
        if let Arg::F(f) = *arg {
            let f = if f < 1.0 {
                f + selmon_mut.mfact0
            } else {
                f - 1.0
            };
            if f < 0.05 || f > 0.95 {
                return;
            }
            selmon_mut.mfact0 = f;
        }
        arrange(selmon.clone());
    }
}
pub fn setlayout(arg: *const Arg) {
    unsafe {
        let mut selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if arg.is_null()
            || if let Arg::Lt(ref lt) = *arg {
                !Rc::ptr_eq(lt, &selmon_mut.lt[selmon_mut.sellt])
            } else {
                true
            }
        {
            selmon_mut.sellt ^= 1;
        }
        if !arg.is_null() {
            if let Arg::Lt(ref lt) = *arg {
                let index = selmon_mut.sellt;
                selmon_mut.lt[index] = lt.clone();
            }
        }
        selmon_mut.ltsymbol = (*selmon_mut.lt[selmon_mut.sellt]).symbol;
        if selmon_mut.sel.is_some() {
            arrange(selmon.clone());
        } else {
            println!("setlayout");
            drawbar(selmon.clone());
        }
    }
}
pub fn zoom(_arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        let mut c = selmon_mut.sel.clone();

        let sellt = selmon_mut.sellt;
        if (*selmon_mut.lt[sellt]).arrange.is_none()
            || c.is_none()
            || c.as_ref().unwrap().borrow_mut().isfloating
        {
            return;
        }
        if c == nexttiled(selmon_mut.clients.clone()) {
            let next = nexttiled(c.as_ref().unwrap().borrow_mut().next.clone());
            c = next;
            if c.is_none() {
                return;
            }
        }
        pop(c);
    }
}
pub fn view(arg: *const Arg) {
    unsafe {
        let ui = if let Arg::Ui(ui) = *arg { ui } else { 0 };
        {
            let mut selmon_mut = selmon.as_ref().unwrap().borrow_mut();
            if (ui & TAGMASK()) == selmon_mut.tagset[selmon_mut.seltags] {
                return;
            }
            // toggle sel tagset.
            selmon_mut.seltags ^= 1;
            if ui & TAGMASK() > 0 {
                let index = selmon_mut.seltags;
                selmon_mut.tagset[index] = ui & TAGMASK();
            }
        }
        focus(None);
        arrange(selmon.clone());
    }
}
pub fn toggleview(arg: *const Arg) {
    unsafe {
        if let Arg::Ui(ui) = *arg {
            let mut selmon_mut = selmon.as_ref().unwrap().borrow_mut();
            let newtagset = selmon_mut.tagset[selmon_mut.seltags] ^ (ui & TAGMASK());
            if newtagset > 0 {
                let index = selmon_mut.seltags;
                selmon_mut.tagset[index] = newtagset;
                focus(None);
                arrange(selmon.clone());
            }
        }
    }
}
pub fn toggletag(arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if selmon_mut.sel.is_none() {
            return;
        }
        if let Arg::Ui(ui) = *arg {
            let newtags = selmon_mut.sel.as_ref().unwrap().borrow_mut().tags0 ^ (ui & TAGMASK());
            if newtags > 0 {
                selmon_mut.sel.as_ref().unwrap().borrow_mut().tags0 = newtags;
                focus(None);
                arrange(selmon.clone());
            }
        }
    }
}
pub fn quit(_arg: *const Arg) {
    unsafe {
        running = false;
    }
}
pub fn setup() {
    unsafe {
        let mut wa: XSetWindowAttributes = zeroed();
        let mut sa: sigaction = zeroed();
        // do not transform children into zombies whien they terminate
        sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = SA_NOCLDSTOP | SA_NOCLDWAIT | SA_RESTART;
        sa.sa_sigaction = SIG_IGN;
        sigaction(SIGCHLD, &sa, null_mut());

        // clean up any zombies (inherited from .xinitrc etc) immediately
        while waitpid(-1, null_mut(), WNOHANG) > 0 {}

        // init screen
        screen = XDefaultScreen(dpy);
        sw = XDisplayWidth(dpy, screen);
        sh = XDisplayHeight(dpy, screen);
        root = XRootWindow(dpy, screen);
        drw = Some(Box::new(drw_create(
            dpy, screen, root, sw as u32, sh as u32,
        )));
        println!("[setup] drw_fontset_create");
        if drw_fontset_create(drw.as_mut().unwrap().as_mut(), &*fonts, fonts.len() as u64).is_none()
        {
            eprintln!("no fonts could be loaded");
            exit(0);
        }
        {
            let h = drw.as_ref().unwrap().fonts.as_ref().unwrap().borrow_mut().h as i32;
            lrpad = h;
            bh = h + 2;
        }
        println!("[setup] updategeom");
        updategeom();
        // init atoms
        let mut c_string = CString::new("UTF8_STRING").expect("fail to convert");
        let utf8string = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("WM_PROTOCOLS").expect("fail to convert");
        wmatom[WM::WMProtocols as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("WM_DELETE_WINDOW").expect("fail to convert");
        wmatom[WM::WMDelete as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("WM_STATE").expect("fail to convert");
        wmatom[WM::WMState as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("WM_TAKE_FOCUS").expect("fail to convert");
        wmatom[WM::WMTakeFocus as usize] = XInternAtom(dpy, c_string.as_ptr(), False);

        c_string = CString::new("_NET_ACTIVE_WINDOW").expect("fail to convert");
        netatom[NET::NetActiveWindow as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("_NET_SUPPORTED").expect("fail to convert");
        netatom[NET::NetSupported as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("_NET_WM_NAME").expect("fail to convert");
        netatom[NET::NetWMName as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("_NET_WM_STATE").expect("fail to convert");
        netatom[NET::NetWMState as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("_NET_SUPPORTING_WM_CHECK").expect("fail to convert");
        netatom[NET::NetWMCheck as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("_NET_WM_STATE_FULLSCREEN").expect("fail to convert");
        netatom[NET::NetWMFullscreen as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("_NET_WM_WINDOW_TYPE").expect("fail to convert");
        netatom[NET::NetWMWindowType as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("_NET_WM_WINDOW_TYPE_DIALOG").expect("fail to convert");
        netatom[NET::NetWMWindowTypeDialog as usize] = XInternAtom(dpy, c_string.as_ptr(), False);
        c_string = CString::new("_NET_CLIENT_LIST").expect("fail to convert");
        netatom[NET::NetClientList as usize] = XInternAtom(dpy, c_string.as_ptr(), False);

        // init cursors
        cursor[CUR::CurNormal as usize] =
            drw_cur_create(drw.as_mut().unwrap().as_mut(), XC_left_ptr as i32);
        cursor[CUR::CurResize as usize] =
            drw_cur_create(drw.as_mut().unwrap().as_mut(), XC_sizing as i32);
        cursor[CUR::CurMove as usize] =
            drw_cur_create(drw.as_mut().unwrap().as_mut(), XC_fleur as i32);
        // init appearance
        scheme = vec![vec![]; colors.len()];
        for i in 0..colors.len() {
            scheme[i] = drw_scm_create(drw.as_mut().unwrap().as_mut(), colors[i]);
        }
        // init bars
        println!("[setup] updatebars");
        updatebars();
        println!("[setup] updatestatus");
        updatestatus();
        // supporting window fot NetWMCheck
        wmcheckwin = XCreateSimpleWindow(dpy, root, 0, 0, 1, 1, 0, 0, 0);
        XChangeProperty(
            dpy,
            wmcheckwin,
            netatom[NET::NetWMCheck as usize],
            XA_WINDOW,
            32,
            PropModeReplace,
            addr_of_mut!(wmcheckwin) as *const _,
            1,
        );
        c_string = CString::new("jwm").unwrap();
        XChangeProperty(
            dpy,
            wmcheckwin,
            netatom[NET::NetWMName as usize],
            utf8string,
            8,
            PropModeReplace,
            c_string.as_ptr() as *const _,
            1,
        );
        XChangeProperty(
            dpy,
            root,
            netatom[NET::NetWMCheck as usize],
            XA_WINDOW,
            32,
            PropModeReplace,
            addr_of_mut!(wmcheckwin) as *const _,
            1,
        );
        // EWMH support per view
        XChangeProperty(
            dpy,
            root,
            netatom[NET::NetSupported as usize],
            XA_ATOM,
            32,
            PropModeReplace,
            netatom.as_ptr() as *const _,
            NET::NetLast as i32,
        );
        XDeleteProperty(dpy, root, netatom[NET::NetClientList as usize]);
        // select events
        wa.cursor = cursor[CUR::CurNormal as usize].as_ref().unwrap().cursor;
        wa.event_mask = SubstructureRedirectMask
            | SubstructureNotifyMask
            | ButtonPressMask
            | PointerMotionMask
            | EnterWindowMask
            | LeaveWindowMask
            | StructureNotifyMask
            | PropertyChangeMask;
        XChangeWindowAttributes(dpy, root, CWEventMask | CWCursor, &mut wa);
        XSelectInput(dpy, root, wa.event_mask);
        println!("[setup] grabkeys");
        grabkeys();
        println!("[setup] focus");
        focus(None);
    }
}
pub fn killclient(_arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if selmon_mut.sel.is_none() {
            return;
        }
        if !sendevent(
            &mut *selmon_mut.sel.as_ref().unwrap().borrow_mut(),
            wmatom[WM::WMDelete as usize],
        ) {
            XGrabServer(dpy);
            XSetErrorHandler(Some(transmute(xerrordummy as *const ())));
            XSetCloseDownMode(dpy, DestroyAll);
            XKillClient(dpy, selmon_mut.sel.as_ref().unwrap().borrow_mut().win);
            XSync(dpy, False);
            XSetErrorHandler(Some(transmute(xerror as *const ())));
            XUngrabServer(dpy);
        }
    }
}
pub fn nexttiled(mut c: Option<Rc<RefCell<Client>>>) -> Option<Rc<RefCell<Client>>> {
    while let Some(ref c_ref) = c {
        println!("{} nexttiled", line!());
        let isfloating = c_ref.borrow_mut().isfloating;
        if isfloating || ISVISIBLE(c_ref) <= 0 {
            let next = c_ref.borrow_mut().next.clone();
            c = next;
        } else {
            break;
        }
    }
    return c;
}
pub fn pop(c: Option<Rc<RefCell<Client>>>) {
    detach(c.clone());
    attach(c.clone());
    focus(c.clone());
    arrange(c.as_ref().unwrap().borrow_mut().mon.clone());
}
pub fn propertynotify(e: *mut XEvent) {
    unsafe {
        let c: Option<Rc<RefCell<Client>>>;
        let ev = (*e).property;
        let mut trans: Window = 0;
        if ev.window == root && ev.atom == XA_WM_NAME {
            updatestatus();
        } else if ev.state == PropertyDelete {
            // ignore
            return;
        } else if {
            c = wintoclient(ev.window);
            c.is_some()
        } {
            match ev.atom {
                XA_WM_TRANSIENT_FOR => {
                    if !c.as_ref().unwrap().borrow_mut().isfloating
                        && XGetTransientForHint(
                            dpy,
                            c.as_ref().unwrap().borrow_mut().win,
                            &mut trans,
                        ) > 0
                        && {
                            c.as_ref().unwrap().borrow_mut().isfloating =
                                wintoclient(trans).is_some();
                            c.as_ref().unwrap().borrow_mut().isfloating
                        }
                    {
                        arrange(c.as_ref().unwrap().borrow_mut().mon.clone());
                    }
                }
                XA_WM_NORMAL_HINTS => {
                    c.as_ref().unwrap().borrow_mut().hintsvalid = false;
                }
                XA_WM_HINTS => {
                    updatewmhints(c.as_ref().unwrap());
                    drawbars();
                }
                _ => {}
            }
            if ev.atom == XA_WM_NAME || ev.atom == netatom[NET::NetWMName as usize] {
                updatetitle(c.as_ref().unwrap());
                if Rc::ptr_eq(
                    c.as_ref().unwrap(),
                    c.as_ref()
                        .unwrap()
                        .borrow_mut()
                        .mon
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .sel
                        .as_ref()
                        .unwrap(),
                ) {
                    println!("propertynotify");
                    let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
                    drawbar(mon);
                }
            }
            if ev.atom == netatom[NET::NetWMWindowType as usize] {
                updatewindowtype(c.as_ref().unwrap());
            }
        }
    }
}
pub fn movemouse(_arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        let c = selmon_mut.sel.clone();
        if c.is_none() {
            return;
        }
        if c.as_ref().unwrap().borrow_mut().isfullscreen {
            // no support mmoving fullscreen windows by mouse
            return;
        }
        restack(selmon.clone());
        let ocx = c.as_ref().unwrap().borrow_mut().x;
        let ocy = c.as_ref().unwrap().borrow_mut().y;
        if XGrabPointer(
            dpy,
            root,
            False,
            MOUSEMASK as u32,
            GrabModeAsync,
            GrabModeAsync,
            0,
            cursor[CUR::CurMove as usize].as_ref().unwrap().cursor,
            CurrentTime,
        ) != GrabSuccess
        {
            return;
        }
        let mut x: i32 = 0;
        let mut y: i32 = 0;
        let mut lasttime: Time = 0;
        if getrootptr(&mut x, &mut y) <= 0 {
            return;
        }
        let mut ev: XEvent = zeroed();
        loop {
            XMaskEvent(
                dpy,
                MOUSEMASK | ExposureMask | SubstructureRedirectMask,
                &mut ev,
            );
            match ev.type_ {
                ConfigureRequest | Expose | MapRequest => {
                    if let Some(ha) = handler[ev.type_ as usize] {
                        ha(&mut ev);
                    }
                }
                MotionNotify => {
                    if ev.motion.time - lasttime <= (1000 / 60) {
                        continue;
                    }
                    lasttime = ev.motion.time;

                    let mut nx = ocx + ev.motion.x - x;
                    let mut ny = ocy + ev.motion.y - y;
                    if (selmon_mut.wx - nx).abs() < snap as i32 {
                        nx = selmon_mut.wx;
                    } else if ((selmon_mut.wx + selmon_mut.ww)
                        - (nx + WIDTH(&mut *c.as_ref().unwrap().borrow_mut())))
                    .abs()
                        < snap as i32
                    {
                        nx = selmon_mut.wx + selmon_mut.ww
                            - WIDTH(&mut *c.as_ref().unwrap().borrow_mut());
                    }
                    if (selmon_mut.wy - ny).abs() < snap as i32 {
                        ny = selmon_mut.wy;
                    } else if ((selmon_mut.wy + selmon_mut.wh)
                        - (ny + HEIGHT(&mut *c.as_ref().unwrap().borrow_mut())))
                    .abs()
                        < snap as i32
                    {
                        ny = selmon_mut.wy + selmon_mut.wh
                            - HEIGHT(&mut *c.as_ref().unwrap().borrow_mut());
                    }
                    if !c.as_ref().unwrap().borrow_mut().isfloating
                        && (*selmon_mut.lt[selmon_mut.sellt]).arrange.is_some()
                        && (nx - c.as_ref().unwrap().borrow_mut().x).abs() > snap as i32
                        || (ny - c.as_ref().unwrap().borrow_mut().y).abs() > snap as i32
                    {
                        togglefloating(null_mut());
                    }
                    if (*selmon_mut.lt[selmon_mut.sellt]).arrange.is_none()
                        || (*c.as_ref().unwrap().borrow_mut()).isfloating
                    {
                        resize(
                            c.as_ref().unwrap(),
                            nx,
                            ny,
                            c.as_ref().unwrap().borrow_mut().w,
                            c.as_ref().unwrap().borrow_mut().h,
                            true,
                        );
                    }
                }
                _ => {}
            }
            if ev.type_ == ButtonRelease {
                break;
            }
        }
        XUngrabPointer(dpy, CurrentTime);
        let m = recttomon(
            c.as_ref().unwrap().borrow_mut().x,
            c.as_ref().unwrap().borrow_mut().y,
            c.as_ref().unwrap().borrow_mut().w,
            c.as_ref().unwrap().borrow_mut().h,
        );
        if m != selmon {
            sendmon(c, &m);
            selmon = m;
            focus(None);
        }
    }
}
pub fn resizemouse(_arg: *const Arg) {
    unsafe {
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        let c = selmon_mut.sel.clone();
        if c.is_none() {
            return;
        }
        if (*c.as_ref().unwrap().borrow_mut()).isfullscreen {
            // no support mmoving fullscreen windows by mouse
            return;
        }
        restack(selmon.clone());
        let ocx = (*c.as_ref().unwrap().borrow_mut()).x;
        let ocy = (*c.as_ref().unwrap().borrow_mut()).y;
        if XGrabPointer(
            dpy,
            root,
            False,
            MOUSEMASK as u32,
            GrabModeAsync,
            GrabModeAsync,
            0,
            cursor[CUR::CurMove as usize].as_ref().unwrap().cursor,
            CurrentTime,
        ) != GrabSuccess
        {
            return;
        }
        XWarpPointer(
            dpy,
            0,
            (*c.as_ref().unwrap().borrow_mut()).win,
            0,
            0,
            0,
            0,
            (*c.as_ref().unwrap().borrow_mut()).w + (*c.as_ref().unwrap().borrow_mut()).bw - 1,
            (*c.as_ref().unwrap().borrow_mut()).h + (*c.as_ref().unwrap().borrow_mut()).bw - 1,
        );
        let mut lasttime: Time = 0;
        let mut ev: XEvent = zeroed();
        loop {
            XMaskEvent(
                dpy,
                MOUSEMASK | ExposureMask | SubstructureRedirectMask,
                &mut ev,
            );
            match ev.type_ {
                ConfigureRequest | Expose | MapRequest => {
                    if let Some(ha) = handler[ev.type_ as usize] {
                        ha(&mut ev);
                    }
                }
                MotionNotify => {
                    if ev.motion.time - lasttime <= (1000 / 60) {
                        continue;
                    }
                    lasttime = ev.motion.time;
                    let nw =
                        (ev.motion.x - ocx - 2 * (*c.as_ref().unwrap().borrow_mut()).bw + 1).max(1);
                    let nh =
                        (ev.motion.y - ocy - 2 * (*c.as_ref().unwrap().borrow_mut()).bw + 1).max(1);
                    if (*c.as_ref().unwrap().borrow_mut())
                        .mon
                        .as_ref()
                        .unwrap()
                        .borrow_mut()
                        .wx
                        + nw
                        >= selmon_mut.wx
                        && (*c.as_ref().unwrap().borrow_mut())
                            .mon
                            .as_ref()
                            .unwrap()
                            .borrow_mut()
                            .wx
                            + nw
                            <= selmon_mut.wx + selmon_mut.ww
                        && (*c.as_ref().unwrap().borrow_mut())
                            .mon
                            .as_ref()
                            .unwrap()
                            .borrow_mut()
                            .wy
                            + nh
                            >= selmon_mut.wy
                        && (*c.as_ref().unwrap().borrow_mut())
                            .mon
                            .as_ref()
                            .unwrap()
                            .borrow_mut()
                            .wy
                            + nh
                            <= selmon_mut.wy + selmon_mut.wh
                    {
                        if !(*c.as_ref().unwrap().borrow_mut()).isfloating
                            && (*selmon_mut.lt[selmon_mut.sellt]).arrange.is_some()
                            && ((nw - (*c.as_ref().unwrap().borrow_mut()).w).abs() > snap as i32
                                || (nh - (*c.as_ref().unwrap().borrow_mut()).h).abs() > snap as i32)
                        {
                            togglefloating(null_mut());
                        }
                    }
                    if (*selmon_mut.lt[selmon_mut.sellt]).arrange.is_none()
                        || (*c.as_ref().unwrap().borrow_mut()).isfloating
                    {
                        resize(
                            c.as_ref().unwrap(),
                            (*c.as_ref().unwrap().borrow_mut()).x,
                            (*c.as_ref().unwrap().borrow_mut()).y,
                            nw,
                            nh,
                            true,
                        );
                    }
                }
                _ => {}
            }
            if ev.type_ == ButtonRelease {
                break;
            }
        }
        XWarpPointer(
            dpy,
            0,
            (*c.as_ref().unwrap().borrow_mut()).win,
            0,
            0,
            0,
            0,
            (*c.as_ref().unwrap().borrow_mut()).w + (*c.as_ref().unwrap().borrow_mut()).bw - 1,
            (*c.as_ref().unwrap().borrow_mut()).h + (*c.as_ref().unwrap().borrow_mut()).bw - 1,
        );
        XUngrabPointer(dpy, CurrentTime);
        while XCheckMaskEvent(dpy, EnterWindowMask, &mut ev) > 0 {}
        let m = recttomon(
            (*c.as_ref().unwrap().borrow_mut()).x,
            (*c.as_ref().unwrap().borrow_mut()).y,
            (*c.as_ref().unwrap().borrow_mut()).w,
            (*c.as_ref().unwrap().borrow_mut()).h,
        );
        if m != selmon {
            sendmon(c, &m);
            selmon = m;
            focus(None);
        }
    }
}
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
pub fn grabbuttons(c: Option<Rc<RefCell<Client>>>, focused: bool) {
    updatenumlockmask();
    unsafe {
        let modifiers = [0, LockMask, numlockmask, numlockmask | LockMask];
        let c = c.as_ref().unwrap().borrow_mut();
        XUngrabButton(dpy, AnyButton as u32, AnyModifier, c.win);
        if !focused {
            XGrabButton(
                dpy,
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
        for i in 0..buttons.len() {
            if buttons[i].click == CLICK::ClkClientWin as u32 {
                for j in 0..modifiers.len() {
                    XGrabButton(
                        dpy,
                        buttons[i].button,
                        buttons[i].mask | modifiers[j],
                        c.win,
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
pub fn sendevent(c: &mut Client, proto: Atom) -> bool {
    let mut protocols: *mut Atom = null_mut();
    let mut n: i32 = 0;
    let mut exists: bool = false;
    unsafe {
        let mut ev: XEvent = zeroed();
        if XGetWMProtocols(dpy, c.win, &mut protocols, &mut n) > 0 {
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
            ev.client_message.message_type = wmatom[WM::WMProtocols as usize];
            ev.client_message.format = 32;
            // This data is cool!
            ev.client_message.data.as_longs_mut()[0] = proto as i64;
            ev.client_message.data.as_longs_mut()[1] = CurrentTime as i64;
            XSendEvent(dpy, c.win, False, NoEventMask, &mut ev);
        }
    }
    return exists;
}
pub fn setfocus(c: &Rc<RefCell<Client>>) {
    unsafe {
        let mut c = c.borrow_mut();
        if !c.nerverfocus {
            XSetInputFocus(dpy, c.win, RevertToPointerRoot, CurrentTime);
            XChangeProperty(
                dpy,
                root,
                netatom[NET::NetActiveWindow as usize],
                XA_WINDOW,
                32,
                PropModeReplace,
                &mut c.win as *const u64 as *const _,
                1,
            );
        }
        sendevent(&mut *c, wmatom[WM::WMTakeFocus as usize]);
    }
}
pub fn drawbars() {
    unsafe {
        let mut m = mons.clone();
        while m.is_some() {
            println!("drawbar: {}", m.as_ref().unwrap().borrow_mut().barwin);
            drawbar(m.clone());
            let next = m.as_ref().unwrap().borrow_mut().next.clone();
            m = next;
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
        let m = if c.is_some() {
            c.as_ref().unwrap().borrow_mut().mon.clone()
        } else {
            wintomon(ev.window)
        };
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if !Rc::ptr_eq(m.as_ref().unwrap(), selmon.as_ref().unwrap()) {
            unfocus(selmon_mut.sel.clone(), true);
            selmon = m;
        } else if c.is_none() || Rc::ptr_eq(c.as_ref().unwrap(), selmon_mut.sel.as_ref().unwrap()) {
            return;
        }
        focus(c);
    }
}
pub fn expose(e: *mut XEvent) {
    unsafe {
        let ev = (*e).expose;
        let m = wintomon(ev.window);

        if ev.count == 0 && m.is_some() {
            println!("expose");
            drawbar(m);
        }
    }
}
pub fn focus(mut c: Option<Rc<RefCell<Client>>>) {
    unsafe {
        {
            if c.is_none() || ISVISIBLE(c.as_ref().unwrap()) <= 0 {
                c = {
                    let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
                    selmon_mut.stack.clone()
                };
                while c.is_some() && ISVISIBLE(c.as_ref().unwrap()) <= 0 {
                    let next = { c.as_ref().unwrap().borrow_mut().snext.clone() };
                    c = next;
                }
            }
            let sel = { selmon.as_ref().unwrap().borrow_mut().sel.clone() };
            if sel.is_some() && !Rc::ptr_eq(sel.as_ref().unwrap(), c.as_ref().unwrap()) {
                unfocus(sel.clone(), false);
            }
        }
        if c.is_some() {
            if !Rc::ptr_eq(
                c.as_ref().unwrap().borrow_mut().mon.as_ref().unwrap(),
                selmon.as_ref().unwrap(),
            ) {
                selmon = c.as_ref().unwrap().borrow_mut().mon.clone();
            }
            if c.as_ref().unwrap().borrow_mut().isurgent {
                seturgent(&mut *c.as_ref().unwrap().borrow_mut(), false);
            }
            detachstack(c.clone());
            attachstack(c.clone());
            grabbuttons(c.clone(), true);
            XSetWindowBorder(
                dpy,
                c.as_ref().unwrap().borrow_mut().win,
                scheme[SCHEME::SchemeSel as usize][Col::ColBorder as usize]
                    .as_ref()
                    .unwrap()
                    .pixel,
            );
            setfocus(c.as_ref().unwrap());
        } else {
            XSetInputFocus(dpy, root, RevertToPointerRoot, CurrentTime);
            XDeleteProperty(dpy, root, netatom[NET::NetActiveWindow as usize]);
        }
        {
            let mut selmon_mut = selmon.as_ref().unwrap().borrow_mut();
            selmon_mut.sel = c;
        }
        drawbars();
    }
}
pub fn unfocus(c: Option<Rc<RefCell<Client>>>, setfocus: bool) {
    if c.is_none() {
        return;
    }
    grabbuttons(c.clone(), false);
    unsafe {
        XSetWindowBorder(
            dpy,
            c.as_ref().unwrap().borrow_mut().win,
            scheme[SCHEME::SchemeNorm as usize][Col::ColBorder as usize]
                .as_ref()
                .unwrap()
                .pixel,
        );
        if setfocus {
            XSetInputFocus(dpy, root, RevertToPointerRoot, CurrentTime);
            XDeleteProperty(dpy, root, netatom[NET::NetActiveWindow as usize]);
        }
    }
}
pub fn sendmon(c: Option<Rc<RefCell<Client>>>, m: &Option<Rc<RefCell<Monitor>>>) {
    if Rc::ptr_eq(
        c.as_ref().unwrap().borrow_mut().mon.as_ref().unwrap(),
        m.as_ref().unwrap(),
    ) {
        return;
    }
    unfocus(c.clone(), true);
    detach(c.clone());
    detachstack(c.clone());
    c.as_ref().unwrap().borrow_mut().mon = m.clone();
    // assign tags of target monitor.
    c.as_ref().unwrap().borrow_mut().tags0 =
        m.as_ref().unwrap().borrow_mut().tagset[m.as_ref().unwrap().borrow_mut().seltags];
    attach(c.clone());
    attachstack(c.clone());
    focus(None);
    arrange(None);
}
pub fn setclientstate(c: &Rc<RefCell<Client>>, mut state: i64) {
    unsafe {
        let win = c.borrow_mut().win;
        XChangeProperty(
            dpy,
            win,
            wmatom[WM::WMState as usize],
            wmatom[WM::WMState as usize],
            32,
            PropModeReplace,
            &mut state as *const i64 as *const _,
            2,
        );
    }
}
pub fn keypress(e: *mut XEvent) {
    unsafe {
        let ev = (*e).key;
        let keysym = XKeycodeToKeysym(dpy, ev.keycode as u8, 0);
        println!("fuck: keysym: {}, mask: {}", keysym, CLEANMASK(ev.state));
        for i in 0..keys.len() {
            if keysym == keys[i].keysym
                && CLEANMASK(keys[i].mod0) == CLEANMASK(ev.state)
                && keys[i].func.is_some()
            {
                println!("key: {}, arg: {:?}", i, keys[i].arg);
                println!("keysym: {}, mask: {}", keysym, CLEANMASK(keys[i].mod0));
                keys[i].func.unwrap()(&keys[i].arg);
            }
        }
    }
}
pub fn manage(w: Window, wa: *mut XWindowAttributes) {
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
        }

        updatetitle(c.as_ref().unwrap());
        if XGetTransientForHint(dpy, w, &mut trans) > 0 && {
            t = wintoclient(trans);
            t.is_some()
        } {
            c.as_ref().unwrap().borrow_mut().mon = t.as_ref().unwrap().borrow_mut().mon.clone();
            c.as_ref().unwrap().borrow_mut().tags0 = t.as_ref().unwrap().borrow_mut().tags0;
        } else {
            c.as_ref().unwrap().borrow_mut().mon = selmon.clone();
            applyrules(c.as_ref().unwrap());
        }

        let width;
        let ww;
        let wh;
        let wx;
        let wy;
        {
            width = WIDTH(&mut c.as_ref().unwrap().borrow_mut());
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
            let height = HEIGHT(&mut c.as_ref().unwrap().borrow_mut());
            if c.as_ref().unwrap().borrow_mut().y + height > wy + wh {
                c.as_ref().unwrap().borrow_mut().y = wy + wh - height;
            }
            let x = c.as_ref().unwrap().borrow_mut().x;
            c.as_ref().unwrap().borrow_mut().x = x.max(wx);
            let y = c.as_ref().unwrap().borrow_mut().y;
            c.as_ref().unwrap().borrow_mut().y = y.max(wy);
            c.as_ref().unwrap().borrow_mut().bw = borderpx as i32;
            wc.border_width = c.as_ref().unwrap().borrow_mut().bw;
            XConfigureWindow(dpy, w, CWBorderWidth as u32, &mut wc);
            XSetWindowBorder(
                dpy,
                w,
                scheme[SCHEME::SchemeNorm as usize][Col::ColBorder as usize].as_ref().unwrap().pixel,
            );
            configure(&mut *c.as_ref().unwrap().borrow_mut());
        }
        updatewindowtype(c.as_ref().unwrap());
        updatesizehints(c.as_ref().unwrap());
        updatewmhints(c.as_ref().unwrap());
        XSelectInput(
            dpy,
            w,
            EnterWindowMask | FocusChangeMask | PropertyChangeMask | StructureNotifyMask,
        );
        grabbuttons(c.clone(), false);
        {
            if !c.as_ref().unwrap().borrow_mut().isfloating {
                let isfixed = c.as_ref().unwrap().borrow_mut().isfixed;
                c.as_ref().unwrap().borrow_mut().oldstate = trans != 0 || isfixed;
                let oldstate = c.as_ref().unwrap().borrow_mut().oldstate;
                c.as_ref().unwrap().borrow_mut().isfloating = oldstate;
            }
            if c.as_ref().unwrap().borrow_mut().isfloating {
                XRaiseWindow(dpy, c.as_ref().unwrap().borrow_mut().win);
            }
        }
        attach(c.clone());
        attachstack(c.clone());
        {
            XChangeProperty(
                dpy,
                root,
                netatom[NET::NetClientList as usize],
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
            XMoveResizeWindow(dpy, win, x + 2 * sw, y, w as u32, h as u32);
        }
        setclientstate(c.as_ref().unwrap(), NormalState as i64);
        let mon_eq_selmon;
        {
            mon_eq_selmon = Rc::ptr_eq(
                c.as_ref().unwrap().borrow_mut().mon.as_ref().unwrap(),
                selmon.as_ref().unwrap(),
            );
        }
        if mon_eq_selmon {
            unfocus(selmon.as_ref().unwrap().borrow_mut().sel.clone(), false);
        }
        {
            let mon = c.as_ref().unwrap().borrow_mut().mon.clone();
            mon.as_ref().unwrap().borrow_mut().sel = c.clone();
            arrange(mon);
        }
        {
            XMapWindow(dpy, c.as_ref().unwrap().borrow_mut().win);
        }
        focus(None);
    }
}
pub fn mappingnotify(e: *mut XEvent) {
    unsafe {
        let mut ev = (*e).mapping;
        XRefreshKeyboardMapping(&mut ev);
        if ev.request == MappingKeyboard {
            grabkeys();
        }
    }
}
pub fn maprequest(e: *mut XEvent) {
    unsafe {
        let ev = (*e).map_request;
        static mut wa: XWindowAttributes = unsafe { zeroed() };
        if XGetWindowAttributes(dpy, ev.window, addr_of_mut!(wa)) <= 0 || wa.override_redirect > 0 {
            return;
        }
        if wintoclient(ev.window).is_none() {
            manage(ev.window, addr_of_mut!(wa));
        }
    }
}
pub fn monocle(m: *mut Monitor) {
    unsafe {
        // This idea is cool!.
        static mut formatted_string: String = String::new();
        let mut n: u32 = 0;
        let mut c = (*m).clients.clone();
        while c.is_some() {
            if ISVISIBLE(c.as_ref().unwrap()) > 0 {
                n += 1;
            }
            let next = c.as_ref().unwrap().borrow_mut().next.clone();
            c = next;
        }
        if n > 0 {
            // override layout symbol
            formatted_string = format!("[{}]", n);
            (*m).ltsymbol = formatted_string.as_str();
        }
        let mut c = nexttiled((*m).clients.clone());
        while c.is_some() {
            let bw = c.as_ref().unwrap().borrow_mut().bw;
            resize(
                c.as_ref().unwrap(),
                (*m).wx,
                (*m).wy,
                (*m).ww - 2 * bw,
                (*m).wh - 2 * bw,
                false,
            );
            let next = nexttiled(c.as_ref().unwrap().borrow_mut().next.clone());
            c = next;
        }
    }
}
pub fn motionnotify(e: *mut XEvent) {
    unsafe {
        // This idea is cool
        static mut motionmon: Option<Rc<RefCell<Monitor>>> = None;
        let ev = (*e).motion;
        if ev.window != root {
            return;
        }
        let m = recttomon(ev.x_root, ev.y_root, 1, 1);
        if m != motionmon && motionmon.is_some() {
            let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
            unfocus(selmon_mut.sel.clone(), true);
            selmon = m.clone();
            focus(None);
        }
        motionmon = m;
    }
}
pub fn unmanage(c: Option<Rc<RefCell<Client>>>, destroyed: bool) {
    unsafe {
        let mut wc: XWindowChanges = zeroed();
        detach(c.clone());
        detachstack(c.clone());
        if !destroyed {
            let oldbw = c.as_ref().unwrap().borrow_mut().oldbw;
            let win = c.as_ref().unwrap().borrow_mut().win;
            wc.border_width = oldbw;
            // avoid race conditions.
            XGrabServer(dpy);
            XSetErrorHandler(Some(transmute(xerrordummy as *const ())));
            XSelectInput(dpy, win, NoEventMask);
            // restore border.
            XConfigureWindow(dpy, win, CWBorderWidth as u32, &mut wc);
            XUngrabButton(dpy, AnyButton as u32, AnyModifier, win);
            setclientstate(c.as_ref().unwrap(), WithdrawnState as i64);
            XSync(dpy, False);
            XSetErrorHandler(Some(transmute(xerror as *const ())));
            XUngrabServer(dpy);
        }
        focus(None);
        updateclientlist();
        arrange(c.as_ref().unwrap().borrow_mut().mon.clone());
    }
}
pub fn unmapnotify(e: *mut XEvent) {
    unsafe {
        let ev = (*e).unmap;
        let c = wintoclient(ev.window);
        if c.is_some() {
            if ev.send_event > 0 {
                setclientstate(c.as_ref().unwrap(), WithdrawnState as i64);
            } else {
                unmanage(c, false);
            }
        }
    }
}

pub fn updategeom() -> bool {
    let mut dirty: bool = false;
    unsafe {
        if mons.is_none() {
            mons = Some(Rc::new(RefCell::new(createmon())));
        }
        // Be careful not to import borrow_mut!
        // Rc/RefCell is cool
        {
            let mut mons_mut = mons.as_ref().unwrap().borrow_mut();
            if mons_mut.mw != sw || mons_mut.mh != sh {
                dirty = true;
                mons_mut.mw = sw;
                mons_mut.ww = sw;
                mons_mut.mh = sh;
                mons_mut.wh = sh;
                updatebarpos(&mut *mons_mut);
            }
        }
        if dirty {
            selmon = mons.clone();
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
    // (TODO), polish this in need.
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
        println!("updatestatus");
        drawbar(selmon.clone());
    }
}
pub fn updatewindowtype(c: &Rc<RefCell<Client>>) {
    unsafe {
        // todo
        let c = &mut *c.borrow_mut();
        let state = getatomprop(c, netatom[NET::NetWMState as usize]);
        let wtype = getatomprop(c, netatom[NET::NetWMWindowType as usize]);

        if state == netatom[NET::NetWMFullscreen as usize] {
            setfullscreen(c, true);
        }
        if wtype == netatom[NET::NetWMWindowTypeDialog as usize] {
            c.isfloating = true;
        }
    }
}
pub fn updatewmhints(c: &Rc<RefCell<Client>>) {
    unsafe {
        let mut cc = c.borrow_mut();
        let wmh = XGetWMHints(dpy, cc.win);
        let selmon_mut = selmon.as_ref().unwrap().borrow_mut();
        if !wmh.is_null() {
            if selmon_mut.sel.is_some()
                && Rc::ptr_eq(c, selmon_mut.sel.as_ref().unwrap())
                && ((*wmh).flags & XUrgencyHint) > 0
            {
                (*wmh).flags &= !XUrgencyHint;
                XSetWMHints(dpy, cc.win, wmh);
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
pub fn updatetitle(c: &Rc<RefCell<Client>>) {
    unsafe {
        let mut c = c.borrow_mut();
        if !gettextprop(
            c.win,
            netatom[NET::NetWMName as usize],
            c.name,
            c.name.len(),
        ) {
            gettextprop(c.win, XA_WM_NAME, c.name, c.name.len());
        }
        // hack to mark broken clients
        if let Some(b) = c.name.chars().nth(0) {
            if b == '\0' {
                c.name = broken;
            }
        }
    }
}
