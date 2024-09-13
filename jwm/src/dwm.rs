use std::ffi::{c_char, CStr};
use std::mem::zeroed;
use std::ops::Deref;
use std::ptr::null_mut;
use std::{os::raw::c_long, usize};

use x11::keysym::XK_Num_Lock;
use x11::xinput::_XAnyClassinfo;
use x11::xlib::{
    AnyButton, AnyKey, AnyModifier, Atom, Below, ButtonPressMask, ButtonReleaseMask, CWBorderWidth,
    CWHeight, CWSibling, CWStackMode, CWWidth, ConfigureNotify, ControlMask, CurrentTime, Display,
    EnterWindowMask, False, GrabModeSync, GrayScale, KeySym, LockMask, Mod1Mask, Mod2Mask,
    Mod3Mask, Mod4Mask, Mod5Mask, PAspect, PBaseSize, PMaxSize, PMinSize, PResizeInc, PSize,
    PointerMotionMask, ReplayPointer, RevertToPointerRoot, ShiftMask, StructureNotifyMask, Success,
    True, Window, XAllowEvents, XCheckMaskEvent, XClassHint, XConfigureEvent, XConfigureWindow,
    XDeleteProperty, XDisplayKeycodes, XEvent, XFree, XFreeModifiermap, XGetClassHint,
    XGetKeyboardMapping, XGetModifierMapping, XGetWMNormalHints, XGetWindowProperty, XGrabButton,
    XGrabKey, XKeysymToKeycode, XMoveWindow, XQueryPointer, XRaiseWindow, XSendEvent,
    XSetInputFocus, XSetWindowBorder, XSizeHints, XSync, XUngrabButton, XUngrabKey, XWindowChanges,
    CWX, CWY, XA_ATOM,
};

use std::cmp::{max, min};

use crate::config::{self, buttons, keys, resizehints, rules, tags};
use crate::drw::{
    drw_fontset_getwidth, drw_map, drw_rect, drw_setscheme, drw_text, Clr, Cur, Drw, _Col,
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
    pub isfixed: bool,
    pub isfloating: bool,
    pub isurgent: bool,
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
        isfixed: bool,
        isfloating: bool,
        isurgent: bool,
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
        let mut ch: XClassHint = zeroed();
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
        c = (*m).clients;
        while !c.is_null() {
            occ |= (*c).tags0;
            if (*c).isurgent {
                urg |= (*c).tags0;
            }
            c = (*c).next;
        }
        x = 0;
        for i in 0..tags.len() {
            w = TEXTW(drw, tags[i]) as i32;
            let idx = if (*m).tagset[(*m).seltags as usize] & 1 << i > 0 {
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
    let mut di: i32 = 0;
    let mut dl: u64 = 0;
    let mut da: Atom = 0;
    let mut atom: Atom = 0;
    let mut p: *mut u8 = null_mut();
    unsafe {
        di = 3;
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
    let mut a: i32 = 0;
    let mut area: i32 = 0;

    unsafe {
        let mut r: *mut Monitor = selmon;
        let mut m = mons;
        while !m.is_null() {
            a = INTERSECT(x, y, w, h, m);
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
    let click = _CLICK::ClkRootWin;
    let mut i: u32 = 0;
    let mut x: u32 = 0;
    let mut arg: Arg = Arg::i(0);
    // (TODO)
    unsafe {
        let mut c: *mut Client = null_mut();
        let mut ev = (*e).button;
        let mut click = _CLICK::ClkRootWin;
        // focus monitor if necessary.
        let mut m = wintomon(ev.window);
        if m != selmon {
            unfocus((*selmon).sel, true);
            selmon = m;
            focus(null_mut());
        }
        if ev.window == (*selmon).barwin {
            loop {
                x += TEXTW(drw, tags[i as usize]);
                if ev.x >= x as i32
                    && ({
                        i += 1;
                        i
                    } < tags.len() as u32)
                {
                    break;
                }
            }
            if i < tags.len() as u32 {
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
                buttons[i].func.unwrap()(
                    if let Arg::i(0) = arg
                        && (click as u32 == _CLICK::ClkTagBar as u32)
                    {
                        &mut arg
                    } else {
                        &mut buttons[i].arg
                    },
                );
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
pub fn incnmaster(arg: *const Arg) {
    unsafe {
        if let Arg::i(i0) = *arg {
            (*selmon).nmaster = 0.max((*selmon).nmaster + i0);
        }
    }
}
// (TODO): XINERAMA
pub fn setmfact(arg: *const Arg) {}
pub fn setlayout(arg: *const Arg) {}
pub fn zoom(arg: *const Arg) {}
pub fn view(arg: *const Arg) {}
pub fn toggleview(arg: *const Arg) {}
pub fn toggletag(arg: *const Arg) {}
pub fn tag(arg: *const Arg) {}
pub fn quit(arg: *const Arg) {
    unsafe {
        running = 0;
    }
}
pub fn killclient(arg: *const Arg) {}
pub fn movemouse(arg: *const Arg) {}
pub fn resizemouse(arg: *const Arg) {}
pub fn updatenumlockmask() {
    unsafe {
        numlockmask = 0;
        let mut modmap = XGetModifierMapping(dpy);
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
pub fn focus(c: *mut Client) {}
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
