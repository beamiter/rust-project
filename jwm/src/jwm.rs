#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

use libc::{close, setsid, sigaction, sigemptyset, SIGCHLD, SIG_DFL};
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
use std::fmt;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::rc::Rc;
use std::str::FromStr; // 用于从字符串解析 // 用于格式化输出，如 Display trait
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use std::usize;
use x11::xft::XftColor;
use x11rb::connection::Connection;
use x11rb::errors::{ReplyError, ReplyOrIdError};
use x11rb::properties::WmSizeHints;
use x11rb::protocol::render::Pictforminfo;
use x11rb::protocol::xinput::KeyCode;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::x11_utils::Serialize;
use x11rb::COPY_DEPTH_FROM_PARENT;

use x11::keysym::XK_Num_Lock;

use std::cmp::{max, min};

use crate::config::CONFIG;
use crate::xcb_util::{test_all_cursors, Atoms, CursorManager};
use crate::xproto::{IconicState, NormalState, WithdrawnState};

lazy_static::lazy_static! {
    pub static ref BUTTONMASK: u32 = (EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE).bits();
    pub static ref MOUSEMASK: u32 = (EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION).bits();
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

#[derive(Debug, Clone, Default)]
pub struct WindowGeom {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
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
    pub func: Option<WMFunc>,
    pub arg: Arg,
}
impl Button {
    #[allow(unused)]
    pub fn new(click: u32, mask: u32, button: u32, func: Option<WMFunc>, arg: Arg) -> Self {
        Self {
            click,
            mask,
            button,
            func,
            arg,
        }
    }
}

pub type WMFunc = fn(&mut Jwm, &Arg) -> Result<(), Box<dyn std::error::Error>>;
#[derive(Debug, Clone)]
pub struct Key {
    pub mod0: u32,
    pub keysym: Keysym,
    pub func: Option<WMFunc>,
    pub arg: Arg,
}
impl Key {
    #[allow(unused)]
    pub fn new(mod0: u32, keysym: Keysym, func: Option<WMFunc>, arg: Arg) -> Self {
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
            show_bars: vec![true; CONFIG.tags_length() + 1],
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

pub struct Jwm {
    pub stext_max_len: usize,
    pub s_w: i32,
    pub s_h: i32,
    pub numlock_mask: u32,
    pub running: AtomicBool,
    pub cursor_manager: CursorManager,
    pub theme_manager: ThemeManager,
    pub mons: Option<Rc<RefCell<Monitor>>>,
    pub motion_mon: Option<Rc<RefCell<Monitor>>>,
    pub sel_mon: Option<Rc<RefCell<Monitor>>>,
    pub wm_check_win: Window,
    pub visual_id: Visualid,
    pub depth: u8,
    pub color_map: Colormap,
    pub sender: Sender<u8>,
    pub status_bar_shmem: HashMap<i32, SharedRingBuffer>,
    pub status_bar_child: HashMap<i32, Child>,
    pub message: SharedMessage,

    // 状态栏专用管理
    pub status_bar_clients: HashMap<i32, Rc<RefCell<Client>>>, // monitor_id -> statusbar_client
    pub status_bar_windows: HashMap<Window, i32>,              // window_id -> monitor_id (快速查找)

    pub pending_bar_updates: HashSet<i32>,

    pub x11rb_conn: RustConnection,
    pub x11rb_root: Window,
    pub x11rb_screen_num: usize,
    pub atoms: Atoms,

    keycode_cache: HashMap<u8, u32>,
}

impl Jwm {
    pub fn drw_clr_create_direct(r: u8, g: u8, b: u8, alpha: u8) -> Option<XftColor> {
        unsafe {
            let mut xcolor: XftColor = std::mem::zeroed();
            // 手动构造像素值 (ARGB格式)
            xcolor.pixel =
                ((alpha as u64) << 24) | ((r as u64) << 16) | ((g as u64) << 8) | (b as u64);
            // 设置其他字段
            xcolor.color.red = (r as u16) << 8;
            xcolor.color.green = (g as u16) << 8;
            xcolor.color.blue = (b as u16) << 8;
            xcolor.color.alpha = (alpha as u16) << 8;
            Some(xcolor)
        }
    }

    pub fn drw_clr_create_from_hex(hex_color: &str, alpha: u8) -> Option<XftColor> {
        // 解析 "#ff0000" 格式
        if hex_color.starts_with('#') && hex_color.len() == 7 {
            let r = u8::from_str_radix(&hex_color[1..3], 16).ok()?;
            let g = u8::from_str_radix(&hex_color[3..5], 16).ok()?;
            let b = u8::from_str_radix(&hex_color[5..7], 16).ok()?;
            return Self::drw_clr_create_direct(r, g, b, alpha);
        }
        None
    }

    fn handler(&mut self, event: Event) -> Result<(), Box<dyn std::error::Error>> {
        match event {
            Event::ButtonPress(e) => self.buttonpress(&e)?,
            Event::ClientMessage(e) => self.clientmessage(&e)?,
            Event::ConfigureRequest(e) => self.configurerequest(&e)?,
            Event::ConfigureNotify(e) => self.configurenotify(&e)?,
            Event::DestroyNotify(e) => self.destroynotify(&e)?,
            Event::EnterNotify(e) => self.enternotify(&e)?,
            Event::Expose(e) => self.expose(&e)?,
            Event::FocusIn(e) => self.focusin(&e)?,
            Event::KeyPress(e) => self.keypress(&e)?,
            Event::MappingNotify(e) => self.mappingnotify(&e)?,
            Event::MapRequest(e) => self.maprequest(&e)?,
            Event::MotionNotify(e) => self.motionnotify(&e)?,
            Event::PropertyNotify(e) => self.propertynotify(&e)?,
            Event::UnmapNotify(e) => self.unmapnotify(&e)?,
            _ => {
                debug!("Unsupported event type: {:?}", event);
            }
        }
        Ok(())
    }

    pub fn new(sender: Sender<u8>) -> Self {
        let theme_manager = ThemeManager::new(
            ColorScheme::new(
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().dark_sea_green1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().light_sky_blue1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().light_sky_blue1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
            ),
            ColorScheme::new(
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().dark_sea_green2,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Self::drw_clr_create_from_hex(
                    &CONFIG.colors().pale_turquoise1,
                    CONFIG.colors().opaque,
                )
                .unwrap(),
                Self::drw_clr_create_from_hex(&CONFIG.colors().cyan, CONFIG.colors().opaque)
                    .unwrap(),
            ),
        );

        let (x11rb_conn, x11rb_screen_num) =
            x11rb::rust_connection::RustConnection::connect(None).unwrap();
        let atoms = Atoms::new(&x11rb_conn).unwrap().reply().unwrap();
        let _ = test_all_cursors(&x11rb_conn);
        let x11rb_screen = &x11rb_conn.setup().roots[x11rb_screen_num];
        let s_w = x11rb_screen.width_in_pixels.into();
        let s_h = x11rb_screen.height_in_pixels.into();
        let x11rb_root = x11rb_screen.root;
        let cursor_manager = CursorManager::new(&x11rb_conn).unwrap();
        Jwm {
            stext_max_len: 512,
            s_w,
            s_h,
            numlock_mask: 0,
            running: AtomicBool::new(true),
            theme_manager,
            cursor_manager,
            mons: None,
            motion_mon: None,
            sel_mon: None,
            wm_check_win: 0,
            visual_id: 0,
            depth: 0,
            color_map: 0,
            sender,
            status_bar_shmem: HashMap::new(),
            status_bar_child: HashMap::new(),
            message: SharedMessage::default(),
            status_bar_clients: HashMap::new(),
            status_bar_windows: HashMap::new(),
            pending_bar_updates: HashSet::new(),

            x11rb_conn,
            x11rb_root,
            x11rb_screen_num,
            atoms,
            keycode_cache: HashMap::new(),
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

    fn clean_mask(&self, mask: u32) -> u32 {
        // 第一步：移除NumLock和CapsLock
        let mask_without_locks = mask & !(self.numlock_mask | KeyButMask::LOCK.bits() as u32);
        // 第二步：只保留真正的修饰键
        let modifier_mask = KeyButMask::SHIFT.bits()
            | KeyButMask::CONTROL.bits()
            | KeyButMask::MOD1.bits()
            | KeyButMask::MOD2.bits()
            | KeyButMask::MOD3.bits()
            | KeyButMask::MOD4.bits()
            | KeyButMask::MOD5.bits();
        mask_without_locks & modifier_mask as u32
    }

    /// 获取窗口的 WM_CLASS（即类名和实例名）
    pub fn get_wm_class<C: Connection>(conn: &C, window: Window) -> Option<(String, String)> {
        // Get the WM_NAME property of the window
        let cookie = conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .unwrap();
        if let Ok(prop) = cookie.reply() {
            // 2. 检查属性是否存在且格式正确
            if prop.type_ != AtomEnum::STRING.into() || prop.format != 8 {
                return None;
            }
            let value = prop.value; // 字节流
            if value.is_empty() {
                return None;
            }
            // 3. WM_CLASS 包含两个以 '\0' 结尾的字符串：instance\0class\0
            let mut parts = value.split(|&b| b == 0u8).filter(|s| !s.is_empty());
            let instance = parts
                .next()
                .and_then(|s| String::from_utf8(s.to_vec()).ok())?;
            let class = parts
                .next()
                .and_then(|s| String::from_utf8(s.to_vec()).ok())?;
            return Some((instance.to_lowercase(), class.to_lowercase()));
        }
        None
    }

    // function declarations and implementations.
    pub fn applyrules(&mut self, c: &Rc<RefCell<Client>>) {
        info!("[applyrules]");
        // rule matching
        let mut c = c.borrow_mut();
        c.is_floating = false;
        if let Some((inst, cls)) = Self::get_wm_class(&self.x11rb_conn, c.win as u32) {
            c.instance = inst;
            c.class = cls;
            info!("instance: {}, class: {}", c.instance, c.class);
        }

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

    pub fn updatesizehints(
        &mut self,
        c: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 获取 WM_NORMAL_HINTS
        let reply =
            match WmSizeHints::get_normal_hints(&self.x11rb_conn, c.borrow().win)?.reply()? {
                Some(reply) => reply,
                None => {
                    // 没有 WM_NORMAL_HINTS 属性，使用默认值
                    let mut c_mut = c.borrow_mut();
                    c_mut.hints_valid = false;
                    return Ok(());
                }
            };

        let mut c_mut = c.borrow_mut();
        if let Some((w, h)) = reply.base_size {
            c_mut.base_w = w;
            c_mut.base_h = h;
        }
        if let Some((w, h)) = reply.size_increment {
            c_mut.inc_w = w;
            c_mut.inc_h = h;
        }
        if let Some((w, h)) = reply.max_size {
            c_mut.max_w = w;
            c_mut.max_h = h;
        }
        if let Some((w, h)) = reply.min_size {
            c_mut.min_w = w;
            c_mut.min_h = h;
        }
        if let Some((min_aspect, max_aspect)) = reply.aspect {
            c_mut.min_a = min_aspect.numerator as f32 / min_aspect.denominator as f32;
            c_mut.max_a = max_aspect.numerator as f32 / max_aspect.denominator as f32;
        }
        c_mut.is_fixed = (c_mut.max_w > 0)
            && (c_mut.max_h > 0)
            && (c_mut.max_w == c_mut.min_w)
            && (c_mut.max_h == c_mut.min_h);

        c_mut.hints_valid = true;

        Ok(())
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
                let _ = self.updatesizehints(c); // This will mutably borrow internally
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

    pub fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup] Starting window manager cleanup");

        // 清理状态栏
        let statusbar_monitor_ids: Vec<i32> = self.status_bar_clients.keys().cloned().collect();
        for monitor_id in statusbar_monitor_ids {
            self.unmanage_statusbar(monitor_id, false)?;
        }

        // 常规清理逻辑
        drop(self.sender.clone());

        // 切换到显示所有窗口的视图
        let show_all_arg = Arg::Ui(!0);
        let _ = self.view(&show_all_arg);

        // 卸载所有客户端
        self.cleanup_all_clients()?;

        // 释放所有按键抓取
        self.cleanup_key_grabs()?;

        // 清理所有监视器
        self.cleanup_all_monitors();

        // 销毁 WM 检查窗口
        self.cleanup_wm_check_window()?;

        // 重置输入焦点到根窗口
        self.reset_input_focus()?;

        // 清理 EWMH 属性
        self.cleanup_ewmh_properties()?;

        // 确保所有操作都被发送
        self.x11rb_conn.flush()?;

        info!("[cleanup] Window manager cleanup completed");
        Ok(())
    }

    fn cleanup_all_clients(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut m = self.mons.clone();

        while let Some(ref m_opt) = m {
            // 不断卸载当前监视器的第一个客户端，直到没有客户端为止
            loop {
                let stack_client = m_opt.borrow().stack.clone();
                if let Some(client_rc) = stack_client {
                    if let Err(e) = self.unmanage(Some(client_rc), false) {
                        warn!("[cleanup_all_clients] Failed to unmanage client: {:?}", e);
                        // 继续处理下一个客户端，避免无限循环
                        break;
                    }
                } else {
                    break;
                }
            }

            let next = m_opt.borrow().next.clone();
            m = next;
        }

        Ok(())
    }

    fn cleanup_key_grabs(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 释放所有按键抓取
        match self
            .x11rb_conn
            .ungrab_key(Grab::ANY, self.x11rb_root, ModMask::ANY.into())
        {
            Ok(cookie) => {
                if let Err(e) = cookie.check() {
                    warn!("[cleanup_key_grabs] Failed to ungrab keys: {:?}", e);
                }
            }
            Err(e) => {
                warn!(
                    "[cleanup_key_grabs] Failed to send ungrab_key request: {:?}",
                    e
                );
            }
        }

        Ok(())
    }

    fn cleanup_all_monitors(&mut self) {
        while self.mons.is_some() {
            self.cleanupmon(self.mons.clone());
        }
    }

    fn cleanup_wm_check_window(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.wm_check_win != 0 {
            match self.x11rb_conn.destroy_window(self.wm_check_win) {
                Ok(cookie) => {
                    if let Err(e) = cookie.check() {
                        warn!(
                            "[cleanup_wm_check_window] Failed to destroy WM check window: {:?}",
                            e
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "[cleanup_wm_check_window] Failed to send destroy_window request: {:?}",
                        e
                    );
                }
            }
        }

        Ok(())
    }

    fn reset_input_focus(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 将输入焦点重置到根窗口
        match self.x11rb_conn.set_input_focus(
            InputFocus::POINTER_ROOT,
            self.x11rb_root,
            0u32, // CurrentTime equivalent
        ) {
            Ok(cookie) => {
                if let Err(e) = cookie.check() {
                    warn!("[reset_input_focus] Failed to reset input focus: {:?}", e);
                }
            }
            Err(e) => {
                warn!(
                    "[reset_input_focus] Failed to send set_input_focus request: {:?}",
                    e
                );
            }
        }

        Ok(())
    }

    fn cleanup_ewmh_properties(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 清理 _NET_ACTIVE_WINDOW 属性
        if let Err(e) = self
            .x11rb_conn
            .delete_property(self.x11rb_root, self.atoms._NET_ACTIVE_WINDOW)
        {
            warn!(
                "[cleanup_ewmh_properties] Failed to delete _NET_ACTIVE_WINDOW: {:?}",
                e
            );
        }

        // 清理客户端列表
        if let Err(e) = self
            .x11rb_conn
            .delete_property(self.x11rb_root, self.atoms._NET_CLIENT_LIST)
        {
            warn!(
                "[cleanup_ewmh_properties] Failed to delete _NET_CLIENT_LIST: {:?}",
                e
            );
        }

        // 清理支持的协议列表
        if let Err(e) = self
            .x11rb_conn
            .delete_property(self.x11rb_root, self.atoms._NET_SUPPORTED)
        {
            warn!(
                "[cleanup_ewmh_properties] Failed to delete _NET_SUPPORTED: {:?}",
                e
            );
        }

        Ok(())
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

    pub fn clientmessage(
        &mut self,
        e: &ClientMessageEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[clientmessage]");
        let c = self.wintoclient(e.window);
        if c.is_none() {
            return Ok(());
        }
        let c = c.as_ref().unwrap();

        // 检查是否是窗口状态消息
        if e.type_ == self.atoms._NET_WM_STATE {
            // 检查是否是全屏状态变更
            if self.is_fullscreen_state_message(e) {
                let isfullscreen = { c.borrow().is_fullscreen };

                // 解析操作类型
                let action = self.get_client_message_long(e, 0)?;
                let fullscreen = match action {
                    1 => true,          // NET_WM_STATE_ADD
                    0 => false,         // NET_WM_STATE_REMOVE
                    2 => !isfullscreen, // NET_WM_STATE_TOGGLE
                    _ => return Ok(()), // 未知操作
                };

                self.setfullscreen(c, fullscreen)?;
            }
        }
        // 检查是否是激活窗口消息
        else if e.type_ == self.atoms._NET_ACTIVE_WINDOW {
            let is_urgent = { c.borrow().is_urgent };
            let sel = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
            if !Self::are_equal_rc(&Some(c.clone()), &sel) && !is_urgent {
                self.seturgent(c, true);
            }
        }

        Ok(())
    }

    /// 检查是否是全屏状态消息
    fn is_fullscreen_state_message(&self, e: &ClientMessageEvent) -> bool {
        let state1 = self.get_client_message_long(e, 1).unwrap_or(0);
        let state2 = self.get_client_message_long(e, 2).unwrap_or(0);
        state1 == self.atoms._NET_WM_STATE_FULLSCREEN
            || state2 == self.atoms._NET_WM_STATE_FULLSCREEN
    }

    /// 从ClientMessage中获取long数据
    fn get_client_message_long(
        &self,
        e: &ClientMessageEvent,
        index: usize,
    ) -> Result<u32, Box<dyn std::error::Error>> {
        if index >= 5 {
            return Err("ClientMessage data index out of range".into());
        }
        match e.format {
            32 => {
                // 32位数据
                let data = e.data.as_data32();
                Ok(data[index])
            }
            16 => {
                // 16位数据 - 需要组合两个16位值成一个32位值
                let data = e.data.as_data16();
                if index * 2 + 1 < data.len() {
                    let low = data[index * 2] as u32;
                    let high = data[index * 2 + 1] as u32;
                    Ok(low | (high << 16))
                } else {
                    Err("16-bit data index out of range".into())
                }
            }
            8 => {
                // 8位数据 - 需要组合四个8位值成一个32位值
                let data = e.data.as_data8();
                if index * 4 + 3 < data.len() {
                    let b0 = data[index * 4] as u32;
                    let b1 = data[index * 4 + 1] as u32;
                    let b2 = data[index * 4 + 2] as u32;
                    let b3 = data[index * 4 + 3] as u32;
                    Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
                } else {
                    Err("8-bit data index out of range".into())
                }
            }
            _ => Err(format!("Unsupported data format: {}", e.format).into()),
        }
    }

    pub fn configurenotify(
        &mut self,
        e: &ConfigureNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[configurenotify]");
        // 检查是否是根窗口的配置变更
        if e.window == self.x11rb_root {
            let dirty = self.s_w != e.width as i32 || self.s_h != e.height as i32;
            self.s_w = e.width as i32;
            self.s_h = e.height as i32;

            if self.updategeom() || dirty {
                self.handle_screen_geometry_change()?;
            }
        }

        Ok(())
    }

    fn handle_screen_geometry_change(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 遍历所有显示器和客户端
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
            // 处理该显示器上的所有客户端
            self.update_fullscreen_clients_on_monitor(m_opt)?;
            // 移动到下一个显示器
            let next = m_opt.borrow().next.clone();
            m = next;
        }
        // 重新聚焦和排列
        let _ = self.focus(None);
        self.arrange(None);
        Ok(())
    }

    fn update_fullscreen_clients_on_monitor(
        &mut self,
        monitor: &Rc<RefCell<Monitor>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let monitor_geometry = {
            let m_borrow = monitor.borrow();
            (m_borrow.m_x, m_borrow.m_y, m_borrow.m_w, m_borrow.m_h)
        };
        let mut c = monitor.borrow().clients.clone();
        while let Some(ref client_rc) = c {
            // 检查是否是全屏客户端
            let is_fullscreen = { client_rc.borrow().is_fullscreen };
            if is_fullscreen {
                // 调整全屏客户端到新的显示器尺寸
                let _ = self.resizeclient(
                    &mut client_rc.borrow_mut(),
                    monitor_geometry.0,
                    monitor_geometry.1,
                    monitor_geometry.2,
                    monitor_geometry.3,
                );
            }
            // 移动到下一个客户端
            let next = client_rc.borrow().next.clone();
            c = next;
        }
        Ok(())
    }

    pub fn configure(&self, c: &mut Client) -> Result<(), Box<dyn std::error::Error>> {
        let event = ConfigureNotifyEvent {
            event: c.win,
            window: c.win,
            x: c.x as i16,
            y: c.y as i16,
            width: c.w as u16,
            height: c.h as u16,
            border_width: c.border_w as u16,
            above_sibling: 0,
            override_redirect: false,
            response_type: CONFIGURE_NOTIFY_EVENT,
            sequence: 0,
        };

        self.x11rb_conn
            .send_event(false, c.win, EventMask::STRUCTURE_NOTIFY, event)?;
        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn set_window_border_width(
        &self,
        window: u32,
        border_width: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ConfigureWindowAux::new().border_width(border_width);
        self.x11rb_conn.configure_window(window, &aux)?;
        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn set_window_border_pixel(
        &self,
        window: u32,
        border_pixel: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ChangeWindowAttributesAux::new().border_pixel(border_pixel);
        self.x11rb_conn.change_window_attributes(window, &aux)?;
        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn grabkeys(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[grabkeys]");
        self.update_num_lock_mask()?;

        let modifiers_to_try = [
            0,
            KeyButMask::LOCK.bits() as u32,
            self.numlock_mask,
            self.numlock_mask | KeyButMask::LOCK.bits() as u32,
        ];

        // 取消之前的按键抓取
        self.x11rb_conn
            .ungrab_key(Grab::ANY, self.x11rb_root, ModMask::ANY.into())?;

        // 获取键盘映射
        let setup = self.x11rb_conn.setup();
        let mapping = self
            .x11rb_conn
            .get_keyboard_mapping(
                setup.min_keycode,
                (setup.max_keycode - setup.min_keycode) + 1,
            )?
            .reply()?;

        // 遍历所有键码
        for (keycode_offset, keysyms_for_keycode) in mapping
            .keysyms
            .chunks(mapping.keysyms_per_keycode as usize)
            .enumerate()
        {
            let keycode = setup.min_keycode + keycode_offset as u8;

            if let Some(&keysym) = keysyms_for_keycode.first() {
                // 检查是否匹配配置中的按键
                for key_config in CONFIG.get_keys().iter() {
                    if key_config.keysym == keysym.into() {
                        for &modifier_combo in modifiers_to_try.iter() {
                            self.x11rb_conn.grab_key(
                                false, // owner_events
                                self.x11rb_root,
                                ModMask::from((key_config.mod0 | modifier_combo) as u16),
                                keycode,
                                GrabMode::ASYNC,
                                GrabMode::ASYNC,
                            )?;
                        }
                    }
                }
            }
        }

        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn grabbuttons(
        &mut self,
        client_opt: Option<Rc<RefCell<Client>>>,
        focused: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_win_id = match client_opt.as_ref() {
            Some(c_rc) => c_rc.borrow().win,
            None => return Ok(()),
        };

        let modifiers_to_try = [
            0,
            KeyButMask::LOCK.bits() as u32,
            self.numlock_mask,
            self.numlock_mask | KeyButMask::LOCK.bits() as u32,
        ];

        // 取消之前的按钮抓取
        self.x11rb_conn
            .ungrab_button(ButtonIndex::ANY, client_win_id, ModMask::ANY.into())?;

        if !focused {
            self.x11rb_conn.grab_button(
                false, // owner_events
                client_win_id,
                (*BUTTONMASK as u32).into(),
                GrabMode::SYNC,
                GrabMode::SYNC,
                0u32, // confine_to
                0u32, // cursor
                ButtonIndex::ANY,
                ModMask::ANY.into(),
            )?;
        }

        for button_config in CONFIG.get_buttons().iter() {
            if button_config.click == CLICK::ClkClientWin as u32 {
                for &modifier_combo in modifiers_to_try.iter() {
                    self.x11rb_conn.grab_button(
                        false,
                        client_win_id,
                        (*BUTTONMASK as u32).into(),
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                        0u32,
                        0u32,
                        ButtonIndex::from(button_config.button as u8),
                        ModMask::from((button_config.mask | modifier_combo) as u16),
                    )?;
                }
            }
        }

        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn setfullscreen(
        &mut self,
        c: &Rc<RefCell<Client>>,
        fullscreen: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[setfullscreen]");
        use x11rb::wrapper::ConnectionExt;
        let isfullscreen = { c.borrow_mut().is_fullscreen };
        let win = { c.borrow_mut().win };
        if fullscreen && !isfullscreen {
            self.x11rb_conn.change_property32(
                PropMode::REPLACE,
                win,
                self.atoms._NET_WM_STATE,
                AtomEnum::ATOM,
                &[self.atoms._NET_WM_STATE_FULLSCREEN],
            )?;
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
            let _ = self.resizeclient(&mut *c.borrow_mut(), mx, my, mw, mh);
            // Raise the window to the top of the stacking order
            let config = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
            self.x11rb_conn.configure_window(win, &config)?;
        } else if !fullscreen && isfullscreen {
            self.x11rb_conn.change_property32(
                PropMode::REPLACE,
                win,
                self.atoms._NET_WM_STATE,
                AtomEnum::ATOM,
                &[],
            )?;
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
                let _ = self.resizeclient(&mut *c, x, y, w, h);
            }
            let mon = { c.borrow_mut().mon.clone() };
            self.arrange(mon);
        }
        Ok(())
    }

    pub fn resizeclient(
        &mut self,
        c: &mut Client,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[resizeclient] {x}, {y}, {w}, {h}");
        // 保存旧的位置和大小
        c.old_x = c.x;
        c.old_y = c.y;
        c.old_w = c.w;
        c.old_h = c.h;
        // 更新新的位置和大小
        c.x = x;
        c.y = y;
        c.w = w;
        c.h = h;
        // 构建配置值向量
        let values = ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .width(w as u32)
            .height(h as u32)
            .border_width(c.border_w as u32);
        // 发送配置窗口请求
        self.x11rb_conn.configure_window(c.win, &values)?;
        // 调用configure方法
        self.configure(c)?;
        // 同步连接（刷新所有待发送的请求）
        self.x11rb_conn.flush()?;

        Ok(())
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
            let _ = self.resizeclient(&mut *c.borrow_mut(), x, y, w, h);
        }
    }

    /// 设置窗口的 urgent 状态（XUrgencyHint）
    pub fn seturgent(&self, c_rc: &Rc<RefCell<Client>>, urg: bool) {
        {
            c_rc.borrow_mut().is_urgent = urg;
        }
        let win = c_rc.borrow().win;
        // 1. 先读取现有的 WM_HINTS 属性
        let cookie = match self.x11rb_conn.get_property(
            false, // delete: 不删除
            win,   // window
            AtomEnum::WM_HINTS,
            AtomEnum::CARDINAL, // type: 期望 CARDINAL（实际是位图）
            0,                  // long_offset
            20,                 // 足够读取所有字段（flags + 数据）
        ) {
            Ok(cookie) => cookie,
            Err(_) => {
                error!("seturgent: failed to send get_property request for WM_HINTS");
                return;
            }
        };
        let reply = match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => {
                // 属性不存在，我们视为 flags = 0
                debug!("WM_HINTS not set, treating as zero");
                return self.send_wm_hints_with_flags(win, if urg { 256 } else { 0 });
                // 256 = XUrgencyHint
            }
        };
        // 2. 解析 flags（第一个 u32）
        let mut values = if let Some(values) = reply.value32() {
            values
        } else {
            return;
        };
        let mut flags = match values.next() {
            Some(f) => f,
            None => {
                debug!("WM_HINTS has no data");
                return self.send_wm_hints_with_flags(win, if urg { 256 } else { 0 });
            }
        };

        // 3. 修改 XUrgencyHint 位（第 9 位，值为 256）
        const X_URGENCY_HINT: u32 = 1 << 8; // 256
        if urg {
            flags |= X_URGENCY_HINT;
        } else {
            flags &= !X_URGENCY_HINT;
        }
        // 4. 重新设置 WM_HINTS 属性
        self.send_wm_hints_with_flags(win, flags);
    }

    /// 辅助函数：通过 `_NET_WM_STATE` 或直接设置 `WM_HINTS`
    /// 这里我们选择直接使用 `change_property` 设置 `WM_HINTS`
    fn send_wm_hints_with_flags(&self, window: u32, flags: u32) {
        // 构造属性值：flags + 其余字段保持原样（这里我们只设置 flags）
        // 如果你需要保留其他字段（如 input, initial_state 等），需从原 reply 中复制
        let data: [u32; 1] = [flags]; // 至少写入 flags
        use x11rb::wrapper::ConnectionExt;
        let _ = self
            .x11rb_conn
            .change_property32(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_HINTS,
                AtomEnum::CARDINAL,
                &data,
            )
            .and_then(|_| self.x11rb_conn.flush());
    }

    #[allow(dead_code)]
    fn send_wm_hints_with_flags_vec(&self, window: u32, flags: u32, rest: Vec<u32>) {
        let mut data = Vec::with_capacity(rest.len() + 1);
        data.push(flags);
        data.extend(rest);
        use x11rb::wrapper::ConnectionExt;
        let _ = self
            .x11rb_conn
            .change_property32(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_HINTS,
                AtomEnum::CARDINAL,
                &data,
            )
            .and_then(|_| self.x11rb_conn.flush());
    }

    pub fn showhide(&mut self, client_opt: Option<Rc<RefCell<Client>>>) {
        let client_rc = match client_opt {
            Some(c) => c,
            None => return,
        };

        let isvisible = {
            let client_borrow = client_rc.borrow();
            client_borrow.isvisible()
        };

        if isvisible {
            // 显示客户端 - 从上到下
            self.show_client(&client_rc);
        } else {
            // 隐藏客户端 - 从下到上
            self.hide_client(&client_rc);
        }
    }

    fn show_client(&mut self, client_rc: &Rc<RefCell<Client>>) {
        let (win, x, y, is_floating, is_fullscreen) = {
            let client_borrow = client_rc.borrow();
            (
                client_borrow.win,
                client_borrow.x,
                client_borrow.y,
                client_borrow.is_floating,
                client_borrow.is_fullscreen,
            )
        };

        // 移动窗口到可见位置
        if let Err(e) = self.move_window(win, x, y) {
            warn!("[show_client] Failed to move window {}: {:?}", win, e);
        }

        // 如果是浮动窗口且非全屏，调整大小
        if is_floating && !is_fullscreen {
            let (w, h) = {
                let client_borrow = client_rc.borrow();
                (client_borrow.w, client_borrow.h)
            };
            self.resize(client_rc, x, y, w, h, false);
        }

        // 递归处理下一个客户端
        let snext = {
            let client_borrow = client_rc.borrow();
            client_borrow.stack_next.clone()
        };
        self.showhide(snext);
    }

    fn hide_client(&mut self, client_rc: &Rc<RefCell<Client>>) {
        // 先递归处理下一个客户端（底部优先）
        let snext = {
            let client_borrow = client_rc.borrow();
            client_borrow.stack_next.clone()
        };
        self.showhide(snext);

        // 然后隐藏当前客户端
        let (win, y, width) = {
            let client_borrow = client_rc.borrow();
            (client_borrow.win, client_borrow.y, client_borrow.width())
        };

        // 将窗口移动到屏幕外隐藏
        let hidden_x = width * -2;
        if let Err(e) = self.move_window(win, hidden_x, y) {
            warn!("[hide_client] Failed to hide window {}: {:?}", win, e);
        }
    }

    fn move_window(
        &mut self,
        win: Window,
        x: i32,
        y: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ConfigureWindowAux::new().x(x).y(y);

        self.x11rb_conn.configure_window(win, &aux)?;
        Ok(())
    }

    pub fn configurerequest(
        &mut self,
        e: &ConfigureRequestEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let c = self.wintoclient(e.window);
        if let Some(ref client_rc) = c {
            // 检查是否是状态栏
            if let Some(&monitor_id) = self.status_bar_windows.get(&e.window) {
                info!("[configurerequest] statusbar on monitor {}", monitor_id);
                self.handle_statusbar_configure_request(monitor_id, e)?;
            } else {
                // 常规客户端的配置请求处理
                self.handle_regular_configure_request(client_rc, e)?;
            }
        } else {
            // 未管理的窗口，直接应用配置请求
            self.handle_unmanaged_configure_request(e)?;
        }
        Ok(())
    }

    fn handle_statusbar_configure_request(
        &mut self,
        monitor_id: i32,
        e: &ConfigureRequestEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "[handle_statusbar_configure_request] StatusBar resize request for monitor {}: {}x{}+{}+{} (mask: {:?})",
            monitor_id, e.width, e.height, e.x, e.y, e.value_mask
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
            if e.value_mask.contains(ConfigWindow::X) {
                statusbar_mut.x = e.x as i32;
                geometry_changed = true;
            }
            if e.value_mask.contains(ConfigWindow::Y) {
                statusbar_mut.y = e.y as i32;
                geometry_changed = true;
                needs_workarea_update = true; // Y 位置变化影响工作区
            }
            if e.value_mask.contains(ConfigWindow::WIDTH) {
                statusbar_mut.w = e.width as i32;
                geometry_changed = true;
            }
            if e.value_mask.contains(ConfigWindow::HEIGHT) {
                statusbar_mut.h = e.height as i32;
                geometry_changed = true;
                needs_workarea_update = true; // 高度变化是最主要的关注点
            }

            if geometry_changed {
                info!(
                    "[handle_statusbar_configure_request] StatusBar geometry updated: {:?} -> ({}, {}, {}, {})",
                    old_geometry, statusbar_mut.x, statusbar_mut.y, statusbar_mut.w, statusbar_mut.h
                );

                let values = ConfigureWindowAux::new()
                    .x(statusbar_mut.x)
                    .y(statusbar_mut.y)
                    .width(statusbar_mut.w as u32)
                    .height(statusbar_mut.h as u32);
                self.x11rb_conn.configure_window(e.window, &values)?;

                // 确保状态栏始终在最上层
                self.x11rb_conn.configure_window(
                    e.window,
                    &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
                )?;

                // 发送确认配置事件给 status bar
                self.configure(&mut statusbar_mut)?;
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
            self.handle_unmanaged_configure_request(e)?;
        }

        Ok(())
    }

    fn handle_regular_configure_request(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        e: &ConfigureRequestEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut client_mut = client_rc.borrow_mut();
        let is_floating = client_mut.is_floating;

        if e.value_mask.contains(ConfigWindow::BORDER_WIDTH) {
            client_mut.border_w = e.border_width as i32;
        }

        if is_floating {
            // 浮动窗口或无布局时，允许自由调整
            let (mx, my, mw, mh) = {
                let m = client_mut.mon.as_ref().unwrap().borrow();
                (m.m_x, m.m_y, m.m_w, m.m_h)
            };

            if e.value_mask.contains(ConfigWindow::X) {
                client_mut.old_x = client_mut.x;
                client_mut.x = mx + e.x as i32;
            }
            if e.value_mask.contains(ConfigWindow::Y) {
                client_mut.old_y = client_mut.y;
                client_mut.y = my + e.y as i32;
            }
            if e.value_mask.contains(ConfigWindow::WIDTH) {
                client_mut.old_w = client_mut.w;
                client_mut.w = e.width as i32;
            }
            if e.value_mask.contains(ConfigWindow::HEIGHT) {
                client_mut.old_h = client_mut.h;
                client_mut.h = e.height as i32;
            }

            // 确保窗口不超出显示器边界
            if (client_mut.x + client_mut.w) > mx + mw && client_mut.is_floating {
                client_mut.x = mx + (mw / 2 - client_mut.width() / 2);
            }
            if (client_mut.y + client_mut.h) > my + mh && client_mut.is_floating {
                client_mut.y = my + (mh / 2 - client_mut.height() / 2);
            }

            // 如果只是位置变化，发送配置确认
            if e.value_mask.contains(ConfigWindow::X | ConfigWindow::Y)
                && !e
                    .value_mask
                    .contains(ConfigWindow::WIDTH | ConfigWindow::HEIGHT)
            {
                self.configure(&mut client_mut)?;
            }

            let isvisible = client_mut.isvisible();
            if isvisible {
                self.x11rb_conn.configure_window(
                    client_mut.win,
                    &ConfigureWindowAux::new()
                        .x(client_mut.x)
                        .y(client_mut.y)
                        .width(client_mut.w as u32)
                        .height(client_mut.h as u32),
                )?;
            }
        } else {
            // 平铺布局中的窗口，只允许有限的配置更改
            self.configure(&mut client_mut)?;
        }

        Ok(())
    }

    fn handle_unmanaged_configure_request(
        &mut self,
        e: &ConfigureRequestEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 对于未管理的窗口，构建并应用配置请求
        let mut values = ConfigureWindowAux::new();

        if e.value_mask.contains(ConfigWindow::X) {
            values = values.x(e.x as i32);
        }
        if e.value_mask.contains(ConfigWindow::Y) {
            values = values.y(e.y as i32);
        }
        if e.value_mask.contains(ConfigWindow::WIDTH) {
            values = values.width(e.width as u32);
        }
        if e.value_mask.contains(ConfigWindow::HEIGHT) {
            values = values.height(e.height as u32);
        }
        if e.value_mask.contains(ConfigWindow::BORDER_WIDTH) {
            values = values.border_width(e.border_width as u32);
        }
        if e.value_mask.contains(ConfigWindow::SIBLING) {
            values = values.sibling(e.sibling);
        }
        if e.value_mask.contains(ConfigWindow::STACK_MODE) {
            values = values.stack_mode(e.stack_mode);
        }

        self.x11rb_conn.configure_window(e.window, &values)?;
        Ok(())
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

    pub fn destroynotify(
        &mut self,
        e: &DestroyNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[destroynotify]");
        let c = self.wintoclient(e.window);
        if let Some(client_opt) = c {
            self.unmanage(Some(client_opt), true)?;
        }
        Ok(())
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

    pub fn dirtomon(&mut self, dir: &i32) -> Option<Rc<RefCell<Monitor>>> {
        let selected_monitor = self.sel_mon.as_ref()?; // Return None if selmon is None
        let monitors_head = self.mons.as_ref()?; // Return None if mons is None
        if *dir > 0 {
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
                    error!("写入错误: {}", e);
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

    pub fn restack(
        &mut self,
        mon_rc_opt: Option<Rc<RefCell<Monitor>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[restack]");

        let mon_rc = match mon_rc_opt {
            Some(monitor) => monitor,
            None => return Ok(()),
        };
        self.mark_bar_update_needed(Some(mon_rc.borrow().num));
        let mon_borrow = mon_rc.borrow();
        // 收集所有需要重新排列的窗口
        let mut windows_to_restack = Vec::new();
        // 1. 首先处理选中的浮动窗口
        if let Some(ref sel) = mon_borrow.sel {
            let sel_borrow = sel.borrow();
            if sel_borrow.is_floating {
                windows_to_restack.push((sel_borrow.win, StackMode::ABOVE, None));
            }
        }

        // 2. 处理状态栏（确保在最顶层）
        let monitor_id = mon_borrow.num;
        if let Some(statusbar) = self.status_bar_clients.get(&monitor_id) {
            let statusbar_win = statusbar.borrow().win;
            windows_to_restack.push((statusbar_win, StackMode::ABOVE, None));
        }

        // 3. 收集非浮动可见窗口
        let mut non_floating_windows = Vec::new();
        let mut client_rc_opt = mon_borrow.stack.clone();
        while let Some(ref client_rc) = client_rc_opt.clone() {
            let client_borrow = client_rc.borrow();
            if !client_borrow.is_floating && client_borrow.isvisible() {
                non_floating_windows.push(client_borrow.win);
            }
            let next = client_borrow.stack_next.clone();
            client_rc_opt = next;
        }

        // 4. 按堆叠顺序排列非浮动窗口
        for (i, &win) in non_floating_windows.iter().enumerate() {
            let sibling = if i == 0 {
                None
            } else {
                Some(non_floating_windows[i - 1])
            };
            windows_to_restack.push((win, StackMode::BELOW, sibling));
        }

        // 5. 批量执行所有窗口重排操作
        for (window, stack_mode, sibling) in windows_to_restack {
            let mut config = ConfigureWindowAux::new().stack_mode(stack_mode);
            if let Some(sibling_win) = sibling {
                config = config.sibling(sibling_win);
            }
            self.x11rb_conn.configure_window(window, &config)?;
        }

        // 6. 同步所有操作
        self.x11rb_conn.flush()?;

        // 7. 刷新进入事件
        self.flush_enter_events();

        Ok(())
    }

    pub fn flush_enter_events(&self) {
        loop {
            match self.x11rb_conn.poll_for_event() {
                Ok(Some(event)) => {
                    if EventMask::from(event.response_type()) == EventMask::ENTER_WINDOW {
                        continue;
                    } else {
                        // (TODO): need to store the event?
                        break;
                    }
                }
                Ok(None) => {
                    break;
                }
                Err(e) => {
                    error!("Error polling for event: {:?}", e);
                    break;
                }
            }
        }

        self.x11rb_conn
            .flush()
            .expect("Failed to flush X11 connection");
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

    pub async fn run_async(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 初始同步
        self.x11rb_conn.flush()?;
        let mut event_count: u64 = 0;
        let mut update_timer = tokio::time::interval(Duration::from_millis(10)); // 10ms for ~100 FPS
        info!("Starting async event loop");
        while self.running.load(Ordering::SeqCst) {
            let mut events_processed = false;
            // 处理所有挂起的X11事件
            while let Some(event) = self.x11rb_conn.poll_for_event()? {
                event_count = event_count.wrapping_add(1);
                let _ = self.handler(event);
                events_processed = true;
            }

            // 处理来自status bar的命令 (保持不变)
            self.process_commands_from_status_bar();

            // ✨ 在事件循环结束后，批量更新状态栏 (保持不变)
            if events_processed || !self.pending_bar_updates.is_empty() {
                self.flush_pending_bar_updates();
            }

            // 使用tokio的异步等待替代select
            tokio::select! {
                _ = update_timer.tick() => {
                    // 超时，检查是否有挂起的更新
                    if !self.pending_bar_updates.is_empty() {
                        self.flush_pending_bar_updates();
                    }
                }
                _ = self.wait_for_x11_ready() => {
                    // X11事件就绪，下次循环会处理
                }
            }
        }
        Ok(())
    }

    // 等待X11事件就绪的辅助函数
    async fn wait_for_x11_ready(&self) -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::io::AsRawFd;
        use tokio::io::unix::AsyncFd;
        // 获取底层的TCP流或Unix域套接字
        let stream = self.x11rb_conn.stream();
        let fd = stream.as_raw_fd();
        let async_fd = AsyncFd::new(fd)?;
        let mut guard = async_fd.readable().await?;
        guard.clear_ready();
        Ok(())
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
                    let _ = self.view(&arg);
                }
                CommandType::ToggleTag => {
                    // 切换标签
                    info!(
                        "[process_commands] ToggleTag command received: {}",
                        cmd.parameter
                    );
                    let arg = Arg::Ui(cmd.parameter);
                    let _ = self.toggletag(&arg);
                }
                CommandType::SetLayout => {
                    // 设置布局
                    info!(
                        "[process_commands] SetLayout command received: {}",
                        cmd.parameter
                    );
                    let arg = Arg::Lt(Rc::new(Layout::try_from(cmd.parameter as u8).unwrap()));
                    let _ = self.setlayout(&arg);
                }
                CommandType::None => {}
            }
        }
    }

    pub fn scan(&mut self) -> Result<(), ReplyOrIdError> {
        // info!("[scan]");
        let tree_reply = self.x11rb_conn.query_tree(self.x11rb_root)?.reply()?;
        let mut cookies = Vec::with_capacity(tree_reply.children.len());
        for win in tree_reply.children {
            let attr = self.get_window_attributes(win)?;
            let geom = Self::get_and_query_window_geom(&self.x11rb_conn, win)?;
            let trans = self.get_transient_for(win);
            cookies.push((win, attr, geom, trans));
        }
        for (win, attr, geom, trans) in &cookies {
            if attr.override_redirect || trans.is_some() {
                continue;
            }
            if attr.map_state == MapState::VIEWABLE || self.get_wm_state(*win) == IconicState as i64
            {
                self.manage(*win, geom);
            }
        }
        for (win, attr, geom, trans) in &cookies {
            {
                if trans.is_some() {
                    if attr.map_state == MapState::VIEWABLE
                        || self.get_wm_state(*win) == IconicState as i64
                    {
                        self.manage(*win, geom);
                    }
                }
            }
        }
        Ok(())
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
            let _ = self.restack(Some(mon_rc)); // Pass Some(mon_rc) to restack
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

    /// 从窗口属性中读取一个 Atom 值

    /// 如果失败或属性不存在，返回 0

    pub fn getatomprop(&self, c: &Client, prop: Atom) -> Atom {
        // 发送 GetProperty 请求
        let cookie = match self.x11rb_conn.get_property(
            false,          // delete: 是否删除属性（false）
            c.win,          // window
            prop,           // property
            AtomEnum::ATOM, // req_type: 期望的类型（Atom）
            0,              // long_offset
            1,              // long_length (最多读取 1 个 Atom)
        ) {
            Ok(cookie) => cookie,
            Err(_) => return 0, // 请求发送失败
        };

        // 等待回复
        let reply = match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => return 0, // 无回复或属性不存在
        };
        let mut values = if let Some(values) = reply.value32() {
            values
        } else {
            return 0;
        };

        // 提取第一个 Atom 值（32 位）
        values.next().unwrap_or(0)
    }

    pub fn getrootptr(&mut self) -> Result<(i32, i32), ReplyError> {
        let cookie = self.x11rb_conn.query_pointer(self.x11rb_root)?;
        let reply = cookie.reply()?;
        Ok((reply.root_x as i32, reply.root_y as i32))
    }

    /// 获取窗口的 WM_STATE 状态
    /// 返回值：1 = NormalState, 3 = IconicState, -1 = 失败
    pub fn get_wm_state(&self, window: u32) -> i64 {
        // 发送 GetProperty 请求
        let cookie = match self.x11rb_conn.get_property(
            false,               // delete: 不删除属性
            window,              // window
            self.atoms.WM_STATE, // property: _NET_WM_STATE
            self.atoms.WM_STATE, // type: 期望类型也是 WM_STATE
            0,                   // long_offset
            2,                   // long_length: 最多读取 2 个 32-bit 值
        ) {
            Ok(cookie) => cookie,
            Err(_) => {
                error!("get_wm_state: failed to send get_property request");
                return -1;
            }
        };

        // 等待回复
        let reply = match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => {
                // 属性不存在或类型不匹配
                return -1;
            }
        };

        // 检查格式是否为 32 位
        if reply.format != 32 {
            return -1;
        }
        let mut values = if let Some(values) = reply.value32() {
            values
        } else {
            return -1;
        };

        // 提取第一个值（state）
        let state = match values.next() {
            Some(s) => s as i64,
            None => return -1, // 空数据
        };
        // 可选：第二个值是 icon_window，我们不使用
        // let _icon_window = iter.next();
        state
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
        if w == self.x11rb_root {
            if let Ok((x, y)) = self.getrootptr() {
                return self.recttomon(x, y, 1, 1);
            }
        }
        let c = self.wintoclient(w);
        if let Some(ref client_opt) = c {
            return client_opt.borrow().mon.clone();
        }
        return self.sel_mon.clone();
    }

    pub fn buttonpress(&mut self, e: &ButtonPressEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[buttonpress]");
        let _arg: Arg = Arg::Ui(0);

        let c: Option<Rc<RefCell<Client>>>;
        let mut click = CLICK::ClkRootWin;

        // focus monitor if necessary.
        let m = self.wintomon(e.event as u32);
        if m.is_some() && !Rc::ptr_eq(m.as_ref().unwrap(), self.sel_mon.as_ref().unwrap()) {
            let sel = self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone();
            self.unfocus(sel, true)?;
            self.sel_mon = m;
            self.focus(None)?;
        }

        // 检查是否点击了客户端窗口
        c = self.wintoclient(e.event as u32);
        if c.is_some() {
            self.focus(c)?;
            let _ = self.restack(self.sel_mon.clone());

            // 使用x11rb的allow_events
            self.x11rb_conn
                .allow_events(Allow::REPLAY_POINTER, e.time)?;
            click = CLICK::ClkClientWin;
        }

        // 处理按钮配置
        let buttons = CONFIG.get_buttons();
        for button_config in buttons.iter() {
            if click as u32 == button_config.click
                && button_config.func.is_some()
                && button_config.button == e.detail as u32
                && self.clean_mask(button_config.mask) == self.clean_mask(e.state.bits().into())
            {
                if let Some(ref func) = button_config.func {
                    info!(
                        "[buttonpress] click: {}, button: {}, mask: {}",
                        button_config.click, button_config.button, button_config.mask
                    );
                    info!("[buttonpress] use button arg");
                    let _ = func(self, &button_config.arg);
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn checkotherwm(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[checkotherwm]");
        // 在 XCB 中，我们通过尝试选择 SubstructureRedirect 事件来检查
        // 如果有其他窗口管理器运行，这个操作会失败
        let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_REDIRECT);
        match self
            .x11rb_conn
            .change_window_attributes(self.x11rb_root, &aux)
        {
            Ok(cookie) => {
                // 等待请求完成，检查是否有错误
                match cookie.check() {
                    Ok(_) => {
                        info!("[checkotherwm] Successfully acquired SubstructureRedirect, no other WM running");
                        Ok(())
                    }
                    Err(e) => {
                        error!(
                            "[checkotherwm] Failed to acquire SubstructureRedirect: {:?}",
                            e
                        );
                        // 检查错误类型
                        match e {
                            x11rb::errors::ReplyError::X11Error(ref x11_error) => {
                                if x11_error.error_kind == x11rb::protocol::ErrorKind::Access {
                                    error!("jwm: another window manager is already running");
                                    std::process::exit(1);
                                }
                            }
                            _ => {
                                error!("jwm: X11 connection error during WM check");
                                std::process::exit(1);
                            }
                        }
                        Err(e.into())
                    }
                }
            }
            Err(e) => {
                error!(
                    "[checkotherwm] Failed to send change_window_attributes request: {:?}",
                    e
                );
                error!("jwm: failed to communicate with X server");
                std::process::exit(1);
            }
        }
    }

    pub fn spawn(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[spawn]");

        let mut mut_arg: Arg = arg.clone();
        if let Arg::V(ref mut v) = mut_arg {
            // 处理 dmenu 命令的特殊情况
            if *v == *CONFIG.get_dmenucmd() {
                let monitor_num = self.sel_mon.as_ref().unwrap().borrow().num;
                let tmp = (b'0' + monitor_num as u8) as char;
                let tmp = tmp.to_string();
                info!("[spawn] dmenumon tmp: {}, num: {}", tmp, monitor_num);
                (*v)[2] = tmp;
            }

            info!("[spawn] spawning command: {:?}", v);

            // 使用 Rust 的 Command API，它会自动处理 fork/exec
            let mut command = Command::new(&v[0]);
            command.args(&v[1..]);

            // 配置子进程
            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());

            // 使用 pre_exec 来设置子进程环境
            use std::os::unix::io::AsRawFd;
            use std::os::unix::process::CommandExt;

            let x11_fd = self.x11rb_conn.stream().as_raw_fd();

            unsafe {
                command.pre_exec(move || {
                    // 关闭继承的 X11 连接
                    close(x11_fd);
                    setsid();

                    // 重置 SIGCHLD 信号处理
                    let mut sa: sigaction = std::mem::zeroed();
                    sigemptyset(&mut sa.sa_mask);
                    sa.sa_flags = 0;
                    sa.sa_sigaction = SIG_DFL;
                    sigaction(SIGCHLD, &sa, std::ptr::null_mut());
                    Ok(())
                });
            }
            // 启动子进程
            match command.spawn() {
                Ok(child) => {
                    debug!(
                        "[spawn] successfully spawned process with PID: {}",
                        child.id()
                    );
                    // 不等待子进程，让它在后台运行
                }
                Err(e) => {
                    error!("[spawn] failed to spawn command {:?}: {}", v, e);
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    fn xinit_visual(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let x11rb_screen = self.x11rb_conn.setup().roots[self.x11rb_screen_num].clone();

        // 首先尝试找到支持 alpha 通道的 32 位视觉效果
        for depth in &x11rb_screen.allowed_depths {
            if depth.depth != 32 {
                continue;
            }

            for visualtype in &depth.visuals {
                if visualtype.class != VisualClass::TRUE_COLOR {
                    continue;
                }

                // 检查 render 扩展中是否有对应的格式
                match self.find_render_format_for_visual(visualtype.visual_id) {
                    Ok(Some(format)) if self.has_alpha_channel(&format) => {
                        // 找到了支持 alpha 的格式
                        return self.setup_argb_visual(visualtype, &format);
                    }
                    Ok(_) => continue, // 格式不支持 alpha，继续查找
                    Err(e) => {
                        warn!("[xinit_visual] Failed to query render format: {:?}", e);
                        continue;
                    }
                }
            }
        }

        // 如果没找到 32 位 ARGB 视觉效果，回退到默认
        info!("[xinit_visual] No 32-bit ARGB visual found. Falling back to default.");
        self.setup_default_visual()
    }

    fn find_render_format_for_visual(
        &self,
        visual_id: Visualid,
    ) -> Result<Option<Pictforminfo>, Box<dyn std::error::Error>> {
        use x11rb::protocol::render::ConnectionExt;

        let format_cookie = self.x11rb_conn.render_query_pict_formats()?;
        let format_reply = format_cookie.reply()?;

        // 查找匹配的 PictFormat
        for format in &format_reply.formats {
            if format.id == visual_id {
                return Ok(Some(*format));
            }
        }

        Ok(None)
    }

    fn has_alpha_channel(&self, format: &Pictforminfo) -> bool {
        // 检查是否有 alpha 通道
        format.direct.alpha_mask > 0
    }

    fn setup_argb_visual(
        &mut self,
        visualtype: &Visualtype,
        _format: &Pictforminfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.visual_id = visualtype.visual_id;
        self.depth = 32;

        // 创建 colormap
        let colormap_id = self.x11rb_conn.generate_id()?;
        self.x11rb_conn
            .create_colormap(
                ColormapAlloc::NONE,
                colormap_id,
                self.x11rb_root,
                visualtype.visual_id,
            )?
            .check()?;

        self.color_map = colormap_id.into();

        // 测试颜色分配（使用更安全的颜色值）
        match self.test_color_allocation(colormap_id) {
            Ok(_) => {
                info!("[xinit_visual] Successfully set up 32-bit ARGB visual. VisualID: 0x{:x}, ColormapID: 0x{:x}",
                  self.visual_id, self.color_map);
                Ok(())
            }
            Err(e) => {
                warn!("[xinit_visual] Color allocation test failed: {:?}", e);
                // 清理失败的 colormap
                let _ = self.x11rb_conn.free_colormap(colormap_id);
                self.setup_default_visual()
            }
        }
    }

    fn setup_default_visual(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let x11rb_screen = &self.x11rb_conn.setup().roots[self.x11rb_screen_num];

        self.visual_id = x11rb_screen.root_visual;
        self.depth = x11rb_screen.root_depth;
        self.color_map = x11rb_screen.default_colormap.into();

        info!(
            "[xinit_visual] Using default visual. VisualID: 0x{:x}, Depth: {}, ColormapID: 0x{:x}",
            self.visual_id, self.depth, self.color_map
        );

        Ok(())
    }

    fn test_color_allocation(
        &self,
        colormap_id: Colormap,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 测试分配一个简单的颜色（红色）
        let color_reply = self
            .x11rb_conn
            .alloc_color(colormap_id, 65535, 0, 0)?
            .reply()?;

        debug!(
            "[test_color_allocation] Successfully allocated test color, pixel: {}",
            color_reply.pixel
        );

        // 可选：释放测试颜色
        let _ = self
            .x11rb_conn
            .free_colors(colormap_id, 0, &[color_reply.pixel]);

        Ok(())
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

    pub fn togglefloating(&mut self, _arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[togglefloating]");
        if self.sel_mon.is_none() {
            return Ok(());
        }
        let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
        if let Some(ref sel_opt) = sel {
            // no support for fullscreen windows.
            let isfullscreen = { sel_opt.borrow_mut().is_fullscreen };
            if isfullscreen {
                return Ok(());
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
        }
        Ok(())
    }

    pub fn focusin(&mut self, e: &FocusInEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[focusin]");
        let sel = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
        if let Some(ref sel_client) = sel {
            if e.event != sel_client.borrow().win {
                self.setfocus(sel_client)?;
            }
        }
        Ok(())
    }

    pub fn focusmon(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[focusmon]");
        if let Some(ref mons_opt) = self.mons {
            if mons_opt.borrow_mut().next.is_none() {
                return Ok(());
            }
        }
        if let Arg::I(i) = arg {
            let m = self.dirtomon(i);
            if Rc::ptr_eq(m.as_ref().unwrap(), self.sel_mon.as_ref().unwrap()) {
                return Ok(());
            }
            let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
            self.unfocus(sel, false)?;
            self.sel_mon = m;
            self.focus(None)?;
        }
        Ok(())
    }

    pub fn tag(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[tag]");
        if let Arg::Ui(ui) = *arg {
            let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
            let target_tag = ui & CONFIG.tagmask();
            if let Some(ref sel_opt) = sel {
                if target_tag > 0 {
                    sel_opt.borrow_mut().tags = target_tag;
                    let _ = self.setclienttagprop(sel_opt);
                    self.focus(None)?;
                    self.arrange(self.sel_mon.clone());
                }
            }
        }
        Ok(())
    }

    pub fn tagmon(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[tagmon]");
        if let Some(ref selmon_opt) = self.sel_mon {
            if selmon_opt.borrow_mut().sel.is_none() {
                return Ok(());
            }
        } else {
            return Ok(());
        }
        if let Some(ref mons_opt) = self.mons {
            if mons_opt.borrow_mut().next.is_none() {
                return Ok(());
            }
        } else {
            return Ok(());
        }
        if let Arg::I(i) = *arg {
            let selmon_clone = self.sel_mon.clone();
            if let Some(ref selmon_opt) = selmon_clone {
                let dir_i_mon = self.dirtomon(&i);
                let sel = { selmon_opt.borrow_mut().sel.clone() };
                self.sendmon(sel, dir_i_mon);
            }
        }
        Ok(())
    }

    pub fn focusstack(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[focusstack]");
        {
            let sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
            if sel_mon_mut.sel.is_none()
                || (sel_mon_mut.sel.as_ref().unwrap().borrow_mut().is_fullscreen
                    && CONFIG.behavior().lock_fullscreen)
            {
                return Ok(());
            }
        }
        let mut c: Option<Rc<RefCell<Client>>> = None;
        let i = if let Arg::I(i) = *arg { i } else { 0 };
        if i == 0 {
            return Ok(());
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
            self.focus(c)?;
            let _ = self.restack(self.sel_mon.clone());
        }
        Ok(())
    }

    pub fn togglebar(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[togglebar]");
        if let Arg::I(_) = arg {
            let mut monitor_num = None;
            if let Some(sel_mon_ref) = self.sel_mon.as_ref() {
                let mut sel_mon_borrow_mut = sel_mon_ref.borrow_mut();
                if let Some(pertag_mut) = sel_mon_borrow_mut.pertag.as_mut() {
                    let cur_tag = pertag_mut.cur_tag;
                    if let Some(show_bar) = pertag_mut.show_bars.get_mut(cur_tag) {
                        *show_bar = !(*show_bar);
                        info!("[togglebar] {}", show_bar);
                        monitor_num = Some(sel_mon_borrow_mut.num);
                    }
                }
            }
            if monitor_num.is_some() {
                self.mark_bar_update_needed(monitor_num);
            }
        }
        Ok(())
    }

    pub fn incnmaster(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[incnmaster]");
        if let Arg::I(i) = *arg {
            let mut sel_mon_mut = self.sel_mon.as_mut().unwrap().borrow_mut();
            let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
            sel_mon_mut.pertag.as_mut().unwrap().n_masters[cur_tag] =
                0.max(sel_mon_mut.n_master as i32 + i) as u32;

            sel_mon_mut.n_master = sel_mon_mut.pertag.as_ref().unwrap().n_masters[cur_tag];
        }
        self.arrange(self.sel_mon.clone());
        Ok(())
    }

    pub fn setcfact(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[setcfact]");
        let c = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
        if c.is_none() {
            return Ok(());
        }
        if let Arg::F(f0) = *arg {
            let mut f = f0 + c.as_ref().unwrap().borrow().client_fact;
            if f0.abs() < 0.0001 {
                f = 1.0;
            } else if f < 0.25 || f > 4.0 {
                return Ok(());
            }
            c.as_ref().unwrap().borrow_mut().client_fact = f;
            self.arrange(self.sel_mon.clone());
        }
        Ok(())
    }

    pub fn movestack(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        let mut c: Option<Rc<RefCell<Client>>> = None;
        let mut i: Option<Rc<RefCell<Client>>>;
        let mut p: Option<Rc<RefCell<Client>>> = None;
        let mut pc: Option<Rc<RefCell<Client>>> = None;
        if let Arg::I(arg_i) = arg {
            if arg_i > &0 {
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
                let temp = if sel_next.is_some() && c.is_some() && Self::are_equal_rc(&sel_next, &c)
                {
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
            return Ok(());
        }
        Ok(())
    }

    pub fn setmfact(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[setmfact]");
        if let Arg::F(f) = arg {
            let mut sel_mon_mut = self.sel_mon.as_mut().unwrap().borrow_mut();
            let f = if f < &1.0 {
                f + sel_mon_mut.m_fact
            } else {
                f - 1.0
            };
            if f < 0.05 || f > 0.95 {
                return Ok(());
            }
            let cur_tag = sel_mon_mut.pertag.as_ref().unwrap().cur_tag;
            sel_mon_mut.pertag.as_mut().unwrap().m_facts[cur_tag] = f;
            sel_mon_mut.m_fact = sel_mon_mut.pertag.as_mut().unwrap().m_facts[cur_tag];
        }
        self.arrange(self.sel_mon.clone());
        Ok(())
    }

    pub fn setlayout(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[setlayout]");
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
        Ok(())
    }

    pub fn zoom(&mut self, _arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[zoom]");
        let mut c;
        let sel_c;
        let nexttiled_c;
        {
            let sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow();
            c = sel_mon_mut.sel.clone();
            if c.is_none() || c.as_ref().unwrap().borrow().is_floating {
                return Ok(());
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
                return Ok(());
            }
        }
        self.pop(c);
        Ok(())
    }

    pub fn loopview(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[loopview]");
        let direction = if let Arg::I(val) = arg {
            val
        } else {
            return Ok(());
        };
        if direction == &0 {
            return Ok(());
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
            let next_tag_index = if direction > &0 {
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
                return Ok(());
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
            sel_mon_mut.lt[sel_lt] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs[cur_tag][sel_lt]
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

        self.focus(sel_opt)?;
        self.arrange(self.sel_mon.clone());
        Ok(())
    }

    pub fn view(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[view]");
        let ui = if let Arg::Ui(val) = arg {
            val
        } else {
            return Ok(());
        };
        let target_tag = ui & CONFIG.tagmask();
        let cur_tag;
        {
            let mut sel_mon_mut = self.sel_mon.as_ref().unwrap().borrow_mut();
            info!("[view] ui: {ui}, {target_tag}, {:?}", sel_mon_mut.tag_set);
            if target_tag == sel_mon_mut.tag_set[sel_mon_mut.sel_tags] {
                return Ok(());
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
                if *ui == !0 {
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
            sel_mon_mut.lt[sel_lt] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs[cur_tag][sel_lt]
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
        self.focus(sel_opt)?;
        self.arrange(self.sel_mon.clone());
        Ok(())
    }

    pub fn toggleview(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[toggleview]");
        if let Arg::Ui(ui) = *arg {
            if self.sel_mon.is_none() {
                return Ok(());
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
                    sel_mon_mut.n_master = sel_mon_mut.pertag.as_ref().unwrap().n_masters[cur_tag];
                    sel_mon_mut.m_fact = sel_mon_mut.pertag.as_ref().unwrap().m_facts[cur_tag];
                    sel_mon_mut.sel_lt = sel_mon_mut.pertag.as_ref().unwrap().sel_lts[cur_tag];
                    let sel_lt = sel_mon_mut.sel_lt;
                    sel_mon_mut.lt[sel_lt] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs[cur_tag]
                        [sel_lt]
                        .clone()
                        .expect("None unwrap");
                    sel_mon_mut.lt[sel_lt ^ 1] = sel_mon_mut.pertag.as_ref().unwrap().lt_idxs
                        [cur_tag][sel_lt ^ 1]
                        .clone()
                        .expect("None unwrap");
                }
                self.focus(None)?;
                self.arrange(self.sel_mon.clone());
            }
        }
        Ok(())
    }

    pub fn togglefullscr(&mut self, _: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[togglefullscr]");
        if let Some(ref selmon_opt) = self.sel_mon {
            let sel = { selmon_opt.borrow_mut().sel.clone() };
            if sel.is_none() {
                return Ok(());
            }
            let isfullscreen = { sel.as_ref().unwrap().borrow_mut().is_fullscreen };
            let _ = self.setfullscreen(sel.as_ref().unwrap(), !isfullscreen);
        }
        Ok(())
    }

    pub fn toggletag(&mut self, arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[toggletag]");
        let sel = { self.sel_mon.as_ref().unwrap().borrow_mut().sel.clone() };
        if sel.is_none() {
            return Ok(());
        }
        if let Arg::Ui(ui) = *arg {
            let newtags = sel.as_ref().unwrap().borrow_mut().tags ^ (ui & CONFIG.tagmask());
            if newtags > 0 {
                sel.as_ref().unwrap().borrow_mut().tags = newtags;
                let _ = self.setclienttagprop(sel.as_ref().unwrap());
                self.focus(None)?;
                self.arrange(self.sel_mon.clone());
            }
        }
        Ok(())
    }

    pub fn quit(&mut self, _arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[quit]");
        self.running.store(false, Ordering::SeqCst);
        let _ = self.sender.send(0);
        Ok(())
    }

    pub fn setup_ewmh(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // --- 1. 创建 _NET_SUPPORTING_WM_CHECK 窗口 ---
        let frame_win = self.x11rb_conn.generate_id()?;
        let x11rb_screen = &self.x11rb_conn.setup().roots[self.x11rb_screen_num];
        self.wm_check_win = frame_win;
        let win_aux = CreateWindowAux::new()
            .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS)
            .background_pixel(x11rb_screen.white_pixel);
        self.x11rb_conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            frame_win,
            x11rb_screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &win_aux,
        )?;

        // --- 2. 设置 _NET_SUPPORTING_WM_CHECK 窗口的属性 ---

        // _NET_SUPPORTING_WM_CHECK = wm_check_win (Atom 类型 WINDOW)
        use x11rb::wrapper::ConnectionExt;
        self.x11rb_conn.change_property32(
            PropMode::REPLACE,
            frame_win,
            self.atoms._NET_SUPPORTING_WM_CHECK,
            AtomEnum::WINDOW,
            &[frame_win],
        )?;

        // _NET_WM_NAME = "jwm" (UTF-8)
        self.x11rb_conn.change_property8(
            PropMode::REPLACE,
            frame_win,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            b"jwm",
        )?;

        self.x11rb_conn.change_property32(
            PropMode::REPLACE,
            x11rb_screen.root,
            self.atoms._NET_SUPPORTING_WM_CHECK,
            AtomEnum::WINDOW,
            &[frame_win],
        )?;

        // --- 4. 声明支持的 EWMH 属性 (_NET_SUPPORTED) ---
        let supported_atoms = [
            self.atoms._NET_ACTIVE_WINDOW,
            self.atoms._NET_SUPPORTED,
            self.atoms._NET_WM_NAME,
            self.atoms._NET_WM_STATE,
            self.atoms._NET_SUPPORTING_WM_CHECK,
            self.atoms._NET_WM_STATE_FULLSCREEN,
            self.atoms._NET_WM_WINDOW_TYPE,
            self.atoms._NET_WM_WINDOW_TYPE_DIALOG,
            self.atoms._NET_CLIENT_LIST,
            self.atoms._NET_CLIENT_INFO,
        ];
        self.x11rb_conn.change_property32(
            PropMode::REPLACE,
            x11rb_screen.root,
            self.atoms._NET_SUPPORTED,
            AtomEnum::ATOM,
            &supported_atoms,
        )?;

        // --- 5. 清除 _NET_CLIENT_LIST 和 _NET_CLIENT_INFO ---
        let _ = self
            .x11rb_conn
            .delete_property(x11rb_screen.root, self.atoms._NET_CLIENT_LIST);
        let _ = self
            .x11rb_conn
            .delete_property(x11rb_screen.root, self.atoms._NET_CLIENT_INFO);

        // --- 6. 刷新请求 ---
        let _ = self.x11rb_conn.flush();
        Ok(())
    }

    pub fn setup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[setup]");

        // 初始化视觉效果
        self.xinit_visual()?;

        // 更新几何信息
        self.updategeom();

        // 设置 EWMH
        self.setup_ewmh()?;

        // 选择根窗口事件
        let aux = ChangeWindowAttributesAux::new()
            .event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::BUTTON_PRESS
                    | EventMask::POINTER_MOTION
                    | EventMask::ENTER_WINDOW
                    | EventMask::LEAVE_WINDOW
                    | EventMask::PROPERTY_CHANGE,
            )
            .cursor(
                self.cursor_manager
                    .get_cursor(&self.x11rb_conn, crate::xcb_util::StandardCursor::LeftPtr)?,
            );

        self.x11rb_conn
            .change_window_attributes(self.x11rb_root, &aux)?;

        // 抓取按键
        self.grabkeys()?;

        // 设置焦点
        self.focus(None)?;

        self.x11rb_conn.flush()?;
        Ok(())
    }

    fn register_client_events(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = client_rc.borrow().win;

        // 选择窗口事件
        let aux = ChangeWindowAttributesAux::new().event_mask(
            EventMask::ENTER_WINDOW
                | EventMask::FOCUS_CHANGE
                | EventMask::PROPERTY_CHANGE
                | EventMask::STRUCTURE_NOTIFY,
        );

        self.x11rb_conn.change_window_attributes(win, &aux)?;

        // 抓取按钮
        self.grabbuttons(Some(client_rc.clone()), false)?;

        // 更新 EWMH _NET_CLIENT_LIST
        use x11rb::wrapper::ConnectionExt;
        self.x11rb_conn.change_property32(
            PropMode::APPEND,
            self.x11rb_root,
            self.atoms._NET_CLIENT_LIST,
            AtomEnum::WINDOW,
            &[client_rc.borrow().win],
        )?;

        info!(
            "[register_client_events] Events registered for window {}",
            win
        );
        Ok(())
    }

    pub fn killclient(&mut self, _arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[killclient]");
        let sel = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
        if sel.is_none() {
            return Ok(());
        }
        let client_rc = sel.as_ref().unwrap();
        info!("[killclient] {}", client_rc.borrow());
        // 首先尝试发送 WM_DELETE_WINDOW 协议消息（优雅关闭）
        if self.sendevent(&mut client_rc.borrow_mut(), self.atoms.WM_DELETE_WINDOW) {
            info!("[killclient] Sent WM_DELETE_WINDOW protocol message");
            return Ok(());
        }
        // 如果优雅关闭失败，强制终止客户端
        info!("[killclient] WM_DELETE_WINDOW failed, force killing client");
        self.force_kill_client(client_rc)?;
        Ok(())
    }

    fn force_kill_client(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = client_rc.borrow().win;
        // 抓取服务器以确保操作的原子性
        self.x11rb_conn.grab_server()?;
        // 设置关闭模式为销毁所有资源
        self.x11rb_conn
            .set_close_down_mode(CloseDown::DESTROY_ALL)?;
        // 强制终止客户端
        match self.x11rb_conn.kill_client(win) {
            Ok(cookie) => {
                // 同步并检查结果
                self.x11rb_conn.flush()?;
                if let Err(e) = cookie.check() {
                    warn!("[force_kill_client] Kill client failed: {:?}", e);
                    // 即使失败也继续，因为窗口可能已经被销毁
                }
            }
            Err(e) => {
                warn!(
                    "[force_kill_client] Failed to send kill_client request: {:?}",
                    e
                );
            }
        }
        // 释放服务器
        self.x11rb_conn.ungrab_server()?;
        self.x11rb_conn.flush()?;
        info!(
            "[force_kill_client] Force kill completed for window {}",
            win
        );
        Ok(())
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
        let _ = self.focus(c.clone());
        let mon = { c.as_ref().unwrap().borrow_mut().mon.clone() };
        self.arrange(mon);
    }

    pub fn gettextprop(&mut self, w: Window, atom: Atom, text: &mut String) -> bool {
        // 清空输出字符串
        text.clear();

        // 获取窗口属性
        let property = match self.x11rb_conn.get_property(
            false,         // delete: 不删除属性
            w,             // window
            atom,          // property
            AtomEnum::ANY, // type: 接受任何类型
            0,             // long_offset
            u32::MAX,      // long_length: 读取全部内容
        ) {
            Ok(cookie) => match cookie.reply() {
                Ok(prop) => prop,
                Err(e) => {
                    debug!("[gettextprop] Failed to get property reply: {:?}", e);
                    return false;
                }
            },
            Err(e) => {
                debug!("[gettextprop] Failed to send get_property request: {:?}", e);
                return false;
            }
        };

        // 检查属性是否有效
        if property.value.is_empty() {
            debug!("[gettextprop] Property value is empty");
            return false;
        }

        // 根据属性类型和格式处理文本
        match (property.type_, property.format) {
            // UTF8_STRING 类型 (现代应用首选)
            (type_, 8) if type_ == self.atoms.UTF8_STRING => {
                match String::from_utf8(property.value) {
                    Ok(utf8_string) => {
                        *text = self.truncate_text(utf8_string);
                        return true;
                    }
                    Err(e) => {
                        debug!("[gettextprop] Invalid UTF-8 in UTF8_STRING: {:?}", e);
                        return false;
                    }
                }
            }

            // STRING 类型 (传统的 Latin-1 编码)
            (type_, 8) if type_ == AtomEnum::STRING.into() => {
                // 尝试将 Latin-1 转换为 UTF-8
                let latin1_string: String = property
                    .value
                    .iter()
                    .map(|&b| b as char) // Latin-1 直接映射到 Unicode
                    .collect();
                *text = self.truncate_text(latin1_string);
                return true;
            }

            // COMPOUND_TEXT 类型 (需要特殊处理)
            (type_, 8) if type_ == self.atoms.COMPOUND_TEXT => {
                // 对于 COMPOUND_TEXT，我们尝试简单的 UTF-8 解析
                // 如果失败，回退到 Latin-1
                match String::from_utf8(property.value.clone()) {
                    Ok(utf8_string) => {
                        *text = self.truncate_text(utf8_string);
                        return true;
                    }
                    Err(_) => {
                        // 回退到 Latin-1
                        let latin1_string: String =
                            property.value.iter().map(|&b| b as char).collect();
                        *text = self.truncate_text(latin1_string);
                        return true;
                    }
                }
            }

            // 其他类型，尝试作为原始字节处理
            (_, 8) => {
                match String::from_utf8(property.value.clone()) {
                    Ok(utf8_string) => {
                        *text = self.truncate_text(utf8_string);
                        return true;
                    }
                    Err(_) => {
                        // 回退到 Latin-1
                        let latin1_string: String =
                            property.value.iter().map(|&b| b as char).collect();
                        *text = self.truncate_text(latin1_string);
                        return true;
                    }
                }
            }

            // 非 8 位格式
            _ => {
                debug!(
                    "[gettextprop] Unsupported property format: {}",
                    property.format
                );
                return false;
            }
        }
    }

    fn truncate_text(&self, input: String) -> String {
        let mut char_count = 0;
        let mut byte_truncate_at = input.len();

        for (idx, _) in input.char_indices() {
            if char_count >= self.stext_max_len {
                byte_truncate_at = idx;
                break;
            }
            char_count += 1;
        }

        let mut result = input;
        result.truncate(byte_truncate_at);
        result
    }

    /// 获取窗口的 transient_for 窗口，如果存在且有效
    pub fn get_transient_for(&self, window: Window) -> Option<u32> {
        let cookie = self
            .x11rb_conn
            .get_property(
                false,
                window,
                self.atoms.WM_TRANSIENT_FOR,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .ok()?;
        let reply = cookie.reply().ok()?;
        let mut values = if let Some(values) = reply.value32() {
            values
        } else {
            return None;
        };
        values.next().map(|w| w as u32).filter(|&w| w != 0)
    }

    pub fn propertynotify(
        &mut self,
        e: &PropertyNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[propertynotify]");
        // 处理根窗口属性变更
        if e.window == self.x11rb_root && e.atom == AtomEnum::WM_NAME.into() {
            // 根窗口名称变更，通常不需要处理
            debug!("Root window name property changed");
            return Ok(());
        }
        // 忽略属性删除事件
        if e.state == Property::DELETE {
            debug!("Ignoring property delete event for window {}", e.window);
            return Ok(());
        }
        // 处理客户端窗口属性变更
        if let Some(client_rc) = self.wintoclient(e.window) {
            self.handle_client_property_change(&client_rc, e)?;
        } else {
            debug!("Property change for unmanaged window: {}", e.window);
        }

        Ok(())
    }

    fn handle_client_property_change(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        e: &PropertyNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match e.atom {
            atom if atom == self.atoms.WM_TRANSIENT_FOR => {
                self.handle_transient_for_change(client_rc)?;
            }
            atom if atom == AtomEnum::WM_NORMAL_HINTS.into() => {
                self.handle_normal_hints_change(client_rc)?;
            }
            atom if atom == AtomEnum::WM_HINTS.into() => {
                self.handle_wm_hints_change(client_rc)?;
            }
            atom if atom == AtomEnum::WM_NAME.into() || atom == self.atoms._NET_WM_NAME => {
                self.handle_title_change(client_rc)?;
            }
            atom if atom == self.atoms._NET_WM_WINDOW_TYPE => {
                self.handle_window_type_change(client_rc)?;
            }
            _ => {
                debug!("Unhandled property change: atom {}", e.atom);
            }
        }

        Ok(())
    }

    fn handle_transient_for_change(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut client = client_rc.borrow_mut();
        if !client.is_floating {
            // 获取transient_for属性
            let transient_for = self.get_transient_for_hint(client.win)?;
            if let Some(parent_window) = transient_for {
                // 检查父窗口是否是我们管理的客户端
                if self.wintoclient(parent_window).is_some() {
                    client.is_floating = true;
                    debug!(
                        "Window {} became floating due to transient_for: {}",
                        client.win, parent_window
                    );
                    // 重新排列布局
                    let monitor = client.mon.clone();
                    drop(client); // 释放借用
                    self.arrange(monitor);
                }
            }
        }
        Ok(())
    }

    fn handle_normal_hints_change(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut client = client_rc.borrow_mut();
        client.hints_valid = false;
        debug!(
            "Normal hints changed for window {}, invalidating cache",
            client.win
        );
        Ok(())
    }

    fn handle_wm_hints_change(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.updatewmhints(client_rc);
        // WM_HINTS 改变可能影响紧急状态，需要重绘状态栏
        self.mark_bar_update_needed(None);
        debug!("WM hints updated for window {}", client_rc.borrow().win);
        Ok(())
    }

    fn handle_title_change(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.updatetitle(&mut client_rc.borrow_mut());
        // 检查是否需要更新状态栏
        let should_update_bar = {
            let client = client_rc.borrow();
            if let Some(ref monitor) = client.mon {
                let monitor_borrow = monitor.borrow();
                Self::are_equal_rc(&monitor_borrow.sel, &Some(client_rc.clone()))
            } else {
                false
            }
        };
        if should_update_bar {
            let monitor_id = client_rc.borrow().mon.as_ref().unwrap().borrow().num;
            self.mark_bar_update_needed(Some(monitor_id));

            debug!(
                "Title updated for selected window {}, updating status bar",
                client_rc.borrow().win
            );
        }
        Ok(())
    }

    fn handle_window_type_change(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.updatewindowtype(client_rc);
        debug!("Window type updated for window {}", client_rc.borrow().win);
        Ok(())
    }

    pub fn movemouse(&mut self, _arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[movemouse]");
        // 1. 获取当前选中的客户端
        let client_rc = match self.get_selected_client() {
            Some(client) => client,
            None => {
                debug!("No selected client for move");
                return Ok(());
            }
        };

        // 2. 全屏检查
        if client_rc.borrow().is_fullscreen {
            debug!("Cannot move fullscreen window");
            return Ok(());
        }

        // 3. 准备工作
        self.restack(self.sel_mon.clone())?;

        // 保存窗口开始移动时的信息
        let (original_x, original_y, window_id) = {
            let client = client_rc.borrow();
            (client.x, client.y, client.win)
        };

        // 4. 抓取鼠标指针
        let cursor = self
            .cursor_manager
            .get_cursor(&self.x11rb_conn, crate::xcb_util::StandardCursor::Sizing)?;

        let grab_reply = grab_pointer(
            &self.x11rb_conn,
            false,           // owner_events
            self.x11rb_root, // grab_window
            EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::POINTER_MOTION
                | EventMask::EXPOSURE, // event_mask
            GrabMode::ASYNC, // pointer_mode
            GrabMode::ASYNC, // keyboard_mode
            0u32,            // confine_to
            cursor,          // cursor
            0u32,            // time
        )?;

        let grab_reply = grab_reply.reply()?;
        if grab_reply.status != GrabStatus::SUCCESS {
            debug!("Failed to grab pointer for move");
            return Ok(());
        }

        // 5. 获取鼠标初始位置
        let query_reply = query_pointer(&self.x11rb_conn, self.x11rb_root)?.reply()?;
        let (initial_mouse_x, initial_mouse_y) = (query_reply.root_x, query_reply.root_y);
        info!(
            "[movemouse] initial mouse (root): x={}, y={}",
            initial_mouse_x, initial_mouse_y
        );

        // 6. 进入移动循环
        let result = self.move_loop(
            &client_rc,
            original_x,
            original_y,
            initial_mouse_x as u16,
            initial_mouse_y as u16,
        );

        // 7. 清理工作
        self.cleanup_move(window_id, &client_rc)?;
        result
    }

    fn move_loop(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        original_x: i32,
        original_y: i32,
        initial_mouse_x: u16,
        initial_mouse_y: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut last_motion_time = 0u32;

        loop {
            let event = self.x11rb_conn.wait_for_event()?;

            match event {
                Event::ConfigureRequest(e) => {
                    self.configurerequest(&e)?;
                }
                Event::Expose(e) => {
                    self.expose(&e)?;
                }
                Event::MapRequest(e) => {
                    self.maprequest(&e)?;
                }
                Event::MotionNotify(e) => {
                    // 节流处理
                    if e.time.wrapping_sub(last_motion_time) <= 16 {
                        // ~60 FPS
                        continue;
                    }
                    last_motion_time = e.time;

                    self.handle_move_motion(
                        client_rc,
                        &e,
                        original_x,
                        original_y,
                        initial_mouse_x,
                        initial_mouse_y,
                    )?;
                }
                Event::ButtonRelease(_) => {
                    debug!("Button released, ending move");
                    break;
                }
                _ => {
                    // 忽略其他事件
                }
            }
        }

        Ok(())
    }

    fn handle_move_motion(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        e: &MotionNotifyEvent,
        original_x: i32,
        original_y: i32,
        initial_mouse_x: u16,
        initial_mouse_y: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 计算新的位置
        let current_mouse_x = e.root_x;
        let current_mouse_y = e.root_y;
        let mut new_x = original_x + (current_mouse_x as i32 - initial_mouse_x as i32);
        let mut new_y = original_y + (current_mouse_y as i32 - initial_mouse_y as i32);

        // 获取显示器工作区边界
        let (mon_wx, mon_wy, mon_ww, mon_wh) = {
            let selmon_borrow = self.sel_mon.as_ref().unwrap().borrow();
            (
                selmon_borrow.w_x,
                selmon_borrow.w_y,
                selmon_borrow.w_w,
                selmon_borrow.w_h,
            )
        };

        // 应用边缘吸附
        self.apply_edge_snapping(
            client_rc, &mut new_x, &mut new_y, mon_wx, mon_wy, mon_ww, mon_wh,
        )?;

        // 检查是否需要切换到浮动模式
        self.check_and_toggle_floating_for_move(client_rc, new_x, new_y)?;

        // 如果是浮动窗口或浮动布局，执行移动
        let should_move = {
            let client = client_rc.borrow();
            let monitor = client.mon.as_ref().unwrap().borrow();
            client.is_floating || !monitor.lt[monitor.sel_lt].is_tile()
        };

        if should_move {
            let (window_w, window_h) = {
                let client = client_rc.borrow();
                (client.w, client.h)
            };
            self.resize(client_rc, new_x, new_y, window_w, window_h, true);
        }

        Ok(())
    }

    fn apply_edge_snapping(
        &self,
        client_rc: &Rc<RefCell<Client>>,
        new_x: &mut i32,
        new_y: &mut i32,
        mon_wx: i32,
        mon_wy: i32,
        mon_ww: i32,
        mon_wh: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_total_width = { client_rc.borrow().width() };
        let client_total_height = { client_rc.borrow().height() };
        let snap_distance = CONFIG.snap() as i32;

        // 吸附到左边缘
        if (mon_wx - *new_x).abs() < snap_distance {
            *new_x = mon_wx;
        }
        // 吸附到右边缘
        else if ((mon_wx + mon_ww) - (*new_x + client_total_width)).abs() < snap_distance {
            *new_x = mon_wx + mon_ww - client_total_width;
        }

        // 吸附到上边缘
        if (mon_wy - *new_y).abs() < snap_distance {
            *new_y = mon_wy;
        }
        // 吸附到下边缘
        else if ((mon_wy + mon_wh) - (*new_y + client_total_height)).abs() < snap_distance {
            *new_y = mon_wy + mon_wh - client_total_height;
        }

        Ok(())
    }

    fn check_and_toggle_floating_for_move(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        new_x: i32,
        new_y: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (is_floating, current_x, current_y) = {
            let client = client_rc.borrow();
            (client.is_floating, client.x, client.y)
        };

        let current_layout_is_tile = {
            let selmon_borrow = self.sel_mon.as_ref().unwrap().borrow();
            selmon_borrow.lt[selmon_borrow.sel_lt].is_tile()
        };

        // 如果窗口不是浮动的且当前是平铺布局，并且移动距离超过阈值
        if !is_floating
            && current_layout_is_tile
            && ((new_x - current_x).abs() > CONFIG.snap() as i32
                || (new_y - current_y).abs() > CONFIG.snap() as i32)
        {
            self.togglefloating(&Arg::I(0))?;
        }

        Ok(())
    }

    fn cleanup_move(
        &mut self,
        _window_id: Window,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 释放鼠标抓取
        ungrab_pointer(&self.x11rb_conn, 0u32)?;
        self.x11rb_conn.flush()?;

        // 检查窗口移动后是否跨越了显示器边界
        let (final_x, final_y, final_w, final_h) = {
            let c_borrow = client_rc.borrow();
            (c_borrow.x, c_borrow.y, c_borrow.w, c_borrow.h)
        };

        let target_monitor_opt = self.recttomon(final_x, final_y, final_w, final_h);

        if target_monitor_opt.is_some()
            && !Rc::ptr_eq(
                target_monitor_opt.as_ref().unwrap(),
                self.sel_mon.as_ref().unwrap(),
            )
        {
            self.sendmon(Some(client_rc.clone()), target_monitor_opt.clone());
            self.sel_mon = target_monitor_opt;
            self.focus(None)?;
        }

        Ok(())
    }

    pub fn resizemouse(&mut self, _arg: &Arg) -> Result<(), Box<dyn std::error::Error>> {
        info!("[resizemouse]");

        // 1. 获取当前选中的客户端
        let client_rc = match self.get_selected_client() {
            Some(client) => client,
            None => {
                debug!("No selected client for resize");
                return Ok(());
            }
        };

        // 2. 全屏检查
        if client_rc.borrow().is_fullscreen {
            debug!("Cannot resize fullscreen window");
            return Ok(());
        }

        // 3. 准备工作
        self.restack(self.sel_mon.clone())?;

        // 保存窗口开始调整大小时的信息
        let (original_x, original_y, border_width, window_id, current_w, current_h) = {
            let client = client_rc.borrow();
            (
                client.x,
                client.y,
                client.border_w,
                client.win,
                client.w,
                client.h,
            )
        };

        // 4. 抓取鼠标指针
        let cursor = self
            .cursor_manager
            .get_cursor(&self.x11rb_conn, crate::xcb_util::StandardCursor::Sizing)
            .unwrap();
        let grab_reply = self
            .x11rb_conn
            .grab_pointer(
                false,
                self.x11rb_root,
                EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::EXPOSURE,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                cursor,
                0u32,
            )?
            .reply()?;

        if grab_reply.status != GrabStatus::SUCCESS {
            debug!("Failed to grab pointer for resize");
            return Ok(());
        }

        // 5. 将鼠标移动到窗口右下角
        self.x11rb_conn.warp_pointer(
            0u32,
            window_id,
            0,
            0,
            0,
            0,
            (current_w + border_width - 1) as i16,
            (current_h + border_width - 1) as i16,
        )?;

        // 6. 进入调整大小循环
        let result = self.resize_loop(&client_rc, original_x, original_y, border_width);

        // 7. 清理工作
        self.cleanup_resize(window_id, border_width)?;

        result
    }

    fn get_selected_client(&self) -> Option<Rc<RefCell<Client>>> {
        self.sel_mon.as_ref()?.borrow().sel.clone()
    }

    fn resize_loop(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        original_x: i32,
        original_y: i32,
        border_width: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut last_motion_time = 0u32;

        loop {
            let event = self.x11rb_conn.wait_for_event()?;

            match event {
                Event::ConfigureRequest(e) => {
                    self.configurerequest(&e)?;
                }
                Event::Expose(e) => {
                    self.expose(&e)?;
                }
                Event::MapRequest(e) => {
                    self.maprequest(&e)?;
                }
                Event::MotionNotify(e) => {
                    // 节流处理
                    if e.time.wrapping_sub(last_motion_time) <= 16 {
                        // ~60 FPS
                        continue;
                    }
                    last_motion_time = e.time;

                    self.handle_resize_motion(client_rc, &e, original_x, original_y, border_width)?;
                }
                Event::ButtonRelease(_) => {
                    debug!("Button released, ending resize");
                    break;
                }
                _ => {
                    // 忽略其他事件
                }
            }
        }

        Ok(())
    }

    fn handle_resize_motion(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        e: &MotionNotifyEvent,
        original_x: i32,
        original_y: i32,
        border_width: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 计算新的尺寸
        let new_width =
            ((e.root_x as i32 - original_x).max(1 + 2 * border_width) - 2 * border_width).max(1);
        let new_height =
            ((e.root_y as i32 - original_y).max(1 + 2 * border_width) - 2 * border_width).max(1);

        // 检查是否需要切换到浮动模式
        self.check_and_toggle_floating(client_rc, new_width, new_height)?;

        // 如果是浮动窗口或浮动布局，执行调整大小
        let should_resize = {
            let client = client_rc.borrow();
            let monitor = client.mon.as_ref().unwrap().borrow();
            client.is_floating || !monitor.lt[monitor.sel_lt].is_tile()
        };

        if should_resize {
            self.resize(
                client_rc, original_x, original_y, new_width, new_height, true,
            );
        }

        Ok(())
    }

    fn check_and_toggle_floating(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        new_width: i32,
        new_height: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (is_floating, current_w, current_h, is_tile_layout) = {
            let client = client_rc.borrow();
            let monitor = client.mon.as_ref().unwrap().borrow();
            (
                client.is_floating,
                client.w,
                client.h,
                monitor.lt[monitor.sel_lt].is_tile(),
            )
        };

        if !is_floating && is_tile_layout {
            let snap_threshold = CONFIG.snap() as i32;
            if (new_width - current_w).abs() > snap_threshold
                || (new_height - current_h).abs() > snap_threshold
            {
                debug!("Toggling to floating mode due to size change");
                let _ = self.togglefloating(&Arg::Ui(0));
            }
        }

        Ok(())
    }

    fn cleanup_resize(
        &mut self,
        window_id: Window,
        border_width: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 将鼠标定位到最终位置
        let (final_w, final_h) = {
            let client = self.get_selected_client();
            if let Some(ref client_rc) = client {
                let c = client_rc.borrow();
                (c.w, c.h)
            } else {
                return Ok(());
            }
        };

        self.x11rb_conn.warp_pointer(
            0u32,
            window_id,
            0,
            0,
            0,
            0,
            (final_w + border_width - 1) as i16,
            (final_h + border_width - 1) as i16,
        )?;

        // 释放鼠标抓取
        self.x11rb_conn.ungrab_pointer(0u32)?;

        // 清理事件
        self.flush_enter_events();

        // 检查是否需要移动到不同的显示器
        self.check_monitor_change_after_resize()?;

        Ok(())
    }

    fn check_monitor_change_after_resize(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let client_rc = match self.get_selected_client() {
            Some(client) => client,
            None => return Ok(()),
        };

        let (x, y, w, h) = {
            let client = client_rc.borrow();
            (client.x, client.y, client.w, client.h)
        };

        let target_monitor = self.recttomon(x, y, w, h);

        if let Some(ref target_mon) = target_monitor {
            if !Rc::ptr_eq(target_mon, self.sel_mon.as_ref().unwrap()) {
                debug!("Moving client to different monitor after resize");
                self.sendmon(Some(client_rc), target_monitor.clone());
                self.sel_mon = target_monitor;
                self.focus(None)?;
            }
        }

        Ok(())
    }

    /// 使用 x11rb 更新 Num_Lock 键的修饰符掩码
    // (TODO) fix bug here
    pub fn update_num_lock_mask(&mut self) -> Result<(), ReplyOrIdError> {
        // 1. 初始化 numlockmask 为 0
        self.numlock_mask = 0;

        // 2. 找到 Num_Lock 键对应的 KeyCode
        let num_lock_keysym = XK_Num_Lock;
        let num_lock_keycode =
            match Self::find_keycode_for_keysym(&self.x11rb_conn, num_lock_keysym)? {
                Some(kc) => kc,
                None => {
                    // 如果键盘上没有 Num_Lock 键，直接返回
                    error!("[updatenumlockmask] warning: Could not find a keycode for Num_Lock");
                    return Ok(());
                }
            };

        // 3. 获取当前的修饰键映射
        // conn.get_modifier_mapping() 发送请求，.reply()? 等待并解析回复。
        // 这个调用是异步的，但在 .reply() 这里会阻塞等待结果。
        let modmap = self.x11rb_conn.get_modifier_mapping()?.reply()?;

        // 4. 遍历修饰键映射表 (安全且符合 Rust 习惯的方式)
        // `modmap.keycodes` 是一个 Vec<Keycode>，我们可以用安全的迭代器来处理。
        // info!(
        //     "[updatenumlockmask] keycodes {:?}, length {:?}, sequence {:?}, num_lock_keycode: {}",
        //     modmap.keycodes, modmap.length, modmap.sequence, num_lock_keycode
        // );
        for (index, &keycode) in modmap.keycodes.iter().enumerate() {
            // `keycode` 是映射到某个修饰符的物理键码
            if keycode == num_lock_keycode {
                // 计算当前 keycode 属于哪个修饰符组 (0-7)
                let modifier_index = index as u32 / modmap.length;
                // info!(
                //     "[updatenumlockmask] index: {}, modifier_index: {}",
                //     index, modifier_index
                // );

                // 计算掩码 (1 << modifier_index)，并转换为 x11rb 的 ModMask 类型
                // ModMask 在 xproto 中是 u16 的类型别名
                self.numlock_mask = 1 << modifier_index;

                break;
            }
        }
        self.numlock_mask = 1 << 4;
        Ok(())
    }

    /// 一个辅助函数，用于根据 Keysym (如 XK_Num_Lock) 查找其对应的 Keycode。
    fn find_keycode_for_keysym(
        conn: &impl Connection,
        target_keysym: Keysym,
    ) -> Result<Option<KeyCode>, ReplyOrIdError> {
        // 获取整个键盘的映射信息
        let setup = conn.setup();
        let mapping = conn
            .get_keyboard_mapping(
                setup.min_keycode,
                (setup.max_keycode - setup.min_keycode) + 1,
            )?
            .reply()?;
        // info!(
        //     "[find_keycode_for_keysym] setup: {}, {}, mapping: {}, {}",
        //     setup.min_keycode, setup.max_keycode, mapping.sequence, mapping.keysyms_per_keycode
        // );

        // 遍历每个 Keycode
        for (keycode_offset, keysyms_for_keycode) in mapping.keysyms.iter().enumerate() {
            // info!(
            //     "[find_keycode_for_keysym] keycode_offset: {}, keysyms_for_keycode: {}",
            //     keycode_offset, keysyms_for_keycode
            // );
            // `keysyms_for_keycode` 是一个 Vec<KEYSYM>，包含了该键码在不同状态下（如按下、Shift+按下）的符号
            if *keysyms_for_keycode == target_keysym {
                let keycode = (setup.min_keycode + keycode_offset as u8) as KeyCode;
                return Ok(Some(keycode));
            }
        }

        Ok(None)
    }

    pub fn setclienttagprop(
        &mut self,
        c: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_mut = c.borrow();
        let data: [u32; 2] = [
            client_mut.tags,
            client_mut.mon.as_ref().unwrap().borrow().num as u32,
        ];
        use x11rb::wrapper::ConnectionExt;
        self.x11rb_conn.change_property32(
            PropMode::REPLACE,
            client_mut.win,
            self.atoms._NET_CLIENT_INFO,
            AtomEnum::CARDINAL,
            &data,
        )?;
        Ok(())
    }

    pub fn sendevent(&mut self, client_mut: &mut Client, proto: Atom) -> bool {
        info!(
            "[sendevent] Sending protocol {:?} to window 0x{:x}",
            proto, client_mut.win
        );
        // 1. 获取 WM_PROTOCOLS 属性
        let cookie = self
            .x11rb_conn
            .get_property(
                false,
                client_mut.win,
                self.atoms.WM_PROTOCOLS, // Atom for WM_PROTOCOLS
                AtomEnum::ATOM,
                0,
                1024, // 足够大的长度
            )
            .unwrap();
        let reply = match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => {
                warn!(
                    "[sendevent] Failed to get WM_PROTOCOLS for window 0x{:x}",
                    client_mut.win
                );
                return false;
            }
        };
        // 2. 检查属性值中是否包含目标 proto
        let protocols: Vec<Atom> = reply.value.as_slice().iter().map(|v| (*v).into()).collect();
        let exists = protocols.contains(&proto);
        if !exists {
            info!(
                "[sendevent] Protocol {:?} not supported by window 0x{:x}",
                proto, client_mut.win
            );
            return false;
        }
        // 3. 构造 ClientMessageEvent
        let event = ClientMessageEvent::new(
            32,                      // format: 32 位
            client_mut.win,          // window
            self.atoms.WM_PROTOCOLS, // message_type
            [proto, 0, 0, 0, 0],     // data.l[0] = protocol atom
        );
        // 4. 发送事件
        let buffer = event.serialize();
        let result = self.x11rb_conn.send_event(
            false,
            client_mut.win,
            EventMask::NO_EVENT, // 不需要事件掩码（由接收方决定）
            buffer,
        );
        if let Err(e) = result {
            warn!("[sendevent] Failed to send event: {}", e);
            return false;
        }
        // 5. flush（可选）
        if let Err(e) = self.x11rb_conn.flush() {
            warn!("[sendevent] Failed to flush connection: {}", e);
            return false;
        }
        info!(
            "[sendevent] Successfully sent protocol {:?} to window 0x{:x}",
            proto, client_mut.win
        );
        true
    }

    pub fn enternotify(&mut self, e: &EnterNotifyEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[enternotify]");
        // 过滤不需要处理的事件
        if (e.mode != NotifyMode::NORMAL || e.detail == NotifyDetail::INFERIOR)
            && e.event != self.x11rb_root
        {
            return Ok(());
        }
        // 检查是否进入状态栏
        if self.handle_statusbar_enter(e)? {
            return Ok(());
        }
        // 常规的 enternotify 处理
        self.handle_regular_enter(e)?;
        Ok(())
    }

    fn handle_statusbar_enter(
        &mut self,
        e: &EnterNotifyEvent,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if let Some(&monitor_id) = self.status_bar_windows.get(&e.event) {
            // 状态栏不改变焦点，但可能需要切换显示器
            if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
                if !Rc::ptr_eq(&monitor, self.sel_mon.as_ref().unwrap()) {
                    let sel = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
                    self.unfocus(sel, true)?;
                    self.sel_mon = Some(monitor);
                    self.focus(None)?;
                }
            }
            return Ok(true); // 已处理状态栏事件
        }
        Ok(false) // 不是状态栏事件
    }

    fn handle_regular_enter(
        &mut self,
        e: &EnterNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 确定事件相关的客户端和显示器
        let client_rc_opt = self.wintoclient(e.event);
        let monitor_rc_opt = if let Some(ref c_rc) = client_rc_opt {
            c_rc.borrow().mon.clone()
        } else {
            // 如果事件窗口不是已管理的客户端，尝试根据窗口ID确定显示器
            self.wintomon(e.event)
        };
        // 如果无法确定显示器，则不处理
        let current_event_monitor_rc = match monitor_rc_opt {
            Some(monitor) => monitor,
            None => return Ok(()),
        };
        // 处理显示器焦点切换
        let is_on_selected_monitor =
            Rc::ptr_eq(&current_event_monitor_rc, self.sel_mon.as_ref().unwrap());

        if !is_on_selected_monitor {
            self.switch_to_monitor(&current_event_monitor_rc)?;
        }
        // 处理客户端焦点切换
        if self.should_focus_client(&client_rc_opt, is_on_selected_monitor) {
            let _ = self.focus(client_rc_opt);
        }
        Ok(())
    }

    fn switch_to_monitor(
        &mut self,
        target_monitor: &Rc<RefCell<Monitor>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 获取旧选中显示器上的选中客户端
        let previously_selected_client_opt = {
            let selmon_borrow = self.sel_mon.as_ref().unwrap().borrow();
            selmon_borrow.sel.clone()
        };
        // 从旧显示器的选中客户端上移除焦点，并将X焦点设回根
        self.unfocus(previously_selected_client_opt, true)?;
        // 更新选中显示器为当前事件发生的显示器
        self.sel_mon = Some(target_monitor.clone());
        debug!("Switched to monitor {}", target_monitor.borrow().num);
        Ok(())
    }

    fn should_focus_client(
        &self,
        client_rc_opt: &Option<Rc<RefCell<Client>>>,
        is_on_selected_monitor: bool,
    ) -> bool {
        // 如果切换了显示器，需要重新聚焦
        if !is_on_selected_monitor {
            return true;
        }
        // 如果鼠标进入了根窗口（没有具体客户端），需要重新聚焦
        if client_rc_opt.is_none() {
            return true;
        }
        // 如果进入的客户端与当前选中客户端不同，需要重新聚焦
        let current_selected = &self.sel_mon.as_ref().unwrap().borrow().sel;
        !Self::are_equal_rc(client_rc_opt, current_selected)
    }

    pub fn expose(&mut self, e: &ExposeEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[expose]");
        // 只处理最后一个expose事件（count为0时）
        if e.count != 0 {
            return Ok(());
        }
        // 检查窗口所在的显示器并标记状态栏需要更新
        if let Some(monitor) = self.wintomon(e.window) {
            self.mark_bar_update_needed(Some(monitor.borrow().num));
        }

        Ok(())
    }

    pub fn focus(
        &mut self,
        mut c_opt: Option<Rc<RefCell<Client>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[focus]");

        // 如果传入的是状态栏客户端，忽略并寻找合适的替代
        if let Some(ref c) = c_opt {
            if self.status_bar_windows.contains_key(&c.borrow().win) {
                c_opt = None; // 忽略状态栏
            }
        }

        // 检查客户端是否可见，如果不可见则寻找可见的客户端
        let is_visible = match c_opt.clone() {
            Some(c_rc) => c_rc.borrow().isvisible(),
            None => false,
        };

        if !is_visible {
            c_opt = self.find_visible_client();
        }

        // 处理焦点切换
        self.handle_focus_change(&c_opt)?;

        // 设置新的焦点客户端
        if let Some(c_rc) = c_opt.clone() {
            self.set_client_focus(&c_rc)?;
        } else {
            self.set_root_focus()?;
        }

        // 更新选中监视器的状态
        self.update_monitor_selection(c_opt.clone());

        // 标记状态栏需要更新
        self.mark_bar_update_needed(None);

        Ok(())
    }

    fn find_visible_client(&mut self) -> Option<Rc<RefCell<Client>>> {
        if let Some(ref sel_mon_opt) = self.sel_mon {
            let mut c_opt = sel_mon_opt.borrow().stack.clone();
            while let Some(c_rc) = c_opt.clone() {
                if c_rc.borrow().isvisible() {
                    return Some(c_rc);
                }
                c_opt = c_rc.borrow().stack_next.clone();
            }
        }
        None
    }

    fn handle_focus_change(
        &mut self,
        new_focus: &Option<Rc<RefCell<Client>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let current_sel = { self.sel_mon.as_ref().unwrap().borrow().sel.clone() };
        if current_sel.is_some() && !Self::are_equal_rc(&current_sel, new_focus) {
            self.unfocus(current_sel, false)?;
        }
        Ok(())
    }

    fn set_client_focus(
        &mut self,
        c_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 检查客户端是否在当前选中的监视器上
        let client_monitor = c_rc.borrow().mon.clone();
        if let Some(ref client_mon) = client_monitor {
            if !Rc::ptr_eq(client_mon, self.sel_mon.as_ref().unwrap()) {
                self.sel_mon = Some(client_mon.clone());
            }
        }
        // 清除紧急状态
        if c_rc.borrow().is_urgent {
            self.seturgent(c_rc, false);
        }
        // 重新排列堆栈顺序
        self.detachstack(Some(c_rc.clone()));
        self.attachstack(Some(c_rc.clone()));
        // 抓取按钮事件
        self.grabbuttons(Some(c_rc.clone()), true)?;
        // 设置边框颜色为选中状态
        self.set_window_border_pixel(
            c_rc.borrow().win,
            self.theme_manager.get_scheme(SchemeType::Sel).border.pixel as u32,
        )?;

        // 设置焦点
        self.setfocus(c_rc)?;

        Ok(())
    }

    fn set_root_focus(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 将焦点设置到根窗口
        self.x11rb_conn.set_input_focus(
            InputFocus::POINTER_ROOT,
            self.x11rb_root,
            0u32, // CurrentTime equivalent
        )?;

        // 清除 _NET_ACTIVE_WINDOW 属性
        self.x11rb_conn
            .delete_property(self.x11rb_root, self.atoms._NET_ACTIVE_WINDOW)?;

        self.x11rb_conn.flush()?;
        Ok(())
    }

    fn update_monitor_selection(&mut self, c_opt: Option<Rc<RefCell<Client>>>) {
        if let Some(ref sel_mon_opt) = self.sel_mon {
            let mut sel_mon_mut = sel_mon_opt.borrow_mut();
            sel_mon_mut.sel = c_opt.clone();

            if let Some(ref pertag) = sel_mon_mut.pertag {
                let cur_tag = pertag.cur_tag;

                sel_mon_mut.pertag.as_mut().unwrap().sel[cur_tag] = c_opt;
            }
        }
    }

    pub fn setfocus(&mut self, c: &Rc<RefCell<Client>>) -> Result<(), Box<dyn std::error::Error>> {
        let mut c_mut = c.borrow_mut();

        if !c_mut.never_focus {
            self.x11rb_conn.set_input_focus(
                InputFocus::POINTER_ROOT,
                c_mut.win,
                0u32, // time
            )?;

            use x11rb::wrapper::ConnectionExt;
            self.x11rb_conn.change_property32(
                PropMode::REPLACE,
                self.x11rb_root,
                self.atoms._NET_ACTIVE_WINDOW,
                AtomEnum::WINDOW,
                &[c_mut.win],
            )?;
        }

        self.sendevent(&mut c_mut, self.atoms.WM_TAKE_FOCUS);
        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn unfocus(
        &mut self,
        c: Option<Rc<RefCell<Client>>>,
        setfocus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if c.is_none() {
            return Ok(());
        }

        let client_rc = c.unwrap();
        self.grabbuttons(Some(client_rc.clone()), false)?;

        self.set_window_border_pixel(
            client_rc.borrow().win,
            self.theme_manager.get_scheme(SchemeType::Norm).border.pixel as u32,
        )?;

        if setfocus {
            self.x11rb_conn
                .set_input_focus(InputFocus::POINTER_ROOT, self.x11rb_root, 0u32)?;

            self.x11rb_conn
                .delete_property(self.x11rb_root, self.atoms._NET_ACTIVE_WINDOW)?;
        }

        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn sendmon(&mut self, c: Option<Rc<RefCell<Client>>>, m: Option<Rc<RefCell<Monitor>>>) {
        // info!("[sendmon]");
        if Self::are_equal_rc(&c.as_ref().unwrap().borrow_mut().mon, &m) {
            return;
        }
        let _ = self.unfocus(c.clone(), true);
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
        let _ = self.setclienttagprop(c.as_ref().unwrap());
        let _ = self.focus(None);
        self.arrange(None);
    }

    pub fn setclientstate(
        &mut self,
        c: &Rc<RefCell<Client>>,
        state: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[setclientstate]");
        let data_to_set: [u32; 2] = [state as u32, 0]; // 0 代表 None (无图标窗口)
        let win = c.borrow().win;
        use x11rb::wrapper::ConnectionExt;
        self.x11rb_conn.change_property32(
            PropMode::REPLACE,
            win,
            self.atoms.WM_STATE,
            self.atoms.WM_STATE,
            &data_to_set,
        )?;
        Ok(())
    }

    pub fn keypress(&mut self, e: &KeyPressEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[keypress]");
        // 使用缓存的键盘映射转换keycode到keysym
        let keysym = self.get_keysym_from_keycode(e.detail)?;
        debug!(
            "[keypress] keycode: {}, keysym: 0x{:x}, raw_state: 0x{:x}, clean_state: 0x{:x}",
            e.detail,
            keysym,
            e.state.bits(),
            self.clean_mask(e.state.bits().into())
        );
        // 处理按键绑定
        if self.execute_key_binding(keysym, e.state)? {
            debug!("Key binding executed successfully");
        } else {
            debug!("No matching key binding found for keysym 0x{:x}", keysym);
        }
        Ok(())
    }

    fn get_keysym_from_keycode(&mut self, keycode: u8) -> Result<u32, Box<dyn std::error::Error>> {
        // 检查缓存
        if let Some(&keysym) = self.keycode_cache.get(&keycode) {
            return Ok(keysym);
        }
        // 查询键盘映射
        let keyboard_mapping = self.x11rb_conn.get_keyboard_mapping(keycode, 1)?.reply()?;
        let keysym = if !keyboard_mapping.keysyms.is_empty() {
            keyboard_mapping.keysyms[0]
        } else {
            0
        };
        // 缓存结果
        self.keycode_cache.insert(keycode, keysym);
        Ok(keysym)
    }

    fn execute_key_binding(
        &mut self,
        keysym: u32,
        state: KeyButMask,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let keys = CONFIG.get_keys();
        let clean_state = self.clean_mask(state.bits().into());
        for (i, key_config) in keys.iter().enumerate() {
            if self.is_key_match(key_config, keysym, clean_state.into()) {
                info!(
                    "[keypress] executing binding {}: keysym=0x{:x}, mod=0x{:x}, arg={:?}",
                    i, key_config.keysym, key_config.mod0, key_config.arg
                );
                if let Some(func) = key_config.func {
                    let _ = func(self, &key_config.arg);
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn is_key_match(&self, key_config: &Key, keysym: u32, clean_state: u32) -> bool {
        keysym == key_config.keysym as u32
            && self.clean_mask(key_config.mod0) == clean_state
            && key_config.func.is_some()
    }

    /// 清除键盘映射缓存（在键盘映射变更时调用）
    pub fn clear_keycode_cache(&mut self) {
        self.keycode_cache.clear();
        info!("Keycode cache cleared");
    }

    pub fn manage(&mut self, w: Window, geom: &GetGeometryReply) {
        // info!("[manage]"); // 日志
        // --- 1. 创建新的 Client 对象 ---
        let client_rc_opt: Option<Rc<RefCell<Client>>> = Some(Rc::new(RefCell::new(Client::new())));
        let client_rc = client_rc_opt.as_ref().unwrap();
        // --- 2. 初始化 Client 结构体的基本属性 ---
        {
            let mut client_mut = client_rc.borrow_mut();
            // 设置窗口 ID
            client_mut.win = w;
            // 从传入的 XWindowAttributes 中获取初始的几何信息和边框宽度
            client_mut.x = geom.x.into();
            client_mut.old_x = geom.x.into();
            client_mut.y = geom.y.into();
            client_mut.old_y = geom.y.into();
            client_mut.w = geom.width.into();
            client_mut.old_w = geom.width.into();
            client_mut.h = geom.height.into();
            client_mut.old_h = geom.height.into();
            client_mut.old_border_w = geom.border_width.into();
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
                self.manage_statusbar(client_rc);
                return; // 直接返回，不执行常规管理流程
            }
        }

        // 常规客户端管理流程
        let _ = self.manage_regular_client(client_rc);
    }

    fn setup_client_window(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = client_rc.borrow().win;
        info!("[setup_client_window] Setting up window {}", win);

        // 1. 设置边框宽度
        {
            let mut client_mut = client_rc.borrow_mut();
            client_mut.border_w = CONFIG.border_px() as i32;
            self.set_window_border_width(win, client_mut.border_w as u32)?;
        }

        // 2. 设置边框颜色为"正常"状态的颜色
        let border_color = self.theme_manager.get_scheme(SchemeType::Norm).border.pixel;
        self.set_window_border_pixel(win, border_color as u32)?;

        // 3. 发送 ConfigureNotify 事件给客户端
        {
            let mut client_mut = client_rc.borrow_mut();
            self.configure(&mut client_mut)?;
        }

        // 4. 设置窗口在屏幕外的临时位置（避免闪烁）
        {
            let client_borrow = client_rc.borrow();
            let offscreen_x = client_borrow.x + 2 * self.s_w; // 移到屏幕外

            let aux = ConfigureWindowAux::new()
                .x(offscreen_x)
                .y(client_borrow.y)
                .width(client_borrow.w as u32)
                .height(client_borrow.h as u32);

            self.x11rb_conn.configure_window(win, &aux)?;
        }

        // 5. 设置客户端的 WM_STATE 为 NormalState
        self.setclientstate(client_rc, NormalState as i64)?;

        // 6. 同步所有操作
        self.x11rb_conn.flush()?;

        info!("[setup_client_window] Window setup completed for {}", win);
        Ok(())
    }

    // 更新完整的客户端列表（在需要时调用）
    fn update_net_client_list(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;
        // 清空现有列表
        let _ = self
            .x11rb_conn
            .delete_property(self.x11rb_root, self.atoms._NET_CLIENT_LIST);

        // 重新构建列表
        let mut m = self.mons.clone();
        while let Some(ref m_opt) = m {
            let mut c = m_opt.borrow().clients.clone();
            while let Some(ref client_opt) = c {
                self.x11rb_conn.change_property32(
                    PropMode::APPEND,
                    self.x11rb_root,
                    self.atoms._NET_CLIENT_LIST,
                    AtomEnum::WINDOW,
                    &[client_opt.borrow().win],
                )?;
                let next = client_opt.borrow().next.clone();
                c = next;
            }
            let next = m_opt.borrow().next.clone();
            m = next;
        }

        info!("[update_net_client_list] Updated _NET_CLIENT_LIST");
        Ok(())
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
                let _ = self.unfocus(prev_sel_opt, false); // false: 不立即设置根窗口焦点
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
                let _ = self.focus(Some(client_rc.clone()));
                info!(
                    "[handle_new_client_focus] Focused new client: {}",
                    client_rc.borrow().name
                );
            } else {
                // 如果新窗口是 never_focus，重新评估焦点
                let _ = self.focus(None);
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
                    let _ = self.unfocus(old_sel, true);
                    self.sel_mon = Some(new_mon);
                    let _ = self.focus(Some(client_rc.clone()));
                    info!("[handle_new_client_focus] Switched to new window's monitor");
                }
            }
        }
    }

    // 分离出来的常规客户端管理
    fn manage_regular_client(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 处理 WM_TRANSIENT_FOR
        self.handle_transient_for(&client_rc)?;

        // 调整窗口位置
        self.adjust_client_position(&client_rc);

        // 设置窗口属性
        self.setup_client_window(&client_rc)?;

        // 更新各种提示
        self.updatewindowtype(&client_rc);
        self.updatesizehints(&client_rc)?;
        self.updatewmhints(&client_rc);

        // 添加到管理链表
        self.attach(Some(client_rc.clone()));
        self.attachstack(Some(client_rc.clone()));

        // 注册事件和抓取按钮
        self.register_client_events(&client_rc)?;

        // 更新客户端列表
        self.update_net_client_list()?;

        // 映射窗口
        self.map_client_window(&client_rc)?;

        // 处理焦点
        self.handle_new_client_focus(&client_rc);

        Ok(())
    }

    fn handle_transient_for(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = client_rc.borrow().win;

        // 使用 x11rb 获取 WM_TRANSIENT_FOR 属性
        match self.get_transient_for_hint(win) {
            Ok(Some(transient_for_win)) => {
                // 找到 transient_for 窗口对应的客户端
                if let Some(parent_client) = self.wintoclient(transient_for_win) {
                    let mut client_mut = client_rc.borrow_mut();
                    let parent_borrow = parent_client.borrow();
                    client_mut.mon = parent_borrow.mon.clone();
                    client_mut.tags = parent_borrow.tags;

                    info!(
                        "[handle_transient_for] Client {} is transient for {}",
                        win, transient_for_win
                    );
                } else {
                    // 父窗口不是我们管理的客户端
                    client_rc.borrow_mut().mon = self.sel_mon.clone();
                    self.applyrules(&client_rc);
                }
            }
            Ok(None) => {
                // 没有 WM_TRANSIENT_FOR 属性
                client_rc.borrow_mut().mon = self.sel_mon.clone();
                self.applyrules(&client_rc);
            }
            Err(e) => {
                warn!(
                    "[handle_transient_for] Failed to get transient_for hint: {:?}",
                    e
                );
                // 失败时使用默认行为
                client_rc.borrow_mut().mon = self.sel_mon.clone();
                self.applyrules(&client_rc);
            }
        }

        Ok(())
    }

    fn get_transient_for_hint(
        &self,
        window: Window,
    ) -> Result<Option<Window>, Box<dyn std::error::Error>> {
        let cookie = self.x11rb_conn.get_property(
            false,                       // delete
            window,                      // window
            self.atoms.WM_TRANSIENT_FOR, // property
            AtomEnum::WINDOW,            // type
            0,                           // long_offset
            1,                           // long_length
        )?;

        let reply = cookie.reply()?;

        if reply.format == 32 && reply.value.len() >= 4 {
            // 解析 32位的窗口ID
            let mut values = reply.value32().unwrap();
            if let Some(transient_for) = values.next() {
                if transient_for != 0 && transient_for != window {
                    return Ok(Some(transient_for));
                }
            }
        }

        Ok(None)
    }

    fn map_client_window(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = client_rc.borrow().win;

        match self.x11rb_conn.map_window(win) {
            Ok(cookie) => {
                // 检查映射是否成功
                if let Err(e) = cookie.check() {
                    error!("[map_client_window] Failed to map window {}: {:?}", win, e);
                    return Err(e.into());
                }
            }
            Err(e) => {
                error!(
                    "[map_client_window] Failed to send map_window request for {}: {:?}",
                    win, e
                );
                return Err(e.into());
            }
        }

        // 确保请求被发送
        self.x11rb_conn.flush()?;

        info!("[map_client_window] Successfully mapped window {}", win);
        Ok(())
    }

    fn manage_statusbar(&mut self, client_rc: &Rc<RefCell<Client>>) {
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
            let _ = self.setup_statusbar_window(&mut client_mut);
        }

        // 注册状态栏到管理映射中
        self.status_bar_clients
            .insert(monitor_id, client_rc.clone());
        self.status_bar_windows
            .insert(client_rc.borrow().win, monitor_id);

        // 映射状态栏窗口 - 使用 x11rb 替代 XMapWindow
        let win = client_rc.borrow().win;
        if let Err(e) = self.x11rb_conn.map_window(win) {
            error!(
                "[manage_statusbar] Failed to map statusbar window {}: {:?}",
                win, e
            );
        } else {
            debug!("[manage_statusbar] Mapped statusbar window {}", win);
        }

        info!(
            "[manage_statusbar] Successfully managed statusbar on monitor {}",
            monitor_id
        );
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
    fn setup_statusbar_window(
        &mut self,
        client_mut: &mut Client,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = client_mut.win;
        info!(
            "[setup_statusbar_window] Setting up statusbar window {}",
            win
        );
        // 设置状态栏窗口的事件监听
        let aux = ChangeWindowAttributesAux::new().event_mask(
            EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE | EventMask::ENTER_WINDOW,
        );
        self.x11rb_conn.change_window_attributes(win, &aux)?;
        // 发送配置通知
        self.configure(client_mut)?;
        // 同步操作
        self.x11rb_conn.flush()?;
        info!(
            "[setup_statusbar_window] Statusbar window setup completed for {}",
            win
        );
        Ok(())
    }

    pub fn client_y_offset(&mut self, m: &Monitor) -> i32 {
        let monitor_id = m.num;

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
                    let _ = self.configure(&mut statusbar_mut);
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
        // 检查窗口是否存在
        if self
            .x11rb_conn
            .get_geometry(client_mut.win)
            .and_then(|c| Ok(c.reply()))
            .is_err()
        {
            return Err(format!("Invalid window: 0x{:x}", client_mut.win));
        }
        let mut data = Vec::with_capacity(res_name.len() + res_class.len() + 2);
        data.extend_from_slice(res_name.as_bytes());
        data.push(0);
        data.extend_from_slice(res_class.as_bytes());
        data.push(0);
        use x11rb::wrapper::ConnectionExt;
        self.x11rb_conn
            .change_property8(
                PropMode::REPLACE,
                client_mut.win,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                &data,
            )
            .map_err(|e| format!("X11 error: {}", e))?;
        self.x11rb_conn
            .flush()
            .map_err(|e| format!("Flush error: {}", e))?;
        client_mut.class = res_class.to_string();
        client_mut.instance = res_name.to_string();
        info!(
            "[set_class_info] Set class='{}', instance='{}' for window 0x{:x}",
            res_class, res_name, client_mut.win
        );
        self.verify_class_info_set(client_mut, res_class, res_name);
        Ok(())
    }

    // 验证设置是否成功的辅助函数
    #[allow(dead_code)]
    fn verify_class_info_set(
        &mut self,
        client: &Client,
        expected_class: &str,
        expected_instance: &str,
    ) {
        if let Some((inst, cls)) = Self::get_wm_class(&self.x11rb_conn, client.win as u32) {
            if cls == expected_class && inst == expected_instance {
                info!("[verify_class_info_set] Verification successful");
            } else {
                warn!(
                    "[verify_class_info_set] Verification failed. Expected: class='{}', instance='{}'. Actual: class='{}', instance='{}'",
                    expected_class, expected_instance, cls, inst
                );
            }
        } else {
            warn!("[verify_class_info_set] Failed to get class hint for verification");
        }
    }

    // 更新窗口类信息
    fn update_class_info(&mut self, client_mut: &mut Client) {
        if let Some((inst, cls)) = Self::get_wm_class(&self.x11rb_conn, client_mut.win as u32) {
            client_mut.instance = inst;
            client_mut.class = cls;
        }
    }

    pub fn mappingnotify(
        &mut self,
        e: &MappingNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[mappingnotify]");
        match e.request {
            Mapping::KEYBOARD => {
                self.grabkeys()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn get_and_query_window_geom<C: Connection>(
        conn: &C,
        win: Window,
    ) -> Result<GetGeometryReply, ReplyError> {
        let geom = conn.get_geometry(win)?;
        let tree = conn.query_tree(win)?;

        let mut geom = geom.reply()?;
        let tree = tree.reply()?;

        let trans = conn
            .translate_coordinates(win, tree.parent, geom.x, geom.y)?
            .reply()?;

        // the translated coordinates are in trans.dst_x and trans.dst_y
        geom.x = trans.dst_x;
        geom.y = trans.dst_y;
        Ok(geom)
    }

    pub fn get_window_attributes(
        &self,
        window: Window,
    ) -> Result<GetWindowAttributesReply, ReplyError> {
        let geom = self.x11rb_conn.get_window_attributes(window)?.reply()?;
        return Ok(geom);
    }

    pub fn maprequest(&mut self, e: &MapRequestEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[maprequest]");
        // 获取窗口属性
        let window_attr = self.x11rb_conn.get_window_attributes(e.window)?.reply()?;
        // 忽略设置了override_redirect的窗口
        if window_attr.override_redirect {
            debug!(
                "Ignoring map request for override_redirect window: {}",
                e.window
            );
            return Ok(());
        }
        // 检查窗口是否已经被管理
        if self.wintoclient(e.window).is_none() {
            // 获取窗口几何信息并开始管理
            let geom = Self::get_and_query_window_geom(&self.x11rb_conn, e.window)?;
            self.manage(e.window, &geom);
        } else {
            debug!(
                "Window {} is already managed, ignoring map request",
                e.window
            );
        }
        Ok(())
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

    pub fn motionnotify(
        &mut self,
        e: &MotionNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[motionnotify]");
        // 只处理根窗口上的鼠标移动事件
        if e.event != self.x11rb_root {
            return Ok(());
        }
        // 根据鼠标位置确定当前显示器
        let m = self.recttomon(e.root_x as i32, e.root_y as i32, 1, 1);
        // 检查是否切换了显示器
        if !Self::are_equal_rc(&m, &self.motion_mon) {
            self.handle_monitor_switch(&m)?;
        }
        // 更新motion_mon
        self.motion_mon = m;
        Ok(())
    }

    fn handle_monitor_switch(
        &mut self,
        new_monitor: &Option<Rc<RefCell<Monitor>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 从当前选中显示器的选中客户端上移除焦点
        let selmon_sel = self.sel_mon.as_ref().unwrap().borrow().sel.clone();
        self.unfocus(selmon_sel, true)?;
        // 切换到新显示器
        self.sel_mon = new_monitor.clone();
        // 在新显示器上设置焦点
        self.focus(None)?;
        if let Some(ref monitor) = new_monitor {
            debug!(
                "Switched to monitor {} via mouse motion",
                monitor.borrow().num
            );
        }
        Ok(())
    }

    pub fn unmanage(
        &mut self,
        c: Option<Rc<RefCell<Client>>>,
        destroyed: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_rc = match c {
            Some(c) => c,
            None => return Ok(()),
        };
        let win = client_rc.borrow().win;
        // 检查是否是状态栏
        if let Some(&monitor_id) = self.status_bar_windows.get(&win) {
            self.unmanage_statusbar(monitor_id, destroyed)?;
            return Ok(());
        }
        // 常规客户端的 unmanage 逻辑
        self.unmanage_regular_client(&client_rc, destroyed)?;
        Ok(())
    }

    fn unmanage_statusbar(
        &mut self,
        monitor_id: i32,
        destroyed: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "[unmanage_statusbar] Removing statusbar for monitor {}",
            monitor_id
        );

        let statusbar = match self.status_bar_clients.remove(&monitor_id) {
            Some(bar) => bar,
            None => {
                warn!(
                    "[unmanage_statusbar] No statusbar found for monitor {}",
                    monitor_id
                );
                return Ok(());
            }
        };

        let win = statusbar.borrow().win;
        self.status_bar_windows.remove(&win);

        // 清理窗口状态（如果未被销毁）
        if !destroyed {
            self.cleanup_statusbar_window(win)?;
        }

        // 恢复显示器工作区域
        if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
            let mut monitor_mut = monitor.borrow_mut();
            monitor_mut.w_y = monitor_mut.m_y;
            monitor_mut.w_h = monitor_mut.m_h;
            info!(
                "[unmanage_statusbar] Restored workarea for monitor {}",
                monitor_id
            );
        }

        // 按顺序清理资源
        let cleanup_results = [
            (
                "terminate_process",
                self.terminate_status_bar_process_safe(monitor_id),
            ),
            (
                "cleanup_shared_memory",
                self.cleanup_shared_memory_safe(monitor_id),
            ),
        ];

        // 记录清理结果但不中断流程
        for (operation, result) in cleanup_results.iter() {
            if let Err(ref e) = result {
                error!(
                    "[unmanage_statusbar] {} failed for monitor {}: {}",
                    operation, monitor_id, e
                );
            }
        }

        // 重新排列客户端
        if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
            self.arrange(Some(monitor));
        }

        info!(
            "[unmanage_statusbar] Successfully removed statusbar for monitor {}",
            monitor_id
        );
        Ok(())
    }

    fn cleanup_statusbar_window(&mut self, win: Window) -> Result<(), Box<dyn std::error::Error>> {
        // 清除事件监听
        let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT);
        self.x11rb_conn.change_window_attributes(win, &aux)?;
        self.x11rb_conn.flush()?;

        debug!(
            "[cleanup_statusbar_window] Cleared events for statusbar window {}",
            win
        );
        Ok(())
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

    pub fn unmanage_regular_client(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
        destroyed: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[unmanage_regular_client]");

        // 清理 pertag 中的选中客户端引用
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

        // 从链表中移除客户端
        self.detach(Some(client_rc.clone()));
        self.detachstack(Some(client_rc.clone()));

        // 如果窗口没有被销毁，需要清理窗口状态
        if !destroyed {
            self.cleanup_window_state(client_rc)?;
        }

        // 重新聚焦和排列
        self.focus(None)?;
        self.update_net_client_list()?;
        self.arrange(client_rc.borrow().mon.clone());

        Ok(())
    }

    fn cleanup_window_state(
        &mut self,
        client_rc: &Rc<RefCell<Client>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (win, old_border_w) = {
            let client = client_rc.borrow();
            (client.win, client.old_border_w)
        };

        // 抓取服务器
        self.x11rb_conn.grab_server()?.check()?;

        // 执行清理操作（将借用范围限制在这个块内）
        let cleanup_result = {
            // 取消事件选择
            let clear_events_result = {
                let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT);
                self.x11rb_conn
                    .change_window_attributes(win, &aux)
                    .and_then(|cookie| Ok(cookie.check()))
            };
            if let Err(e) = clear_events_result {
                warn!("[cleanup_window_state] Failed to clear event mask: {:?}", e);
            }

            // 恢复原始边框宽度
            if let Err(e) = self.set_window_border_width(win, old_border_w as u32) {
                warn!(
                    "[cleanup_window_state] Failed to restore border width: {:?}",
                    e
                );
            }

            // 取消所有按钮抓取
            let ungrab_result = self
                .x11rb_conn
                .ungrab_button(ButtonIndex::ANY, win, ModMask::ANY.into())
                .and_then(|cookie| Ok(cookie.check()));
            if let Err(e) = ungrab_result {
                warn!("[cleanup_window_state] Failed to ungrab buttons: {:?}", e);
            }

            // 设置客户端状态为 WithdrawnState
            if let Err(e) = self.setclientstate(client_rc, WithdrawnState as i64) {
                warn!("[cleanup_window_state] Failed to set client state: {:?}", e);
            }

            // 同步所有操作
            self.x11rb_conn.flush()
        };

        // 释放服务器（无论前面的操作是否成功）
        let ungrab_result = self
            .x11rb_conn
            .ungrab_server()
            .and_then(|_| self.x11rb_conn.flush());

        // 处理结果
        cleanup_result?;
        ungrab_result?;

        info!(
            "[cleanup_window_state] Window cleanup completed for {}",
            win
        );
        Ok(())
    }

    pub fn unmapnotify(&mut self, e: &UnmapNotifyEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[unmapnotify]");
        if let Some(client_rc) = self.wintoclient(e.window) {
            if e.from_configure {
                // 这是由于配置请求导致的unmap（通常是合成窗口管理器）
                debug!("Unmap from configure for window {}", e.window);
                self.setclientstate(&client_rc, WithdrawnState as i64)?;
            } else {
                // 这是真正的窗口销毁或隐藏
                debug!("Real unmap for window {}, unmanaging", e.window);
                self.unmanage(Some(client_rc), false)?;
            }
        } else {
            debug!("Unmap event for unmanaged window: {}", e.window);
        }
        Ok(())
    }

    pub fn updategeom(&mut self) -> bool {
        info!("[updategeom]");
        let dirty;

        // 使用 RandR 扩展替代 Xinerama
        match self.get_monitors_randr() {
            Ok(monitors) => {
                if monitors.is_empty() {
                    // 回退到单显示器模式
                    dirty = self.setup_single_monitor();
                } else {
                    dirty = self.setup_multiple_monitors(monitors);
                }
            }
            Err(_) => {
                // RandR 不可用，使用单显示器模式
                dirty = self.setup_single_monitor();
            }
        }

        if dirty {
            self.sel_mon = self.wintomon(self.x11rb_root);
            if self.sel_mon.is_none() && self.mons.is_some() {
                self.sel_mon = self.mons.clone();
            }
        }

        dirty
    }

    fn setup_multiple_monitors(&mut self, monitors: Vec<(i32, i32, i32, i32)>) -> bool {
        let mut dirty = false;
        let num_detected_monitors = monitors.len() as i32;

        // 计算当前已有的显示器数量
        let mut current_num_monitors = 0;
        let mut m_iter = self.mons.clone();
        while let Some(ref mon_rc) = m_iter {
            current_num_monitors += 1;
            let next = mon_rc.borrow().next.clone();
            m_iter = next;
        }

        // 如果检测到的显示器数量多于当前管理的数量，创建新的显示器
        if num_detected_monitors > current_num_monitors {
            dirty = true;
            for _ in current_num_monitors..num_detected_monitors {
                // 找到链表尾部并添加新显示器
                if let Some(ref mons) = self.mons {
                    let mut tail = mons.clone();
                    while tail.borrow().next.is_some() {
                        let next = tail.borrow().next.clone().unwrap();
                        tail = next;
                    }
                    tail.borrow_mut().next = Some(Rc::new(RefCell::new(self.createmon())));
                } else {
                    self.mons = Some(Rc::new(RefCell::new(self.createmon())));
                }
            }
        }

        // 更新现有显示器的几何信息
        m_iter = self.mons.clone();
        for (i, &(x, y, w, h)) in monitors.iter().enumerate() {
            if let Some(mon_rc) = m_iter {
                let mut mon = mon_rc.borrow_mut();

                // 检查几何信息是否需要更新
                if i as i32 >= current_num_monitors
                    || mon.m_x != x
                    || mon.m_y != y
                    || mon.m_w != w
                    || mon.m_h != h
                {
                    dirty = true;
                    mon.num = i as i32;
                    mon.m_x = x;
                    mon.w_x = x;
                    mon.m_y = y;
                    mon.w_y = y;
                    mon.m_w = w;
                    mon.w_w = w;
                    mon.m_h = h;
                    mon.w_h = h;
                }

                let next = mon.next.clone();
                m_iter = next;
            } else {
                break;
            }
        }

        // 如果当前显示器数量多于检测到的数量，移除多余的显示器
        if num_detected_monitors < current_num_monitors {
            dirty = true;
            self.remove_excess_monitors(num_detected_monitors, current_num_monitors);
        }

        dirty
    }

    fn remove_excess_monitors(&mut self, target_count: i32, current_count: i32) {
        for _ in target_count..current_count {
            // 找到最后一个显示器
            let mut current = self.mons.clone();
            let mut prev: Option<Rc<RefCell<Monitor>>> = None;

            while let Some(ref mon_rc) = current {
                if mon_rc.borrow().next.is_none() {
                    // 找到了最后一个显示器
                    break;
                }
                prev = current.clone();
                let next = mon_rc.borrow().next.clone();
                current = next;
            }

            if let Some(last_mon) = current {
                // 将最后一个显示器上的客户端移到第一个显示器
                self.move_clients_to_first_monitor(&last_mon);

                // 如果被移除的是当前选中的显示器，切换到第一个
                if let Some(ref sel_mon) = self.sel_mon {
                    if Rc::ptr_eq(&last_mon, sel_mon) {
                        self.sel_mon = self.mons.clone();
                    }
                }

                // 从链表中移除
                if let Some(ref prev_mon) = prev {
                    prev_mon.borrow_mut().next = None;
                } else {
                    // 移除的是第一个（也是唯一的）显示器
                    self.mons = None;
                }
            }
        }
    }

    fn move_clients_to_first_monitor(&mut self, from_monitor: &Rc<RefCell<Monitor>>) {
        if self.mons.is_none() {
            return;
        }

        let mut client_iter = from_monitor.borrow_mut().clients.take();

        while let Some(client_rc) = client_iter {
            let next_client = client_rc.borrow_mut().next.take();

            // 更新客户端的显示器和标签
            {
                let mut client_mut = client_rc.borrow_mut();
                client_mut.mon = self.mons.clone();

                if let Some(ref first_mon) = self.mons {
                    let first_mon_borrow = first_mon.borrow();
                    client_mut.tags = first_mon_borrow.tag_set[first_mon_borrow.sel_tags];
                } else {
                    client_mut.tags = 1; // 默认标签
                }
            }

            // 重新附加到第一个显示器
            self.attach(Some(client_rc.clone()));
            self.attachstack(Some(client_rc));

            client_iter = next_client;
        }
    }

    fn get_monitors_randr(&self) -> Result<Vec<(i32, i32, i32, i32)>, Box<dyn std::error::Error>> {
        use x11rb::protocol::randr::ConnectionExt;

        // 首先检查 RandR 扩展是否可用
        let version = self.x11rb_conn.randr_query_version(1, 2)?;
        let _version_reply = version.reply()?;

        let resources = self
            .x11rb_conn
            .randr_get_screen_resources(self.x11rb_root)?
            .reply()?;
        let mut monitors = Vec::new();

        for crtc in resources.crtcs {
            let crtc_info = self.x11rb_conn.randr_get_crtc_info(crtc, 0)?.reply()?;

            if crtc_info.width > 0 && crtc_info.height > 0 {
                monitors.push((
                    crtc_info.x as i32,
                    crtc_info.y as i32,
                    crtc_info.width as i32,
                    crtc_info.height as i32,
                ));
            }
        }

        // 去重，确保没有重复的几何区域
        monitors.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        monitors.dedup();

        Ok(monitors)
    }

    fn setup_single_monitor(&mut self) -> bool {
        let mut dirty = false;

        if self.mons.is_none() {
            self.mons = Some(Rc::new(RefCell::new(self.createmon())));
            dirty = true;
        }

        if let Some(ref mon_rc) = self.mons {
            let mut mon = mon_rc.borrow_mut();
            if mon.m_w != self.s_w || mon.m_h != self.s_h {
                dirty = true;
                mon.num = 0;
                mon.m_x = 0;
                mon.w_x = 0;
                mon.m_y = 0;
                mon.w_y = 0;
                mon.m_w = self.s_w;
                mon.w_w = self.s_w;
                mon.m_h = self.s_h;
                mon.w_h = self.s_h;
            }
        }

        dirty
    }

    pub fn updatewindowtype(&mut self, c: &Rc<RefCell<Client>>) {
        // info!("[updatewindowtype]");
        let state;
        let wtype;
        {
            let c = &mut *c.borrow_mut();
            state = self.getatomprop(c, self.atoms._NET_WM_STATE.into());
            wtype = self.getatomprop(c, self.atoms._NET_WM_WINDOW_TYPE.into());
        }

        if state == self.atoms._NET_WM_STATE_FULLSCREEN.into() {
            let _ = self.setfullscreen(c, true);
        }
        if wtype == self.atoms._NET_WM_WINDOW_TYPE_DIALOG.into() {
            let c = &mut *c.borrow_mut();
            c.is_floating = true;
        }
    }

    /// 更新客户端的 WM_HINTS 状态：urgent 和 never_focus
    pub fn updatewmhints(&self, client_rc: &Rc<RefCell<Client>>) {
        let win = client_rc.borrow().win;
        // 1. 读取 WM_HINTS 属性
        use ConnectionExt;
        let cookie = match self.x11rb_conn.get_property(
            false, // delete: 不删除
            win,   // window
            AtomEnum::WM_HINTS,
            AtomEnum::CARDINAL, // type: 期望 CARDINAL（实际是位图）
            0,                  // long_offset
            20,                 // length
        ) {
            Ok(cookie) => cookie,
            Err(_) => {
                debug!("updatewmhints: failed to send get_property request");
                return;
            }
        };

        let reply = match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => {
                // 属性不存在或无效
                return;
            }
        };

        // 2. 解析 flags（第一个 u32）
        let mut values = reply.value32().into_iter().flatten();
        let flags = match values.next() {
            Some(f) => f,
            None => return, // 无数据
        };

        // 3. 检查是否为当前选中窗口
        let is_focused = {
            if let Some(ref sel_mon) = self.sel_mon {
                let sel_mon_borrow = sel_mon.borrow();
                if let Some(ref sel) = sel_mon_borrow.sel {
                    Rc::ptr_eq(client_rc, sel)
                } else {
                    false
                }
            } else {
                false
            }
        };

        const X_URGENCY_HINT: u32 = 1 << 8;
        const INPUT_HINT: u32 = 1 << 0;

        // 4. 处理 XUrgencyHint
        if (flags & X_URGENCY_HINT) != 0 {
            if is_focused {
                // 如果是当前选中窗口，清除 urgency hint
                let new_flags = flags & !X_URGENCY_HINT;
                let mut data: Vec<u32> = vec![new_flags];
                data.extend(&mut values); // 保留其余字段

                use x11rb::wrapper::ConnectionExt;
                let _ = self
                    .x11rb_conn
                    .change_property32(
                        PropMode::REPLACE,
                        win,
                        AtomEnum::WM_HINTS,
                        AtomEnum::CARDINAL, // type: 期望 CARDINAL（实际是位图）
                        &data,
                    )
                    .and_then(|_| self.x11rb_conn.flush());
            } else {
                // 否则标记为 urgent
                client_rc.borrow_mut().is_urgent = true;
            }
        } else {
            // 没有 urgency hint
            client_rc.borrow_mut().is_urgent = false;
        }

        // 5. 处理 InputHint
        if (flags & INPUT_HINT) != 0 {
            // InputHint 存在，检查 input 字段
            let input = match values.next() {
                Some(i) => i as i32,
                None => return,
            };
            client_rc.borrow_mut().never_focus = input <= 0;
        } else {
            // InputHint 不存在，可聚焦
            client_rc.borrow_mut().never_focus = false;
        }
    }

    pub fn updatetitle(&mut self, c: &mut Client) {
        // info!("[updatetitle]");
        if !self.gettextprop(c.win, self.atoms._NET_WM_NAME.into(), &mut c.name) {
            self.gettextprop(c.win, AtomEnum::WM_NAME.into(), &mut c.name);
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
            monitor_info_for_message.monitor_y = mon_borrow.w_y;
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
            let mon_borrow = mon_rc.borrow(); // 再次不可变借用 m_rc 来获取 tagset 信息
            let active_tagset_for_mon = mon_borrow.tag_set[mon_borrow.sel_tags];
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
            let show_bar = mon_borrow
                .pertag
                .as_ref()
                .unwrap()
                .show_bars
                .get(i + 1)
                .unwrap_or(&true);
            monitor_info_for_message.set_show_bars(i, *show_bar);
        }

        let mut selected_client_name_for_bar = String::new();
        if let Some(ref selected_client_rc) = mon_rc.borrow().sel {
            selected_client_name_for_bar = selected_client_rc.borrow().name.clone();
        }
        monitor_info_for_message.set_client_name(&selected_client_name_for_bar);
        self.message.monitor_info = monitor_info_for_message;
    }
}
