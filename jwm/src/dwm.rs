use std::ffi::CStr;
use std::ptr::null_mut;
use std::{os::raw::c_long, usize};

use x11::xlib::{
    Atom, ButtonPressMask, ButtonReleaseMask, CWBorderWidth, CWHeight, CWWidth, ConfigureNotify,
    ControlMask, Display, KeySym, LockMask, Mod1Mask, Mod2Mask, Mod3Mask, Mod4Mask, Mod5Mask,
    PAspect, PBaseSize, PMaxSize, PMinSize, PResizeInc, PSize, PointerMotionMask, ShiftMask,
    StructureNotifyMask, Window, XClassHint, XConfigureEvent, XConfigureWindow, XEvent, XFree,
    XGetClassHint, XGetWMNormalHints, XMoveWindow, XRaiseWindow, XSendEvent, XSizeHints, XSync,
    XWindowChanges, CWX, CWY,
};

use std::cmp::{max, min};

use crate::config::{self, resizehints, rules, tags};
use crate::drw::{drw_fontset_getwidth, drw_setscheme, drw_text, Clr, Cur, Drw};

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
pub static mut wmatom: [Atom; _WM::WMLast as usize] = unsafe { std::mem::zeroed() };
pub static mut netatom: [Atom; _NET::NetLast as usize] = unsafe { std::mem::zeroed() };
pub static mut running: i32 = 0;
pub static mut cursor: [*mut Cur; _CUR::CurLast as usize] = [null_mut(); _CUR::CurLast as usize];
pub static mut scheme: Vec<Vec<*mut Clr>> = vec![];
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
    pub click: u32,
    pub mask: u32,
    pub button: u32,
    pub func: Option<fn(*const Arg)>,
    pub arg: Arg,
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
    pub mod0: u32,
    pub keysym: KeySym,
    pub func: Option<fn(*const Arg)>,
    pub arg: Arg,
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
    pub symbol: &'static str,
    pub arrange: Option<fn(*mut Monitor)>,
}
impl Layout {
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
    pub hintsvalid: i32,
    pub bw: i32,
    pub oldbw: i32,
    pub tags0: u32,
    pub isfixed: i32,
    pub isfloating: bool,
    pub isurgent: i32,
    pub nerverfocus: i32,
    pub oldstate: i32,
    pub isfullscreen: bool,
    pub next: *mut Client,
    pub snext: *mut Client,
    pub mon: *mut Monitor,
    pub win: Window,
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
        isfloating: bool,
        isurgent: i32,
        nerverfocus: i32,
        oldstate: i32,
        isfullscreen: bool,
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
    pub ltsymbol: &'static str,
    pub mfact: f32,
    pub nmaster: i32,
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
    pub seltags: u32,
    pub sellt: usize,
    pub tagset: [u32; 2],
    pub showbar: bool,
    pub topbar: i32,
    pub clients: *mut Client,
    pub sel: *mut Client,
    pub stack: *mut Client,
    pub next: *mut Monitor,
    pub barwin: Window,
    pub lt: [*mut Layout; 2],
}
impl Monitor {
    pub fn new(
        ltsymbol: &'static str,
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
        sellt: usize,
        tagset: [u32; 2],
        showbar: bool,
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
    unsafe { drw_fontset_getwidth(drw0, X) + lrpad as u32 }
}

pub struct Rule {
    pub class: &'static str,
    pub instance: &'static str,
    pub title: &'static str,
    pub tags0: usize,
    pub isfloating: bool,
    pub monitor: i32,
}
impl Rule {
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

pub fn updatesizehints(c: *mut Client) {
    let mut msize: u32 = 0;
    unsafe {
        let mut size: XSizeHints = std::mem::zeroed();

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
        (*c).isfixed = ((*c).maxw > 0
            && (*c).maxh > 0
            && ((*c).maxw == (*c).minw)
            && ((*c).maxh == (*c).minh)) as i32;
        (*c).hintsvalid = 1;
    }
}

pub fn applysizehints(
    c: *mut Client,
    x: &mut i32,
    y: &mut i32,
    w: &mut i32,
    h: &mut i32,
    interact: i32,
) -> bool {
    unsafe {
        let mut baseismin: bool = false;
        let m = (*c).mon;

        // set minimum possible.
        *w = 1.max(*w);
        *h = 1.max(*h);
        if interact > 0 {
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
        if resizehints > 0
            || (*c).isfloating
            || (*(*(*c).mon).lt[(*(*c).mon).sellt as usize])
                .arrange
                .is_none()
        {
            if (*c).hintsvalid <= 0 {
                updatesizehints(c);
            }
            // see last two sentences in ICCCM 4.1.2.3
            baseismin = (*c).basew == (*c).minw && (*c).baseh == (*c).minh;
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
        let mut ce: XConfigureEvent = std::mem::zeroed();

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

pub fn resizeclient(c: *mut Client, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let mut wc: XWindowChanges = std::mem::zeroed();
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

pub fn resize(c: *mut Client, x: &mut i32, y: &mut i32, w: &mut i32, h: &mut i32, interact: i32) {
    if applysizehints(c, x, y, w, h, interact) {
        resizeclient(c, *x, *y, *w, *h);
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
                resize(c, &mut (*c).x, &mut (*c).y, &mut (*c).w, &mut (*c).h, 0);
            }
            showhide((*c).snext);
        } else {
            // hide clients bottom up.
            showhide((*c).snext);
            XMoveWindow(dpy, (*c).win, WIDTH(c) * -2, (*c).y);
        }
    }
}

pub fn arrangemon(m: *mut Monitor) {
    unsafe {
        (*m).ltsymbol = (*(*m).lt[(*m).sellt]).symbol;
        if let Some(arrange0) = (*(*m).lt[(*m).sellt]).arrange {
            arrange0(m);
        }
    }
}

pub fn drawbar(m: *mut Monitor) {
    let mut x: i32 = 0;
    let mut w: i32 = 0;
    let mut tw: i32 = 0;
    let mut i: u32 = 0;
    let mut occ: u32 = 0;
    let mut urg: u32 = 0;
    unsafe {
        let boxs = (*(*drw).fonts).h / 9;
        let boxw = (*(*drw).fonts).h / 6 + 2;
        let mut c: *mut Client = null_mut();

        if !(*m).showbar {
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
    }
}

pub fn restack(m: *mut Monitor) {
    // (TODO)

    unsafe {
        if (*m).sel.is_null() {
            return;
        }
        if (*(*m).sel).isfloating || (*(*m).lt[(*m).sellt]).arrange.is_none() {
            XRaiseWindow(dpy, (*(*m).sel).win);
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
            // (TODO)
        } else {
            m = mons;
            loop {
                if m.is_null() {
                    break;
                }
                arrangemon((*m).next);
                m = (*m).next;
            }
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
