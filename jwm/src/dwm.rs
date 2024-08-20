use std::os::raw::c_long;

use x11::xlib::{
    ButtonPressMask, ButtonReleaseMask, ControlMask, LockMask, Mod1Mask, Mod2Mask, Mod3Mask,
    Mod4Mask, Mod5Mask, PointerMotionMask, ShiftMask,
};

use crate::config;

pub const BUTTONMASK: c_long = ButtonPressMask | ButtonReleaseMask;
#[inline]
fn CLEANMASK(mask: u32) -> u32 {
    return mask
        & !(numlockmask | LockMask)
        & (ShiftMask | ControlMask | Mod1Mask | Mod2Mask | Mod3Mask | Mod4Mask | Mod5Mask);
}
pub const MOUSEMASK: c_long = BUTTONMASK | PointerMotionMask;
pub const VERSION: &str = "6.5";

pub static numlockmask: u32 = 0;

#[repr(C)]
enum _CUR {
    CurNormal = 0,
    CurResize = 1,
    CurMove = 2,
    CurLast = 3,
}

#[repr(C)]
enum _SCHEME {
    SchemeNorm = 0,
    SchemeSel = 1,
}

#[repr(C)]
enum _NET {
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
enum _WM {
    WMProtocols = 0,
    WMDelete = 1,
    WMState = 2,
    WMTakeFocus = 3,
    WMLast = 4,
}

#[repr(C)]
enum _CLICK {
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
    v(*const u8),
}

pub struct Button {
    click: u32,
    mask: u32,
    button: u32,
    func: fn(*const Arg),
    arg: Arg,
}

pub struct Layout {
    symbol: &'static str,
    // arrange: fn(*mut Monitor),
}

pub struct Rule {
    class: &'static str,
    instance: &'static str,
    title: &'static str,
    tags: usize,
    isfloating: i32,
    monitor: i32,
}

impl Rule {
    pub fn new(
        class: &'static str,
        instance: &'static str,
        title: &'static str,
        tags: usize,
        isfloating: i32,
        monitor: i32,
    ) -> Self {
        Rule {
            class,
            instance,
            title,
            tags,
            isfloating,
            monitor,
        }
    }
}
