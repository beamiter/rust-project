#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

use libc::{
    close, exit, fork, setsid, sigaction, sigemptyset, waitpid, SA_NOCLDSTOP, SA_NOCLDWAIT,
    SA_RESTART, SIGCHLD, SIG_DFL, SIG_IGN, WNOHANG,
};
use libc::{fd_set, select, timeval, FD_ISSET, FD_SET, FD_ZERO};
use log::info;
use log::warn;
use log::{debug, error};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use shared_structures::CommandType;
use shared_structures::SharedCommand;
use shared_structures::{MonitorInfo, SharedMessage, SharedRingBuffer, TagStatus};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_char;
use std::ffi::{CStr, CString};
use std::fmt;
use std::mem::transmute;
use std::mem::zeroed;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::ptr::{addr_of_mut, null, null_mut};
use std::rc::Rc;
use std::str::FromStr; // 用于从字符串解析 // 用于格式化输出，如 Display trait
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use std::{os::raw::c_long, usize};
use x11::xft::XftColor;
use x11::xinerama::{XineramaIsActive, XineramaQueryScreens, XineramaScreenInfo};
use x11::xlib::{XConfigureRequestEvent, XFlush};
use x11::xlib::{XConnectionNumber, XPending};
use x11::xlib::{XFreeStringList, XSetClassHint};
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

use crate::config::CONFIG;
use crate::drw::{Cur, Drw};
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
pub struct ColorScheme {
    pub fg: XftColor,     // 前景色
    pub bg: XftColor,     // 背景色
    pub border: XftColor, // 边框色
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemeType {
    Norm = 0, // 普通状态
    Sel = 1,  // 选中状态
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct ThemeManager {
    pub norm: ColorScheme, // 普通状态的颜色方案
    pub sel: ColorScheme,  // 选中状态的颜色方案
}

#[allow(dead_code)]
impl ColorScheme {
    /// 创建新的颜色方案
    pub fn new(fg: XftColor, bg: XftColor, border: XftColor) -> Self {
        Self { fg, bg, border }
    }

    /// 获取前景色
    pub fn foreground(&self) -> &XftColor {
        &self.fg
    }

    /// 获取背景色
    pub fn background(&self) -> &XftColor {
        &self.bg
    }

    /// 获取边框色
    pub fn border_color(&self) -> &XftColor {
        &self.border
    }

    /// 设置前景色
    pub fn set_foreground(&mut self, color: XftColor) {
        self.fg = color;
    }

    /// 设置背景色
    pub fn set_background(&mut self, color: XftColor) {
        self.bg = color;
    }

    /// 设置边框色
    pub fn set_border(&mut self, color: XftColor) {
        self.border = color;
    }
}

#[allow(dead_code)]
impl ThemeManager {
    /// 创建新的主题管理器
    pub fn new(norm: ColorScheme, sel: ColorScheme) -> Self {
        Self { norm, sel }
    }

    /// 根据方案类型获取颜色方案
    pub fn get_scheme(&self, scheme_type: SchemeType) -> &ColorScheme {
        match scheme_type {
            SchemeType::Norm => &self.norm,
            SchemeType::Sel => &self.sel,
        }
    }

    /// 获取可变颜色方案
    pub fn get_scheme_mut(&mut self, scheme_type: SchemeType) -> &mut ColorScheme {
        match scheme_type {
            SchemeType::Norm => &mut self.norm,
            SchemeType::Sel => &mut self.sel,
        }
    }

    /// 获取指定方案的前景色
    pub fn get_fg(&self, scheme_type: SchemeType) -> &XftColor {
        self.get_scheme(scheme_type).foreground()
    }

    /// 获取指定方案的背景色
    pub fn get_bg(&self, scheme_type: SchemeType) -> &XftColor {
        self.get_scheme(scheme_type).background()
    }

    /// 获取指定方案的边框色
    pub fn get_border(&self, scheme_type: SchemeType) -> &XftColor {
        self.get_scheme(scheme_type).border_color()
    }

    /// 设置整个颜色方案
    pub fn set_scheme(&mut self, scheme_type: SchemeType, color_scheme: ColorScheme) {
        match scheme_type {
            SchemeType::Norm => self.norm = color_scheme,
            SchemeType::Sel => self.sel = color_scheme,
        }
    }
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
    pub func: Option<fn(&mut Jwm, *const Arg)>,
    pub arg: Arg,
}
impl Button {
    #[allow(unused)]
    pub fn new(
        click: u32,
        mask: u32,
        button: u32,
        func: Option<fn(&mut Jwm, *const Arg)>,
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
    pub func: Option<fn(&mut Jwm, *const Arg)>,
    pub arg: Arg,
}
impl Key {
    #[allow(unused)]
    pub fn new(
        mod0: u32,
        keysym: KeySym,
        func: Option<fn(&mut Jwm, *const Arg)>,
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
    pub cur_tag: usize,
    // previous tag
    pub prev_tag: usize,
    // number of windows in master area
    pub n_masters: Vec<u32>,
    // mfacts per tag
    pub m_facts: Vec<f32>,
    // selected layouts
    pub sel_lts: Vec<usize>,
    // matrix of tags and layouts indexes
    lt_idxs: Vec<Vec<Option<Rc<Layout>>>>,
    // display bar for the current tag
    pub show_bars: Vec<bool>,
    // selected client
    pub sel: Vec<Option<Rc<RefCell<Client>>>>,
}
impl Pertag {
    pub fn new() -> Self {
        Self {
            cur_tag: 0,
            prev_tag: 0,
            n_masters: vec![0; CONFIG.tags_length() + 1],
            m_facts: vec![0.; CONFIG.tags_length() + 1],
            sel_lts: vec![0; CONFIG.tags_length() + 1],
            lt_idxs: vec![vec![None; 2]; CONFIG.tags_length() + 1],
            show_bars: vec![false; CONFIG.tags_length() + 1],
            sel: vec![None; CONFIG.tags_length() + 1],
        }
    }
}

// 定义默认符号，当从 u8 或类型字符串创建 Layout 时会用到这些符号
pub const DEFAULT_TILE_SYMBOL: &'static str = "[]=";
pub const DEFAULT_FLOAT_SYMBOL: &'static str = "><>";
pub const DEFAULT_MONOCLE_SYMBOL: &'static str = "[M]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)] // 添加了 Copy 和 Eq
                                             // Debug: 允许使用 {:?} 格式化打印
                                             // Clone: 允许创建副本
                                             // Copy: 允许按值复制（因为 &'static str 是 Copy 的）
                                             // PartialEq: 允许使用 == 和 != 进行比较
                                             // Eq: PartialEq 的一个子集，要求比较是等价关系 (reflexive, symmetric, transitive)
pub enum Layout {
    Tile(&'static str),    // 平铺式布局，关联一个静态字符串作为其符号
    Float(&'static str),   // 浮动式布局
    Monocle(&'static str), // 单窗口最大化布局
}

impl Layout {
    // 获取布局实例的符号
    pub fn symbol(&self) -> &'static str {
        match self {
            Layout::Tile(symbol) |     // 使用模式匹配的 "or" ( | ) 来合并分支
            Layout::Float(symbol) |
            Layout::Monocle(symbol) => symbol, // 返回关联的 symbol
        }
    }

    // 获取布局的类型名称（小写字符串）
    pub fn layout_type(&self) -> &str {
        match self {
            Layout::Tile(_) => "tile",
            Layout::Float(_) => "float",
            Layout::Monocle(_) => "monocle",
        }
    }

    pub fn is_tile(&self) -> bool {
        if let Layout::Float(_) = self {
            false
        } else {
            true
        }
    }
}

// --- 转换 Trait 的实现 ---

// 1. 为可能失败的转换定义错误类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutConversionError {
    InvalidU8(u8),      // 表示提供的 u8 值无法转换为 Layout
    InvalidStr(String), // 表示提供的字符串无法转换为 Layout
}

// 实现 Display trait，使得错误可以被友好地打印出来
impl fmt::Display for LayoutConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutConversionError::InvalidU8(val) => write!(f, "无效的u8值用于Layout转换: {}", val),
            LayoutConversionError::InvalidStr(s) => {
                write!(f, "无效的字符串值用于Layout转换: '{}'", s)
            }
        }
    }
}

// 实现 std::error::Error trait，使得这个错误类型可以与其他标准错误处理机制集成
impl std::error::Error for LayoutConversionError {}

// 2. Layout -> u8 (将 Layout 转换为 u8)
// 由于 Layout 是 Copy 的，From<Layout> 使用起来很方便。
impl From<Layout> for u8 {
    fn from(layout: Layout) -> Self {
        match layout {
            Layout::Tile(_) => 0,    // Tile 对应 0
            Layout::Float(_) => 1,   // Float 对应 1
            Layout::Monocle(_) => 2, // Monocle 对应 2
        }
    }
}

// 为了方便，如果你有一个对 Layout 的引用 &Layout：
impl From<&Layout> for u8 {
    fn from(layout: &Layout) -> Self {
        // 因为 Layout 是 Copy 的，所以先解引用再调用已有的 From<Layout> 实现
        (*layout).into()
    }
}

// 3. u8 -> Layout (将 u8 转换为 Layout，可能失败)
impl TryFrom<u8> for Layout {
    type Error = LayoutConversionError; // 定义转换失败时的错误类型

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Layout::Tile(DEFAULT_TILE_SYMBOL)), // 0 转换为 Tile，使用默认符号
            1 => Ok(Layout::Float(DEFAULT_FLOAT_SYMBOL)), // 1 转换为 Float，使用默认符号
            2 => Ok(Layout::Monocle(DEFAULT_MONOCLE_SYMBOL)), // 2 转换为 Monocle，使用默认符号
            _ => Err(LayoutConversionError::InvalidU8(value)), // 其他 u8 值则返回错误
        }
    }
}

// 4. Layout -> String (通过 Display trait，通常表示布局的类型名称)
impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 调用 layout_type() 方法获取类型名称并写入格式化器
        write!(f, "{}", self.layout_type())
    }
}

// 5. &str -> Layout (将字符串切片转换为 Layout，可能失败，使用 FromStr trait)
// 这个实现会尝试从布局类型名称（如 "tile"）或默认符号（如 "[T]"）进行解析
impl FromStr for Layout {
    type Err = LayoutConversionError; // 定义解析失败时的错误类型

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 首先，尝试匹配已知的布局类型名称（不区分大小写）
        match s.to_lowercase().as_str() {
            // 将输入字符串转为小写进行比较
            "tile" => return Ok(Layout::Tile(DEFAULT_TILE_SYMBOL)),
            "float" => return Ok(Layout::Float(DEFAULT_FLOAT_SYMBOL)),
            "monocle" => return Ok(Layout::Monocle(DEFAULT_MONOCLE_SYMBOL)),
            _ => {} // 如果不是已知的类型名称，则继续检查符号
        }

        // 接下来，尝试匹配已知的默认符号（区分大小写）
        if s == DEFAULT_TILE_SYMBOL {
            Ok(Layout::Tile(DEFAULT_TILE_SYMBOL))
        } else if s == DEFAULT_FLOAT_SYMBOL {
            Ok(Layout::Float(DEFAULT_FLOAT_SYMBOL))
        } else if s == DEFAULT_MONOCLE_SYMBOL {
            Ok(Layout::Monocle(DEFAULT_MONOCLE_SYMBOL))
        } else {
            // 如果所有尝试都失败，则返回错误
            Err(LayoutConversionError::InvalidStr(s.to_string()))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub name: String,
    pub class: String,
    pub instance: String,
    pub min_a: f32,
    pub max_a: f32,
    pub client_fact: f32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub old_x: i32,
    pub old_y: i32,
    pub old_w: i32,
    pub old_h: i32,
    pub base_w: i32,
    pub base_h: i32,
    pub inc_w: i32,
    pub inc_h: i32,
    pub max_w: i32,
    pub max_h: i32,
    pub min_w: i32,
    pub min_h: i32,
    pub hints_valid: bool,
    pub border_w: i32,
    pub old_border_w: i32,
    pub tags: u32,
    pub is_fixed: bool,
    pub is_floating: bool,
    pub is_urgent: bool,
    pub never_focus: bool,
    pub old_state: bool,
    pub is_fullscreen: bool,
    pub next: Option<Rc<RefCell<Client>>>,
    pub stack_next: Option<Rc<RefCell<Client>>>,
    pub mon: Option<Rc<RefCell<Monitor>>>,
    pub win: Window,
}
impl fmt::Display for Client {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Client {{ name: {}, class: {}, instance: {} min_a: {}, max_a: {}, cleint_fact: {}, x: {}, y: {}, w: {}, h: {}, old_x: {}, old_y: {}, old_w: {}, old_h: {}, base_w: {}, base_h: {}, inc_w: {}, inc_h: {}, max_w: {}, max_h: {}, min_w: {}, min_h: {}, hints_valid: {}, border_w: {}, old_border_w: {}, tags: {}, is_fixed: {}, is_floating: {}, is_urgent: {}, never_focus: {}, old_state: {}, is_fullscreen: {}, win: {} }}",
    self.name,
    self.class,
    self.instance,
    self.min_a,
    self.max_a,
    self.client_fact,
    self.x,
    self.y,
    self.w,
    self.h,
    self.old_x,
    self.old_y,
    self.old_w,
    self.old_h,
    self.base_w,
    self.base_h,
    self.inc_w,
    self.inc_h,
    self.max_w,
    self.max_h,
    self.min_w,
    self.min_h,
    self.hints_valid,
    self.border_w,
    self.old_border_w,
    self.tags,
    self.is_fixed,
    self.is_floating,
    self.is_urgent,
    self.never_focus,
    self.old_state,
    self.is_fullscreen,
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
            min_a: 0.,
            max_a: 0.,
            client_fact: 0.,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            old_x: 0,
            old_y: 0,
            old_w: 0,
            old_h: 0,
            base_w: 0,
            base_h: 0,
            inc_w: 0,
            inc_h: 0,
            max_w: 0,
            max_h: 0,
            min_w: 0,
            min_h: 0,
            hints_valid: false,
            border_w: 0,
            old_border_w: 0,
            tags: 0,
            is_fixed: false,
            is_floating: false,
            is_urgent: false,
            never_focus: false,
            old_state: false,
            is_fullscreen: false,
            next: None,
            stack_next: None,
            mon: None,
            win: 0,
        }
    }

    pub fn isvisible(&self) -> bool {
        // info!("[ISVISIBLE]");
        let mon_rc = match self.mon {
            Some(ref m) => m,
            _ => return false,
        };
        let mon_borrow = mon_rc.borrow();
        (self.tags & mon_borrow.tag_set[mon_borrow.sel_tags]) > 0
    }

    pub fn width(&self) -> i32 {
        self.w + 2 * self.border_w
    }

    pub fn height(&self) -> i32 {
        self.h + 2 * self.border_w
    }

    pub fn is_status_bar(&self) -> bool {
        return ((self.name == CONFIG.status_bar_name())
            || (self.name == CONFIG.status_bar_0())
            || (self.name == CONFIG.status_bar_1()))
            && ((self.class == CONFIG.status_bar_0() && self.instance == CONFIG.status_bar_0())
                || (self.class == CONFIG.status_bar_1()
                    && self.instance == CONFIG.status_bar_1()));
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub lt_symbol: String,
    pub m_fact: f32,
    pub n_master: u32,
    pub num: i32,
    pub m_x: i32,
    pub m_y: i32,
    pub m_w: i32,
    pub m_h: i32,
    pub w_x: i32,
    pub w_y: i32,
    pub w_w: i32,
    pub w_h: i32,
    pub sel_tags: usize,
    pub sel_lt: usize,
    pub tag_set: [u32; 2],
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
            lt_symbol: String::new(),
            m_fact: 0.0,
            n_master: 0,
            num: 0,
            m_x: 0,
            m_y: 0,
            m_w: 0,
            m_h: 0,
            w_x: 0,
            w_y: 0,
            w_w: 0,
            w_h: 0,
            sel_tags: 0,
            sel_lt: 0,
            tag_set: [0; 2],
            clients: None,
            sel: None,
            stack: None,
            next: None,
            lt: [
                Rc::new(Layout::try_from(0).unwrap()),
                Rc::new(Layout::try_from(0).unwrap()),
            ],
            pertag: None,
        }
    }
    pub fn intersect(&self, x: i32, y: i32, w: i32, h: i32) -> i32 {
        max(0, min(x + w, self.w_x + self.w_w) - max(x, self.w_x))
            * max(0, min(y + h, self.w_y + self.w_h) - max(y, self.w_y))
    }
}
impl fmt::Display for Monitor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Monitor {{ ltsymbol: {}, m_fact: {}, n_master: {}, num: {}, m_x: {}, m_y: {}, m_w: {}, m_h: {}, wx: {}, w_y: {}, w_w: {}, w_h: {}, sel_tags: {}, sel_lt: {}, tag_set: [{}, {}], }}",
               self.lt_symbol,
               self.m_fact,
               self.n_master,
               self.num,
               self.m_x,
               self.m_y,
               self.m_w,
               self.m_h,
               self.w_x,
               self.w_y,
               self.w_w,
               self.w_h,
               self.sel_tags,
               self.sel_lt,
               self.tag_set[0],
               self.tag_set[1],
        )
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub class: String,
    pub instance: String,
    pub name: String,
    pub tags: usize,
    pub is_floating: bool,
    pub monitor: i32,
}
impl Rule {
    #[allow(unused)]
    pub fn new(
        class: String,
        instance: String,
        name: String,
        tags: usize,
        is_floating: bool,
        monitor: i32,
    ) -> Self {
        Rule {
            class,
            instance,
            name,
            tags,
            is_floating,
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

pub struct Jwm {
    pub stext_max_len: usize,
    pub screen: i32,
    pub s_w: i32,
    pub s_h: i32,
    pub numlock_mask: u32,
    pub wm_atom: [Atom; WM::WMLast as usize],
    pub net_atom: [Atom; NET::NetLast as usize],
    pub running: AtomicBool,
    pub cursor: [Option<Box<Cur>>; CUR::CurLast as usize],
    pub theme_manager: ThemeManager,
    pub dpy: *mut Display,
    pub drw: Option<Box<Drw>>,
    pub mons: Option<Rc<RefCell<Monitor>>>,
    pub motion_mon: Option<Rc<RefCell<Monitor>>>,
    pub sel_mon: Option<Rc<RefCell<Monitor>>>,
    pub root: Window,
    pub wm_check_win: Window,
    pub visual: *mut Visual,
    pub depth: i32,
    pub color_map: Colormap,
    pub sender: Sender<u8>,
    pub status_bar_shmem: HashMap<i32, SharedRingBuffer>,
    pub status_bar_child: HashMap<i32, Child>,
    pub message: SharedMessage,
    pub show_bar: bool,

    // 状态栏专用管理
    pub status_bar_clients: HashMap<i32, Rc<RefCell<Client>>>, // monitor_id -> statusbar_client
    pub status_bar_windows: HashMap<Window, i32>,              // window_id -> monitor_id (快速查找)

    pub pending_bar_updates: HashSet<i32>,
}

impl Jwm {
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
        let theme_manager = ThemeManager::new(
            ColorScheme::new(
                Drw::drw_clr_create_from_hex(
                    &CONFIG.colors().dark_sea_green1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Drw::drw_clr_create_from_hex(
                    &CONFIG.colors().light_sky_blue1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Drw::drw_clr_create_from_hex(
                    &CONFIG.colors().light_sky_blue1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
            ),
            ColorScheme::new(
                Drw::drw_clr_create_from_hex(
                    &CONFIG.colors().dark_sea_green2,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Drw::drw_clr_create_from_hex(
                    &CONFIG.colors().pale_turquoise1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Drw::drw_clr_create_from_hex(&CONFIG.colors().cyan, CONFIG.colors().opaque)
                    .unwrap(),
            ),
        );
        Jwm {
            stext_max_len: 512,
            screen: 0,
            s_w: 0,
            s_h: 0,
            numlock_mask: 0,
            wm_atom: [0; WM::WMLast as usize],
            net_atom: [0; NET::NetLast as usize],
            running: AtomicBool::new(true),
            cursor: [const { None }; CUR::CurLast as usize],
            theme_manager,
            dpy: null_mut(),
            drw: None,
            mons: None,
            motion_mon: None,
            sel_mon: None,
            root: 0,
            wm_check_win: 0,
            visual: null_mut(),
            depth: 0,
            color_map: 0,
            sender,
            show_bar: true,
            status_bar_shmem: HashMap::new(),
            status_bar_child: HashMap::new(),
            message: SharedMessage::default(),
            status_bar_clients: HashMap::new(),
            status_bar_windows: HashMap::new(),
            pending_bar_updates: HashSet::new(),
        }
    }

    fn mark_bar_update_needed(&mut self, monitor_id: Option<i32>) {
        if let Some(id) = monitor_id {
            self.pending_bar_updates.insert(id);
        } else {
            // 如果没有指定monitor，标记所有monitor
            let mut m = self.mons.clone();
            while let Some(ref m_opt) = m {
                self.pending_bar_updates.insert(m_opt.borrow().num);
                let next = m_opt.borrow().next.clone();
                m = next;
            }
        }
    }

    fn are_equal_rc<T>(a: &Option<Rc<RefCell<T>>>, b: &Option<Rc<RefCell<T>>>) -> bool {
        match (a, b) {
            (Some(rc_a), Some(rc_b)) => Rc::ptr_eq(rc_a, rc_b),
            _ => false,
        }
    }

    fn CLEANMASK(&self, mask: u32) -> u32 {
        mask & !(self.numlock_mask | LockMask)
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
            c.is_floating = false;
            let mut ch: XClassHint = zeroed();
            XGetClassHint(self.dpy, c.win, &mut ch);
            c.class = if !ch.res_class.is_null() {
                let c_str = CStr::from_ptr(ch.res_class);
                c_str.to_str().unwrap_or_default().to_string()
            } else {
                String::new()
            };
            c.instance = if !ch.res_name.is_null() {
                let c_str = CStr::from_ptr(ch.res_name);
                c_str.to_str().unwrap_or_default().to_string()
            } else {
                String::new()
            };

            for r in &CONFIG.get_rules() {
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
                    c.is_floating = r.is_floating;
                    c.tags |= r.tags as u32;
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
            let condition = c.tags & CONFIG.tagmask();
            c.tags = if condition > 0 {
                condition
            } else {
                let sel_tags = c.mon.as_ref().unwrap().borrow().sel_tags;
                c.mon.as_ref().unwrap().borrow().tag_set[sel_tags]
            };
            info!(
                "[applyrules] class: {}, instance: {}, name: {}, tags: {}",
                c.class, c.instance, c.name, c.tags
            );
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
                c.base_w = size.base_width;
                c.base_h = size.base_height;
            } else if size.flags & PMinSize > 0 {
                c.base_w = size.min_width;
                c.base_h = size.min_height;
            } else {
                c.base_w = 0;
                c.base_h = 0;
            }
            if size.flags & PResizeInc > 0 {
                c.inc_w = size.width_inc;
                c.inc_h = size.height_inc;
            } else {
                c.inc_w = 0;
                c.inc_h = 0;
            }
            if size.flags & PMaxSize > 0 {
                c.max_w = size.max_width;
                c.max_h = size.max_height;
            } else {
                c.max_w = 0;
                c.max_h = 0;
            }
            if size.flags & PMinSize > 0 {
                c.min_w = size.min_width;
                c.min_h = size.min_height;
            } else if size.flags & PBaseSize > 0 {
                c.min_w = size.base_width;
                c.min_h = size.base_height;
            } else {
                c.min_w = 0;
                c.min_h = 0;
            }
            if size.flags & PAspect > 0 {
                c.min_a = size.min_aspect.y as f32 / size.min_aspect.x as f32;
                c.max_a = size.max_aspect.x as f32 / size.max_aspect.y as f32;
            } else {
                c.max_a = 0.;
                c.min_a = 0.;
            }
            c.is_fixed =
                (c.max_w > 0) && (c.max_h > 0) && (c.max_w == c.min_w) && (c.max_h == c.min_h);
            c.hints_valid = true;
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
            let client_total_width = *w + 2 * cc.border_w; // Use desired w for this check
            let client_total_height = *h + 2 * cc.border_w; // Use desired h for this check

            if *x > self.s_w {
                // Off right edge
                *x = self.s_w - client_total_width;
            }
            if *y > self.s_h {
                // Off bottom edge
                *y = self.s_h - client_total_height;
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
            let wx = mon_borrow.w_x;
            let wy = mon_borrow.w_y;
            let ww = mon_borrow.w_w;
            let wh = mon_borrow.w_h;
            let client_total_width = *w + 2 * cc.border_w; // Use desired w
            let client_total_height = *h + 2 * cc.border_w; // Use desired h

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

        let is_floating = { c.as_ref().borrow().is_floating };

        if CONFIG.behavior().resize_hints || is_floating {
            if !c.as_ref().borrow().hints_valid {
                // Check immutable borrow first
                self.updatesizehints(c); // This will mutably borrow internally
            }

            let cc = c.as_ref().borrow(); // Re-borrow (immutable) after potential updatesizehints

            // Adjust w and h for base dimensions and increments
            // These are client area dimensions (without border)
            let mut current_w = *w;
            let mut current_h = *h;

            // 1. Subtract base size to get the dimensions that increments apply to.
            current_w -= cc.base_w;
            current_h -= cc.base_h;

            // 2. Apply resize increments.
            if cc.inc_w > 0 {
                current_w -= current_w % cc.inc_w;
            }
            if cc.inc_h > 0 {
                current_h -= current_h % cc.inc_h;
            }

            // 3. Add base size back before aspect ratio and min/max checks.
            current_w += cc.base_w;
            current_h += cc.base_h;

            // 4. Apply aspect ratio limits.
            // cc.mina is min_aspect.y / min_aspect.x (target H/W)
            // cc.maxa is max_aspect.x / max_aspect.y (target W/H)
            if cc.min_a > 0.0 && cc.max_a > 0.0 {
                if cc.max_a < current_w as f32 / current_h as f32 {
                    // Too wide (current W/H > max W/H) -> Adjust W
                    current_w = (current_h as f32 * cc.max_a + 0.5) as i32;
                } else if current_h as f32 / current_w as f32 > cc.min_a {
                    // Too tall (current H/W > min H/W) -> Adjust H
                    current_h = (current_w as f32 * cc.min_a + 0.5) as i32;
                }
            }

            // 5. Enforce min and max dimensions.
            // Ensure client area is not smaller than min_width/height.
            current_w = current_w.max(cc.min_w);
            current_h = current_h.max(cc.min_h);

            // Ensure client area is not larger than max_width/height if specified.
            if cc.max_w > 0 {
                current_w = current_w.min(cc.max_w);
            }
            if cc.max_h > 0 {
                current_h = current_h.min(cc.max_h);
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
        // 清理状态栏
        let statusbar_monitor_ids: Vec<i32> = self.status_bar_clients.keys().cloned().collect();
        for monitor_id in statusbar_monitor_ids {
            self.unmanage_statusbar(monitor_id, false);
        }
        // 常规清理逻辑
        drop(self.sender.clone());
        let mut a: Arg = Arg::Ui(!0);
        unsafe {
            self.view(&mut a);
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
            XDestroyWindow(self.dpy, self.wm_check_win);
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
                self.net_atom[NET::NetActiveWindow as usize],
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
                if Jwm::are_equal_rc(&m_opt.borrow_mut().next, &mon) {
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
            if cme.message_type == self.net_atom[NET::NetWMState as usize] {
                if cme.data.get_long(1) == self.net_atom[NET::NetWMFullscreen as usize] as i64
                    || cme.data.get_long(2) == self.net_atom[NET::NetWMFullscreen as usize] as i64
                {
                    // NET_WM_STATE_ADD
                    // NET_WM_STATE_TOGGLE
                    let isfullscreen = { c.borrow_mut().is_fullscreen };
                    let fullscreen =
                        cme.data.get_long(0) == 1 || (cme.data.get_long(0) == 2 && !isfullscreen);
                    self.setfullscreen(c, fullscreen);
                }
            } else if cme.message_type == self.net_atom[NET::NetActiveWindow as usize] {
                let is_urgent = { c.borrow_mut().is_urgent };
                let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
                if !Self::are_equal_rc(&Some(c.clone()), &sel) && !is_urgent {
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
                let dirty = self.s_w != ev.width || self.s_h != ev.height;
                self.s_w = ev.width;
                self.s_h = ev.height;
                if self.updategeom() || dirty {
                    let mut m = self.mons.clone();
                    while let Some(ref m_opt) = m {
                        let mut c = m_opt.borrow_mut().clients.clone();
                        while c.is_some() {
                            if c.as_ref().unwrap().borrow_mut().is_fullscreen {
                                self.resizeclient(
                                    &mut *c.as_ref().unwrap().borrow_mut(),
                                    m_opt.borrow_mut().m_x,
                                    m_opt.borrow_mut().m_y,
                                    m_opt.borrow_mut().m_w,
                                    m_opt.borrow_mut().m_h,
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

    pub fn configure(&self, c: &mut Client) {
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
            ce.border_width = c.border_w;
            ce.above = 0;
            ce.override_redirect = 0;
            let mut xe = XEvent { configure: ce };
            XSendEvent(self.dpy, c.win, 0, StructureNotifyMask, &mut xe);
        }
    }

    pub fn setfullscreen(&mut self, c: &Rc<RefCell<Client>>, fullscreen: bool) {
        info!("[setfullscreen]");
        unsafe {
            let isfullscreen = { c.borrow_mut().is_fullscreen };
            let win = { c.borrow_mut().win };
            if fullscreen && !isfullscreen {
                XChangeProperty(
                    self.dpy,
                    win,
                    self.net_atom[NET::NetWMState as usize],
                    XA_ATOM,
                    32,
                    PropModeReplace,
                    self.net_atom.as_ptr().add(NET::NetWMFullscreen as usize) as *const _,
                    1,
                );
                {
                    let mut c = c.borrow_mut();
                    c.is_fullscreen = true;
                    c.old_state = c.is_floating;
                    c.old_border_w = c.border_w;
                    c.border_w = 0;
                    c.is_floating = true;
                }
                let (mx, my, mw, mh) = {
                    let c_mon = &c.borrow().mon;
                    let mon_mut = c_mon.as_ref().unwrap().borrow();
                    (mon_mut.m_x, mon_mut.m_y, mon_mut.m_w, mon_mut.m_h)
                };
                self.resizeclient(&mut *c.borrow_mut(), mx, my, mw, mh);
                // Raise the window to the top of the stacking order
                XRaiseWindow(self.dpy, win);
            } else if !fullscreen && isfullscreen {
                XChangeProperty(
                    self.dpy,
                    win,
                    self.net_atom[NET::NetWMState as usize],
                    XA_ATOM,
                    32,
                    PropModeReplace,
                    null(),
                    0,
                );
                {
                    let mut c = c.borrow_mut();
                    c.is_fullscreen = false;
                    c.is_floating = c.old_state;
                    c.border_w = c.old_border_w;
                    c.x = c.old_x;
                    c.y = c.old_y;
                    // println!("line: {}, {}", line!(), c.y);
                    c.w = c.old_w;
                    c.h = c.old_h;
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
            c.old_x = c.x;
            c.x = x;
            wc.x = x;
            c.old_y = c.y;
            c.y = y;
            // println!("line: {}, {}", line!(), c.y);
            wc.y = y;
            c.old_w = c.w;
            c.w = w;
            wc.width = w;
            c.old_h = c.h;
            c.h = h;
            wc.height = h;
            wc.border_width = c.border_w;
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

    pub fn seturgent(&mut self, c_rc: &Rc<RefCell<Client>>, urg: bool) {
        // info!("[seturgent]");
        unsafe {
            c_rc.borrow_mut().is_urgent = urg;
            let win = c_rc.borrow().win;
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

    pub fn showhide(&mut self, client_opt: Option<Rc<RefCell<Client>>>) {
        // info!("[showhide]");
        let client_rc = match client_opt {
            Some(c) => c,
            None => return,
        };
        unsafe {
            let isvisible = {
                let client_borrow = client_rc.borrow();
                client_borrow.isvisible()
            };
            if isvisible {
                // show clients top down.
                let is_floating;
                let is_fullscreen;
                {
                    let client_borrow = client_rc.borrow();
                    XMoveWindow(
                        self.dpy,
                        client_borrow.win,
                        client_borrow.x,
                        client_borrow.y,
                    );
                    is_floating = client_borrow.is_floating;
                    is_fullscreen = client_borrow.is_fullscreen;
                }
                {
                    if is_floating && !is_fullscreen {
                        let (x, y, w, h) = {
                            let client_borrow = client_rc.borrow();
                            (
                                client_borrow.x,
                                client_borrow.y,
                                client_borrow.w,
                                client_borrow.h,
                            )
                        };
                        self.resize(&client_rc, x, y, w, h, false);
                    }
                }
                let snext = {
                    let client_borrow = client_rc.borrow();
                    client_borrow.stack_next.clone()
                };
                self.showhide(snext);
            } else {
                // hide clients bottom up.
                let snext = {
                    let client_borrow = client_rc.borrow();
                    client_borrow.stack_next.clone()
                };
                self.showhide(snext);
                let client_borrow = client_rc.borrow();
                XMoveWindow(
                    self.dpy,
                    client_borrow.win,
                    client_borrow.width() * -2,
                    client_borrow.y,
                );
            }
        }
    }

    pub fn configurerequest(&mut self, e: *mut XEvent) {
        unsafe {
            let ev = (*e).configure_request;
            let c = self.wintoclient(ev.window);

            if let Some(ref client_rc) = c {
                // 检查是否是状态栏
                if let Some(&monitor_id) = self.status_bar_windows.get(&ev.window) {
                    info!("[configurerequest] statusbar on monitor {}", monitor_id);
                    self.handle_statusbar_configure_request(monitor_id, &ev);
                    // let mut wc: XWindowChanges = std::mem::zeroed();
                    // wc.x = ev.x;
                    // wc.y = ev.y;
                    // wc.width = ev.width;
                    // wc.height = ev.height;
                    // XConfigureWindow(self.dpy, ev.window, ev.value_mask as u32, &mut wc);
                } else {
                    // 常规客户端的配置请求处理
                    self.handle_regular_configure_request(&client_rc, &ev);
                }
            } else {
                // 未管理的窗口
                let mut wc: XWindowChanges = std::mem::zeroed();
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

    fn handle_statusbar_configure_request(&mut self, monitor_id: i32, ev: &XConfigureRequestEvent) {
        info!(
        "[handle_statusbar_configure_request] StatusBar resize request for monitor {}: {}x{}+{}+{} (mask: {:b})",
        monitor_id, ev.width, ev.height, ev.x, ev.y, ev.value_mask
    );

        if let Some(statusbar) = self.status_bar_clients.get(&monitor_id) {
            let mut statusbar_mut = statusbar.borrow_mut();
            let old_geometry = (
                statusbar_mut.x,
                statusbar_mut.y,
                statusbar_mut.w,
                statusbar_mut.h,
            );
            let mut geometry_changed = false;
            let mut needs_workarea_update = false;

            // 被动接受 status bar 的大小变化请求，不做任何限制或修正
            if ev.value_mask & CWX as u64 > 0 {
                statusbar_mut.x = ev.x;
                geometry_changed = true;
            }
            if ev.value_mask & CWY as u64 > 0 {
                statusbar_mut.y = ev.y;
                geometry_changed = true;
                needs_workarea_update = true; // Y 位置变化影响工作区
            }
            if ev.value_mask & CWWidth as u64 > 0 {
                statusbar_mut.w = ev.width;
                geometry_changed = true;
            }
            if ev.value_mask & CWHeight as u64 > 0 {
                statusbar_mut.h = ev.height;
                geometry_changed = true;
                needs_workarea_update = true; // 高度变化是最主要的关注点
            }

            if geometry_changed {
                info!(
                "[handle_statusbar_configure_request] StatusBar geometry updated: {:?} -> ({}, {}, {}, {})",
                old_geometry, statusbar_mut.x, statusbar_mut.y, statusbar_mut.w, statusbar_mut.h
            );

                // 只是同意 status bar 的请求，允许它按照请求的大小进行配置
                unsafe {
                    let mut wc: XWindowChanges = std::mem::zeroed();
                    wc.x = statusbar_mut.x;
                    wc.y = statusbar_mut.y;
                    wc.width = statusbar_mut.w;
                    wc.height = statusbar_mut.h;

                    // 应用 status bar 请求的配置
                    XConfigureWindow(self.dpy, ev.window, ev.value_mask as u32, &mut wc);

                    // 确保状态栏始终在最上层
                    XRaiseWindow(self.dpy, statusbar_mut.win);
                }

                // 发送确认配置事件给 status bar
                self.configure(&mut statusbar_mut);
            }

            drop(statusbar_mut); // 释放借用

            // 重要：当状态栏大小变化时，需要更新工作区域并重新排列其他窗口
            if needs_workarea_update {
                info!("[handle_statusbar_configure_request] Updating workarea due to statusbar geometry change");

                // 重新排列该显示器上的其他客户端
                if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
                    self.arrange(Some(monitor));
                }
            }
        } else {
            error!(
                "[handle_statusbar_configure_request] StatusBar not found for monitor {}",
                monitor_id
            );

            // 作为后备，直接应用配置请求
            unsafe {
                let mut wc: XWindowChanges = std::mem::zeroed();
                wc.x = ev.x;
                wc.y = ev.y;
                wc.width = ev.width;
                wc.height = ev.height;
                XConfigureWindow(self.dpy, ev.window, ev.value_mask as u32, &mut wc);
            }
        }
    }

    fn handle_regular_configure_request(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        ev: &XConfigureRequestEvent,
    ) {
        unsafe {
            let mut client_mut = client_rc.borrow_mut();
            let is_floating = client_mut.is_floating;

            if ev.value_mask & CWBorderWidth as u64 > 0 {
                client_mut.border_w = ev.border_width;
            } else if is_floating {
                // 浮动窗口或无布局时，允许自由调整
                let (mx, my, mw, mh) = {
                    let m = client_mut.mon.as_ref().unwrap().borrow();
                    (m.m_x, m.m_y, m.m_w, m.m_h)
                };

                if ev.value_mask & CWX as u64 > 0 {
                    client_mut.old_x = client_mut.x;
                    client_mut.x = mx + ev.x;
                }
                if ev.value_mask & CWY as u64 > 0 {
                    client_mut.old_y = client_mut.y;
                    client_mut.y = my + ev.y;
                }
                if ev.value_mask & CWWidth as u64 > 0 {
                    client_mut.old_w = client_mut.w;
                    client_mut.w = ev.width;
                }
                if ev.value_mask & CWHeight as u64 > 0 {
                    client_mut.old_h = client_mut.h;
                    client_mut.h = ev.height;
                }

                // 确保窗口不超出显示器边界
                if (client_mut.x + client_mut.w) > mx + mw && client_mut.is_floating {
                    client_mut.x = mx + (mw / 2 - client_mut.width() / 2);
                }
                if (client_mut.y + client_mut.h) > my + mh && client_mut.is_floating {
                    client_mut.y = my + (mh / 2 - client_mut.height() / 2);
                }

                if (ev.value_mask & (CWX | CWY) as u64) > 0
                    && (ev.value_mask & (CWWidth | CWHeight) as u64) <= 0
                {
                    self.configure(&mut client_mut);
                }

                let isvisible = client_mut.isvisible();
                if isvisible {
                    XMoveResizeWindow(
                        self.dpy,
                        client_mut.win,
                        client_mut.x,
                        client_mut.y,
                        client_mut.w as u32,
                        client_mut.h as u32,
                    );
                }
            } else {
                // 平铺布局中的窗口，只允许有限的配置更改
                self.configure(&mut client_mut);
            }
        }
    }

    pub fn createmon(&mut self) -> Monitor {
        // info!("[createmon]");
        let mut m: Monitor = Monitor::new();
        m.tag_set[0] = 1;
        m.tag_set[1] = 1;
        m.m_fact = CONFIG.m_fact();
        m.n_master = CONFIG.n_master();
        m.lt[0] = Rc::new(Layout::try_from(0).unwrap()).clone();
        m.lt[1] = Rc::new(Layout::try_from(1).unwrap()).clone();
        m.lt_symbol = m.lt[0].symbol().to_string();
        m.pertag = Some(Pertag::new());
        let ref_pertag = m.pertag.as_mut().unwrap();
        ref_pertag.cur_tag = 1;
        ref_pertag.prev_tag = 1;
        let default_layout_0 = m.lt[0].clone();
        let default_layout_1 = m.lt[1].clone();
        for i in 0..=CONFIG.tags_length() {
            ref_pertag.n_masters[i] = m.n_master;
            ref_pertag.m_facts[i] = m.m_fact;

            ref_pertag.lt_idxs[i][0] = Some(default_layout_0.clone());
            ref_pertag.lt_idxs[i][1] = Some(default_layout_1.clone());
            ref_pertag.sel_lts[i] = m.sel_lt;
        }
        info!("[createmon]: {}", m);
        return m;
    }

    pub fn destroynotify(&mut self, e: *mut XEvent) {
        // info!("[destroynotify]");
        unsafe {
            let ev = (*e).destroy_window;
            let c = self.wintoclient(ev.window);
            if let Some(client_opt) = c {
                self.unmanage(Some(client_opt), true);
            }
        }
    }

    pub fn applylayout(&mut self, layout: &Layout, mon_rc: &Rc<RefCell<Monitor>>) {
        match layout {
            Layout::Tile(_) => {
                self.tile(mon_rc);
            }
            Layout::Float(_) => {}
            Layout::Monocle(_) => {
                self.monocle(mon_rc);
            }
        }
    }

    pub fn arrangemon(&mut self, m: &Rc<RefCell<Monitor>>) {
        info!("[arrangemon]");
        let layout;
        {
            let mut mm = m.borrow_mut();
            let sel_lt = (mm).sel_lt;
            mm.lt_symbol = (mm).lt[sel_lt].symbol().to_string();
            info!(
                "[arrangemon] sel_lt: {}, ltsymbol: {:?}",
                sel_lt, mm.lt_symbol
            );
            layout = mm.lt[sel_lt].clone();
        }
        self.applylayout(&layout, m);
    }

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
            |cli| cli.stack_next.clone(),
            |cli, next| cli.stack_next = next,
        );

        if Self::are_equal_rc(&Some(c), &m.borrow().sel) {
            let mut t = { m.borrow().stack.clone() };
            while let Some(ref t_opt) = t {
                let isvisible = { t_opt.borrow_mut().isvisible() };
                if isvisible {
                    break;
                }
                let snext = { t_opt.borrow_mut().stack_next.clone() };
                t = snext;
            }
            m.borrow_mut().sel = t.clone();
        }
    }

    pub fn dirtomon(&mut self, dir: i32) -> Option<Rc<RefCell<Monitor>>> {
        let selected_monitor = self.sel_mon.as_ref()?; // Return None if selmon is None
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
        if let Some(ring_buffer) = self.status_bar_shmem.get_mut(&num) {
            // Assuming get_mut
            match ring_buffer.try_write_message(&message) {
                Ok(true) => {
                    if let Some(statusbar) = self.status_bar_clients.get(&num) {
                        info!("statusbar: {}", statusbar.borrow());
                    }
                    // info!("[write_message] {:?}", message);
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

    fn monitor_to_bar_name(num: i32) -> String {
        match num {
            0 => CONFIG.status_bar_0().to_string(),
            1 => CONFIG.status_bar_1().to_string(),
            _ => String::new(),
        }
    }

    fn ensure_bar_is_running(&mut self, num: i32, shared_path: &str) {
        let mut needs_spawn = true; // 默认需要启动
        if let Some(child) = self.status_bar_child.get_mut(&num) {
            match child.try_wait() {
                // Ok(None) 表示子进程仍在运行
                Ok(None) => {
                    debug!(" checked: status bar for monitor {} is still running.", num);
                    needs_spawn = false; // 不需要启动
                }
                // Ok(Some(status)) 表示子进程已退出
                Ok(Some(status)) => {
                    error!(
                        " checked: status bar for monitor {} has exited with status: {}. Restarting...",
                        num, status
                    );
                    // needs_spawn 保持为 true
                }
                // 检查时发生 I/O 错误
                Err(e) => {
                    error!(
                        " error: Failed to check status of status bar for monitor {}: {}. Assuming it's dead and restarting...",
                        num, e
                    );
                    // needs_spawn 保持为 true
                }
            }
        } else {
            // 哈希表中不存在记录，是第一次启动
            info!(
                "- first time: Spawning status bar for monitor {} for the first time.",
                num
            );
            // needs_spawn 保持为 true
        }
        // --- 执行操作 ---
        // 如果需要启动（无论是第一次还是重启）
        if needs_spawn {
            let mut command: Command;
            // --- 使用 #[cfg] 进行条件编译 ---
            // 这段代码只有在编译时启用了 nixgl feature 时才会存在
            #[cfg(feature = "nixgl")]
            {
                // Hack for nixgl.
                let mut not_fully_initialized = false;
                for (&tmp_num, _) in self.status_bar_child.iter() {
                    if !self.status_bar_clients.contains_key(&tmp_num) {
                        not_fully_initialized = true;
                        break;
                    }
                }
                if not_fully_initialized {
                    return;
                }

                let nixgl_command = "nixGL".to_string();
                info!(
                    "   -> [feature=nixgl] enabled. Launching status bar with '{}'.",
                    nixgl_command
                );
                command = Command::new(&nixgl_command);
                command.arg(CONFIG.status_bar_name());
            }
            // 这段代码只有在编译时 *没有* 启用 nixgl feature 时才会存在
            #[cfg(not(feature = "nixgl"))]
            {
                info!("   -> [feature=nixgl] disabled. Launching status bar directly.");
                command = Command::new(CONFIG.status_bar_name());
            }
            if let Ok(child) = command
                .arg0(&Self::monitor_to_bar_name(num))
                .arg(shared_path)
                .spawn()
            {
                // insert 会自动处理新增和覆盖两种情况
                self.status_bar_child.insert(num, child);
                info!(
                    "   -> spawned: Successfully started/restarted status bar for monitor {}.",
                    num
                );
            }
        }
    }

    pub fn UpdateBarMessage(&mut self, m: Option<Rc<RefCell<Monitor>>>) {
        self.update_bar_message_for_monitor(m);
        let num = self.message.monitor_info.monitor_num;

        let shared_path = format!("/dev/shm/monitor_{}", num);
        if !self.status_bar_shmem.contains_key(&num) {
            let ring_buffer = match SharedRingBuffer::open(&shared_path, None) {
                Ok(rb) => rb,
                Err(_) => {
                    println!("创建新的共享环形缓冲区");
                    SharedRingBuffer::create(&shared_path, None, None).unwrap()
                }
            };
            self.status_bar_shmem.insert(num, ring_buffer);
        }
        self.ensure_bar_is_running(num, shared_path.as_str());

        // info!("[drawbar] num: {}", num);
        // info!("[drawbar] message: {:?}", self.message);
        let _ = self.write_message(num, &self.message.clone());
    }

    pub fn restack(&mut self, mon_rc_opt: Option<Rc<RefCell<Monitor>>>) {
        info!("[restack]");
        let mon_rc = match mon_rc_opt {
            Some(monitor) => monitor,
            None => return,
        };
        self.mark_bar_update_needed(Some(mon_rc.borrow().num));

        unsafe {
            let mon_borrow = mon_rc.borrow();
            let mut wc: XWindowChanges = zeroed();
            let sel = mon_borrow.sel.clone();
            if sel.is_none() {
                return;
            }
            let is_floating = sel.as_ref().unwrap().borrow_mut().is_floating;
            if is_floating {
                let win = sel.as_ref().unwrap().borrow_mut().win;
                XRaiseWindow(self.dpy, win);
            }
            // 确保状态栏始终在最上方
            let monitor_id = mon_borrow.num;
            if let Some(statusbar) = self.status_bar_clients.get(&monitor_id) {
                XRaiseWindow(self.dpy, statusbar.borrow().win);
            }
            wc.stack_mode = Below;
            let mut client_rc_opt = mon_borrow.stack.clone();
            while let Some(ref client_rc) = client_rc_opt.clone() {
                let client_borrow = client_rc.borrow();
                let is_floating = client_borrow.is_floating;
                let isvisible = client_borrow.isvisible();
                if !is_floating && isvisible {
                    let win = client_borrow.win;
                    XConfigureWindow(self.dpy, win, (CWSibling | CWStackMode) as u32, &mut wc);
                    wc.sibling = win;
                }
                let next = client_borrow.stack_next.clone();
                client_rc_opt = next;
            }
            XSync(self.dpy, 0);
            let mut ev: XEvent = zeroed();
            while XCheckMaskEvent(self.dpy, EnterWindowMask, &mut ev) > 0 {}
        }
    }

    fn flush_pending_bar_updates(&mut self) {
        if self.pending_bar_updates.is_empty() {
            return;
        }
        info!(
            "[flush_pending_bar_updates] Updating {} monitors",
            self.pending_bar_updates.len()
        );
        for monitor_id in self.pending_bar_updates.clone() {
            if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
                self.UpdateBarMessage(Some(monitor));
            }
        }

        self.pending_bar_updates.clear();
    }

    pub fn run(&mut self) {
        unsafe {
            let mut ev: XEvent = zeroed();
            XSync(self.dpy, False);
            let x11_fd = XConnectionNumber(self.dpy);
            let mut i: u64 = 0;
            info!("Starting event loop with X11 fd: {}", x11_fd);
            while self.running.load(Ordering::SeqCst) {
                let mut events_processed = false;
                // 处理所有挂起的X11事件
                while XPending(self.dpy) > 0 {
                    XNextEvent(self.dpy, &mut ev);
                    i = i.wrapping_add(1);
                    self.handler(ev.type_, &mut ev);
                    events_processed = true;
                }

                // 处理来自status bar的命令
                self.process_commands_from_status_bar();

                // ✨ 在事件循环结束后，批量更新状态栏
                if events_processed || !self.pending_bar_updates.is_empty() {
                    self.flush_pending_bar_updates();
                }

                // 设置select参数
                let mut read_fds: fd_set = std::mem::zeroed();
                FD_ZERO(&mut read_fds);
                FD_SET(x11_fd, &mut read_fds);

                let mut timeout = timeval {
                    tv_sec: 0,

                    tv_usec: 10000, // 10.000ms for ~100 FPS
                };

                // 等待X11事件或超时
                let result = select(
                    x11_fd + 1,
                    &mut read_fds,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut timeout,
                );

                match result {
                    -1 => {
                        error!("select() error");
                        break;
                    }
                    0 => {
                        // 超时，检查是否有挂起的更新
                        if !self.pending_bar_updates.is_empty() {
                            self.flush_pending_bar_updates();
                        }
                        continue;
                    }
                    _ => {
                        if FD_ISSET(x11_fd, &read_fds) {
                            // X11事件就绪，下次循环会处理
                        }
                    }
                }
            }
        }
    }

    // 新增处理命令的方法
    fn process_commands_from_status_bar(&mut self) {
        // 创建一个临时向量来收集所有命令
        let mut commands_to_process: Vec<(i32, SharedCommand)> = Vec::new();

        // 第一步：遍历共享内存缓冲区并收集命令
        for (&monitor_id, buffer) in &self.status_bar_shmem {
            while let Some(cmd) = buffer.receive_command() {
                // 确保命令是给当前显示器的
                if cmd.monitor_id == monitor_id {
                    commands_to_process.push((monitor_id, cmd));
                }
            }
        }

        // 第二步：处理收集到的命令
        for (_monitor_id, cmd) in commands_to_process {
            match cmd.cmd_type.into() {
                CommandType::ViewTag => {
                    // 切换到指定标签
                    info!(
                        "[process_commands] ViewTag command received: {}",
                        cmd.parameter
                    );
                    let arg = Arg::Ui(cmd.parameter);
                    self.view(&arg);
                }
                CommandType::ToggleTag => {
                    // 切换标签
                    info!(
                        "[process_commands] ToggleTag command received: {}",
                        cmd.parameter
                    );
                    let arg = Arg::Ui(cmd.parameter);
                    self.toggletag(&arg);
                }
                CommandType::SetLayout => {
                    // 设置布局
                    info!(
                        "[process_commands] SetLayout command received: {}",
                        cmd.parameter
                    );
                    let arg = Arg::Lt(Rc::new(Layout::try_from(cmd.parameter as u8).unwrap()));
                    self.setlayout(&arg);
                }
                CommandType::None => {}
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

    pub fn attach(&mut self, client_opt: Option<Rc<RefCell<Client>>>) {
        let client_rc = match client_opt {
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

    pub fn attachstack(&mut self, client_opt: Option<Rc<RefCell<Client>>>) {
        let client_rc = match client_opt {
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
            |cli, next_node| cli.stack_next = next_node,
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
                self.wm_atom[WM::WMState as usize],
                0,
                2,
                False,
                self.wm_atom[WM::WMState as usize],
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

        let mut r = self.sel_mon.clone();
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
        // 首先检查是否是状态栏窗口
        if let Some(&monitor_id) = self.status_bar_windows.get(&w) {
            return self.status_bar_clients.get(&monitor_id).cloned();
        }

        // 然后检查常规客户端
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
            let mut c = { m_opt.borrow().clients.clone() };
            while let Some(ref client_opt) = c {
                let win = { client_opt.borrow().win };
                if win == w {
                    return c;
                }
                let next = { client_opt.borrow().next.clone() };
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
        if let Some(ref client_opt) = c {
            return client_opt.borrow().mon.clone();
        }
        return self.sel_mon.clone();
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
            if m.is_some() && !Rc::ptr_eq(m.as_ref().unwrap(), self.sel_mon.as_ref().unwrap()) {
                let sel = self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone();
                self.unfocus(sel, true);
                self.sel_mon = m;
                self.focus(None);
            }
            if {
                c = self.wintoclient(ev.window);
                c.is_some()
            } {
                self.focus(c);
                self.restack(self.sel_mon.clone());
                XAllowEvents(self.dpy, ReplayPointer, CurrentTime);
                click = CLICK::ClkClientWin;
            }
            let buttons = CONFIG.get_buttons();
            for i in 0..buttons.len() {
                if click as u32 == buttons[i].click
                    && buttons[i].func.is_some()
                    && buttons[i].button == ev.button
                    // 清理（移除NumLock, CapsLock等）后的修饰键掩码与事件中的修饰键状态匹配
                    && self.CLEANMASK(buttons[i].mask) == self.CLEANMASK(ev.state)
                {
                    if let Some(ref func) = buttons[i].func {
                        info!(
                            "[buttonpress] click: {}, button: {}, mask: {}",
                            buttons[i].click, buttons[i].button, buttons[i].mask
                        );
                        info!("[buttonpress] use button arg");
                        func(self, &buttons[i].arg);
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

    pub fn spawn(&mut self, arg: *const Arg) {
        info!("[spawn]");
        unsafe {
            let mut sa: sigaction = zeroed();

            let mut mut_arg: Arg = (*arg).clone();
            if let Arg::V(ref mut v) = mut_arg {
                if *v == *CONFIG.get_dmenucmd() {
                    let tmp =
                        (b'0' + self.sel_mon.as_ref().unwrap().borrow_mut().num as u8) as char;
                    let tmp = tmp.to_string();
                    info!(
                        "[spawn] dmenumon tmp: {}, num: {}",
                        tmp,
                        self.sel_mon.as_ref().unwrap().borrow_mut().num
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
                    self.color_map = XCreateColormap(self.dpy, self.root, self.visual, AllocNone);
                    break;
                }
            }

            XFree(infos as *mut _);

            if self.visual.is_null() {
                self.visual = XDefaultVisual(self.dpy, self.screen);
                self.depth = XDefaultDepth(self.dpy, self.screen);
                self.color_map = XDefaultColormap(self.dpy, self.screen);
            }
        }
    }

    pub fn tile(&mut self, mon_rc: &Rc<RefCell<Monitor>>) {
        info!("[tile]"); // 日志记录，进入 tile 布局函数

        // 初始化变量
        let mut n: u32 = 0; // 可见且平铺的客户端总数
        let mut mfacts: f32 = 0.0; // 主区域 (master area) 客户端的 cfact 总和
        let mut sfacts: f32 = 0.0; // 堆叠区域 (stack area) 客户端的 cfact 总和

        // --- 第一遍遍历：计算客户端数量和 cfact 总和 ---
        {
            // 创建一个新的作用域来限制 mon_borrow 的生命周期
            let mon_borrow = mon_rc.borrow(); // 不可变借用 Monitor
            let mut c = self.nexttiled(mon_borrow.clients.clone()); // 获取第一个可见且平铺的客户端
                                                                    // nexttiled 会跳过浮动和不可见的客户端

            while let Some(client_opt) = c {
                // 遍历所有可见且平铺的客户端
                let c_borrow = client_opt.borrow(); // 可变借用 Client 来读取和修改 cfact (虽然这里只读取)
                if n < mon_borrow.n_master {
                    // 如果当前客户端在主区域
                    mfacts += c_borrow.client_fact; // 累加到主区域的 cfact 总和
                } else {
                    // 如果当前客户端在堆叠区域
                    sfacts += c_borrow.client_fact; // 累加到堆叠区域的 cfact 总和
                }
                let next_c = self.nexttiled(c_borrow.next.clone()); // 获取下一个可见平铺客户端
                drop(c_borrow); // 显式 drop 可变借用，以便 nexttiled 中的借用不会冲突
                c = next_c;
                n += 1; // 客户端总数加一
            }
            info!("[tile] monitor_num: {}", mon_borrow.num);
        } // mon_borrow 在这里被 drop

        if n == 0 {
            // 如果没有可见且平铺的客户端，则直接返回
            return;
        }

        // --- 计算主区域的宽度 (mw) ---
        let (ww, mfact0_val, nmaster0_val, wx_val, wy_val, wh_val) = {
            // 再次借用 Monitor 获取其属性
            let mon_borrow = mon_rc.borrow();
            (
                mon_borrow.w_w,
                mon_borrow.m_fact,
                mon_borrow.n_master,
                mon_borrow.w_x,
                mon_borrow.w_y,
                mon_borrow.w_h,
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
            self.client_y_offset(&mon_rc.borrow())
        };
        info!("[tile] client_y_offset: {}", client_y_offset);

        let mut c_iter = {
            // 重新从头开始获取可见平铺客户端
            let mon_borrow = mon_rc.borrow();
            self.nexttiled(mon_borrow.clients.clone())
        };

        while let Some(ref c_opt_rc) = c_iter {
            let next_client_in_list_opt; // 用于存储下一个迭代的客户端
            let bw;
            {
                // 创建一个新的作用域来限制 c_borrow 的生命周期
                let c_borrow = c_opt_rc.borrow(); // 不可变借用开始
                bw = c_borrow.border_w;
                let current_cfact = c_borrow.client_fact;
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
                    mw as i32 - (2 * bw),
                    h as i32 - (2 * bw) - client_y_offset,
                    false,
                );
                // resize 之后，如果需要读取更新后的 height，需要重新 borrow
                let client_actual_height = c_opt_rc.borrow().height() as u32;
                if my + client_actual_height < wh_val as u32 {
                    my += client_actual_height;
                }
                mfacts -= c_opt_rc.borrow().client_fact; // 重新 borrow 读取 cfact (如果 cfact 不变，可以提前读取)
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
                sfacts -= c_opt_rc.borrow().client_fact; // 同上
            }

            c_iter = self.nexttiled(next_client_in_list_opt); // 使用之前获取的 next
            i += 1;
        }
    }

    pub fn togglefloating(&mut self, _arg: *const Arg) {
        // info!("[togglefloating]");
        if self.sel_mon.is_none() {
            return;
        }
        let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
        if let Some(ref sel_opt) = sel {
            // no support for fullscreen windows.
            let isfullscreen = { sel_opt.borrow_mut().is_fullscreen };
            if isfullscreen {
                return;
            }
            {
                let mut sel_borrow = sel_opt.borrow_mut();
                sel_borrow.is_floating = !sel_borrow.is_floating || sel_borrow.is_fixed;
            }
            let is_floating = { sel_opt.borrow_mut().is_floating };
            if is_floating {
                let (x, y, w, h) = {
                    let sel_opt_mut = sel_opt.borrow_mut();
                    (sel_opt_mut.x, sel_opt_mut.y, sel_opt_mut.w, sel_opt_mut.h)
                };
                self.resize(sel_opt, x, y, w, h, false);
            }
            self.arrange(self.sel_mon.clone());
        } else {
            return;
        }
    }

    pub fn focusin(&mut self, e: *mut XEvent) {
        // info!("[focusin]");
        unsafe {
            let sel = { self.sel_mon.as_mut().unwrap().borrow().sel.clone() };
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
                if Rc::ptr_eq(m.as_ref().unwrap(), self.sel_mon.as_ref().unwrap()) {
                    return;
                }
                let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
                self.unfocus(sel, false);
                self.sel_mon = m;
                self.focus(None);
            }
        }
    }

    pub fn tag(&mut self, arg: *const Arg) {
        // info!("[tag]");
        unsafe {
            if let Arg::Ui(ui) = *arg {
                let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
                let target_tag = ui & CONFIG.tagmask();
                if let Some(ref sel_opt) = sel {
                    if target_tag > 0 {
                        sel_opt.borrow_mut().tags = target_tag;
                        self.setclienttagprop(sel_opt);
                        self.focus(None);
                        self.arrange(self.sel_mon.clone());
                    }
                }
            }
        }
    }

    pub fn tagmon(&mut self, arg: *const Arg) {
        // info!("[tagmon]");
        unsafe {
            if let Some(ref selmon_opt) = self.sel_mon {
                if selmon_opt.borrow_mut().sel.is_none() {
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
                let selmon_clone = self.sel_mon.clone();
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
                let sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                if sel_mon_mut.sel.is_none()
                    || (sel_mon_mut.sel.as_ref().unwrap().borrow_mut().is_fullscreen
                        && CONFIG.behavior().lock_fullscreen)
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
                    self.sel_mon
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
                while let Some(ref client_opt) = c {
                    if client_opt.borrow().isvisible() {
                        break;
                    }
                    let next = client_opt.borrow().next.clone();
                    c = next;
                }
                if c.is_none() {
                    c = {
                        let sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                        sel_mon_mut.clients.clone()
                    };
                    while let Some(ref client_opt) = c {
                        if client_opt.borrow_mut().isvisible() {
                            break;
                        }
                        let next = client_opt.borrow_mut().next.clone();
                        c = next;
                    }
                }
            } else {
                if let Some(ref selmon_opt) = self.sel_mon {
                    let (mut cl, sel) = {
                        let sel_mon_mut = selmon_opt.borrow_mut();
                        (sel_mon_mut.clients.clone(), sel_mon_mut.sel.clone())
                    };
                    while !Self::are_equal_rc(&cl, &sel) {
                        if let Some(ref cl_opt) = cl {
                            if cl_opt.borrow_mut().isvisible() {
                                c = cl.clone();
                            }
                            let next = cl_opt.borrow_mut().next.clone();
                            cl = next;
                        }
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
            }
            if c.is_some() {
                self.focus(c);
                self.restack(self.sel_mon.clone());
            }
        }
    }

    pub fn togglebar(&mut self, arg: *const Arg) {
        info!("[togglebar]");
        unsafe {
            if let Arg::I(_) = *arg {
                self.show_bar = !self.show_bar;
                let mon_num = if let Some(sel_mon_ref) = self.sel_mon.as_ref() {
                    Some(sel_mon_ref.borrow().num)
                } else {
                    None
                };
                info!("[togglebar] {}", self.show_bar);
                self.mark_bar_update_needed(mon_num);
            }
        }
    }

    pub fn incnmaster(&mut self, arg: *const Arg) {
        // info!("[incnmaster]");
        unsafe {
            if let Arg::I(i) = *arg {
                let mut sel_mon_mut = self.sel_mon.as_mut().unwrap().borrow_mut();
                let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                sel_mon_mut.pertag.as_mut().unwrap().n_masters[cur_tag] =
                    0.max(sel_mon_mut.n_master as i32 + i) as u32;

                sel_mon_mut.n_master = sel_mon_mut.pertag.as_ref().unwrap().n_masters[cur_tag];
            }
            self.arrange(self.sel_mon.clone());
        }
    }

    pub fn setcfact(&mut self, arg: *const Arg) {
        // info!("[setcfact]");
        if arg.is_null() {
            return;
        }
        unsafe {
            let c = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
            if c.is_none() {
                return;
            }
            if let Arg::F(f0) = *arg {
                let mut f = f0 + c.as_ref().unwrap().borrow().client_fact;
                if f0.abs() < 0.0001 {
                    f = 1.0;
                } else if f < 0.25 || f > 4.0 {
                    return;
                }
                c.as_ref().unwrap().borrow_mut().client_fact = f;
                self.arrange(self.sel_mon.clone());
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
                        .sel_mon
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
                        let is_floating = c.as_ref().unwrap().borrow_mut().is_floating;
                        let condition = !isvisible || is_floating;
                        if !condition {
                            break;
                        }
                        let next = c.as_ref().unwrap().borrow_mut().next.clone();
                        c = next;
                    }
                    if c.is_none() {
                        c = self.sel_mon.as_ref().unwrap().borrow_mut().clients.clone();
                    }
                    while c.is_some() {
                        let isvisible = c.as_ref().unwrap().borrow_mut().isvisible();
                        let is_floating = c.as_ref().unwrap().borrow_mut().is_floating;
                        let condition = !isvisible || is_floating;
                        if !condition {
                            break;
                        }
                        let next = c.as_ref().unwrap().borrow_mut().next.clone();
                        c = next;
                    }
                } else {
                    // Find the client before selmon->sel
                    i = self.sel_mon.as_ref().unwrap().borrow_mut().clients.clone();
                    let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
                    while !Self::are_equal_rc(&i, &sel) {
                        let isvisible = i.as_ref().unwrap().borrow_mut().isvisible();
                        let is_floating = i.as_ref().unwrap().borrow_mut().is_floating;
                        if isvisible && !is_floating {
                            c = i.clone();
                        }
                        let next = i.as_ref().unwrap().borrow_mut().next.clone();
                        i = next;
                    }
                    if c.is_none() {
                        while i.is_some() {
                            let isvisible = i.as_ref().unwrap().borrow_mut().isvisible();
                            let is_floating = i.as_ref().unwrap().borrow_mut().is_floating;
                            if isvisible && !is_floating {
                                c = i.clone();
                            }
                            let next = i.as_ref().unwrap().borrow_mut().next.clone();
                            i = next;
                        }
                    }
                }
                // Find the client before selmon->sel and c
                i = self.sel_mon.as_ref().unwrap().borrow_mut().clients.clone();
                let sel = self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone();
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
                        .sel_mon
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
                            self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone()
                        } else {
                            sel_next
                        };
                    let sel = self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone();
                    let c_next = c.as_ref().unwrap().borrow_mut().next.clone();
                    self.sel_mon
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
                        let sel = self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone();
                        if !Self::are_equal_rc(&pc, &sel) {
                            pc.as_ref().unwrap().borrow_mut().next = sel;
                        }
                    }

                    let sel = self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone();
                    let clients = self.sel_mon.as_ref().unwrap().borrow_mut().clients.clone();
                    if Self::are_equal_rc(&sel, &clients) {
                        self.sel_mon.as_ref().unwrap().borrow_mut().clients = c;
                    } else if Self::are_equal_rc(&c, &clients) {
                        self.sel_mon.as_ref().unwrap().borrow_mut().clients = sel;
                    }

                    self.arrange(self.sel_mon.clone());
                }
            } else {
                return;
            }
        }
    }

    pub fn setmfact(&mut self, arg: *const Arg) {
        // info!("[setmfact]");
        unsafe {
            if arg.is_null() {
                return;
            }
            if let Arg::F(f) = *arg {
                let mut sel_mon_mut = self.sel_mon.as_mut().unwrap().borrow_mut();
                let f = if f < 1.0 {
                    f + sel_mon_mut.m_fact
                } else {
                    f - 1.0
                };
                if f < 0.05 || f > 0.95 {
                    return;
                }
                let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                sel_mon_mut.pertag.as_mut().unwrap().m_facts[cur_tag] = f;
                sel_mon_mut.m_fact = sel_mon_mut.pertag.as_mut().unwrap().m_facts[cur_tag];
            }
            self.arrange(self.sel_mon.clone());
        }
    }

    pub fn setlayout(&mut self, arg: *const Arg) {
        info!("[setlayout]");
        if arg.is_null() {
            return;
        }
        unsafe {
            let sel_is_some;
            match *arg {
                Arg::Lt(ref lt) => {
                    let sel_mon_layout_type = {
                        let sel_mon = self.sel_mon.as_ref().unwrap().borrow();
                        sel_mon.lt[sel_mon.sel_lt].layout_type().to_string()
                    };
                    if lt.layout_type() == sel_mon_layout_type {
                        let mut sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                        let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                        sel_mon_mut.pertag.as_mut().unwrap().sel_lts[cur_tag] ^= 1;
                        sel_mon_mut.sel_lt = sel_mon_mut.pertag.as_ref().unwrap().sel_lts[cur_tag];
                    } else {
                        let mut sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                        let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                        let sel_lt = sel_mon_mut.sel_lt;
                        sel_mon_mut.pertag.as_mut().unwrap().lt_idxs[cur_tag][sel_lt] =
                            Some(lt.clone());
                        sel_mon_mut.lt[sel_lt] = lt.clone();
                    }
                }
                _ => {
                    let mut sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                    let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                    sel_mon_mut.pertag.as_mut().unwrap().sel_lts[cur_tag] ^= 1;
                    sel_mon_mut.sel_lt = sel_mon_mut.pertag.as_ref().unwrap().sel_lts[cur_tag];
                }
            }
            {
                let mut sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                sel_mon_mut.lt_symbol = sel_mon_mut.lt[sel_mon_mut.sel_lt].symbol().to_string();
                sel_is_some = sel_mon_mut.sel.is_some();
            }
            if sel_is_some {
                self.arrange(self.sel_mon.clone());
            } else {
                let mon_num = if let Some(sel_mon_ref) = self.sel_mon.as_ref() {
                    Some(sel_mon_ref.borrow().num)
                } else {
                    None
                };
                self.mark_bar_update_needed(mon_num);
            }
        }
    }

    pub fn zoom(&mut self, _arg: *const Arg) {
        // info!("[zoom]");
        let mut c;
        let sel_c;
        let nexttiled_c;
        {
            let sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow();
            c = sel_mon_mut.sel.clone();
            if c.is_none() || c.as_ref().unwrap().borrow().is_floating {
                return;
            }
            sel_c = sel_mon_mut.clients.clone();
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

    pub fn loopview(&mut self, arg: *const Arg) {
        // info!("[loopview]");
        unsafe {
            let direction = if let Arg::I(val) = *arg {
                val
            } else {
                return;
            };
            if direction == 0 {
                return;
            }
            let next_tag;
            let current_tag;
            let cur_tag;
            {
                let sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow();
                current_tag = sel_mon_mut.tag_set[sel_mon_mut.sel_tags];
                // 找到当前tag的位置
                let current_tag_index = if current_tag == 0 {
                    0 // 如果当前没有选中的tag，从第一个开始
                } else {
                    current_tag.trailing_zeros() as usize
                };
                // 计算下一个tag的索引（假设支持9个tag，即1-9）
                let max_tags = 9;
                let next_tag_index = if direction > 0 {
                    // 向前循环：1>2>3>...>9>1
                    (current_tag_index + 1) % max_tags
                } else {
                    // 向后循环：1>9>8>...>2>1
                    if current_tag_index == 0 {
                        max_tags - 1
                    } else {
                        current_tag_index - 1
                    }
                };
                // 将索引转换为tag位掩码
                next_tag = 1 << next_tag_index;
                info!(
                    "[loopview] current_tag: {}, next_tag: {}, direction: {}",
                    current_tag, next_tag, direction
                );
            }
            // 如果下一个tag和当前tag相同，不需要切换
            {
                let sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow();
                if next_tag == sel_mon_mut.tag_set[sel_mon_mut.sel_tags] {
                    return;
                }
            }
            {
                let mut sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                // 切换sel tagset
                info!("[loopview] sel_tags: {}", sel_mon_mut.sel_tags);
                sel_mon_mut.sel_tags ^= 1;
                info!("[loopview] sel_tags: {}", sel_mon_mut.sel_tags);
                // 设置新的tag
                let sel_tags = sel_mon_mut.sel_tags;
                sel_mon_mut.tag_set[sel_tags] = next_tag;
                // 更新pertag信息
                if let Some(pertag) = sel_mon_mut.pertag.as_mut() {
                    pertag.prev_tag = pertag.cur_tag;
                    // 计算新的当前tag索引
                    let i = next_tag.trailing_zeros() as usize;
                    pertag.cur_tag = i + 1;
                }
                if let Some(pertag) = sel_mon_mut.pertag.as_ref() {
                    info!(
                        "[loopview] prevtag: {}, cur_tag: {}",
                        pertag.prev_tag, pertag.cur_tag
                    );
                }
                cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
            }
            // 应用pertag设置
            let sel_opt;
            {
                let mut sel_mon_mut = self.sel_mon.as_mut().unwrap().borrow_mut();
                sel_mon_mut.n_master = sel_mon_mut.pertag.as_ref().unwrap().n_masters[cur_tag];
                sel_mon_mut.m_fact = sel_mon_mut.pertag.as_ref().unwrap().m_facts[cur_tag];
                sel_mon_mut.sel_lt = sel_mon_mut.pertag.as_ref().unwrap().sel_lts[cur_tag];
                let sel_lt = sel_mon_mut.sel_lt;
                sel_mon_mut.lt[sel_lt] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs[cur_tag]
                    [sel_lt]
                    .clone()
                    .expect("None unwrap");
                sel_mon_mut.lt[sel_lt ^ 1] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs[cur_tag]
                    [sel_lt ^ 1]
                    .clone()
                    .expect("None unwrap");
                sel_opt = sel_mon_mut.pertag.as_ref().unwrap().sel[cur_tag].clone();
                if sel_opt.is_some() {
                    info!("[loopview] sel_opt: {}", sel_opt.as_ref().unwrap().borrow());
                }
            };

            self.focus(sel_opt);
            self.arrange(self.sel_mon.clone());
        }
    }

    pub fn view(&mut self, arg: *const Arg) {
        // info!("[view]");
        unsafe {
            let ui = if let Arg::Ui(val) = *arg {
                val
            } else {
                return;
            };
            let target_tag = ui & CONFIG.tagmask();
            let cur_tag;
            {
                let mut sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                info!("[view] ui: {ui}, {target_tag}, {:?}", sel_mon_mut.tag_set);
                if target_tag == sel_mon_mut.tag_set[sel_mon_mut.sel_tags] {
                    return;
                }
                // toggle sel tagset.
                info!("[view] sel_tags: {}", sel_mon_mut.sel_tags);
                sel_mon_mut.sel_tags ^= 1;
                info!("[view] sel_tags: {}", sel_mon_mut.sel_tags);
                if target_tag > 0 {
                    let sel_tags = sel_mon_mut.sel_tags;
                    sel_mon_mut.tag_set[sel_tags] = target_tag;
                    if let Some(pertag) = sel_mon_mut.pertag.as_mut() {
                        pertag.prev_tag = pertag.cur_tag;
                    }
                    if ui == !0 {
                        // 会将tag_set设置为包含所有tag的值
                        // 使用 curtag = 0 对应的配置
                        sel_mon_mut.pertag.as_mut().unwrap().cur_tag = 0;
                    } else {
                        let i = ui.trailing_zeros() as usize;
                        sel_mon_mut.pertag.as_mut().unwrap().cur_tag = i + 1;
                    }
                } else {
                    if let Some(pertag) = sel_mon_mut.pertag.as_mut() {
                        std::mem::swap(&mut pertag.prev_tag, &mut pertag.cur_tag);
                    }
                }
                if let Some(pertag) = &sel_mon_mut.pertag {
                    info!(
                        "[view] prevtag: {}, cur_tag: {}",
                        pertag.prev_tag, pertag.cur_tag
                    );
                }
                cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
            }
            let sel_opt;
            {
                let mut sel_mon_mut = self.sel_mon.as_mut().unwrap().borrow_mut();
                sel_mon_mut.n_master = sel_mon_mut.pertag.as_ref().unwrap().n_masters[cur_tag];
                sel_mon_mut.m_fact = sel_mon_mut.pertag.as_ref().unwrap().m_facts[cur_tag];
                sel_mon_mut.sel_lt = sel_mon_mut.pertag.as_ref().unwrap().sel_lts[cur_tag];
                let sel_lt = sel_mon_mut.sel_lt;
                sel_mon_mut.lt[sel_lt] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs[cur_tag]
                    [sel_lt]
                    .clone()
                    .expect("None unwrap");
                sel_mon_mut.lt[sel_lt ^ 1] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs[cur_tag]
                    [sel_lt ^ 1]
                    .clone()
                    .expect("None unwrap");
                sel_opt = sel_mon_mut.pertag.as_ref().unwrap().sel[cur_tag].clone();
                if sel_opt.is_some() {
                    info!("[view] sel_opt: {}", sel_opt.as_ref().unwrap().borrow());
                }
            };
            self.focus(sel_opt);
            self.arrange(self.sel_mon.clone());
        }
    }

    pub fn toggleview(&mut self, arg: *const Arg) {
        info!("[toggleview]");
        unsafe {
            if let Arg::Ui(ui) = *arg {
                if self.sel_mon.is_none() {
                    return;
                }
                let sel_tags;
                let newtagset;
                {
                    let sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
                    sel_tags = sel_mon_mut.sel_tags;
                    newtagset = sel_mon_mut.tag_set[sel_tags] ^ (ui & CONFIG.tagmask());
                }
                if newtagset > 0 {
                    {
                        let mut selmon_clone = self.sel_mon.clone();
                        let mut sel_mon_mut = selmon_clone.as_mut().unwrap().borrow_mut();
                        sel_mon_mut.tag_set[sel_tags] = newtagset;

                        if newtagset == !0 {
                            sel_mon_mut.pertag.as_mut().unwrap().prev_tag =
                                sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                            sel_mon_mut.pertag.as_mut().unwrap().cur_tag = 0;
                        }

                        // test if the user did not select the same tag
                        let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                        if newtagset & 1 << (cur_tag - 1) <= 0 {
                            sel_mon_mut.pertag.as_mut().unwrap().prev_tag = cur_tag;
                            let mut i = 0;
                            loop {
                                let condition = newtagset & 1 << i;
                                if condition > 0 {
                                    break;
                                }
                                i += 1;
                            }
                            sel_mon_mut.pertag.as_mut().unwrap().cur_tag = i + 1;
                        }

                        // apply settings for this view
                        let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                        sel_mon_mut.n_master =
                            sel_mon_mut.pertag.as_ref().unwrap().n_masters[cur_tag];
                        sel_mon_mut.m_fact = sel_mon_mut.pertag.as_ref().unwrap().m_facts[cur_tag];
                        sel_mon_mut.sel_lt = sel_mon_mut.pertag.as_ref().unwrap().sel_lts[cur_tag];
                        let sel_lt = sel_mon_mut.sel_lt;
                        sel_mon_mut.lt[sel_lt] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs
                            [cur_tag][sel_lt]
                            .clone()
                            .expect("None unwrap");
                        sel_mon_mut.lt[sel_lt ^ 1] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs
                            [cur_tag][sel_lt ^ 1]
                            .clone()
                            .expect("None unwrap");
                    }
                    self.focus(None);
                    self.arrange(self.sel_mon.clone());
                }
            }
        }
    }

    pub fn togglefullscr(&mut self, _: *const Arg) {
        info!("[togglefullscr]");
        if let Some(ref selmon_opt) = self.sel_mon {
            let sel = { selmon_opt.borrow_mut().sel.clone() };
            if sel.is_none() {
                return;
            }
            let isfullscreen = { sel.as_ref().unwrap().borrow_mut().is_fullscreen };
            self.setfullscreen(sel.as_ref().unwrap(), !isfullscreen);
        }
    }

    pub fn toggletag(&mut self, arg: *const Arg) {
        info!("[toggletag]");
        unsafe {
            let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
            if sel.is_none() {
                return;
            }
            if let Arg::Ui(ui) = *arg {
                let newtags = sel.as_ref().unwrap().borrow_mut().tags ^ (ui & CONFIG.tagmask());
                if newtags > 0 {
                    sel.as_ref().unwrap().borrow_mut().tags = newtags;
                    self.setclienttagprop(sel.as_ref().unwrap());
                    self.focus(None);
                    self.arrange(self.sel_mon.clone());
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
            self.s_w = XDisplayWidth(self.dpy, self.screen);
            self.s_h = XDisplayHeight(self.dpy, self.screen);
            self.root = XRootWindow(self.dpy, self.screen);
            self.xinitvisual();
            self.drw = Some(Box::new(Drw::drw_create(
                self.dpy,
                self.visual,
                self.color_map,
            )));
            // info!("[setup] updategeom");
            self.updategeom();
            // init atoms
            let mut c_string = CString::new("UTF8_STRING").expect("fail to convert");
            let utf8string = XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("WM_PROTOCOLS").expect("fail to convert");
            self.wm_atom[WM::WMProtocols as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("WM_DELETE_WINDOW").expect("fail to convert");
            self.wm_atom[WM::WMDelete as usize] = XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("WM_STATE").expect("fail to convert");
            self.wm_atom[WM::WMState as usize] = XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("WM_TAKE_FOCUS").expect("fail to convert");
            self.wm_atom[WM::WMTakeFocus as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);

            c_string = CString::new("_NET_ACTIVE_WINDOW").expect("fail to convert");
            self.net_atom[NET::NetActiveWindow as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_SUPPORTED").expect("fail to convert");
            self.net_atom[NET::NetSupported as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_NAME").expect("fail to convert");
            self.net_atom[NET::NetWMName as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_STATE").expect("fail to convert");
            self.net_atom[NET::NetWMState as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_SUPPORTING_WM_CHECK").expect("fail to convert");
            self.net_atom[NET::NetWMCheck as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_STATE_FULLSCREEN").expect("fail to convert");
            self.net_atom[NET::NetWMFullscreen as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_WINDOW_TYPE").expect("fail to convert");
            self.net_atom[NET::NetWMWindowType as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_WM_WINDOW_TYPE_DIALOG").expect("fail to convert");
            self.net_atom[NET::NetWMWindowTypeDialog as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_CLIENT_LIST").expect("fail to convert");
            self.net_atom[NET::NetClientList as usize] =
                XInternAtom(self.dpy, c_string.as_ptr(), False);
            c_string = CString::new("_NET_CLIENT_INFO").expect("fail to convert");
            self.net_atom[NET::NetClientInfo as usize] =
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
            // supporting window fot NetWMCheck
            self.wm_check_win = XCreateSimpleWindow(self.dpy, self.root, 0, 0, 1, 1, 0, 0, 0);
            XChangeProperty(
                self.dpy,
                self.wm_check_win,
                self.net_atom[NET::NetWMCheck as usize],
                XA_WINDOW,
                32,
                PropModeReplace,
                &mut self.wm_check_win as *mut u64 as *const _,
                1,
            );
            c_string = CString::new("jwm").unwrap();
            XChangeProperty(
                self.dpy,
                self.wm_check_win,
                self.net_atom[NET::NetWMName as usize],
                utf8string,
                8,
                PropModeReplace,
                c_string.as_ptr() as *const _,
                1,
            );
            XChangeProperty(
                self.dpy,
                self.root,
                self.net_atom[NET::NetWMCheck as usize],
                XA_WINDOW,
                32,
                PropModeReplace,
                &mut self.wm_check_win as *mut u64 as *const _,
                1,
            );
            // EWMH support per view
            XChangeProperty(
                self.dpy,
                self.root,
                self.net_atom[NET::NetSupported as usize],
                XA_ATOM,
                32,
                PropModeReplace,
                self.net_atom.as_ptr() as *const _,
                NET::NetLast as i32,
            );
            XDeleteProperty(
                self.dpy,
                self.root,
                self.net_atom[NET::NetClientList as usize],
            );
            XDeleteProperty(
                self.dpy,
                self.root,
                self.net_atom[NET::NetClientInfo as usize],
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
            let sel = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
            if sel.is_none() {
                return;
            }
            info!("[killclient] {}", sel.as_ref().unwrap().borrow());
            if !self.sendevent(
                &mut sel.as_ref().unwrap().borrow_mut(),
                self.wm_atom[WM::WMDelete as usize],
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

    pub fn nexttiled(
        &mut self,
        mut client_rc_opt: Option<Rc<RefCell<Client>>>,
    ) -> Option<Rc<RefCell<Client>>> {
        // info!("[nexttiled]");
        while let Some(ref client_rc) = client_rc_opt.clone() {
            let client_borrow = client_rc.borrow();
            let is_floating = client_borrow.is_floating;
            let isvisible = client_borrow.isvisible();
            if is_floating || !isvisible {
                let next = client_borrow.next.clone();
                client_rc_opt = next;
            } else {
                break;
            }
        }
        return client_rc_opt;
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
        // info!("[gettextprop]"); // 日志
        unsafe {
            // unsafe 块，因为直接与 Xlib C API 交互
            let mut name_prop: XTextProperty = std::mem::zeroed(); // 初始化 XTextProperty 结构体

            // 1. 获取窗口属性
            // XGetTextProperty 用于获取指定窗口 w 的文本属性 (由 atom 标识，如 XA_WM_NAME)
            // 结果存储在 name_prop 中。
            // 如果失败或属性为空 (name_prop.nitems <= 0)，则返回 false。
            if XGetTextProperty(self.dpy, w, &mut name_prop, atom) <= 0 || name_prop.nitems == 0 {
                // XFree on name_prop.value might be needed here if XGetTextProperty allocated it
                // even on failure, though docs suggest only on success.
                // For safety, one might consider checking name_prop.value != null and XFreeing it.
                // However, typical Xlib examples free only on success.
                if !name_prop.value.is_null() {
                    XFree(name_prop.value as *mut _);
                }
                return false;
            }

            // 2. 清空输出字符串 text
            *text = String::new(); // 或者 text.clear();

            // 3. 根据属性编码处理文本
            if name_prop.encoding == XA_STRING {
                // 如果编码是 XA_STRING (通常是 Latin-1 或本地编码)
                // XA_STRING 通常被认为是简单的 C 字符串 (null-terminated)
                if !name_prop.value.is_null() {
                    // 确保 value 指针有效
                    let c_str_slice = CStr::from_ptr(name_prop.value as *const c_char);
                    match c_str_slice.to_str() {
                        // 尝试将其转换为 Rust 的 &str (UTF-8)
                        Ok(val) => {
                            // 成功转换为 &str，现在处理长度限制
                            let mut tmp_string = val.to_string();
                            let mut char_count = 0;
                            let mut byte_truncate_at = tmp_string.len();
                            for (idx, _) in tmp_string.char_indices() {
                                if char_count >= self.stext_max_len {
                                    byte_truncate_at = idx;
                                    break;
                                }
                                char_count += 1;
                            }
                            tmp_string.truncate(byte_truncate_at);
                            *text = tmp_string;
                        }
                        Err(e) => {
                            // 转换为 &str 失败 (例如，XA_STRING 内容不是有效的 UTF-8)
                            info!("[gettextprop] text from XA_STRING to_str error: {:?}", e);
                            // 此时 text 仍然是空字符串
                            // return false; // 或者让 text 为空并返回 true，取决于期望行为
                        }
                    }
                }
            } else {
                // 如果编码不是 XA_STRING (通常意味着可能是 COMPOUND_TEXT 或其他需要转换的编码)
                // 尝试使用 XmbTextPropertyToTextList 将 XTextProperty 转换为本地多字节字符串列表
                // (通常是 UTF-8，如果 locale 设置正确的话)
                let mut list_ptr: *mut *mut c_char = std::ptr::null_mut();
                let mut count: i32 = 0;
                // XmbTextPropertyToTextList 返回值 >= Success (0) 表示成功
                if XmbTextPropertyToTextList(self.dpy, &mut name_prop, &mut list_ptr, &mut count)
                    >= Success as i32
                    && count > 0
                    && !list_ptr.is_null()
                    && !(*list_ptr).is_null()
                // 确保列表和第一个元素有效
                {
                    // 通常我们只关心列表中的第一个字符串
                    let c_str_slice = CStr::from_ptr(*list_ptr as *const c_char);
                    match c_str_slice.to_str() {
                        // 尝试转换为 Rust &str
                        Ok(val) => {
                            let mut tmp_string = val.to_string();
                            let mut char_count = 0;
                            let mut byte_truncate_at = tmp_string.len();
                            for (idx, _) in tmp_string.char_indices() {
                                if char_count >= self.stext_max_len {
                                    byte_truncate_at = idx;
                                    break;
                                }
                                char_count += 1;
                            }
                            tmp_string.truncate(byte_truncate_at);
                            *text = tmp_string;
                        }
                        Err(e) => {
                            info!("[gettextprop] text from XmbList to_str error: {:?}", e);
                            return false;
                        }
                    }
                    XFreeStringList(list_ptr); // 必须释放由 XmbTextPropertyToTextList 分配的列表
                } else {
                    // 转换失败
                    info!("[gettextprop] XmbTextPropertyToTextList failed or returned empty list");
                    return false;
                }
            }

            // 4. 释放 XTextProperty 中的 value 字段
            // XGetTextProperty 会为 name_prop.value 分配内存，需要手动释放。
            if !name_prop.value.is_null() {
                XFree(name_prop.value as *mut _);
            }

            return true; // 表示尝试获取属性的操作已完成（不一定文本转换成功）
        }
    }

    pub fn propertynotify(&mut self, e: *mut XEvent) {
        // info!("[propertynotify]");
        unsafe {
            let ev = (*e).property;
            if ev.window == self.root && ev.atom == XA_WM_NAME {
            } else if ev.state == PropertyDelete {
                // ignore
                return;
            } else if let Some(client_rc) = self.wintoclient(ev.window) {
                match ev.atom {
                    XA_WM_TRANSIENT_FOR => {
                        let mut client_borrowd = client_rc.borrow_mut();
                        let mut trans: Window = 0;
                        if !client_borrowd.is_floating
                            && XGetTransientForHint(self.dpy, client_borrowd.win, &mut trans) > 0
                            && {
                                client_borrowd.is_floating = self.wintoclient(trans).is_some();
                                client_borrowd.is_floating
                            }
                        {
                            self.arrange(client_borrowd.mon.clone());
                        }
                    }
                    XA_WM_NORMAL_HINTS => {
                        let mut client_borrowd = client_rc.borrow_mut();
                        client_borrowd.hints_valid = false;
                    }
                    XA_WM_HINTS => {
                        self.updatewmhints(&client_rc);
                        // WM_HINTS 改变可能影响紧急状态，需要重绘状态栏
                        self.mark_bar_update_needed(None);
                    }
                    _ => {}
                }
                if ev.atom == XA_WM_NAME || ev.atom == self.net_atom[NET::NetWMName as usize] {
                    self.updatetitle(&mut client_rc.borrow_mut());
                    // 如果这个改变了标题的窗口，正好是其所在显示器上当前选中的窗口
                    let (is_selected_on_mon, mon_opt) = {
                        let client_borrow_for_sel_check = client_rc.borrow(); // 不可变借用
                        let mon_rc_opt = client_borrow_for_sel_check.mon.clone();
                        if let Some(ref mon_rc_inner) = mon_rc_opt {
                            let mon_borrow_for_sel_check = mon_rc_inner.borrow();
                            (
                                Self::are_equal_rc(
                                    &mon_borrow_for_sel_check.sel,
                                    &Some(client_rc.clone()),
                                ),
                                mon_rc_opt.clone(),
                            )
                        } else {
                            (false, None)
                        }
                    };
                    if is_selected_on_mon {
                        if let Some(ref mon) = mon_opt {
                            self.mark_bar_update_needed(Some(mon.borrow().num));
                        }
                    }
                }
                if ev.atom == self.net_atom[NET::NetWMWindowType as usize] {
                    self.updatewindowtype(&client_rc);
                }
            }
        }
    }

    pub fn movemouse(&mut self, _arg: *const Arg) {
        info!("[movemouse]"); // 日志
        unsafe {
            // unsafe 块
            // 1. 获取当前选中的客户端 (c)
            let client_opt = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
            if client_opt.is_none() {
                return;
            } // 没有选中客户端则返回
            let c_rc = client_opt.as_ref().unwrap(); // c_rc 是 &Rc<RefCell<Client>>

            // 2. 日志记录和全屏检查
            {
                info!("[movemouse] {}", c_rc.borrow().name);
            } // 记录要移动的窗口名
            if c_rc.borrow().is_fullscreen {
                // 如果窗口是全屏状态
                // no support moving fullscreen windows by mouse
                return; // 通常不允许通过鼠标移动全屏窗口
            }

            // 3. 准备工作
            self.restack(self.sel_mon.clone()); // 将当前选中窗口置于堆叠顶部 (视觉上)，并重绘状态栏
            let (original_client_x, original_client_y) = {
                // 保存窗口开始移动时的原始坐标
                let c_borrow = c_rc.borrow();
                (c_borrow.x, c_borrow.y)
            };

            // 4. 抓取鼠标指针 (XGrabPointer)
            //   这使得 JWM 在接下来的鼠标事件中独占鼠标输入，直到释放。
            if XGrabPointer(
                self.dpy,                                                    // X Display 连接
                self.root,        // 抓取事件的窗口 (根窗口)
                False,            // owner_events: False 表示事件报告给抓取窗口 (root)
                MOUSEMASK as u32, // event_mask: 我们关心的鼠标事件 (移动、按钮释放)
                GrabModeAsync,    // pointer_mode: 异步指针模式
                GrabModeAsync,    // keyboard_mode: 异步键盘模式 (通常与指针模式一致)
                0,                // confine_to: 不限制鼠标移动范围 (0 表示不限制)
                self.cursor[CUR::CurMove as usize].as_ref().unwrap().cursor, // cursor: 设置为移动光标样式
                CurrentTime,                                                 // time: 当前时间
            ) != GrabSuccess
            {
                // 如果抓取失败 (例如其他程序已抓取)，则返回
                return;
            }

            // 5. 获取鼠标初始位置
            let mut initial_mouse_root_x: i32 = 0; // 鼠标相对于根窗口的初始 X 坐标
            let mut initial_mouse_root_y: i32 = 0; // 鼠标相对于根窗口的初始 Y 坐标
            let mut last_motion_time: Time = 0; // 上一次处理 MotionNotify 事件的时间 (用于节流)

            // getrootptr 获取鼠标指针相对于根窗口的当前坐标，并存入 initial_mouse_root_x, initial_mouse_root_y
            if self.getrootptr(&mut initial_mouse_root_x, &mut initial_mouse_root_y) <= 0 {
                // 如果获取失败 (例如指针不在屏幕上)
                XUngrabPointer(self.dpy, CurrentTime); // 释放鼠标抓取
                return;
            }
            info!(
                "[movemouse] initial mouse (root): x={}, y={}",
                initial_mouse_root_x, initial_mouse_root_y
            );

            // 6. 进入鼠标移动事件循环
            let mut ev: XEvent = zeroed();
            loop {
                // 等待并获取我们关心的鼠标事件、暴露事件或子窗口结构重定向事件
                XMaskEvent(
                    self.dpy,
                    MOUSEMASK | ExposureMask | SubstructureRedirectMask,
                    &mut ev,
                );

                match ev.type_ {
                    ConfigureRequest | Expose | MapRequest => {
                        // 如果在拖动过程中收到其他重要事件，交给 JWM 的主事件处理器处理
                        self.handler(ev.type_, &mut ev);
                    }
                    MotionNotify => {
                        // 如果是鼠标移动事件
                        // a. 节流：避免过于频繁地处理移动事件，大约每秒 60 次
                        if ev.motion.time - last_motion_time <= (1000 / 60) {
                            // 时间单位是毫秒
                            continue;
                        }
                        last_motion_time = ev.motion.time;

                        // b. 获取当前显示器的工作区边界
                        let (mon_wx, mon_wy, mon_ww, mon_wh) = {
                            let selmon_borrow = self.sel_mon.as_ref().unwrap().borrow();
                            (
                                selmon_borrow.w_x,
                                selmon_borrow.w_y,
                                selmon_borrow.w_w,
                                selmon_borrow.w_h,
                            )
                        };

                        // c. 计算窗口新的左上角坐标 (nx, ny)
                        //    新坐标 = 窗口原始坐标 + (当前鼠标根坐标 - 初始鼠标根坐标)
                        //    ev.motion.x 和 ev.motion.y 是事件发生时鼠标相对于事件窗口的坐标。
                        //    但由于我们是在根窗口抓取的，ev.motion.x_root 和 ev.motion.y_root 才是鼠标相对于根的坐标。
                        //    这里代码使用的是 ev.motion.x 和 ev.motion.y，这通常是相对于事件窗口的。
                        //    如果 grab 是在 root 上，那么 ev.motion.x 和 ev.motion.y 也是相对于 root 的。
                        //    我们假设 ev.motion.x/y 就是相对于 root 的，或者 getrootptr 获得的 x/y 是对应的。
                        //    为了清晰，通常直接使用 ev.motion.x_root 和 ev.motion.y_root。
                        //    根据你的 getrootptr 实现，x 和 y 是 initial_mouse_root_x/y。
                        //    所以，新的鼠标位置是 ev.motion.x_root, ev.motion.y_root。
                        //    位移 = (ev.motion.x_root - initial_mouse_root_x), (ev.motion.y_root - initial_mouse_root_y)
                        //    因此，nx = original_client_x + (ev.motion.x_root - initial_mouse_root_x)
                        //    ny = original_client_y + (ev.motion.y_root - initial_mouse_root_y)
                        //    你代码中用的是 ev.motion.x 和 initial_mouse_root_x (来自 getrootptr)，这需要确认
                        //    getrootptr 返回的 x,y 与 ev.motion.x_root,y_root 是否一致。
                        //    假设 ev.motion.x 和 ev.motion.y 在这里是指针相对于根窗口的当前位置。
                        //    或者更准确地说，是 ev.motion.x_root 和 ev.motion.y_root。
                        //    当前代码：nx = original_client_x + (鼠标当前根X - 鼠标初始根X)
                        let current_mouse_root_x = ev.motion.x_root; // 明确使用 x_root, y_root
                        let current_mouse_root_y = ev.motion.y_root;
                        let mut new_window_x =
                            original_client_x + (current_mouse_root_x - initial_mouse_root_x);
                        let mut new_window_y =
                            original_client_y + (current_mouse_root_y - initial_mouse_root_y);

                        // d. 吸附到屏幕边缘 (Snap to screen edges)
                        let client_total_width = { c_rc.borrow().width() }; // 获取窗口总宽度 (含边框)
                        let client_total_height = { c_rc.borrow().height() }; // 获取窗口总高度 (含边框)

                        if (mon_wx - new_window_x).abs() < CONFIG.snap() as i32 {
                            // 吸附到左边缘
                            new_window_x = mon_wx;
                        } else if ((mon_wx + mon_ww) - (new_window_x + client_total_width)).abs()
                            < CONFIG.snap() as i32
                        {
                            // 吸附到右边缘
                            new_window_x = mon_wx + mon_ww - client_total_width;
                        }
                        if (mon_wy - new_window_y).abs() < CONFIG.snap() as i32 {
                            // 吸附到上边缘
                            new_window_y = mon_wy;
                        } else if ((mon_wy + mon_wh) - (new_window_y + client_total_height)).abs()
                            < CONFIG.snap() as i32
                        {
                            // 吸附到下边缘
                            new_window_y = mon_wy + mon_wh - client_total_height;
                        }

                        // e. 如果窗口是非浮动的，并且移动距离超过了吸附阈值，则将其切换为浮动状态
                        let (is_floating, client_current_x, client_current_y) = {
                            let c_borrow = c_rc.borrow();
                            (c_borrow.is_floating, c_borrow.x, c_borrow.y)
                        };
                        let current_layout_is_tile = {
                            let selmon_borrow = self.sel_mon.as_ref().unwrap().borrow();
                            selmon_borrow.lt[selmon_borrow.sel_lt].is_tile()
                        };

                        if !is_floating && current_layout_is_tile // 如果当前是平铺布局中的非浮动窗口
                        && ((new_window_x - client_current_x).abs() > CONFIG.snap() as i32 // 且 X 或 Y 方向移动超过阈值
                            || (new_window_y - client_current_y).abs() > CONFIG.snap() as i32)
                        {
                            self.togglefloating(std::ptr::null()); // 将其切换为浮动 (null_mut() 作为参数)
                        }

                        // f. 如果窗口是浮动的或者当前布局是浮动布局，则实际调整窗口大小/位置
                        let (window_w, window_h) = {
                            // 获取窗口内容区的宽高
                            let c_borrow = c_rc.borrow();
                            (c_borrow.w, c_borrow.h)
                        };
                        if !current_layout_is_tile || c_rc.borrow().is_floating {
                            // 重新检查 is_floating 因为 togglefloating 可能改变它
                            self.resize(c_rc, new_window_x, new_window_y, window_w, window_h, true);
                            // true 表示交互式调整
                        }
                    }
                    _ => {} // 忽略其他类型的事件
                }

                // g. 如果收到鼠标按钮释放事件，则结束拖动
                if ev.type_ == ButtonRelease {
                    break; // 跳出事件循环
                }
            } // loop 结束

            // 7. 释放鼠标抓取
            XUngrabPointer(self.dpy, CurrentTime);

            // 8. 检查窗口移动后是否跨越了显示器边界
            let (final_x, final_y, final_w, final_h) = {
                let c_borrow = c_rc.borrow();
                (c_borrow.x, c_borrow.y, c_borrow.w, c_borrow.h)
            };
            // 根据窗口最终位置的中心点或主要部分来确定其所属的显示器
            let target_monitor_opt = self.recttomon(final_x, final_y, final_w, final_h);

            // 如果窗口被移动到了不同的显示器
            if target_monitor_opt.is_some()
                && !Rc::ptr_eq(
                    target_monitor_opt.as_ref().unwrap(),
                    self.sel_mon.as_ref().unwrap(),
                )
            {
                self.sendmon(Some(c_rc.clone()), target_monitor_opt.clone()); // 将窗口发送到新显示器
                self.sel_mon = target_monitor_opt; // 更新当前选中的显示器
                self.focus(None); // 在新显示器上重新设置焦点
            }
        }
    }

    pub fn resizemouse(&mut self, _arg: *const Arg) {
        info!("[resizemouse]");
        unsafe {
            // 1. 获取当前选中的客户端 (c)
            let client_opt = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
            if client_opt.is_none() {
                return; // 没有选中客户端则返回
            }
            let c_rc = client_opt.as_ref().unwrap(); // c_rc 是 &Rc<RefCell<Client>>

            // 2. 全屏检查
            if c_rc.borrow().is_fullscreen {
                // no support resizing fullscreen windows by mouse
                return; // 不允许通过鼠标调整全屏窗口大小
            }

            // 3. 准备工作
            self.restack(self.sel_mon.clone()); // 将当前选中窗口置于堆叠顶部，并重绘状态栏

            // 保存窗口开始调整大小时的原始左上角坐标 (用于计算新尺寸的基准)
            // 和边框宽度 (用于从鼠标坐标计算内容区尺寸)
            let (original_client_x, original_client_y, border_width, client_window_id) = {
                let c_borrow = c_rc.borrow();
                (c_borrow.x, c_borrow.y, c_borrow.border_w, c_borrow.win)
            };

            // 4. 抓取鼠标指针 (XGrabPointer)
            if XGrabPointer(
                self.dpy,
                self.root,
                False,
                MOUSEMASK as u32, // event_mask: 关心的鼠标事件
                GrabModeAsync,    // pointer_mode
                GrabModeAsync,    // keyboard_mode
                0,                // confine_to: 不限制范围
                self.cursor[CUR::CurResize as usize]
                    .as_ref()
                    .unwrap()
                    .cursor, // 使用调整大小光标
                CurrentTime,
            ) != GrabSuccess
            {
                return; // 抓取失败则返回
            }

            // 5. 将鼠标指针移动到窗口的右下角，为用户提供调整大小的起点
            // 需要窗口当前的内容区宽高 w, h
            let (current_w, current_h) = {
                let c_borrow = c_rc.borrow();
                (c_borrow.w, c_borrow.h)
            };
            XWarpPointer(
                self.dpy,
                0,                // src_w: source window (0 for root relative to itself)
                client_window_id, // dest_w: destination window (the client window)
                0,
                0,
                0,
                0, // src_x, src_y, src_width, src_height (not used when dest_w is set)
                current_w + border_width - 1, // dest_x: 目标 X 坐标 (右下角内侧)
                current_h + border_width - 1, // dest_y: 目标 Y 坐标 (右下角内侧)
            );

            // 6. 初始化事件循环变量
            let mut last_motion_time: Time = 0; // 上一次处理 MotionNotify 的时间
            let mut ev: XEvent = zeroed(); // 用于存储事件

            // 7. 进入鼠标调整大小事件循环
            loop {
                XMaskEvent(
                    self.dpy,
                    MOUSEMASK | ExposureMask | SubstructureRedirectMask, // 监听的事件掩码
                    &mut ev,
                );

                match ev.type_ {
                    ConfigureRequest | Expose | MapRequest => {
                        // 处理其他可能在调整大小过程中发生的重要窗口事件
                        self.handler(ev.type_, &mut ev);
                    }
                    MotionNotify => {
                        // 如果是鼠标移动事件
                        // a. 节流
                        if ev.motion.time - last_motion_time <= (1000 / 60) {
                            // 约 60 FPS
                            continue;
                        }
                        last_motion_time = ev.motion.time;

                        // b. 计算新的内容区宽度 (new_width) 和高度 (new_height)
                        //    基于鼠标当前位置 (相对于根窗口的 ev.motion.x_root, ev.motion.y_root)
                        //    和窗口原始左上角坐标 (original_client_x, original_client_y)
                        //    以及边框宽度。
                        //    ev.motion.x/y 是相对于事件窗口的，这里我们假设事件窗口是根窗口（因为抓取时指定了root）
                        //    或者直接使用 ev.motion.x_root, ev.motion.y_root 更明确。
                        let current_mouse_root_x = ev.motion.x_root;
                        let current_mouse_root_y = ev.motion.y_root;

                        // let mut new_width =
                        //     (current_mouse_root_x - original_client_x - 2 * border_width + 1)
                        //         .max(1);
                        // let mut new_height =
                        //     (current_mouse_root_y - original_client_y - 2 * border_width + 1)
                        //         .max(1);
                        // .max(1) 确保宽高至少为1像素。
                        // +1 是因为坐标是从0开始，而尺寸是从1开始。
                        // -2*border_width 是因为我们计算的是内容区尺寸。

                        // c. 获取当前显示器工作区信息和客户端在其显示器上的信息
                        // let (mon_wx_of_client, mon_wy_of_client) = {
                        //     let c_borrow = c_rc.borrow();
                        //     let client_mon_borrow = c_borrow.mon.as_ref().unwrap().borrow();
                        //     (client_mon_borrow.wx, client_mon_borrow.wy)
                        // };
                        // let (selmon_wx, selmon_wy, selmon_ww, selmon_wh) = {
                        //     let selmon_borrow = self.selmon.as_ref().unwrap().borrow();
                        //     (
                        //         selmon_borrow.wx,
                        //         selmon_borrow.wy,
                        //         selmon_borrow.ww,
                        //         selmon_borrow.wh,
                        //     )
                        // };

                        // d. (可选) 可以在这里添加对新尺寸的边界检查或吸附逻辑，
                        //    例如，确保窗口的右下角不超过其所在显示器的边界。
                        //    dwp.c 的 resizemouse 逻辑更简单，主要依赖 XWarpPointer 后的相对移动。
                        //    它通常会将窗口的左上角固定，只调整右下角。
                        //    如果你的 resize 函数会处理好边界，这里可以简化。
                        //    我们暂时保持与 movemouse 类似的吸附检查，但作用于尺寸。
                        //    不过，更常见的调整大小是固定左上角，拖动右下角。
                        //    这里的 current_mouse_root_x - original_client_x 直接就是新宽度（内容区）

                        let new_width = (current_mouse_root_x - original_client_x)
                            .max(1 + 2 * border_width)
                            - 2 * border_width;
                        let new_height = (current_mouse_root_y - original_client_y)
                            .max(1 + 2 * border_width)
                            - 2 * border_width;

                        // e. 如果窗口是非浮动的，并且尺寸变化超过了吸附阈值，则将其切换为浮动状态
                        let (is_floating, current_client_w, current_client_h) = {
                            let c_borrow = c_rc.borrow();
                            (c_borrow.is_floating, c_borrow.w, c_borrow.h)
                        };
                        let current_layout_is_tile = {
                            let selmon_borrow = self.sel_mon.as_ref().unwrap().borrow();
                            selmon_borrow.lt[selmon_borrow.sel_lt].is_tile()
                        };

                        if !is_floating
                            && current_layout_is_tile
                            && ((new_width - current_client_w).abs() > CONFIG.snap() as i32
                                || (new_height - current_client_h).abs() > CONFIG.snap() as i32)
                        {
                            self.togglefloating(std::ptr::null_mut()); // null_mut() 作为参数
                        }

                        // f. 如果窗口是浮动的或者当前布局是浮动布局，则实际调整窗口大小
                        //    注意：传递给 resize 的是窗口的原始 X, Y 坐标，以及新的宽度和高度。
                        if !current_layout_is_tile || c_rc.borrow().is_floating {
                            // 重新检查 is_floating
                            self.resize(
                                c_rc,
                                original_client_x,
                                original_client_y,
                                new_width,
                                new_height,
                                true,
                            );
                        }
                    }
                    _ => {} // 忽略其他事件
                }

                // g. 如果收到鼠标按钮释放事件，则结束调整大小
                if ev.type_ == ButtonRelease {
                    break; // 跳出事件循环
                }
            } // loop 结束

            // 8. 再次将鼠标指针定位到窗口右下角 (可选，为了视觉一致性)
            let (final_client_w, final_client_h, final_border_width, final_client_win) = {
                let c_borrow = c_rc.borrow();
                (c_borrow.w, c_borrow.h, c_borrow.border_w, c_borrow.win)
            };
            XWarpPointer(
                self.dpy,
                0,
                final_client_win,
                0,
                0,
                0,
                0,
                final_client_w + final_border_width - 1,
                final_client_h + final_border_width - 1,
            );

            // 9. 释放鼠标抓取
            XUngrabPointer(self.dpy, CurrentTime);

            // 10. 清理可能由于 XWarpPointer 产生的多余 EnterNotify 事件
            //     (dwp.c 中的做法，确保焦点状态正确)
            while XCheckMaskEvent(self.dpy, EnterWindowMask, &mut ev) > 0 {}

            // 11. 检查窗口调整大小后是否跨越了显示器边界 (通常调整大小不改变显示器)
            //     但这部分逻辑与 movemouse 类似，以防万一或特殊情况。
            //     对于 resizemouse，窗口的左上角通常是固定的，所以不太可能改变显示器。
            //     但如果允许从左上角拖动来改变大小，则需要这个检查。
            //     假设当前实现是固定左上角，拖动右下角。
            let (final_x, final_y, final_w_after_resize, final_h_after_resize) = {
                let c_borrow = c_rc.borrow();
                (c_borrow.x, c_borrow.y, c_borrow.w, c_borrow.h)
            };
            let target_monitor_opt =
                self.recttomon(final_x, final_y, final_w_after_resize, final_h_after_resize);

            if target_monitor_opt.is_some()
                && !Rc::ptr_eq(
                    target_monitor_opt.as_ref().unwrap(),
                    self.sel_mon.as_ref().unwrap(),
                )
            {
                // 理论上，如果只拖动右下角调整大小，这部分不太可能触发，除非resize逻辑中有特殊处理
                self.sendmon(Some(c_rc.clone()), target_monitor_opt.clone());
                self.sel_mon = target_monitor_opt;
                self.focus(None);
            }
        }
    }

    pub fn updatenumlockmask(&mut self) {
        // info!("[updatenumlockmask]"); // 日志记录，已注释
        unsafe {
            // unsafe 块，因为直接与 Xlib C API 交互，处理裸指针
            // 1. 初始化 numlockmask 为 0
            self.numlock_mask = 0; // 这个掩码将用于存储 Num_Lock 键对应的修饰符位

            // 2. 获取当前的修饰键映射
            // XGetModifierMapping 返回一个指向 XModifierKeymap 结构体的指针。
            // 这个结构体描述了哪些物理键码 (keycode) 被映射为哪些修饰符 (Shift, Lock, Control, Mod1-Mod5)。
            let modmap_ptr = XGetModifierMapping(self.dpy);
            // modmap_ptr 是 *mut XModifierKeymap

            if modmap_ptr.is_null() {
                // 防御性检查，虽然 XGetModifierMapping 通常会返回有效指针
                error!("[updatenumlockmask] XGetModifierMapping returned null pointer!");
                return;
            }
            let modmap_ref = &*modmap_ptr; // 将裸指针转换为引用，方便访问其字段

            // 3. 遍历修饰键映射表
            // XModifierKeymap 结构中的 modifiermap 字段是一个 KeyCode 数组。
            // 这个数组的组织方式是：
            // - 总共有 8 种修饰符 (Shift, Lock, Control, Mod1, Mod2, Mod3, Mod4, Mod5)。
            // - 每种修饰符可以由多个物理键触发 (由 max_keypermod 指定，例如 CapsLock 和 ShiftLock 都可能触发 Lock 修饰符)。
            // - 数组的布局是：
            //   [Shift的键码1, Shift的键码2, ..., Shift的键码N,
            //    Lock的键码1,  Lock的键码2,  ..., Lock的键码N,
            //    ...
            //    Mod5的键码1,  Mod5的键码2,  ..., Mod5的键码N]
            //   其中 N 是 max_keypermod。

            for i in 0..8 {
                // 遍历 8 种修饰符 (索引 0 到 7)
                for j in 0..modmap_ref.max_keypermod {
                    // 遍历每种修饰符可能对应的物理键码
                    // 计算当前键码在 modifiermap 数组中的索引
                    let keycode_index = (i * modmap_ref.max_keypermod + j) as usize;
                    // 获取该索引处的键码值
                    let current_keycode = *modmap_ref.modifiermap.add(keycode_index);
                    // 使用 .add(index) 进行指针运算，然后解引用 * 来获取值。
                    // wrapping_add 在原始代码中是为了防止溢出，但对于 usize 索引通常用 .add()。

                    // 将 XK_Num_Lock (Keysym) 转换为对应的键码 (KeyCode)
                    let num_lock_keycode = XKeysymToKeycode(self.dpy, XK_Num_Lock as u64);

                    // 如果当前修饰键映射表中的键码等于 Num_Lock 键的键码，
                    // 并且该键码不为0 (0 表示该槽位未使用)
                    if current_keycode != 0 && current_keycode == num_lock_keycode {
                        // 则说明第 i 个修饰符位 (1 << i) 对应于 Num_Lock 键
                        self.numlock_mask = 1 << i;
                        // 找到了 Num_Lock 对应的修饰符位，可以提前退出循环
                        // （假设 Num_Lock 只会映射到一个修饰符位，或者我们只关心第一个找到的）
                        // XFreeModifiermap(modmap_ptr); // 需要在找到后就释放，或者在函数末尾统一释放
                        // return; // 如果只找第一个，找到就返回
                        break; // 假设一个键只映射到一个修饰符类型的一个槽位，或者我们只取第一个
                    }
                }
                if self.numlock_mask != 0 {
                    // 如果在内层循环中已经找到了 numlockmask
                    break; // 也可以跳出外层循环
                }
            }

            // 4. 释放由 XGetModifierMapping 分配的内存
            XFreeModifiermap(modmap_ptr);
        }
    }

    pub fn setclienttagprop(&mut self, c: &Rc<RefCell<Client>>) {
        let client_mut = c.borrow();
        let data: [u32; 2] = [
            client_mut.tags,
            client_mut.mon.as_ref().unwrap().borrow().num as u32,
        ];
        unsafe {
            XChangeProperty(
                self.dpy,
                client_mut.win,
                self.net_atom[NET::NetClientInfo as usize],
                XA_CARDINAL,
                32,
                PropModeReplace,
                data.as_ptr() as *const u8,
                2,
            );
        }
    }

    pub fn grabbuttons(&mut self, client_opt: Option<Rc<RefCell<Client>>>, focused: bool) {
        self.updatenumlockmask();
        let client_win_id = match client_opt.as_ref() {
            // 获取窗口 ID，只需不可变借用
            Some(c_rc) => c_rc.borrow().win,
            None => return, // 如果 client_opt 是 None，则直接返回
        };

        unsafe {
            let modifiers_to_try = [0, LockMask, self.numlock_mask, self.numlock_mask | LockMask];

            XUngrabButton(self.dpy, AnyButton as u32, AnyModifier, client_win_id);

            if !focused {
                XGrabButton(
                    self.dpy,
                    AnyButton as u32,
                    AnyModifier,
                    client_win_id,
                    False,
                    BUTTONMASK as u32,
                    GrabModeSync,
                    GrabModeSync,
                    0,
                    0,
                );
            }

            for button_config in CONFIG.get_buttons().iter() {
                // 使用迭代器
                if button_config.click == CLICK::ClkClientWin as u32 {
                    for &modifier_combo in modifiers_to_try.iter() {
                        XGrabButton(
                            self.dpy,
                            button_config.button,
                            button_config.mask | modifier_combo,
                            client_win_id,
                            False,
                            BUTTONMASK as u32,
                            GrabModeAsync,
                            GrabModeAsync,
                            0,
                            0,
                        );
                    }
                }
            }
        }
    }

    pub fn grabkeys(&mut self) {
        // info!("[grabkeys]"); // 日志
        self.updatenumlockmask(); // 更新 NumLock 修饰键掩码
        unsafe {
            // unsafe 块
            // 准备修饰键组合，与 grabbuttons 中类似
            let modifiers_to_try = [0, LockMask, self.numlock_mask, self.numlock_mask | LockMask];

            // 1. 取消之前对根窗口所有按键的任何抓取
            XUngrabKey(
                self.dpy,
                AnyKey,      // AnyKey: 通配符，表示所有键盘按键
                AnyModifier, // AnyModifier: 通配符，表示任何修饰键组合
                self.root,   // target_window: 根窗口 (全局快捷键通常在根窗口上抓取)
            );

            // 2. 获取当前键盘映射信息
            let mut min_keycode: i32 = 0; // 最小键码
            let mut max_keycode: i32 = 0; // 最大键码
            let mut keysyms_per_keycode: i32 = 0; // 每个键码通常关联的 Keysym 数量 (例如，区分大小写、Shift 等)

            // 获取 X server 当前使用的键码范围
            XDisplayKeycodes(self.dpy, &mut min_keycode, &mut max_keycode);
            // 获取从 min_keycode 到 max_keycode 的所有键码对应的 Keysym 映射表
            // syms_ptr 是一个指向 Keysym 数组的指针。
            // 数组的组织方式是：对于每个键码 k (从 min_keycode 开始)，
            // 其对应的 Keysym 存储在 syms_ptr + (k - min_keycode) * keysyms_per_keycode 的位置。
            // 我们通常只关心每个键码的第一个 Keysym (index 0)。
            let syms_ptr = XGetKeyboardMapping(
                self.dpy,
                min_keycode as u8,             // first_keycode_wanted
                max_keycode - min_keycode + 1, // count: 要获取的键码数量
                &mut keysyms_per_keycode,      // keysyms_per_keycode_return
            );

            if syms_ptr.is_null() {
                // 如果获取映射失败
                error!("[grabkeys] XGetKeyboardMapping returned null pointer!");
                return;
            }

            // 3. 遍历所有可能的物理键码 (keycode)
            for keycode_val in min_keycode..=max_keycode {
                // 4. 遍历 CONFIG.rs 中定义的所有键盘快捷键绑定 (keys)
                for key_config in CONFIG.get_keys().iter() {
                    // 使用迭代器
                    // 获取当前物理键码 keycode_val 对应的第一个 Keysym
                    // (k - start) * skip 逻辑在C中用于访问二维数组，这里是 (keycode_val - min_keycode) * keysyms_per_keycode
                    // 我们只关心第一个 Keysym，所以偏移量是 0
                    let current_keysym =
                        *syms_ptr.add(((keycode_val - min_keycode) * keysyms_per_keycode) as usize);
                    //  wrapping_add 也是为了防止溢出，但对于 usize 索引，直接 .add() 更常见。

                    // 如果当前物理键码对应的 Keysym 与配置中快捷键的 Keysym 匹配
                    if key_config.keysym == current_keysym {
                        // 则为这个快捷键抓取事件，并尝试所有修饰键组合
                        for &modifier_combo in modifiers_to_try.iter() {
                            XGrabKey(
                                self.dpy,
                                keycode_val,                      // keycode: 当前物理键码
                                key_config.mod0 | modifier_combo, // modifiers: 配置的掩码 + 额外修饰符
                                self.root,                        // grab_window: 根窗口
                                True, // owner_events: True, 如果其他窗口也选了这个事件，它们也会收到
                                GrabModeAsync, // pointer_mode
                                GrabModeAsync, // keyboard_mode
                            );
                        }
                    }
                }
            }

            // 5. 释放由 XGetKeyboardMapping 分配的内存
            XFree(syms_ptr as *mut _);
        }
    }

    pub fn sendevent(&mut self, client_mut: &mut Client, proto: Atom) -> bool {
        info!("[sendevent] {}", client_mut);
        let mut protocols: *mut Atom = null_mut();
        let mut n: i32 = 0;
        let mut exists: bool = false;
        unsafe {
            let mut ev: XEvent = zeroed();
            if XGetWMProtocols(self.dpy, client_mut.win, &mut protocols, &mut n) > 0 {
                while !exists && {
                    n -= 1;
                    n
                } > 0
                {
                    exists = *protocols.add(n as usize) == proto;
                }
                if !protocols.is_null() {
                    XFree(protocols as *mut _);
                }
            }
            if exists {
                ev.type_ = ClientMessage;
                ev.client_message.window = client_mut.win;
                ev.client_message.message_type = self.wm_atom[WM::WMProtocols as usize];
                ev.client_message.format = 32;
                ev.client_message.data.as_longs_mut()[0] = proto as i64;
                ev.client_message.data.as_longs_mut()[1] = CurrentTime as i64;
                XSendEvent(self.dpy, client_mut.win, False, NoEventMask, &mut ev);
            }
        }
        return exists;
    }

    pub fn setfocus(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[setfocus]");
        unsafe {
            let mut c = c.borrow_mut();
            if !c.never_focus {
                XSetInputFocus(self.dpy, c.win, RevertToPointerRoot, CurrentTime);
                XChangeProperty(
                    self.dpy,
                    self.root,
                    self.net_atom[NET::NetActiveWindow as usize],
                    XA_WINDOW,
                    32,
                    PropModeReplace,
                    &mut c.win as *const u64 as *const _,
                    1,
                );
            }
            self.sendevent(&mut c, self.wm_atom[WM::WMTakeFocus as usize]);
        }
    }

    pub fn enternotify(&mut self, e: *mut XEvent) {
        // info!("[enternotify]"); // 日志
        unsafe {
            // unsafe 块
            let ev = (*e).crossing; // 获取 CrossingEvent (EnterNotify 和 LeaveNotify 共用)

            if (ev.mode != NotifyNormal || ev.detail == NotifyInferior) && ev.window != self.root {
                return;
            }

            // 检查是否进入状态栏
            if let Some(&monitor_id) = self.status_bar_windows.get(&ev.window) {
                // 状态栏不改变焦点，但可能需要切换显示器
                if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
                    if !Rc::ptr_eq(&monitor, self.sel_mon.as_ref().unwrap()) {
                        let sel = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
                        self.unfocus(sel, true);
                        self.sel_mon = Some(monitor);
                        self.focus(None);
                    }
                }
                return;
            }
            // 常规的 enternotify 处理

            // 2. 确定事件相关的客户端 (c) 和显示器 (m)
            let client_rc_opt = self.wintoclient(ev.window); // 尝试将事件窗口 ID 转换为 JWM 管理的 Client
            let monitor_rc_opt = if let Some(ref c_rc) = client_rc_opt {
                c_rc.borrow().mon.clone() // 克隆 Option<Rc<RefCell<Monitor>>>
            } else {
                // 如果事件窗口不是已管理的客户端 (例如，是根窗口)
                self.wintomon(ev.window) // 则尝试根据窗口 ID (可能是根窗口) 确定其所在的显示器
            };

            // 如果无法确定显示器 (例如，wintomon 返回 None)，则不处理
            if monitor_rc_opt.is_none() {
                return;
            }
            let current_event_monitor_rc = monitor_rc_opt.as_ref().unwrap(); // &Rc<RefCell<Monitor>>

            // 3. 处理显示器焦点切换 (如果鼠标进入了非当前选中显示器)
            let is_on_selected_monitor =
                Rc::ptr_eq(current_event_monitor_rc, self.sel_mon.as_ref().unwrap());
            // .unwrap() 可能 panic

            if !is_on_selected_monitor {
                // 如果鼠标进入的显示器不是当前 JWM 选中的显示器
                let previously_selected_client_opt = {
                    // 获取旧选中显示器上的选中客户端
                    let selmon_borrow = self.sel_mon.as_ref().unwrap().borrow();
                    selmon_borrow.sel.clone()
                };
                self.unfocus(previously_selected_client_opt, true); // 从旧显示器的选中客户端上移除焦点 (视觉上)，并将 X 焦点设回根
                self.sel_mon = Some(current_event_monitor_rc.clone()); // 更新 JWM 的选中显示器为当前事件发生的显示器
                                                                       // .clone() 是克隆 Rc
            }

            // 4. 处理客户端焦点切换 (如果鼠标进入了与当前选中客户端不同的客户端)
            //    或者，如果显示器切换了，即使 client_rc_opt 是 None (进入根窗口)，也需要重新聚焦
            if !is_on_selected_monitor // 如果切换了显示器
           || client_rc_opt.is_none() // 或者鼠标进入了根窗口 (没有具体客户端)
           || !Self::are_equal_rc(&client_rc_opt, &self.sel_mon.as_ref().unwrap().borrow().sel)
            {
                self.focus(client_rc_opt);
            }
        }
    }

    pub fn expose(&mut self, e: *mut XEvent) {
        // info!("[expose]");
        unsafe {
            let ev = (*e).expose;
            if ev.count != 0 {
                return;
            }

            if let Some(m_ref) = self.wintomon(ev.window).as_ref() {
                self.mark_bar_update_needed(Some(m_ref.borrow().num));
            }
        }
    }

    pub fn focus(&mut self, mut c_opt: Option<Rc<RefCell<Client>>>) {
        // info!("[focus]");
        unsafe {
            {
                // 如果传入的是状态栏客户端，忽略并寻找合适的替代
                if let Some(ref c) = c_opt {
                    if self.status_bar_windows.contains_key(&c.borrow().win) {
                        c_opt = None; // 忽略状态栏
                    }
                }

                let (isvisible, _) = match c_opt.clone() {
                    Some(c_rc) => (c_rc.borrow().isvisible(), c_rc.borrow().is_status_bar()),
                    _ => (false, false),
                };
                if !isvisible {
                    if let Some(ref sel_mon_opt) = self.sel_mon {
                        c_opt = sel_mon_opt.borrow_mut().stack.clone();
                    }
                    while let Some(c_rc) = c_opt.clone() {
                        let c_borrow = c_rc.borrow();
                        if c_borrow.isvisible() {
                            break;
                        }
                        let next = c_rc.borrow().stack_next.clone();
                        c_opt = next;
                    }
                }
                let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
                if sel.is_some() && !Self::are_equal_rc(&sel, &c_opt) {
                    self.unfocus(sel.clone(), false);
                }
            }
            if let Some(c_rc) = c_opt.clone() {
                if !Rc::ptr_eq(
                    c_rc.borrow().mon.as_ref().unwrap(),
                    self.sel_mon.as_ref().unwrap(),
                ) {
                    self.sel_mon = c_rc.borrow().mon.clone();
                }
                if c_rc.borrow().is_urgent {
                    self.seturgent(&c_rc, false);
                }
                self.detachstack(Some(c_rc.clone()));
                self.attachstack(Some(c_rc.clone()));
                self.grabbuttons(Some(c_rc.clone()), true);
                XSetWindowBorder(
                    self.dpy,
                    c_rc.borrow().win,
                    self.theme_manager
                        .get_scheme(SchemeType::Sel)
                        .border_color()
                        .pixel,
                );
                self.setfocus(&c_rc);
            } else {
                XSetInputFocus(self.dpy, self.root, RevertToPointerRoot, CurrentTime);
                XDeleteProperty(
                    self.dpy,
                    self.root,
                    self.net_atom[NET::NetActiveWindow as usize],
                );
            }
            if let Some(sel_mon_opt) = self.sel_mon.as_mut() {
                let mut sel_mon_mut = sel_mon_opt.borrow_mut();
                sel_mon_mut.sel = c_opt.clone();
                let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
                sel_mon_mut.pertag.as_mut().unwrap().sel[cur_tag] = c_opt.clone();
            }

            self.mark_bar_update_needed(None);
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
                self.theme_manager
                    .get_scheme(SchemeType::Norm)
                    .border_color()
                    .pixel,
            );
            if setfocus {
                XSetInputFocus(self.dpy, self.root, RevertToPointerRoot, CurrentTime);
                XDeleteProperty(
                    self.dpy,
                    self.root,
                    self.net_atom[NET::NetActiveWindow as usize],
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
        let sel_tags = { m.as_ref().unwrap().borrow().sel_tags };
        {
            c.as_ref().unwrap().borrow_mut().tags = m.as_ref().unwrap().borrow().tag_set[sel_tags]
        };
        self.attach(c.clone());
        self.attachstack(c.clone());
        self.setclienttagprop(c.as_ref().unwrap());
        self.focus(None);
        self.arrange(None);
    }

    pub fn setclientstate(&mut self, c: &Rc<RefCell<Client>>, state: i64) {
        // info!("[setclientstate]");
        unsafe {
            let data_to_set: [i64; 2] = [state, 0]; // 0 代表 None (无图标窗口)
            let win = c.borrow().win;
            XChangeProperty(
                self.dpy,
                win,
                self.wm_atom[WM::WMState as usize],
                self.wm_atom[WM::WMState as usize],
                32,
                PropModeReplace,
                data_to_set.as_ptr() as *const u8,
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
            let keys = CONFIG.get_keys();
            for i in 0..keys.len() {
                if keysym == keys[i].keysym
                    && self.CLEANMASK(keys[i].mod0) == self.CLEANMASK(ev.state)
                    && keys[i].func.is_some()
                {
                    info!("[keypress] i: {}, arg: {:?}", i, keys[i].arg);
                    keys[i].func.unwrap()(self, &keys[i].arg);
                }
            }
        }
    }

    pub fn manage(&mut self, w: Window, wa_ptr: *mut XWindowAttributes) {
        // info!("[manage]"); // 日志

        // --- 1. 创建新的 Client 对象 ---
        let client_rc_opt: Option<Rc<RefCell<Client>>> = Some(Rc::new(RefCell::new(Client::new())));
        let client_rc = client_rc_opt.as_ref().unwrap();
        unsafe {
            let window_attributes = &*wa_ptr;
            // --- 2. 初始化 Client 结构体的基本属性 ---
            {
                let mut client_mut = client_rc.borrow_mut();
                // 设置窗口 ID
                client_mut.win = w;
                // 从传入的 XWindowAttributes 中获取初始的几何信息和边框宽度
                client_mut.x = window_attributes.x;
                client_mut.old_x = window_attributes.x;
                client_mut.y = window_attributes.y;
                client_mut.old_y = window_attributes.y;
                client_mut.w = window_attributes.width;
                client_mut.old_w = window_attributes.width;
                client_mut.h = window_attributes.height;
                client_mut.old_h = window_attributes.height;
                client_mut.old_border_w = window_attributes.border_width;
                client_mut.client_fact = 1.0;

                // 获取并设置窗口标题
                self.updatetitle(&mut client_mut);
                #[cfg(any(feature = "nixgl", feature = "tauri_bar"))]
                {
                    if client_mut.name == CONFIG.status_bar_name() {
                        let mut instance_name = String::new();
                        for &tmp_num in self.status_bar_child.keys() {
                            if !self.status_bar_clients.contains_key(&tmp_num) {
                                instance_name = match tmp_num {
                                    0 => CONFIG.status_bar_0().to_string(),
                                    1 => CONFIG.status_bar_1().to_string(),
                                    _ => CONFIG.status_bar_name().to_string(),
                                };
                            }
                        }
                        if !instance_name.is_empty() {
                            let _ = self.set_class_info(
                                &mut client_mut,
                                instance_name.as_str(),
                                instance_name.as_str(),
                            );
                        }
                    }
                }
                self.update_class_info(&mut client_mut);
                info!("[manage] {}", client_mut);

                if client_mut.is_status_bar() {
                    drop(client_mut);
                    info!("[manage] Detected status bar, managing as statusbar");
                    self.manage_statusbar(client_rc, wa_ptr);
                    return; // 直接返回，不执行常规管理流程
                }
            }

            // 常规客户端管理流程
            self.manage_regular_client(client_rc, wa_ptr);
        }
    }

    fn setup_client_window(&mut self, client_rc: &Rc<RefCell<Client>>) {
        unsafe {
            let mut window_changes: XWindowChanges = std::mem::zeroed();
            let win = client_rc.borrow().win;

            // 设置边框
            {
                let mut client_mut = client_rc.borrow_mut();
                client_mut.border_w = CONFIG.border_px() as i32;
                window_changes.border_width = client_mut.border_w;
            }

            // 应用边框宽度到实际窗口
            XConfigureWindow(self.dpy, win, CWBorderWidth as u32, &mut window_changes);

            // 设置边框颜色为"正常"状态的颜色
            XSetWindowBorder(
                self.dpy,
                win,
                self.theme_manager
                    .get_scheme(SchemeType::Norm)
                    .border_color()
                    .pixel,
            );

            // 发送 ConfigureNotify 事件给客户端
            {
                let mut client_mut = client_rc.borrow_mut();
                self.configure(&mut client_mut);
            }

            // 设置窗口在屏幕外的临时位置（避免闪烁）
            {
                let client_borrow = client_rc.borrow();
                XMoveResizeWindow(
                    self.dpy,
                    win,
                    client_borrow.x + 2 * self.s_w, // 移到屏幕外
                    client_borrow.y,
                    client_borrow.w as u32,
                    client_borrow.h as u32,
                );
            }

            // 设置客户端的 WM_STATE 为 NormalState
            self.setclientstate(client_rc, NormalState as i64);

            info!("[setup_client_window] Window setup completed for {}", win);
        }
    }

    fn register_client_events(&mut self, client_rc: &Rc<RefCell<Client>>) {
        unsafe {
            let win = client_rc.borrow().win;

            // 为窗口选择要监听的事件
            XSelectInput(
                self.dpy,
                win,
                EnterWindowMask |        // 鼠标进入窗口
                FocusChangeMask |        // 焦点变化
                PropertyChangeMask |     // 窗口属性变化
                StructureNotifyMask, // 窗口结构变化（如被销毁、调整大小等）
            );

            // 为窗口抓取鼠标按钮事件（用于窗口管理操作）
            self.grabbuttons(Some(client_rc.clone()), false); // false 表示窗口初始不是焦点

            // 更新 EWMH _NET_CLIENT_LIST 属性
            XChangeProperty(
                self.dpy,
                self.root,
                self.net_atom[NET::NetClientList as usize],
                XA_WINDOW,
                32,
                PropModeAppend, // 追加到现有列表
                &client_rc.borrow().win as *const Window as *const _,
                1,
            );

            info!(
                "[register_client_events] Events registered for window {}",
                win
            );
        }
    }

    // 更新完整的客户端列表（在需要时调用）
    fn update_net_client_list(&mut self) {
        unsafe {
            // 清空现有列表
            XDeleteProperty(
                self.dpy,
                self.root,
                self.net_atom[NET::NetClientList as usize],
            );

            // 重新构建列表
            let mut m = self.mons.clone();
            while let Some(ref m_opt) = m {
                let mut c = m_opt.borrow().clients.clone();
                while let Some(ref client_opt) = c {
                    XChangeProperty(
                        self.dpy,
                        self.root,
                        self.net_atom[NET::NetClientList as usize],
                        XA_WINDOW,
                        32,
                        PropModeAppend,
                        &client_opt.borrow().win as *const Window as *const _,
                        1,
                    );
                    let next = client_opt.borrow().next.clone();
                    c = next;
                }
                let next = m_opt.borrow().next.clone();
                m = next;
            }

            info!("[update_net_client_list] Updated _NET_CLIENT_LIST");
        }
    }

    fn handle_new_client_focus(&mut self, client_rc: &Rc<RefCell<Client>>) {
        // 检查新窗口所在的显示器是否是当前选中的显示器
        let current_client_monitor_is_selected_monitor = {
            let client_borrow = client_rc.borrow();
            match &client_borrow.mon {
                Some(client_mon) => match &self.sel_mon {
                    Some(sel_mon) => Rc::ptr_eq(client_mon, sel_mon),
                    None => false,
                },
                None => false,
            }
        };

        if current_client_monitor_is_selected_monitor {
            // 取消当前选中窗口的焦点
            let prev_sel_opt = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
            if prev_sel_opt.is_some() {
                self.unfocus(prev_sel_opt, false); // false: 不立即设置根窗口焦点
                info!("[handle_new_client_focus] Unfocused previous client");
            }

            // 将新窗口设为其所在显示器的选中窗口
            {
                let client_monitor_rc = client_rc.borrow().mon.clone().unwrap();
                client_monitor_rc.borrow_mut().sel = Some(client_rc.clone());
            }

            // 重新排列该显示器的窗口
            let client_monitor_rc = client_rc.borrow().mon.clone().unwrap();
            self.arrange(Some(client_monitor_rc));

            // 设置焦点到新窗口（如果它不是 never_focus）
            if !client_rc.borrow().never_focus {
                self.focus(Some(client_rc.clone()));
                info!(
                    "[handle_new_client_focus] Focused new client: {}",
                    client_rc.borrow().name
                );
            } else {
                // 如果新窗口是 never_focus，重新评估焦点
                self.focus(None);
                info!("[handle_new_client_focus] New client is never_focus, re-evaluated focus");
            }
        } else {
            // 如果新窗口不在当前选中的显示器上
            // 将新窗口设为其所在显示器的选中窗口
            {
                let client_monitor_rc = client_rc.borrow().mon.clone().unwrap();
                client_monitor_rc.borrow_mut().sel = Some(client_rc.clone());
            }

            // 只重新排列该显示器，不改变全局焦点
            let client_monitor_rc = client_rc.borrow().mon.clone().unwrap();
            self.arrange(Some(client_monitor_rc));

            info!("[handle_new_client_focus] New client on non-selected monitor, arranged only");
        }

        // 根据配置决定是否自动切换到新窗口的显示器
        if CONFIG.behavior().focus_follows_new_window {
            let client_monitor = client_rc.borrow().mon.clone();
            if let Some(new_mon) = client_monitor {
                if !Rc::ptr_eq(&new_mon, self.sel_mon.as_ref().unwrap()) {
                    // 切换到新窗口的显示器
                    let old_sel = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
                    self.unfocus(old_sel, true);
                    self.sel_mon = Some(new_mon);
                    self.focus(Some(client_rc.clone()));
                    info!("[handle_new_client_focus] Switched to new window's monitor");
                }
            }
        }
    }

    // 分离出来的常规客户端管理
    fn manage_regular_client(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        _: *mut XWindowAttributes,
    ) {
        // 处理 WM_TRANSIENT_FOR
        let mut transient_for_win: Window = 0;
        unsafe {
            if XGetTransientForHint(self.dpy, client_rc.borrow().win, &mut transient_for_win) > 0 {
                if let Some(temp_transient_client) = self.wintoclient(transient_for_win) {
                    let mut client_mut = client_rc.borrow_mut();
                    let transient_main_client = temp_transient_client.borrow();
                    client_mut.mon = transient_main_client.mon.clone();
                    client_mut.tags = transient_main_client.tags;
                } else {
                    client_rc.borrow_mut().mon = self.sel_mon.clone();
                    self.applyrules(&client_rc);
                }
            } else {
                client_rc.borrow_mut().mon = self.sel_mon.clone();
                self.applyrules(&client_rc);
            }
        }
        // 调整窗口位置
        self.adjust_client_position(&client_rc);
        // 设置窗口属性
        self.setup_client_window(&client_rc);
        // 更新各种提示
        self.updatewindowtype(&client_rc);
        self.updatesizehints(&client_rc);
        self.updatewmhints(&client_rc);
        // 添加到管理链表
        self.attach(Some(client_rc.clone()));
        self.attachstack(Some(client_rc.clone()));
        // 注册事件和抓取按钮
        self.register_client_events(&client_rc);
        // 更新客户端列表
        self.update_net_client_list();
        // 映射窗口
        unsafe {
            XMapWindow(self.dpy, client_rc.borrow().win);
        }
        // 处理焦点
        self.handle_new_client_focus(&client_rc);
    }

    fn manage_statusbar(&mut self, client_rc: &Rc<RefCell<Client>>, _: *mut XWindowAttributes) {
        unsafe {
            // 确定状态栏所属的显示器
            let monitor_id;
            // 配置状态栏客户端
            {
                let mut client_mut = client_rc.borrow_mut();
                monitor_id = self.determine_statusbar_monitor(&mut client_mut);
                info!("[manage_statusbar] monitor_id: {}", monitor_id);
                client_mut.mon = self.get_monitor_by_id(monitor_id);
                client_mut.never_focus = true;
                client_mut.is_floating = true;
                client_mut.tags = CONFIG.tagmask(); // 在所有标签可见
                client_mut.border_w = CONFIG.border_px() as i32;

                // 调整状态栏位置（通常在顶部）
                self.position_statusbar(&mut client_mut, monitor_id);
                // 设置状态栏特有的窗口属性
                self.setup_statusbar_window(&mut client_mut);
            }

            // 注册状态栏到管理映射中
            self.status_bar_clients
                .insert(monitor_id, client_rc.clone());
            self.status_bar_windows
                .insert(client_rc.borrow().win, monitor_id);

            // 映射状态栏窗口
            XMapWindow(self.dpy, client_rc.borrow().win);

            // 确保状态栏位于最上层
            XRaiseWindow(self.dpy, client_rc.borrow().win);

            info!(
                "[manage_statusbar] Successfully managed statusbar on monitor {}",
                monitor_id
            );
        }
    }

    // 确定状态栏应该在哪个显示器
    fn determine_statusbar_monitor(&self, client: &Client) -> i32 {
        info!("[determine_statusbar_monitor]: {}", client);
        if let Some(suffix) = client
            .name
            .strip_prefix(&format!("{}_", CONFIG.status_bar_name()))
        {
            if let Ok(monitor_id) = suffix.parse::<i32>() {
                return monitor_id;
            }
        }
        if let Some(suffix) = client
            .class
            .strip_prefix(&format!("{}_", CONFIG.status_bar_name()))
        {
            if let Ok(monitor_id) = suffix.parse::<i32>() {
                return monitor_id;
            }
        }
        if let Some(suffix) = client
            .instance
            .strip_prefix(&format!("{}_", CONFIG.status_bar_name()))
        {
            if let Ok(monitor_id) = suffix.parse::<i32>() {
                return monitor_id;
            }
        }
        self.sel_mon.as_ref().map(|m| m.borrow().num).unwrap_or(0)
    }

    // 定位状态栏
    fn position_statusbar(&mut self, client_mut: &mut Client, monitor_id: i32) {
        if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
            let monitor_borrow = monitor.borrow();

            // 将状态栏放在显示器顶部
            client_mut.x = monitor_borrow.m_x;
            client_mut.y = monitor_borrow.m_y;
            client_mut.w = monitor_borrow.m_w;
            // 高度由 status bar 自己决定，或使用默认值
            if client_mut.h <= 0 {
                client_mut.h = 30;
            }
            info!(
                "[position_statusbar] Positioned at ({}, {}) {}x{}",
                client_mut.x, client_mut.y, client_mut.w, client_mut.h
            );
        }
    }

    // 设置状态栏窗口属性
    fn setup_statusbar_window(&mut self, client_mut: &mut Client) {
        unsafe {
            let win = client_mut.win;
            // 状态栏只需要监听结构变化和属性变化
            XSelectInput(
                self.dpy,
                win,
                StructureNotifyMask | PropertyChangeMask | EnterWindowMask,
            );
            // 发送配置通知
            self.configure(client_mut);
        }
    }

    // 实时同步状态栏几何信息
    #[allow(dead_code)]
    pub fn sync_statusbar_geometry(&mut self, monitor_id: i32) -> bool {
        if let Some(statusbar) = self.status_bar_clients.get(&monitor_id) {
            unsafe {
                let win = statusbar.borrow().win;
                let mut wa: XWindowAttributes = std::mem::zeroed();

                // 从 X11 获取实际的窗口几何信息
                if XGetWindowAttributes(self.dpy, win, &mut wa) > 0 {
                    let mut statusbar_mut = statusbar.borrow_mut();
                    let mut changed = false;

                    // 比较并更新几何信息
                    if statusbar_mut.x != wa.x {
                        info!(
                            "[sync_statusbar_geometry] X: {} -> {}",
                            statusbar_mut.x, wa.x
                        );
                        statusbar_mut.x = wa.x;
                        changed = true;
                    }
                    if statusbar_mut.y != wa.y {
                        info!(
                            "[sync_statusbar_geometry] Y: {} -> {}",
                            statusbar_mut.y, wa.y
                        );
                        statusbar_mut.y = wa.y;
                        changed = true;
                    }
                    if statusbar_mut.w != wa.width {
                        info!(
                            "[sync_statusbar_geometry] W: {} -> {}",
                            statusbar_mut.w, wa.width
                        );
                        statusbar_mut.w = wa.width;
                        changed = true;
                    }
                    if statusbar_mut.h != wa.height {
                        info!(
                            "[sync_statusbar_geometry] H: {} -> {}",
                            statusbar_mut.h, wa.height
                        );
                        statusbar_mut.h = wa.height;
                        changed = true;
                    }

                    return changed;
                }
            }
        }
        false
    }

    pub fn client_y_offset(&mut self, m: &Monitor) -> i32 {
        let monitor_id = m.num;
        // 同步状态栏几何信息
        if self.sync_statusbar_geometry(monitor_id) {
            info!(
                "[client_y_offset] Synced statusbar geometry for monitor {}",
                monitor_id
            );
        }

        if let Some(statusbar) = self.status_bar_clients.get(&monitor_id) {
            let statusbar_borrow = statusbar.borrow();
            let offset = statusbar_borrow.y + statusbar_borrow.h + CONFIG.status_bar_pad();
            info!(
                "[client_y_offset] Monitor {}: offset = {} (statusbar_h: {} + pad: {})",
                monitor_id,
                offset,
                statusbar_borrow.h,
                CONFIG.status_bar_pad()
            );
            return offset.max(0);
        }

        0
    }

    // 验证并修正状态栏几何配置
    #[allow(dead_code)]
    fn validate_statusbar_geometry(&mut self, monitor_id: i32) -> bool {
        if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
            if let Some(statusbar) = self.status_bar_clients.get(&monitor_id) {
                let monitor_borrow = monitor.borrow();
                let mut statusbar_mut = statusbar.borrow_mut();
                let mut changed = false;
                // 确保状态栏在显示器范围内
                if statusbar_mut.x != monitor_borrow.m_x {
                    info!(
                        "[validate_statusbar_geometry] Correcting statusbar X: {} -> {}",
                        statusbar_mut.x, monitor_borrow.m_x
                    );
                    statusbar_mut.x = monitor_borrow.m_x;
                    changed = true;
                }
                if statusbar_mut.y != monitor_borrow.m_y {
                    info!(
                        "[validate_statusbar_geometry] Correcting statusbar Y: {} -> {}",
                        statusbar_mut.y, monitor_borrow.m_y
                    );
                    statusbar_mut.y = monitor_borrow.m_y;
                    changed = true;
                }
                if statusbar_mut.w != monitor_borrow.m_w {
                    info!(
                        "[validate_statusbar_geometry] Correcting statusbar width: {} -> {}",
                        statusbar_mut.w, monitor_borrow.m_w
                    );
                    statusbar_mut.w = monitor_borrow.m_w;
                    changed = true;
                }
                // 高度检查 - 确保不超过显示器高度的一半
                let max_height = monitor_borrow.m_h / 2;
                if statusbar_mut.h > max_height {
                    info!(
                        "[validate_statusbar_geometry] Limiting statusbar height: {} -> {}",
                        statusbar_mut.h, max_height
                    );
                    statusbar_mut.h = max_height;
                    changed = true;
                }
                if changed {
                    self.configure(&mut statusbar_mut);
                    drop(statusbar_mut);
                    drop(monitor_borrow);
                    info!(
                        "[validate_statusbar_geometry] Applied corrections for monitor {}",
                        monitor_id
                    );
                }
                return changed;
            }
        }
        false
    }

    // 辅助函数：根据ID获取显示器
    fn get_monitor_by_id(&self, monitor_id: i32) -> Option<Rc<RefCell<Monitor>>> {
        let mut m_iter = self.mons.clone();
        while let Some(ref mon_rc) = m_iter.clone() {
            if mon_rc.borrow().num == monitor_id {
                return Some(mon_rc.clone());
            }
            m_iter = mon_rc.borrow().next.clone();
        }
        None
    }

    #[allow(dead_code)]
    fn set_class_info(
        &mut self,
        client_mut: &mut Client,
        res_class: &str,
        res_name: &str,
    ) -> Result<(), String> {
        if res_class.is_empty() && res_name.is_empty() {
            return Err("Both class and name cannot be empty".to_string());
        }
        unsafe {
            let class_cstring = CString::new(res_class)
                .map_err(|e| format!("Invalid class string '{}': {}", res_class, e))?;
            let name_cstring = CString::new(res_name)
                .map_err(|e| format!("Invalid name string '{}': {}", res_name, e))?;
            // 检查窗口是否有效
            let mut window_attrs: XWindowAttributes = std::mem::zeroed();
            if XGetWindowAttributes(self.dpy, client_mut.win, &mut window_attrs) == 0 {
                return Err(format!("Window 0x{:x} is not valid", client_mut.win));
            }
            // 创建 XClassHint 结构
            let mut ch: XClassHint = std::mem::zeroed();
            ch.res_class = class_cstring.as_ptr() as *mut _;
            ch.res_name = name_cstring.as_ptr() as *mut _;
            // 设置窗口的 class hint
            let result = XSetClassHint(self.dpy, client_mut.win, &mut ch);
            if result != 0 {
                info!(
                "[set_class_info] Successfully set class: '{}', instance: '{}' for window 0x{:x}",
                res_class, res_name, client_mut.win
            );
                // 更新客户端的本地记录
                client_mut.class = res_class.to_string();
                client_mut.instance = res_name.to_string();
                // 刷新X服务器
                XFlush(self.dpy);
                // 验证设置是否成功
                self.verify_class_info_set(client_mut, res_class, res_name);
                Ok(())
            } else {
                Err(format!(
                    "XSetClassHint failed for window 0x{:x}",
                    client_mut.win
                ))
            }
        }
    }

    // 验证设置是否成功的辅助函数
    #[allow(dead_code)]
    fn verify_class_info_set(
        &mut self,
        client: &Client,
        expected_class: &str,
        expected_name: &str,
    ) {
        unsafe {
            let mut ch: XClassHint = std::mem::zeroed();
            if XGetClassHint(self.dpy, client.win, &mut ch) > 0 {
                let actual_class = if !ch.res_class.is_null() {
                    CStr::from_ptr(ch.res_class).to_str().unwrap_or("")
                } else {
                    ""
                };
                let actual_name = if !ch.res_name.is_null() {
                    CStr::from_ptr(ch.res_name).to_str().unwrap_or("")
                } else {
                    ""
                };
                if actual_class == expected_class && actual_name == expected_name {
                    info!("[verify_class_info_set] Verification successful");
                } else {
                    warn!(
                    "[verify_class_info_set] Verification failed. Expected: class='{}', name='{}'. Actual: class='{}', name='{}'",
                    expected_class, expected_name, actual_class, actual_name
                );
                }
                // 清理内存
                if !ch.res_class.is_null() {
                    XFree(ch.res_class as *mut _);
                }
                if !ch.res_name.is_null() {
                    XFree(ch.res_name as *mut _);
                }
            } else {
                warn!("[verify_class_info_set] Failed to get class hint for verification");
            }
        }
    }

    // 更新窗口类信息
    fn update_class_info(&mut self, client_mut: &mut Client) {
        unsafe {
            let mut ch: XClassHint = std::mem::zeroed();
            if XGetClassHint(self.dpy, client_mut.win, &mut ch) > 0 {
                client_mut.class = if !ch.res_class.is_null() {
                    info!(
                        "[update_class_info] class: {:?}",
                        CStr::from_ptr(ch.res_class)
                    );
                    CStr::from_ptr(ch.res_class)
                        .to_str()
                        .unwrap_or("")
                        .to_lowercase()
                } else {
                    String::new()
                };
                client_mut.instance = if !ch.res_name.is_null() {
                    info!(
                        "[update_class_info] instance: {:?}",
                        CStr::from_ptr(ch.res_name)
                    );
                    CStr::from_ptr(ch.res_name)
                        .to_str()
                        .unwrap_or("")
                        .to_lowercase()
                } else {
                    String::new()
                };

                if !ch.res_class.is_null() {
                    XFree(ch.res_class as *mut _);
                }
                if !ch.res_name.is_null() {
                    XFree(ch.res_name as *mut _);
                }
            }
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
            let mut wa: XWindowAttributes = zeroed();
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

    pub fn monocle(&mut self, mon_rc: &Rc<RefCell<Monitor>>) {
        info!("[monocle]");
        // --- 1. 计算当前显示器上可见且可聚焦的客户端数量 (n) ---
        let mut n: u32 = 0;
        let mut c_iter_opt = {
            let mon_borrow = mon_rc.borrow();
            mon_borrow.clients.clone()
        }; // 从客户端链表头开始
        while let Some(ref c_rc) = c_iter_opt.clone() {
            let c_client_borrow = c_rc.borrow();
            if c_client_borrow.isvisible() {
                n += 1;
            }
            c_iter_opt = c_client_borrow.next.clone(); // 移动到下一个客户端
        }

        // --- 2. 更新布局符号 (ltsymbol) ---
        if n > 0 {
            // 如果有可见的客户端
            // 将布局符号更新为 "[n]"，例如 "[3]" 表示有3个窗口在此布局下
            let formatted_string = format!("[{}]", n);
            let mut mon_borrow = mon_rc.borrow_mut();
            info!(
                "[monocle] formatted_string: {}, monitor_num: {}",
                formatted_string, mon_borrow.num
            );
            mon_borrow.lt_symbol = formatted_string;
        }
        // 如果 n == 0，ltsymbol 保持不变 (或者可以设为默认的 monocle 符号)

        // --- 3. 将所有可见且非浮动的客户端调整为占据整个工作区大小 ---
        let (wx, wy, ww, wh, clients_head_opt_for_resize) = {
            let m_read_borrow = mon_rc.borrow(); // 先进行只读操作
            (
                m_read_borrow.w_x,
                m_read_borrow.w_y,
                m_read_borrow.w_w,
                m_read_borrow.w_h,
                m_read_borrow.clients.clone(),
            )
        };
        let client_y_offset = self.client_y_offset(&mon_rc.borrow());
        info!("[monocle] client_y_offset: {}", client_y_offset);

        let mut c_resize_iter_opt = self.nexttiled(clients_head_opt_for_resize); // 获取第一个可见平铺客户端
        while let Some(ref c_rc_to_resize) = c_resize_iter_opt.clone() {
            let border_width = c_rc_to_resize.borrow().border_w; // 不可变借用获取边框宽度

            // 将客户端调整为占据整个显示器工作区的大小（减去边框和Y轴偏移）
            self.resize(
                c_rc_to_resize,
                wx,
                wy + client_y_offset,
                ww - 2 * border_width,
                wh - 2 * border_width - client_y_offset,
                false,
            );

            c_resize_iter_opt = self.nexttiled(c_rc_to_resize.borrow().next.clone());
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
            if !Self::are_equal_rc(&m, &self.motion_mon) {
                let selmon_mut_sel = self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone();
                self.unfocus(selmon_mut_sel, true);
                self.sel_mon = m.clone();
                self.focus(None);
            }
            self.motion_mon = m;
        }
    }
    pub fn unmanage(&mut self, c: Option<Rc<RefCell<Client>>>, destroyed: bool) {
        let client_rc = match c {
            Some(c) => c,
            None => return,
        };
        let win = client_rc.borrow().win;
        // 检查是否是状态栏
        if let Some(&monitor_id) = self.status_bar_windows.get(&win) {
            self.unmanage_statusbar(monitor_id, destroyed);
            return;
        }
        // 常规客户端的 unmanage 逻辑
        self.unmanage_regular_client(&client_rc, destroyed);
    }

    fn unmanage_statusbar(&mut self, monitor_id: i32, destroyed: bool) {
        info!(
            "[unmanage_statusbar] Removing statusbar for monitor {}",
            monitor_id
        );

        if let Some(statusbar) = self.status_bar_clients.remove(&monitor_id) {
            let win = statusbar.borrow().win;
            self.status_bar_windows.remove(&win);

            if !destroyed {
                unsafe {
                    XSelectInput(self.dpy, win, NoEventMask);
                }
            }

            // 恢复显示器工作区域
            if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
                let mut monitor_mut = monitor.borrow_mut();
                monitor_mut.w_y = monitor_mut.m_y;
                monitor_mut.w_h = monitor_mut.m_h;
            }

            // 🚀 优化的资源清理顺序

            // 1. 首先终止子进程
            if let Err(e) = self.terminate_status_bar_process_safe(monitor_id) {
                error!(
                    "[unmanage_statusbar] Failed to terminate process for monitor {}: {}",
                    monitor_id, e
                );
            }

            // 2. 然后清理共享内存
            if let Err(e) = self.cleanup_shared_memory_safe(monitor_id) {
                error!(
                    "[unmanage_statusbar] Failed to cleanup shared memory for monitor {}: {}",
                    monitor_id, e
                );
            }

            // 3. 最后重新排列客户端
            if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
                self.arrange(Some(monitor));
            }
        }
    }

    fn terminate_status_bar_process_safe(&mut self, monitor_id: i32) -> Result<(), String> {
        if let Some(mut child) = self.status_bar_child.remove(&monitor_id) {
            info!(
                "[terminate_status_bar_process_safe] Terminating process for monitor {}",
                monitor_id
            );

            // 获取进程 ID
            let pid = child.id();

            let nix_pid = Pid::from_raw(pid as i32);

            // 检查进程是否存在
            match signal::kill(nix_pid, None) {
                Err(_) => {
                    // 进程已经不存在
                    info!("[terminate_status_bar_process_safe] Process already terminated for monitor {}", monitor_id);
                    return Ok(());
                }
                Ok(_) => {} // 进程存在，继续终止流程
            }

            // 尝试优雅终止
            if let Ok(_) = signal::kill(nix_pid, Signal::SIGTERM) {
                let timeout = Duration::from_secs(3);
                let start = Instant::now();

                // 等待进程退出
                while start.elapsed() < timeout {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            info!(
                                "[terminate_status_bar_process_safe] Process exited gracefully: {:?}",
                                status
                            );
                            return Ok(());
                        }
                        Ok(None) => {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        Err(e) => {
                            return Err(format!("Error waiting for process: {}", e));
                        }
                    }
                }

                // 超时后强制终止
                warn!(
                    "[terminate_status_bar_process_safe] Graceful termination timeout, forcing kill"
                );
            }

            // 强制终止
            match signal::kill(nix_pid, Signal::SIGKILL) {
                Ok(_) => match child.wait() {
                    Ok(status) => {
                        info!(
                            "[terminate_status_bar_process_safe] Process force killed: {:?}",
                            status
                        );
                        Ok(())
                    }
                    Err(e) => Err(format!("Failed to wait for killed process: {}", e)),
                },
                Err(e) => Err(format!("Failed to send SIGKILL: {}", e)),
            }
        } else {
            info!(
                "[terminate_status_bar_process_safe] No process found for monitor {}",
                monitor_id
            );
            Ok(())
        }
    }

    /// 安全的共享内存清理方法
    fn cleanup_shared_memory_safe(&mut self, monitor_id: i32) -> Result<(), String> {
        if let Some(shmem) = self.status_bar_shmem.remove(&monitor_id) {
            info!(
                "[cleanup_shared_memory_safe] Cleaning up shared memory for monitor {}",
                monitor_id
            );

            // 释放共享内存对象
            drop(shmem);

            // 如果需要手动删除系统共享内存对象
            #[cfg(unix)]
            {
                let shmem_name = format!("{}_{}", CONFIG.status_bar_name(), monitor_id);
                if let Ok(c_name) = std::ffi::CString::new(shmem_name) {
                    unsafe {
                        let result = libc::shm_unlink(c_name.as_ptr());
                        if result != 0 {
                            let errno = *libc::__errno_location();
                            if errno != libc::ENOENT {
                                return Err(format!("shm_unlink failed with errno: {}", errno));
                            }
                        }
                    }
                }
            }

            info!(
                "[cleanup_shared_memory_safe] Shared memory cleaned successfully for monitor {}",
                monitor_id
            );
            Ok(())
        } else {
            info!(
                "[cleanup_shared_memory_safe] No shared memory found for monitor {}",
                monitor_id
            );
            Ok(())
        }
    }

    fn adjust_client_position(&mut self, client_rc: &Rc<RefCell<Client>>) {
        let (client_total_width, client_mon_rc_opt) = {
            let client_borrow = client_rc.borrow();
            (client_borrow.width(), client_borrow.mon.clone())
        };

        if let Some(ref client_mon_rc) = client_mon_rc_opt {
            let (mon_wx, mon_wy, mon_ww, mon_wh) = {
                let client_mon_borrow = client_mon_rc.borrow();
                (
                    client_mon_borrow.w_x,
                    client_mon_borrow.w_y,
                    client_mon_borrow.w_w,
                    client_mon_borrow.w_h,
                )
            };

            let mut client_mut = client_rc.borrow_mut();

            // 确保窗口的右边界不超过显示器工作区的右边界
            if client_mut.x + client_total_width > mon_wx + mon_ww {
                client_mut.x = mon_wx + mon_ww - client_total_width;
                info!(
                    "[adjust_client_position] Adjusted X to prevent overflow: {}",
                    client_mut.x
                );
            }

            // 确保窗口的下边界不超过显示器工作区的下边界
            let client_total_height = client_mut.height();
            if client_mut.y + client_total_height > mon_wy + mon_wh {
                client_mut.y = mon_wy + mon_wh - client_total_height;
                info!(
                    "[adjust_client_position] Adjusted Y to prevent overflow: {}",
                    client_mut.y
                );
            }

            // 确保窗口的左边界不小于显示器工作区的左边界
            if client_mut.x < mon_wx {
                client_mut.x = mon_wx;
                info!(
                    "[adjust_client_position] Adjusted X to workarea left: {}",
                    client_mut.x
                );
            }

            // 确保窗口的上边界不小于显示器工作区的上边界
            if client_mut.y < mon_wy {
                client_mut.y = mon_wy;
                info!(
                    "[adjust_client_position] Adjusted Y to workarea top: {}",
                    client_mut.y
                );
            }

            // 对于小窗口，居中显示
            if client_mut.w < mon_ww / 3 && client_mut.h < mon_wh / 3 {
                client_mut.x = mon_wx + (mon_ww - client_total_width) / 2;
                client_mut.y = mon_wy + (mon_wh - client_total_height) / 2;
                info!(
                    "[adjust_client_position] Centered small window at ({}, {})",
                    client_mut.x, client_mut.y
                );
            }

            info!(
                "[adjust_client_position] Final position: ({}, {}) {}x{}",
                client_mut.x, client_mut.y, client_mut.w, client_mut.h
            );
        } else {
            error!("[adjust_client_position] Client has no monitor assigned!");
        }
    }

    pub fn unmanage_regular_client(&mut self, client_rc: &Rc<RefCell<Client>>, destroyed: bool) {
        // info!("[unmanage]");
        unsafe {
            let mut wc: XWindowChanges = zeroed();

            for i in 0..=CONFIG.tags_length() {
                let sel_i = client_rc
                    .borrow()
                    .mon
                    .as_ref()
                    .unwrap()
                    .borrow()
                    .pertag
                    .as_ref()
                    .unwrap()
                    .sel[i]
                    .clone();
                if Self::are_equal_rc(&sel_i, &Some(client_rc.clone())) {
                    client_rc
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

            self.detach(Some(client_rc.clone()));
            self.detachstack(Some(client_rc.clone()));
            if !destroyed {
                let oldbw = client_rc.borrow().old_border_w;
                let win = client_rc.borrow().win;
                wc.border_width = oldbw;
                // avoid race conditions.
                XGrabServer(self.dpy);
                XSetErrorHandler(Some(transmute(xerrordummy as *const ())));
                XSelectInput(self.dpy, win, NoEventMask);
                // restore border.
                XConfigureWindow(self.dpy, win, CWBorderWidth as u32, &mut wc);
                XUngrabButton(self.dpy, AnyButton as u32, AnyModifier, win);
                self.setclientstate(client_rc, WithdrawnState as i64);
                XSync(self.dpy, False);
                XSetErrorHandler(Some(transmute(xerror as *const ())));
                XUngrabServer(self.dpy);
            }
            self.focus(None);
            self.update_net_client_list();
            self.arrange(client_rc.borrow().mon.clone());
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
        // info!("[updategeom]"); // 日志
        let mut dirty: bool = false; // 标记显示器配置是否发生了变化
        unsafe {
            // unsafe 块，因为大量使用 Xlib 调用和裸指针
            let mut num_xinerama_screens: i32 = 0; // Xinerama 报告的屏幕数量

            // --- 1. 检查 Xinerama 是否激活并获取屏幕信息 ---
            if XineramaIsActive(self.dpy) > 0 {
                // 如果 Xinerama 扩展可用且激活
                // info!("[updategeom] XineramaIsActive");
                // 查询所有 Xinerama 屏幕的信息
                let xinerama_info_ptr = XineramaQueryScreens(self.dpy, &mut num_xinerama_screens);
                // xinerama_info_ptr 是一个指向 XineramaScreenInfo 数组的指针
                // num_xinerama_screens 会被设置为数组中元素的数量

                // a. 过滤出唯一的屏幕几何区域
                let mut unique_screens_info: Vec<XineramaScreenInfo> = vec![];
                if !xinerama_info_ptr.is_null() {
                    // 确保指针有效
                    unique_screens_info.reserve(num_xinerama_screens as usize); // 预分配空间
                    for i in 0..num_xinerama_screens as usize {
                        // self.isuniquegeom 检查新的屏幕信息是否与已收集的唯一屏幕信息重复
                        if self.isuniquegeom(&mut unique_screens_info, xinerama_info_ptr.add(i)) {
                            unique_screens_info.push(*xinerama_info_ptr.add(i));
                        }
                    }
                    XFree(xinerama_info_ptr as *mut _); // 释放 XineramaQueryScreens 分配的内存
                }
                let num_effective_screens = unique_screens_info.len() as i32; // 实际有效的、不重复的屏幕数量

                // b. 获取当前 JWM 内部管理的 Monitor 数量 (current_num_monitors)
                let mut current_num_monitors = 0;
                let mut m_iter = self.mons.clone(); // 从 JWM 的 Monitor 链表头开始
                while let Some(ref mon_rc) = m_iter.clone() {
                    current_num_monitors += 1;
                    m_iter = mon_rc.borrow().next.clone(); // 移动到下一个 (不可变借用)
                }

                // c. 如果检测到的有效屏幕数量 (num_effective_screens) 多于 JWM 当前管理的 Monitor 数量，
                //    则创建新的 Monitor 对象。
                if num_effective_screens > current_num_monitors {
                    dirty = true; // 配置已改变
                    for _ in current_num_monitors..num_effective_screens {
                        // 找到当前 Monitor 链表的尾部
                        let mut tail_mon_opt = self.mons.clone();
                        if tail_mon_opt.is_some() {
                            // 如果链表不为空
                            while tail_mon_opt.as_ref().unwrap().borrow().next.is_some() {
                                let next_mon = tail_mon_opt.as_ref().unwrap().borrow().next.clone();
                                tail_mon_opt = next_mon;
                            }
                            // 在尾部添加新的 Monitor
                            tail_mon_opt.as_mut().unwrap().borrow_mut().next =
                                Some(Rc::new(RefCell::new(self.createmon())));
                        } else {
                            // 如果链表为空 (self.mons 是 None)
                            self.mons = Some(Rc::new(RefCell::new(self.createmon())));
                        }
                    }
                }

                // d. 更新现有 Monitor 的几何信息，并根据 Xinerama 信息调整
                m_iter = self.mons.clone(); // 重新从头开始遍历 JWM 的 Monitor 链表
                for i in 0..num_effective_screens as usize {
                    if m_iter.is_none() {
                        break;
                    } // 不应发生，因为上面已确保 Monitor 数量足够
                    let mon_rc = m_iter.as_ref().unwrap().clone();

                    // 比较当前 Monitor 的几何信息与 Xinerama 报告的第 i 个唯一屏幕信息
                    let (current_mx, current_my, current_mw, current_mh) = {
                        let mon_borrow = mon_rc.borrow();
                        (
                            mon_borrow.m_x,
                            mon_borrow.m_y,
                            mon_borrow.m_w,
                            mon_borrow.m_h,
                        )
                    };
                    let xinerama_screen = &unique_screens_info[i];

                    // 如果 Monitor 是新创建的 (i >= current_num_monitors)，
                    // 或者其几何信息与 Xinerama 报告的不符，则更新它。
                    if i as i32 >= current_num_monitors
                        || xinerama_screen.x_org as i32 != current_mx
                        || xinerama_screen.y_org as i32 != current_my
                        || xinerama_screen.width as i32 != current_mw
                        || xinerama_screen.height as i32 != current_mh
                    {
                        dirty = true; // 配置已改变
                        let mut mon_mut_borrow = mon_rc.borrow_mut(); // 可变借用以更新
                        mon_mut_borrow.num = i as i32; // 设置显示器编号
                                                       // mx, my: 物理屏幕左上角坐标
                                                       // mw, mh: 物理屏幕宽高
                                                       // wx, wy: 工作区左上角坐标 (初始等于物理屏幕坐标)
                                                       // ww, wh: 工作区宽高 (初始等于物理屏幕宽高)
                                                       //         后续 arrange 会根据状态栏调整工作区
                        mon_mut_borrow.m_x = xinerama_screen.x_org as i32;
                        mon_mut_borrow.w_x = xinerama_screen.x_org as i32;
                        mon_mut_borrow.m_y = xinerama_screen.y_org as i32;
                        mon_mut_borrow.w_y = xinerama_screen.y_org as i32;
                        mon_mut_borrow.m_w = xinerama_screen.width as i32;
                        mon_mut_borrow.w_w = xinerama_screen.width as i32;
                        mon_mut_borrow.m_h = xinerama_screen.height as i32;
                        mon_mut_borrow.w_h = xinerama_screen.height as i32;
                    }
                    m_iter = mon_rc.borrow().next.clone(); // 移动到下一个 Monitor
                }

                // e. 如果 JWM 当前管理的 Monitor 数量多于检测到的有效屏幕数量，
                //    则移除多余的 Monitor，并将其上的客户端移到第一个 Monitor。
                for _ in num_effective_screens..current_num_monitors {
                    dirty = true; // 配置已改变
                                  // 找到链表倒数第二个 Monitor (即要被移除的 Monitor 的前一个)
                                  // 或者如果只有一个 Monitor 且 num_effective_screens 为0，则移除 self.mons
                    if num_effective_screens == 0
                        && Rc::ptr_eq(self.mons.as_ref().unwrap(), self.sel_mon.as_ref().unwrap())
                    {
                        // 特殊情况：没有有效屏幕了，但之前有一个选中的 monitor
                        let mon_to_remove = self.mons.take().unwrap(); // 取出唯一的 monitor
                        let mut client_iter_opt = mon_to_remove.borrow_mut().clients.take(); // 取出其所有 clients
                        while let Some(client_rc) = client_iter_opt {
                            // 如果没有其他 monitor，这些 client 实际上无处可去，
                            // 除非 JWM 退出或有一个默认的 fallback 行为。
                            // JWM.c 会将它们移到第一个 monitor，但这里没有第一个 monitor 了。
                            // 这里简化为直接 unmanage，或者可以尝试隐藏它们。
                            // 此处简单地让它们随着 mon_to_remove 的 drop 而被处理（如果 Client 的 Drop 正确）
                            // 或者将它们标记为不可见/无 monitor。
                            // 最安全的做法是先 unmanage 它们。
                            let next_client = client_rc.borrow_mut().next.take();
                            self.unmanage(Some(client_rc), false); // unmanage 会处理 focus 和 arrange
                            client_iter_opt = next_client;
                        }
                        if Rc::ptr_eq(&mon_to_remove, self.sel_mon.as_ref().unwrap()) {
                            self.sel_mon = None; // 没有可选的 monitor 了
                        }
                        break; // 已经处理完所有多余的 monitor (因为只有一个或没有了)
                    }

                    // 找到链表尾部的 Monitor (即最后一个要被移除的 Monitor)
                    let mut current_iter = self.mons.clone();
                    while current_iter.as_ref().unwrap().borrow().next.is_some() {
                        let next_mon = current_iter.as_ref().unwrap().borrow().next.clone();
                        current_iter = next_mon;
                    }
                    // current_iter 现在是最后一个 Monitor (要被移除的)

                    if let Some(last_mon_rc) = current_iter {
                        // 确认 last_mon_rc 存在
                        // 将 last_mon_rc 上的所有客户端移动到第一个 Monitor (self.mons)
                        let mut client_iter_opt = last_mon_rc.borrow_mut().clients.take(); // 取出所有客户端
                        while let Some(client_rc) = client_iter_opt {
                            let next_client_opt = client_rc.borrow_mut().next.take(); // 从旧链表断开
                                                                                      // 更新客户端的 mon 和 tags
                            {
                                let mut client_mut = client_rc.borrow_mut();
                                client_mut.mon = self.mons.clone(); // 指向第一个 monitor
                                if let Some(ref first_mon_rc) = self.mons {
                                    // 确保第一个 monitor 存在
                                    let first_mon_borrow = first_mon_rc.borrow();
                                    client_mut.tags =
                                        first_mon_borrow.tag_set[first_mon_borrow.sel_tags];
                                } else {
                                    client_mut.tags = 1; // 回退到默认标签
                                }
                            }
                            // 将客户端附加到第一个 Monitor 的管理列表
                            self.attach(Some(client_rc.clone()));
                            self.attachstack(Some(client_rc));
                            client_iter_opt = next_client_opt;
                        }

                        // 如果被移除的 Monitor 是当前选中的 Monitor，则将 selmon 指向第一个 Monitor
                        if Rc::ptr_eq(&last_mon_rc, self.sel_mon.as_ref().unwrap()) {
                            self.sel_mon = self.mons.clone();
                        }
                        // 从链表中移除 last_mon_rc
                        self.cleanupmon(Some(last_mon_rc)); // cleanupmon 会处理链表指针的断开
                    }
                }
            } else {
                // --- 如果 Xinerama 未激活，则使用单显示器模式 ---
                dirty = true; // 假设可能需要更新 (例如从无到有，或尺寸变化)
                if self.mons.is_none() {
                    // 如果还没有 Monitor 对象
                    self.mons = Some(Rc::new(RefCell::new(self.createmon())));
                }
                // 更新这个唯一的 Monitor 的几何信息为整个屏幕的大小
                // （这里假设 self.sw, self.sh 是屏幕总尺寸）
                if let Some(ref mon_rc) = self.mons {
                    let mut mon_mut_borrow = mon_rc.borrow_mut();
                    if mon_mut_borrow.m_w != self.s_w || mon_mut_borrow.m_h != self.s_h {
                        dirty = true;
                        mon_mut_borrow.num = 0; // 单显示器编号为0
                        mon_mut_borrow.m_x = 0;
                        mon_mut_borrow.w_x = 0;
                        mon_mut_borrow.m_y = 0;
                        mon_mut_borrow.w_y = 0;
                        mon_mut_borrow.m_w = self.s_w;
                        mon_mut_borrow.w_w = self.s_w;
                        mon_mut_borrow.m_h = self.s_h;
                        mon_mut_borrow.w_h = self.s_h;
                    }
                }
            }

            // --- 6. 如果配置发生了变化，更新 JWM 的选中显示器 ---
            if dirty {
                self.sel_mon = self.wintomon(self.root);
                if self.sel_mon.is_none() && self.mons.is_some() {
                    self.sel_mon = self.mons.clone();
                }
            }
            return dirty; // 返回配置是否发生变化的标志
        }
    }

    pub fn updatewindowtype(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[updatewindowtype]");
        let state;
        let wtype;
        {
            let c = &mut *c.borrow_mut();
            state = self.getatomprop(c, self.net_atom[NET::NetWMState as usize]);
            wtype = self.getatomprop(c, self.net_atom[NET::NetWMWindowType as usize]);
        }

        if state == self.net_atom[NET::NetWMFullscreen as usize] {
            self.setfullscreen(c, true);
        }
        if wtype == self.net_atom[NET::NetWMWindowTypeDialog as usize] {
            let c = &mut *c.borrow_mut();
            c.is_floating = true;
        }
    }

    pub fn updatewmhints(&mut self, client_rc: &Rc<RefCell<Client>>) {
        // info!("[updatewmhints]");
        unsafe {
            let mut client_mut = client_rc.borrow_mut();
            let wmh = XGetWMHints(self.dpy, client_mut.win);
            if !wmh.is_null() {
                let sel_mon_borrow = self.sel_mon.as_ref().unwrap().borrow();
                if sel_mon_borrow.sel.is_some()
                    && Rc::ptr_eq(client_rc, sel_mon_borrow.sel.as_ref().unwrap())
                    && ((*wmh).flags & XUrgencyHint) > 0
                {
                    (*wmh).flags &= !XUrgencyHint;
                    XSetWMHints(self.dpy, client_mut.win, wmh);
                } else {
                    client_mut.is_urgent = if (*wmh).flags & XUrgencyHint > 0 {
                        true
                    } else {
                        false
                    };
                }
                if (*wmh).flags & InputHint > 0 {
                    client_mut.never_focus = (*wmh).input <= 0;
                } else {
                    client_mut.never_focus = false;
                }
                XFree(wmh as *mut _);
            }
        }
    }

    pub fn updatetitle(&mut self, c: &mut Client) {
        // info!("[updatetitle]");
        if !self.gettextprop(c.win, self.net_atom[NET::NetWMName as usize], &mut c.name) {
            self.gettextprop(c.win, XA_WM_NAME, &mut c.name);
        }
    }

    pub fn update_bar_message_for_monitor(&mut self, m_opt: Option<Rc<RefCell<Monitor>>>) {
        // info!("[update_bar_message_for_monitor]");
        if m_opt.is_none() {
            error!("[update_bar_message_for_monitor] Monitor option is None, cannot update bar message.");
            return;
        }
        let mon_rc = m_opt.as_ref().unwrap(); // &Rc<RefCell<Monitor>>

        self.message = SharedMessage::default();
        let mut monitor_info_for_message = MonitorInfo::default();
        let mut occupied_tags_mask: u32 = 0;
        let mut urgent_tags_mask: u32 = 0;
        {
            let mon_borrow = mon_rc.borrow();
            // info!("[update_bar_message_for_monitor], {}", mon_borrow);
            monitor_info_for_message.monitor_x = mon_borrow.w_x;
            let offscreen_offset = if self.show_bar { 0 } else { -1000 };
            monitor_info_for_message.monitor_y = mon_borrow.w_y + offscreen_offset;
            monitor_info_for_message.monitor_width = mon_borrow.w_w;
            monitor_info_for_message.monitor_height = mon_borrow.w_h;
            monitor_info_for_message.monitor_num = mon_borrow.num;
            monitor_info_for_message.set_ltsymbol(&mon_borrow.lt_symbol);
            monitor_info_for_message.border_w = CONFIG.border_px() as i32;

            let mut c_iter_opt = mon_borrow.clients.clone();
            while let Some(ref client_rc_iter) = c_iter_opt.clone() {
                let client_borrow_iter = client_rc_iter.borrow();
                occupied_tags_mask |= client_borrow_iter.tags;
                if client_borrow_iter.is_urgent {
                    urgent_tags_mask |= client_borrow_iter.tags;
                }
                c_iter_opt = client_borrow_iter.next.clone();
            }
        }

        for i in 0..CONFIG.tags_length() {
            let tag_bit = 1 << i;
            // is_filled_tag 的正确计算方式 (与你之前版本类似，但确保变量名一致和借用正确)
            let is_filled_tag_calculated: bool; // 声明变量
            {
                is_filled_tag_calculated = if let Some(ref global_selmon_rc) = self.sel_mon {
                    if Rc::ptr_eq(mon_rc, global_selmon_rc) {
                        // 当前 monitor 是全局选中的 monitor
                        if let Some(ref selected_client_on_selmon) = global_selmon_rc.borrow().sel {
                            (selected_client_on_selmon.borrow().tags & tag_bit) != 0
                        } else {
                            false // 全局选中的 monitor 上没有选中的 client
                        }
                    } else {
                        false // 当前 monitor 不是全局选中的 monitor
                    }
                } else {
                    false // JWM 根本没有全局选中的 monitor
                };
            }
            let m_borrow_for_tagset = mon_rc.borrow(); // 再次不可变借用 m_rc 来获取 tagset 信息
            let active_tagset_for_mon = m_borrow_for_tagset.tag_set[m_borrow_for_tagset.sel_tags];
            // drop(m_borrow_for_tagset); // 可选，如果下面不再需要

            let is_selected_tag = (active_tagset_for_mon & tag_bit) != 0;
            let is_urgent_tag = (urgent_tags_mask & tag_bit) != 0;
            let is_occupied_tag = (occupied_tags_mask & tag_bit) != 0;

            let tag_status = TagStatus::new(
                is_selected_tag,
                is_urgent_tag,
                is_filled_tag_calculated,
                is_occupied_tag,
            );
            monitor_info_for_message.set_tag_status(i, tag_status);
        }

        let mut selected_client_name_for_bar = String::new();
        if let Some(ref selected_client_rc) = mon_rc.borrow().sel {
            selected_client_name_for_bar = selected_client_rc.borrow().name.clone();
        }
        monitor_info_for_message.set_client_name(&selected_client_name_for_bar);
        self.message.monitor_info = monitor_info_for_message;
    }
}
