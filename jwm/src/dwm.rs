use std::ffi::CStr;
use std::ptr::null_mut;
use std::{os::raw::c_long, usize};

use x11::xlib::{
    Atom, ButtonPressMask, ButtonReleaseMask, ControlMask, Display, KeySym, LockMask, Mod1Mask,
    Mod2Mask, Mod3Mask, Mod4Mask, Mod5Mask, PointerMotionMask, ShiftMask, Window, XClassHint,
    XFree, XGetClassHint,
};

use lazy_static::lazy_static;
use std::cmp::{max, min};
use std::sync::Mutex;

use crate::config::{self, rules, tags};
use crate::drw::{drw_fontset_getwidth, Clr, Cur, Drw};

pub const BUTTONMASK: c_long = ButtonPressMask | ButtonReleaseMask;
#[inline]
fn CLEANMASK(mask: u32) -> u32 {
    return mask
        & unsafe { !(*numlockmask.lock().unwrap() | LockMask) }
        & (ShiftMask | ControlMask | Mod1Mask | Mod2Mask | Mod3Mask | Mod4Mask | Mod5Mask);
}
pub const MOUSEMASK: c_long = BUTTONMASK | PointerMotionMask;
pub const VERSION: &str = "6.5";

// Variables.
pub const broken: &str = "broken";
pub static mut stext: Mutex<&str> = Mutex::new("");
pub static mut screen: Mutex<i32> = Mutex::new(0);
pub static mut sw: Mutex<i32> = Mutex::new(0);
pub static mut sh: Mutex<i32> = Mutex::new(0);
pub static mut bh: Mutex<i32> = Mutex::new(0);
pub static mut lrpad: Mutex<i32> = Mutex::new(0);
pub static mut numlockmask: Mutex<u32> = Mutex::new(0);
pub static mut wmatom: Mutex<[Atom; _WM::WMLast as usize]> =
    Mutex::new(unsafe { std::mem::zeroed() });
pub static mut netatom: Mutex<[Atom; _NET::NetLast as usize]> =
    Mutex::new(unsafe { std::mem::zeroed() });
pub static mut running: Mutex<i32> = Mutex::new(0);
pub static mut cursor: Mutex<[*mut Cur; _CUR::CurLast as usize]> =
    Mutex::new([null_mut(); _CUR::CurLast as usize]);
pub static mut scheme: Mutex<Vec<Vec<Clr>>> = Mutex::new(vec![]);
pub static mut dpy: *mut Display = null_mut();
pub static mut drw: *mut Drw = null_mut();
pub static mut mons: *mut Monitor = null_mut();
pub static mut selmon: *mut Monitor = null_mut();
pub static mut root: Window = 0;
pub static mut wmcheckwin: Window = 0;

#[repr(C)]
pub enum _CUR {
    CurNormal = 0,
    CurResize = 1,
    CurMove = 2,
    CurLast = 3,
}

#[repr(C)]
pub enum _SCHEME {
    SchemeNorm = 0,
    SchemeSel = 1,
}

#[repr(C)]
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
pub enum _WM {
    WMProtocols = 0,
    WMDelete = 1,
    WMState = 2,
    WMTakeFocus = 3,
    WMLast = 4,
}

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

pub enum Arg {
    i(i32),
    ui(u32),
    f(f32),
    v(Vec<&'static str>),
    lo(Layout),
}

pub struct Button {
    click: u32,
    mask: u32,
    button: u32,
    func: Option<fn(*const Arg)>,
    arg: Arg,
}
impl Button {
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

pub struct Key {
    mod0: u32,
    keysym: KeySym,
    func: Option<fn(*const Arg)>,
    arg: Arg,
}
impl Key {
    pub fn new(mod0: u32, keysym: KeySym, func: Option<fn(*const Arg)>, arg: Arg) -> Self {
        Self {
            mod0,
            keysym,
            func,
            arg,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    symbol: &'static str,
    arrange: Option<fn(*mut Monitor)>,
}
impl Layout {
    pub fn new(symbol: &'static str, arrange: Option<fn(*mut Monitor)>) -> Self {
        Self { symbol, arrange }
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    name: &'static str,
    mina: f32,
    maxa: f32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    oldx: i32,
    oldy: i32,
    oldw: i32,
    oldh: i32,
    basew: i32,
    baseh: i32,
    incw: i32,
    inch: i32,
    maxw: i32,
    maxh: i32,
    minw: i32,
    minh: i32,
    hintsvalid: i32,
    bw: i32,
    oldbw: i32,
    tags0: u32,
    isfixed: i32,
    isfloating: i32,
    isurgent: i32,
    nerverfocus: i32,
    oldstate: i32,
    isfullscreen: i32,
    next: *mut Client,
    snext: *mut Client,
    mon: *mut Monitor,
    win: Window,
}
impl Client {
    pub fn new(
        name: &'static str,
        mina: f32,
        maxa: f32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        oldx: i32,
        oldy: i32,
        oldw: i32,
        oldh: i32,
        basew: i32,
        baseh: i32,
        incw: i32,
        inch: i32,
        maxw: i32,
        maxh: i32,
        minw: i32,
        minh: i32,
        hintsvalid: i32,
        bw: i32,
        oldbw: i32,
        tags0: u32,
        isfixed: i32,
        isfloating: i32,
        isurgent: i32,
        nerverfocus: i32,
        oldstate: i32,
        isfullscreen: i32,
        next: *mut Client,
        snext: *mut Client,
        mon: *mut Monitor,
        win: Window,
    ) -> Self {
        Self {
            name,
            mina,
            maxa,
            x,
            y,
            w,
            h,
            oldx,
            oldy,
            oldw,
            oldh,
            basew,
            baseh,
            incw,
            inch,
            maxw,
            maxh,
            minw,
            minh,
            hintsvalid,
            bw,
            oldbw,
            tags0,
            isfixed,
            isfloating,
            isurgent,
            nerverfocus,
            oldstate,
            isfullscreen,
            next,
            snext,
            mon,
            win,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Monitor {
    ltsymbol: [u8; 16],
    mfact: f32,
    nmaster: i32,
    num: i32,
    by: i32,
    mx: i32,
    my: i32,
    mw: i32,
    mh: i32,
    wx: i32,
    wy: i32,
    ww: i32,
    wh: i32,
    seltags: u32,
    sellt: u32,
    tagset: [u32; 2],
    showbar: i32,
    topbar: i32,
    clients: *mut Client,
    sel: *mut Client,
    stack: *mut Client,
    next: *mut Monitor,
    barwin: Window,
    lt: [*mut Layout; 2],
}
impl Monitor {
    pub fn new(
        ltsymbol: [u8; 16],
        mfact: f32,
        nmaster: i32,
        num: i32,
        by: i32,
        mx: i32,
        my: i32,
        mw: i32,
        mh: i32,
        wx: i32,
        wy: i32,
        ww: i32,
        wh: i32,
        seltags: u32,
        sellt: u32,
        tagset: [u32; 2],
        showbar: i32,
        topbar: i32,
        clients: *mut Client,
        sel: *mut Client,
        stack: *mut Client,
        next: *mut Monitor,
        barwin: Window,
        lt: [*mut Layout; 2],
    ) -> Self {
        Self {
            ltsymbol,
            mfact,
            nmaster,
            num,
            by,
            mx,
            my,
            mw,
            mh,
            wx,
            wy,
            ww,
            wh,
            seltags,
            sellt,
            tagset,
            showbar,
            topbar,
            clients,
            sel,
            stack,
            next,
            barwin,
            lt,
        }
    }
}

pub fn INTERSECT(x: i32, y: i32, w: i32, h: i32, m: *const Monitor) -> i32 {
    unsafe {
        max(0, min(x + w, (*m).wx + (*m).ww) - max(x, (*m).wx))
            * max(0, min(y + h, (*m).wy + (*m).wh) - max(y, (*m).wy))
    }
}

pub fn ISVISIBLE(C: *const Client) -> u32 {
    unsafe { (*C).tags0 & (*(*C).mon).tagset[(*(*C).mon).seltags as usize] }
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
    unsafe { drw_fontset_getwidth(drw0, X) + *lrpad.lock().unwrap() as u32 }
}

pub struct Rule {
    class: &'static str,
    instance: &'static str,
    title: &'static str,
    tags0: usize,
    isfloating: i32,
    monitor: i32,
}
impl Rule {
    pub fn new(
        class: &'static str,
        instance: &'static str,
        title: &'static str,
        tags0: usize,
        isfloating: i32,
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
        (*c).isfloating = 0;
        (*c).tags0 = 0;
        let mut ch: XClassHint = std::mem::zeroed();
        XGetClassHint(dpy, (*c).win, &mut ch);
        let mut class: &str = "";
        let mut instance: &str = "";
        if !ch.res_class.is_null() {
            let c_str = CStr::from_ptr(ch.res_class);
            class = c_str.to_str().unwrap();
        } else {
            class = broken;
        };
        if !ch.res_name.is_null() {
            let c_str = CStr::from_ptr(ch.res_name);
            instance = c_str.to_str().unwrap();
        } else {
            instance = broken;
        }

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
            (*(*c).mon).tagset[(*(*c).mon).seltags as usize]
        }
    }
}

pub fn spawn(arg: *const Arg) {}
pub fn togglebar(arg: *const Arg) {}
pub fn togglefloating(arg: *const Arg) {}
pub fn focusmon(arg: *const Arg) {}
pub fn tagmon(arg: *const Arg) {}
pub fn focusstack(arg: *const Arg) {}
pub fn incnmaster(arg: *const Arg) {}
pub fn setmfact(arg: *const Arg) {}
pub fn setlayout(arg: *const Arg) {}
pub fn zoom(arg: *const Arg) {}
pub fn view(arg: *const Arg) {}
pub fn toggleview(arg: *const Arg) {}
pub fn toggletag(arg: *const Arg) {}
pub fn tag(arg: *const Arg) {}
pub fn quit(arg: *const Arg) {}
pub fn killclient(arg: *const Arg) {}
pub fn movemouse(arg: *const Arg) {}
pub fn resizemouse(arg: *const Arg) {}
