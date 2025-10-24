use libc::{setsid, sigaction, sigemptyset, SIGCHLD, SIG_DFL};

use log::info;
use log::warn;
use log::{debug, error};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;

use serde::{Deserialize, Serialize};
use slotmap::{DefaultKey, SecondaryMap, SlotMap};
use std::cmp::{max, min};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::io::Write;
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::usize;

use crate::backend::api::AllowMode;
use crate::backend::api::BackendEvent;
use crate::backend::api::EwmhFeature;
use crate::backend::api::Geometry;
use crate::backend::api::NetWmAction;
use crate::backend::api::NetWmState;
use crate::backend::api::PropertyKind;
use crate::backend::api::{Backend, WindowId};
use crate::backend::common_define::ArgbColor;
use crate::backend::common_define::ColorScheme;
use crate::backend::common_define::ConfigWindowBits;
use crate::backend::common_define::EventMaskBits;
use crate::backend::common_define::SchemeType;
use crate::backend::common_define::{KeySym, Mods, MouseButton, StdCursorKind};
use crate::config::CONFIG;

use shared_structures::CommandType;
use shared_structures::SharedCommand;
use shared_structures::{MonitorInfo, SharedMessage, SharedRingBuffer, TagStatus};

use bincode::config::standard;
use bincode::{Decode, Encode};

// definitions for initial window state.
pub const WITHDRAWN_STATE: u8 = 0;
pub const STEXT_MAX_LEN: usize = 512;
pub const NORMAL_STATE: u8 = 1;
pub const ICONIC_STATE: u8 = 2;
pub const RESTART_SNAPSHOT_PATH: &str = "/var/tmp/jwm/restart_snapshot.bin";
pub const SHARED_PATH: &str = "/dev/shm/jwm_bar_global";

pub type ClientKey = DefaultKey;
pub type MonitorKey = DefaultKey;

lazy_static::lazy_static! {
    pub static ref BUTTONMASK: EventMaskBits  = EventMaskBits::BUTTON_PRESS | EventMaskBits::BUTTON_RELEASE;
    pub static ref MOUSEMASK: EventMaskBits   = EventMaskBits::BUTTON_PRESS | EventMaskBits::BUTTON_RELEASE | EventMaskBits::POINTER_MOTION;
}

#[derive(Debug, Serialize, Deserialize, Decode, Encode)]
pub struct RestartSnapshot {
    pub version: u32,
    pub timestamp: u64,

    // 全局
    pub sel_monitor_num: Option<i32>,
    pub current_bar_monitor_id: Option<i32>,

    // 按 monitor.num 排序或原有顺序保存
    pub monitors: Vec<MonitorSnapshot>,

    // Window -> WMClient（保留状态、tags、is_floating、client_fact、is_fullscreen、geometry 等）
    pub clients: HashMap<u32, WMClient>,
}

#[derive(Debug, Serialize, Deserialize, Decode, Encode)]
pub struct MonitorSnapshot {
    pub num: i32,

    // tag 集与当前选择
    pub tag_set: [u32; 2],
    pub sel_tags: usize,

    // per-tag 信息
    pub pertag: PertagSnapshot,

    // 顺序（使用 Window ID 表示）
    pub monitor_clients_order: Vec<u32>, // 建议定义为“底->顶”（一致即可）
    pub monitor_stack_order: Vec<u32>,   // 建议定义为“底->顶”（与 restack 对应）
}

#[derive(Debug, Serialize, Deserialize, Decode, Encode)]
pub struct PertagSnapshot {
    pub cur_tag: usize,
    pub prev_tag: usize,
    pub n_masters: Vec<u32>,
    pub m_facts: Vec<f32>,
    pub sel_lts: Vec<usize>,
    pub lt_pairs: Vec<[u32; 2]>, // 每 tag 两个 layout 的编号：0=TILE,1=FLOAT,2=MONOCLE
    pub show_bars: Vec<bool>,
    pub sel_by_tag: Vec<Option<u32>>, // 每个 tag 的选中窗口（Window）
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Decode, Encode)]
pub struct WMClient {
    // === 基本信息 ===
    pub name: String,
    pub class: String,
    pub instance: String,
    pub win: u32,

    // === 几何信息 ===
    pub geometry: ClientGeometry,
    pub size_hints: SizeHints,

    // === 状态信息 ===
    pub state: ClientState,

    // === 链表和关联 ===
    #[bincode(with_serde)]
    pub mon: Option<MonitorKey>,

    // === 重启时记录，方便映射到对应monitor ===
    pub monitor_num: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Decode, Encode)]
pub struct ClientGeometry {
    // 当前位置和大小
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,

    // 之前的位置和大小
    pub old_x: i32,
    pub old_y: i32,
    pub old_w: i32,
    pub old_h: i32,

    // 边框
    pub border_w: i32,
    pub old_border_w: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Decode, Encode)]
pub struct SizeHints {
    pub base_w: i32,
    pub base_h: i32,
    pub inc_w: i32,
    pub inc_h: i32,
    pub max_w: i32,
    pub max_h: i32,
    pub min_w: i32,
    pub min_h: i32,
    pub min_aspect: f32,
    pub max_aspect: f32,
    pub hints_valid: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Decode, Encode)]
pub struct ClientState {
    pub tags: u32,
    pub client_fact: f32,
    pub is_fixed: bool,
    pub is_floating: bool,
    pub is_urgent: bool,
    pub never_focus: bool,
    pub old_state: bool,
    pub is_fullscreen: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WMMonitor {
    // === 基本信息 ===
    pub num: i32,
    pub lt_symbol: String,

    // === 布局信息 ===
    pub layout: MonitorLayout,

    // === 几何信息 ===
    pub geometry: MonitorGeometry,

    // === 标签和布局选择 ===
    pub sel_tags: usize,
    pub sel_lt: usize,
    pub tag_set: [u32; 2],

    // === 客户端管理 ===
    pub sel: Option<ClientKey>,

    // === 布局和扩展 ===
    pub lt: [Rc<LayoutEnum>; 2],
    pub pertag: Option<Pertag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorLayout {
    pub m_fact: f32,
    pub n_master: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorGeometry {
    // 显示器区域
    pub m_x: i32,
    pub m_y: i32,
    pub m_w: i32,
    pub m_h: i32,

    // 工作区域
    pub w_x: i32,
    pub w_y: i32,
    pub w_w: i32,
    pub w_h: i32,
}

impl Default for ClientGeometry {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            old_x: 0,
            old_y: 0,
            old_w: 0,
            old_h: 0,
            border_w: 0,
            old_border_w: 0,
        }
    }
}

impl Default for SizeHints {
    fn default() -> Self {
        Self {
            base_w: 0,
            base_h: 0,
            inc_w: 0,
            inc_h: 0,
            max_w: 0,
            max_h: 0,
            min_w: 0,
            min_h: 0,
            min_aspect: 0.0,
            max_aspect: 0.0,
            hints_valid: false,
        }
    }
}

impl Default for ClientState {
    fn default() -> Self {
        Self {
            tags: 0,
            client_fact: 0.0,
            is_fixed: false,
            is_floating: false,
            is_urgent: false,
            never_focus: false,
            old_state: false,
            is_fullscreen: false,
        }
    }
}

impl Default for MonitorLayout {
    fn default() -> Self {
        Self {
            m_fact: 0.55, // 默认主区域比例
            n_master: 1,  // 默认主窗口数量
        }
    }
}

impl Default for MonitorGeometry {
    fn default() -> Self {
        Self {
            m_x: 0,
            m_y: 0,
            m_w: 0,
            m_h: 0,
            w_x: 0,
            w_y: 0,
            w_w: 0,
            w_h: 0,
        }
    }
}

impl WMClient {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            class: String::new(),
            instance: String::new(),
            win: 0,
            geometry: ClientGeometry::default(),
            size_hints: SizeHints::default(),
            state: ClientState::default(),
            mon: None,
            monitor_num: 1000,
        }
    }

    /// 获取包含边框的总宽度
    pub fn total_width(&self) -> i32 {
        self.geometry.w + 2 * self.geometry.border_w
    }

    /// 获取包含边框的总高度
    pub fn total_height(&self) -> i32 {
        self.geometry.h + 2 * self.geometry.border_w
    }

    /// 检查是否为状态栏
    pub fn is_status_bar(&self) -> bool {
        self.name == CONFIG.status_bar_name()
    }

    /// 获取客户端矩形区域
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (
            self.geometry.x,
            self.geometry.y,
            self.geometry.w,
            self.geometry.h,
        )
    }

    /// 设置客户端位置
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.geometry.old_x = self.geometry.x;
        self.geometry.old_y = self.geometry.y;
        self.geometry.x = x;
        self.geometry.y = y;
    }

    /// 设置客户端大小
    pub fn set_size(&mut self, w: i32, h: i32) {
        self.geometry.old_w = self.geometry.w;
        self.geometry.old_h = self.geometry.h;
        self.geometry.w = w;
        self.geometry.h = h;
    }

    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        let geom = &self.geometry;
        x >= geom.x && x < geom.x + geom.w && y >= geom.y && y < geom.y + geom.h
    }
}

impl WMMonitor {
    pub fn new() -> Self {
        Self {
            num: 0,
            lt_symbol: String::new(),
            layout: MonitorLayout::default(),
            geometry: MonitorGeometry::default(),
            sel_tags: 0,
            sel_lt: 0,
            tag_set: [0; 2],
            sel: None,
            lt: [Rc::new(LayoutEnum::TILE), Rc::new(LayoutEnum::TILE)],
            pertag: None,
        }
    }

    /// 计算与给定矩形的交集面积
    pub fn intersect_area(&self, x: i32, y: i32, w: i32, h: i32) -> i32 {
        let geom = &self.geometry;
        max(0, min(x + w, geom.w_x + geom.w_w) - max(x, geom.w_x))
            * max(0, min(y + h, geom.w_y + geom.w_h) - max(y, geom.w_y))
    }

    /// 检查点是否在工作区域内
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        let geom = &self.geometry;
        x >= geom.w_x && x < geom.w_x + geom.w_w && y >= geom.w_y && y < geom.w_y + geom.w_h
    }

    /// 获取工作区域矩形
    pub fn work_area(&self) -> (i32, i32, i32, i32) {
        let geom = &self.geometry;
        (geom.w_x, geom.w_y, geom.w_w, geom.w_h)
    }
}

impl fmt::Display for WMClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WMClient {{\n\
            \x20\x20name: \"{}\",\n\
            \x20\x20class: \"{}\",\n\
            \x20\x20instance: \"{}\",\n\
            \x20\x20win: 0x{},\n\
            \x20\x20geometry: {},\n\
            \x20\x20size_hints: {:?},\n\
            \x20\x20state: {:?},\n\
            \x20\x20monitor: {}\n\
            }}",
            self.name,
            self.class,
            self.instance,
            self.win,
            self.geometry,
            self.size_hints,
            self.state,
            // 对于 monitor，我们只显示是否存在，避免循环引用问题
            if self.mon.is_some() {
                "Some(Monitor)"
            } else {
                "None"
            }
        )
    }
}

impl fmt::Display for WMMonitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WMMonitor {{\n\
            \x20\x20num: {},\n\
            \x20\x20lt_symbol: \"{}\",\n\
            \x20\x20layout: {:?},\n\
            \x20\x20geometry: {:?},\n\
            \x20\x20sel_tags: {},\n\
            \x20\x20sel_lt: {},\n\
            \x20\x20tag_set: [{}, {}],\n\
            \x20\x20has_selection: {},\n\
            \x20\x20pertag: {}\n\
            }}",
            self.num,
            self.lt_symbol,
            self.layout,
            self.geometry,
            self.sel_tags,
            self.sel_lt,
            self.tag_set[0],
            self.tag_set[1],
            // 显示客户端数量而不是整个链表
            self.sel.is_some(),
            if self.pertag.is_some() {
                "Some(Pertag)"
            } else {
                "None"
            }
        )
    }
}

impl fmt::Display for ClientGeometry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}x{}+{}+{}", self.w, self.h, self.x, self.y)
    }
}

#[derive(Debug, Clone, Default)]
pub struct WMWindowGeom {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WMClickType {
    ClickClientWin,
    ClickRootWin,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WMArgEnum {
    Int(i32),
    UInt(u32),
    Float(f32),
    StringVec(Vec<String>),
    Layout(Rc<LayoutEnum>),
}

#[derive(Debug, Clone)]
pub struct WMButton {
    pub click_type: WMClickType,
    pub mask: Mods,
    pub button: MouseButton,
    pub func: Option<WMFuncType>,
    pub arg: WMArgEnum,
}
impl WMButton {
    pub fn new(
        click_type: WMClickType,
        mask: Mods,
        button: MouseButton,
        func: Option<WMFuncType>,
        arg_enum: WMArgEnum,
    ) -> Self {
        Self {
            click_type,
            mask,
            button,
            func,
            arg: arg_enum,
        }
    }
}

pub type WMFuncType = fn(&mut Jwm, &WMArgEnum) -> Result<(), Box<dyn std::error::Error>>;
#[derive(Debug, Clone)]
pub struct WMKey {
    pub mask: Mods,
    pub key_sym: KeySym,
    pub func_opt: Option<WMFuncType>,
    pub arg: WMArgEnum,
}
impl WMKey {
    pub fn new(mod0: Mods, keysym: KeySym, func: Option<WMFuncType>, arg: WMArgEnum) -> Self {
        Self {
            mask: mod0,
            key_sym: keysym,
            func_opt: func,
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
    lt_idxs: Vec<Vec<Option<Rc<LayoutEnum>>>>,
    // display bar for the current tag
    pub show_bars: Vec<bool>,
    // selected client
    pub sel: Vec<Option<ClientKey>>,
}
impl Pertag {
    pub fn new(show_bar: bool) -> Self {
        Self {
            cur_tag: 0,
            prev_tag: 0,
            n_masters: vec![0; CONFIG.tags_length() + 1],
            m_facts: vec![0.; CONFIG.tags_length() + 1],
            sel_lts: vec![0; CONFIG.tags_length() + 1],
            lt_idxs: vec![vec![None; 2]; CONFIG.tags_length() + 1],
            show_bars: vec![show_bar; CONFIG.tags_length() + 1],
            sel: vec![None; CONFIG.tags_length() + 1],
        }
    }
}

pub const DEFAULT_TILE_SYMBOL: &'static str = "[]=";
pub const DEFAULT_FLOAT_SYMBOL: &'static str = "><>";
pub const DEFAULT_MONOCLE_SYMBOL: &'static str = "[M]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutEnum(&'static str);
impl LayoutEnum {
    pub const ANY: Self = Self("");
    pub const TILE: Self = Self("tile");
    pub const FLOAT: Self = Self("float");
    pub const MONOCLE: Self = Self("monocle");
    pub fn symbol(&self) -> &str {
        match self {
            &LayoutEnum::TILE => DEFAULT_TILE_SYMBOL,
            &LayoutEnum::FLOAT => DEFAULT_FLOAT_SYMBOL,
            &LayoutEnum::MONOCLE => DEFAULT_MONOCLE_SYMBOL,
            _ => "",
        }
    }
    pub fn is_tile(&self) -> bool {
        self == &LayoutEnum::TILE
    }
    pub fn is_float(&self) -> bool {
        self == &LayoutEnum::FLOAT
    }
    pub fn is_monocle(&self) -> bool {
        self == &LayoutEnum::MONOCLE
    }
}

impl From<u32> for LayoutEnum {
    #[inline]
    fn from(value: u32) -> Self {
        match value {
            0 => LayoutEnum::TILE,
            1 => LayoutEnum::FLOAT,
            2 => LayoutEnum::MONOCLE,
            _ => LayoutEnum::ANY,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WMRule {
    pub class: String,
    pub instance: String,
    pub name: String,
    pub tags: usize,
    pub is_floating: bool,
    pub monitor: i32,
}
impl WMRule {
    pub fn new(
        class: String,
        instance: String,
        name: String,
        tags: usize,
        is_floating: bool,
        monitor: i32,
    ) -> Self {
        WMRule {
            class,
            instance,
            name,
            tags,
            is_floating,
            monitor,
        }
    }
}

pub type MonitorIndex = i32;

pub struct Jwm {
    // 基础/环境
    pub s_w: i32,
    pub s_h: i32,
    pub numlock_mask_bits: u16,
    pub running: AtomicBool,
    pub is_restarting: AtomicBool,

    backend: Box<dyn Backend>,

    // 与状态栏进程通信的消息缓存（写到 ring buffer）
    pub message: SharedMessage,

    // 客户端/显示器存储（SlotMap 体系）
    pub clients: SlotMap<ClientKey, WMClient>,
    pub monitors: SlotMap<MonitorKey, WMMonitor>,
    pub client_order: Vec<ClientKey>,
    pub client_stack_order: Vec<ClientKey>,
    pub monitor_order: Vec<MonitorKey>,
    pub sel_mon: Option<MonitorKey>,
    pub motion_mon: Option<MonitorKey>,
    pub monitor_clients: SecondaryMap<MonitorKey, Vec<ClientKey>>,
    pub monitor_stack: SecondaryMap<MonitorKey, Vec<ClientKey>>,

    // ——— 单实例状态栏（Single Bar）———
    // 共享内存与子进程（单实例）
    pub status_bar_shmem: Option<SharedRingBuffer>, // 全局唯一 ring buffer（例如 /dev/shm/jwm_bar_global）
    pub status_bar_child: Option<Child>,            // 单个状态栏进程
    pub status_bar_pid: Option<u32>,                // 子进程 PID（可选）

    // 状态栏窗口（单实例）
    pub status_bar_client: Option<ClientKey>, // 唯一的 bar 客户端
    pub status_bar_window: Option<u32>,       // 唯一的 bar 窗口
    pub current_bar_monitor_id: Option<i32>,  // bar 当前所在显示器的编号（monitor.num）

    // 去抖/差异更新
    pub last_bar_payload: Option<Vec<u8>>,
    pub last_bar_update_at: Option<std::time::Instant>,
    pub bar_min_interval: std::time::Duration,

    // per-monitor 的待刷新集合（仍按显示器维度存）
    pub pending_bar_updates: HashSet<MonitorIndex>,

    pub suppress_mouse_focus_until: Option<std::time::Instant>,

    pub restoring_from_snapshot: bool,

    pub last_stacking: SecondaryMap<MonitorKey, Vec<u32>>,
}

impl Jwm {
    pub fn new(mut backend: Box<dyn Backend>) -> Result<Self, Box<dyn std::error::Error>> {
        info!("[new] Starting JWM initialization");
        // 显示当前的 X11 环境信息
        Self::log_x11_environment();
        backend.cursor_provider().preload_common()?;
        // 屏幕尺寸来自 OutputOps
        let si = backend.output_ops().screen_info();
        let s_w = si.width;
        let s_h = si.height;
        info!(
            "[new] Screen info - resolution: {}x{}, root: 0x{:x}",
            s_w,
            s_h,
            backend.root_window().0
        );
        let alloc = backend.color_allocator();
        let colors = crate::config::CONFIG.colors();
        alloc.set_scheme(
            SchemeType::Norm,
            ColorScheme::new(
                ArgbColor::from_hex(&colors.dark_sea_green1, colors.opaque)?,
                ArgbColor::from_hex(&colors.light_sky_blue1, colors.opaque)?,
                ArgbColor::from_hex(&colors.light_sky_blue1, colors.opaque)?,
            ),
        );
        alloc.set_scheme(
            SchemeType::Sel,
            ColorScheme::new(
                ArgbColor::from_hex(&colors.dark_sea_green2, colors.opaque)?,
                ArgbColor::from_hex(&colors.pale_turquoise1, colors.opaque)?,
                ArgbColor::from_hex(&colors.cyan, colors.opaque)?,
            ),
        );
        // 预分配
        backend.color_allocator().allocate_schemes_pixels()?;
        info!("[new] JWM initialization completed successfully");
        Ok(Jwm {
            s_w,
            s_h,
            numlock_mask_bits: 0,
            running: AtomicBool::new(true),
            is_restarting: AtomicBool::new(false),

            backend,

            clients: SlotMap::new(),
            monitors: SlotMap::new(),
            client_order: Vec::new(),
            client_stack_order: Vec::new(),
            monitor_order: Vec::new(),
            sel_mon: None,
            motion_mon: None,
            monitor_clients: SecondaryMap::new(),
            monitor_stack: SecondaryMap::new(),

            status_bar_shmem: None,
            status_bar_child: None,
            message: SharedMessage::default(),
            status_bar_client: None,
            status_bar_window: None,
            current_bar_monitor_id: None,
            last_bar_payload: None,
            last_bar_update_at: None,
            bar_min_interval: std::time::Duration::from_millis(10),
            status_bar_pid: None,
            pending_bar_updates: HashSet::new(),

            suppress_mouse_focus_until: None,

            restoring_from_snapshot: false,
            last_stacking: SecondaryMap::new(),
        })
    }

    fn clean_mask(&self, raw: u16) -> Mods {
        // 使用 KeyOps 将后端原始修饰位转换为通用 Mods 并去掉 NUMLOCK/CAPS
        let mods_all = self
            .backend
            .key_ops()
            .mods_from_raw_mask(raw, self.numlock_mask_bits);
        mods_all
            & (Mods::SHIFT
                | Mods::CONTROL
                | Mods::ALT
                | Mods::SUPER
                | Mods::MOD2
                | Mods::MOD3
                | Mods::MOD5)
    }

    fn on_key_press(
        &mut self,
        keycode: u8,
        state_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let keysym = self.backend.key_ops_mut().keysym_from_keycode(keycode)?;
        let clean_state = self.clean_mask(state_bits);
        for key_config in CONFIG.get_keys().iter() {
            let kc_mask = key_config.mask
                & (Mods::SHIFT
                    | Mods::CONTROL
                    | Mods::ALT
                    | Mods::SUPER
                    | Mods::MOD2
                    | Mods::MOD3
                    | Mods::MOD5);
            if keysym == key_config.key_sym && kc_mask == clean_state {
                if let Some(func) = key_config.func_opt {
                    let _ = func(self, &key_config.arg);
                }
                break;
            }
        }
        Ok(())
    }

    fn on_button_press(
        &mut self,
        window: u32,
        state_bits: u16,
        detail_btn: u8,
        time: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut click_type = WMClickType::ClickRootWin;

        if let Some(target_mon_key) = self.wintomon(window) {
            if Some(target_mon_key) != self.sel_mon {
                if let Some(cur) = self.get_selected_client_key() {
                    self.unfocus(cur, true)?;
                }
                self.sel_mon = Some(target_mon_key);
                self.focus(None)?;
            }
        }

        let mut is_client_click = false;
        if let Some(client_key) = self.wintoclient(window) {
            is_client_click = true;
            self.focus(Some(client_key))?;
            let _ = self.restack(self.sel_mon);
            click_type = WMClickType::ClickClientWin;
        }

        let event_mask = self.clean_mask(state_bits);
        let mouse_button = MouseButton::from_u8(detail_btn);

        let mut handled_by_wm = false;
        for config in CONFIG.get_buttons().iter() {
            let kc_mask = config.mask
                & (Mods::SHIFT
                    | Mods::CONTROL
                    | Mods::ALT
                    | Mods::SUPER
                    | Mods::MOD2
                    | Mods::MOD3
                    | Mods::MOD5);
            if config.click_type == click_type
                && config.func.is_some()
                && config.button == mouse_button
                && kc_mask == event_mask
            {
                handled_by_wm = true;
                if let Some(ref func) = config.func {
                    let _ = func(self, &config.arg);
                }
                break;
            }
        }

        if is_client_click {
            let _ = if handled_by_wm {
                self.backend
                    .input_ops()
                    .allow_events(AllowMode::AsyncPointer, time)
            } else {
                self.backend
                    .input_ops()
                    .allow_events(AllowMode::ReplayPointer, time)
            };
        }
        Ok(())
    }

    fn on_motion_notify(
        &mut self,
        window: u32,
        root_x: i16,
        root_y: i16,
        _time: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if window != self.backend.root_window().0 as u32 {
            return Ok(());
        }
        if self.mouse_focus_blocked() {
            return Ok(());
        }
        let new_monitor_key = self.recttomon(root_x as i32, root_y as i32, 1, 1);
        if new_monitor_key != self.motion_mon {
            self.handle_monitor_switch_by_key(new_monitor_key)?;
        }
        self.motion_mon = new_monitor_key;
        Ok(())
    }

    // 后端无关：配置请求（包括 unmanaged 和 managed）
    fn on_configure_request(
        &mut self,
        window: u32,
        mask_bits: u16,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        border: u16,
        sibling: Option<u32>,
        stack_mode: u8,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 优先判断是否为状态栏
        if Some(window) == self.status_bar_window {
            return self.handle_statusbar_configure_request_params(
                window, mask_bits, x, y, w, h, border, sibling, stack_mode,
            );
        }

        // 是否 managed 客户端
        let client_key_opt = self.wintoclient(window);
        if let Some(client_key) = client_key_opt {
            return self.handle_regular_configure_request_params(
                client_key, mask_bits, x, y, w, h, border, sibling, stack_mode,
            );
        } else {
            // 未管理的窗口
            return self.handle_unmanaged_configure_request_params(
                window, mask_bits, x, y, w, h, border, sibling, stack_mode,
            );
        }
    }

    fn handle_statusbar_configure_request_params(
        &mut self,
        window: u32,
        mask_bits: u16,
        x: i16,
        y: i16,
        _w: u16,
        h: u16,
        _border: u16,
        _sibling: Option<u32>,
        _stack_mode: u8,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.status_bar_client.is_none() {
            error!("[handle_statusbar_configure_request] StatusBar not found");
            return self.handle_unmanaged_configure_request_params(
                window, mask_bits, x, y, 0, h, 0, None, 0,
            );
        }
        let mask = ConfigWindowBits::from_bits_truncate(mask_bits);
        {
            let bar_key = self.status_bar_client.unwrap();
            let statusbar_mut = self.clients.get_mut(bar_key).unwrap();

            if mask.contains(ConfigWindowBits::X) {
                statusbar_mut.geometry.x = x as i32;
            }
            if mask.contains(ConfigWindowBits::Y) {
                statusbar_mut.geometry.y = y as i32;
            }
            if mask.contains(ConfigWindowBits::HEIGHT) {
                statusbar_mut.geometry.h = (h.max(CONFIG.status_bar_height() as u16)) as i32;
            }

            self.backend.window_ops().configure_xywh_border(
                WindowId(window.into()),
                Some(statusbar_mut.geometry.x),
                Some(statusbar_mut.geometry.y),
                Some(statusbar_mut.geometry.w as u32),
                Some(statusbar_mut.geometry.h as u32),
                None,
            )?;
        }
        let monitor_key = self.get_monitor_by_id(self.current_bar_monitor_id.unwrap());
        self.arrange(monitor_key);
        if let Some(client_key) = self.wintoclient(window) {
            self.configure_client(client_key)?;
        }
        Ok(())
    }

    fn handle_regular_configure_request_params(
        &mut self,
        client_key: ClientKey,
        mask_bits: u16,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        border: u16,
        _sibling: Option<u32>,
        _stack_mode: u8,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let is_popup = self.is_popup_like(client_key);

        // 边框更新
        let mask = ConfigWindowBits::from_bits_truncate(mask_bits);
        if mask.contains(ConfigWindowBits::BORDER_WIDTH) {
            if let Some(client) = self.clients.get_mut(client_key) {
                client.geometry.border_w = border as i32;
            }
        }

        let (is_floating, mon_key_opt) = if let Some(client) = self.clients.get(client_key) {
            (client.state.is_floating, client.mon)
        } else {
            return Err("Client not found".into());
        };

        if is_floating {
            let (mx, my, mw, mh) = if let Some(mon_key) = mon_key_opt {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    (
                        monitor.geometry.m_x,
                        monitor.geometry.m_y,
                        monitor.geometry.m_w,
                        monitor.geometry.m_h,
                    )
                } else {
                    return Err("Monitor not found".into());
                }
            } else {
                return Err("Client has no monitor assigned".into());
            };

            if let Some(client) = self.clients.get_mut(client_key) {
                if mask.contains(ConfigWindowBits::X) {
                    client.geometry.old_x = client.geometry.x;
                    client.geometry.x = mx + x as i32;
                }
                if mask.contains(ConfigWindowBits::Y) {
                    client.geometry.old_y = client.geometry.y;
                    client.geometry.y = my + y as i32;
                }
                if mask.contains(ConfigWindowBits::WIDTH) {
                    client.geometry.old_w = client.geometry.w;
                    client.geometry.w = w as i32;
                }
                if mask.contains(ConfigWindowBits::HEIGHT) {
                    client.geometry.old_h = client.geometry.h;
                    client.geometry.h = h as i32;
                }

                if is_popup {
                    self.backend.window_ops().configure_xywh_border(
                        WindowId(client.win.into()),
                        Some(client.geometry.x),
                        Some(client.geometry.y),
                        Some(client.geometry.w as u32),
                        Some(client.geometry.h as u32),
                        None,
                    )?;
                    self.backend.window_ops().flush()?;
                    return Ok(());
                }

                // 保持在 monitor 内
                if (client.geometry.x + client.geometry.w) > mx + mw && client.state.is_floating {
                    client.geometry.x = mx + (mw / 2 - client.total_width() / 2);
                }
                if (client.geometry.y + client.geometry.h) > my + mh && client.state.is_floating {
                    client.geometry.y = my + (mh / 2 - client.total_height() / 2);
                }
            }

            // 如果只是位置变化，发送配置确认
            if mask.contains(ConfigWindowBits::X | ConfigWindowBits::Y)
                && !mask.contains(ConfigWindowBits::WIDTH | ConfigWindowBits::HEIGHT)
            {
                self.configure_client(client_key)?;
            }

            // 可见则应用配置
            if self.is_client_visible_by_key(client_key) {
                if let Some(client) = self.clients.get(client_key) {
                    self.backend.window_ops().configure_xywh_border(
                        WindowId(client.win.into()),
                        Some(client.geometry.x),
                        Some(client.geometry.y),
                        Some(client.geometry.w as u32),
                        Some(client.geometry.h as u32),
                        None,
                    )?;
                    self.backend.window_ops().flush()?;
                }
            }
        } else {
            // 平铺窗口：仅确认当前几何
            self.configure_client(client_key)?;
        }

        Ok(())
    }

    // 后端无关：unmanaged 窗口 configure
    fn handle_unmanaged_configure_request_params(
        &mut self,
        window: u32,
        mask_bits: u16,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        border: u16,
        sibling: Option<u32>,
        _stack_mode: u8,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "[handle_unmanaged_configure_request] unmanaged window=0x{:x}",
            window
        );
        let mask = ConfigWindowBits::from_bits_truncate(mask_bits);
        // 先用 window_ops 配置 xywh/border（逐步替换）
        let ox = if mask.contains(ConfigWindowBits::X) {
            Some(x as i32)
        } else {
            None
        };
        let oy = if mask.contains(ConfigWindowBits::Y) {
            Some(y as i32)
        } else {
            None
        };
        let ow = if mask.contains(ConfigWindowBits::WIDTH) {
            Some(w as u32)
        } else {
            None
        };
        let oh = if mask.contains(ConfigWindowBits::HEIGHT) {
            Some(h as u32)
        } else {
            None
        };
        let ob = if mask.contains(ConfigWindowBits::BORDER_WIDTH) {
            Some(border as u32)
        } else {
            None
        };
        if ox.is_some() || oy.is_some() || ow.is_some() || oh.is_some() || ob.is_some() {
            let _ = self.backend.window_ops().configure_xywh_border(
                WindowId(window.into()),
                ox,
                oy,
                ow,
                oh,
                ob,
            );
        }

        if mask.contains(ConfigWindowBits::SIBLING) || mask.contains(ConfigWindowBits::STACK_MODE) {
            self.backend.window_ops().configure_stack_above(
                WindowId(window.into()),
                sibling.map(|s| WindowId(s.into())),
            )?;
        }
        self.backend.window_ops().flush()?;

        Ok(())
    }

    fn handle_backend_event(&mut self, ev: BackendEvent) -> Result<(), Box<dyn std::error::Error>> {
        match ev {
            BackendEvent::ButtonPress {
                window,
                state,
                detail,
                time,
            } => self.on_button_press(window.0 as u32, state, detail, time),
            BackendEvent::MotionNotify {
                window,
                root_x,
                root_y,
                time,
            } => self.on_motion_notify(window.0 as u32, root_x, root_y, time),
            BackendEvent::ConfigureRequest {
                window,
                mask,
                x,
                y,
                w,
                h,
                border,
                sibling,
                stack_mode,
            } => self.on_configure_request(
                window.0 as u32,
                mask,
                x,
                y,
                w,
                h,
                border,
                sibling.map(|s| s.0 as u32),
                stack_mode,
            ),
            BackendEvent::KeyPress { keycode, state } => self.on_key_press(keycode, state),
            BackendEvent::ConfigureNotify { window, x, y, w, h } => {
                self.configurenotify(window.0 as u32, x, y, w, h)
            }
            BackendEvent::DestroyNotify { window } => self.destroynotify(window.0 as u32),
            BackendEvent::EnterNotify {
                window,
                event,
                mode,
                detail,
            } => self.enter_notify(window.0 as u32, event.0 as u32, mode, detail),
            BackendEvent::Expose { window, count } => self.expose(window.0 as u32, count),
            BackendEvent::FocusIn { event } => self.focusin(event.0 as u32),
            BackendEvent::MapRequest { window } => self.maprequest(window.0 as u32),
            BackendEvent::UnmapNotify {
                window,
                from_configure,
            } => self.unmapnotify(window.0 as u32, from_configure),

            BackendEvent::MappingNotify { request: _ } => {
                // 统一处理：键盘映射变化，清缓存+重新抓取
                self.backend.key_ops_mut().clear_cache();
                self.grabkeys()
            }

            BackendEvent::PropertyChanged {
                window,
                kind,
                deleted,
            } => {
                if deleted {
                    return Ok(());
                }
                if let Some(client_key) = self.wintoclient(window.0 as u32) {
                    match kind {
                        PropertyKind::WmTransientFor => {
                            self.handle_transient_for_change(client_key)?
                        }
                        PropertyKind::WmNormalHints => {
                            self.handle_normal_hints_change(client_key)?
                        }
                        PropertyKind::WmHints => self.handle_wm_hints_change(client_key)?,
                        PropertyKind::WmName | PropertyKind::NetWmName => {
                            self.handle_title_change(client_key)?
                        }
                        PropertyKind::NetWmWindowType => {
                            self.handle_window_type_change(client_key)?
                        }
                        PropertyKind::Other => {}
                    }
                }
                Ok(())
            }
            BackendEvent::EwmhState {
                window,
                action,
                states,
            } => {
                let fullscreen_requested = states
                    .iter()
                    .flatten()
                    .any(|s| matches!(s, NetWmState::Fullscreen));
                if fullscreen_requested {
                    if let Some(ck) = self.wintoclient(window.0 as u32) {
                        let is_fullscreen = self
                            .clients
                            .get(ck)
                            .map(|c| c.state.is_fullscreen)
                            .unwrap_or(false);
                        let fullscreen = match action {
                            NetWmAction::Add => true,
                            NetWmAction::Remove => false,
                            NetWmAction::Toggle => !is_fullscreen,
                        };
                        self.setfullscreen(ck, fullscreen)?;
                    }
                }
                Ok(())
            }
            BackendEvent::ActiveWindowMessage { window } => {
                if let Some(ck) = self.wintoclient(window.0 as u32) {
                    let is_urgent = self
                        .clients
                        .get(ck)
                        .map(|c| c.state.is_urgent)
                        .unwrap_or(false);
                    if !self.is_client_selected(ck) && !is_urgent {
                        self.seturgent(ck, true)?;
                    }
                }
                Ok(())
            }
            BackendEvent::ClientMessage { .. } | BackendEvent::PropertyNotify { .. } => Ok(()),
            BackendEvent::ButtonRelease { .. } => Ok(()),
        }
    }

    fn layout_to_id(l: &LayoutEnum) -> u32 {
        match *l {
            LayoutEnum::TILE => 0,
            LayoutEnum::FLOAT => 1,
            LayoutEnum::MONOCLE => 2,
            _ => 0,
        }
    }
    fn id_to_layout(id: u32) -> Rc<LayoutEnum> {
        Rc::new(LayoutEnum::from(id))
    }

    fn atomic_write(path: &str, data: &[u8]) -> std::io::Result<()> {
        let tmp = format!("{}.tmp", path);
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    fn unix_ts() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn save_restart_snapshot(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut snapshot = RestartSnapshot {
            version: 1,
            timestamp: Self::unix_ts(),
            sel_monitor_num: self
                .sel_mon
                .and_then(|k| self.monitors.get(k))
                .map(|m| m.num),
            current_bar_monitor_id: self.current_bar_monitor_id,
            monitors: Vec::new(),
            clients: HashMap::new(),
        };

        // 监视器快照
        for &mon_key in &self.monitor_order {
            let m = self.monitors.get(mon_key).unwrap();

            // pertag 拆出
            let pertag_snap = if let Some(p) = m.pertag.as_ref() {
                let mut lt_pairs = Vec::with_capacity(p.lt_idxs.len());
                for i in 0..p.lt_idxs.len() {
                    let id0 = p.lt_idxs[i][0]
                        .as_ref()
                        .map(|rc| Self::layout_to_id(&*rc))
                        .unwrap_or(0);
                    let id1 = p.lt_idxs[i][1]
                        .as_ref()
                        .map(|rc| Self::layout_to_id(&*rc))
                        .unwrap_or(1);
                    lt_pairs.push([id0, id1]);
                }
                let sel_by_tag = p
                    .sel
                    .iter()
                    .map(|opt_ck| opt_ck.and_then(|ck| self.clients.get(ck)).map(|c| c.win))
                    .collect();

                PertagSnapshot {
                    cur_tag: p.cur_tag,
                    prev_tag: p.prev_tag,
                    n_masters: p.n_masters.clone(),
                    m_facts: p.m_facts.clone(),
                    sel_lts: p.sel_lts.clone(),
                    lt_pairs,
                    show_bars: p.show_bars.clone(),
                    sel_by_tag,
                }
            } else {
                // fallback：按 tags_length()+1 填入基本值
                let len = CONFIG.tags_length() + 1;
                PertagSnapshot {
                    cur_tag: 1,
                    prev_tag: 1,
                    n_masters: vec![m.layout.n_master; len],
                    m_facts: vec![m.layout.m_fact; len],
                    sel_lts: vec![m.sel_lt; len],
                    lt_pairs: vec![[0, 1]; len],
                    show_bars: vec![true; len],
                    sel_by_tag: vec![None; len],
                }
            };

            // 顺序（Window）
            let mc_order = self
                .monitor_clients
                .get(mon_key)
                .map(|v| {
                    v.iter()
                        .filter_map(|&ck| self.clients.get(ck).map(|c| c.win))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let ms_order = self
                .monitor_stack
                .get(mon_key)
                .map(|v| {
                    v.iter()
                        .filter_map(|&ck| self.clients.get(ck).map(|c| c.win))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            snapshot.monitors.push(MonitorSnapshot {
                num: m.num,
                tag_set: m.tag_set,
                sel_tags: m.sel_tags,
                pertag: pertag_snap,
                monitor_clients_order: mc_order,
                monitor_stack_order: ms_order,
            });
        }

        // 客户端快照（Window -> WMClient）
        for (_, c) in self.clients.iter() {
            let mut cc = c.clone();
            cc.monitor_num = c
                .mon
                .and_then(|mk| self.monitors.get(mk))
                .map(|m| m.num as u32)
                .unwrap_or(0);
            cc.mon = None; // 快照不存 SlotMap 键
            snapshot.clients.insert(cc.win, cc);
        }

        // 写盘（原子）
        let data = bincode::encode_to_vec(&snapshot, standard())?;
        Self::atomic_write(RESTART_SNAPSHOT_PATH, &data)?;
        Ok(())
    }

    fn load_restart_snapshot() -> Option<RestartSnapshot> {
        let path = std::path::Path::new(RESTART_SNAPSHOT_PATH);
        if !path.exists() {
            return None;
        }
        let data = std::fs::read(path).ok()?;
        bincode::decode_from_slice(&data, standard())
            .ok()
            .map(|(snapshot, _bytes_read)| snapshot)
    }

    fn apply_snapshot(&mut self, snap: &RestartSnapshot) {
        // 0) 先把 snapshot 中的 client 状态应用到已管理的 clients（tags、is_floating、client_fact、fullscreen、geometry 等）
        for (win, sc) in &snap.clients {
            if let Some(ck) = self.wintoclient(*win) {
                let mon_key_opt = self.get_monitor_by_id(sc.monitor_num as i32);
                if let Some(c) = self.clients.get_mut(ck) {
                    c.state = sc.state.clone();
                    c.geometry = sc.geometry.clone();
                    c.size_hints = sc.size_hints.clone();
                    // 监视器先根据 monitor_num 设置，后续在重建顺序时会覆盖
                    if mon_key_opt.is_some() {
                        c.mon = mon_key_opt;
                    }
                }
            }
        }

        // 1) 恢复 monitor 的 tag_set/sel_tags 与 pertag（layout、nmaster、mfact、show_bar）
        for ms in &snap.monitors {
            if let Some(mon_key) = self.get_monitor_by_id(ms.num) {
                if let Some(m) = self.monitors.get_mut(mon_key) {
                    m.tag_set = ms.tag_set;
                    m.sel_tags = ms.sel_tags;

                    if let Some(p) = m.pertag.as_mut() {
                        p.cur_tag = ms.pertag.cur_tag;
                        p.prev_tag = ms.pertag.prev_tag;
                        p.n_masters = ms.pertag.n_masters.clone();
                        p.m_facts = ms.pertag.m_facts.clone();
                        p.sel_lts = ms.pertag.sel_lts.clone();
                        p.show_bars = ms.pertag.show_bars.clone();
                        // 重建 lt_idxs
                        for i in 0..p.lt_idxs.len().min(ms.pertag.lt_pairs.len()) {
                            let [id0, id1] = ms.pertag.lt_pairs[i];
                            p.lt_idxs[i][0] = Some(Self::id_to_layout(id0));
                            p.lt_idxs[i][1] = Some(Self::id_to_layout(id1));
                        }
                        // 应用当前 tag 的选择到 WMMonitor
                        let cur = p.cur_tag;
                        m.layout.n_master = p.n_masters[cur];
                        m.layout.m_fact = p.m_facts[cur];
                        m.sel_lt = p.sel_lts[cur];
                        m.lt[0] = p.lt_idxs[cur][0].as_ref().unwrap().clone();
                        m.lt[1] = p.lt_idxs[cur][1].as_ref().unwrap().clone();
                    }
                }
            }
        }

        // 2) 清空并按快照重建 monitor_clients/monitor_stack（保持顺序）
        for &mon_key in &self.monitor_order {
            if let Some(v) = self.monitor_clients.get_mut(mon_key) {
                v.clear();
            }
            if let Some(v) = self.monitor_stack.get_mut(mon_key) {
                v.clear();
            }
        }
        for ms in &snap.monitors {
            if let Some(mon_key) = self.get_monitor_by_id(ms.num) {
                // clients 顺序
                for &win in &ms.monitor_clients_order {
                    if let Some(ck) = self.wintoclient(win) {
                        self.attach_to_monitor_end(ck, mon_key);
                    }
                }
                // stack 顺序
                for &win in &ms.monitor_stack_order {
                    if let Some(ck) = self.wintoclient(win) {
                        self.attach_to_monitor_stack_end(ck, mon_key);
                    }
                }
            }
        }

        // 3) 恢复 per-tag 的选中 client 与 monitor.sel
        for ms in &snap.monitors {
            if let Some(mon_key) = self.get_monitor_by_id(ms.num) {
                // 收集所有需要的信息
                let mut updates = Vec::new();
                for (i, &win_opt) in ms.pertag.sel_by_tag.iter().enumerate() {
                    let client_key = win_opt.and_then(|w| self.wintoclient(w));
                    updates.push((i, client_key));
                }
                let next_visible = self.find_next_visible_client_by_mon(mon_key);
                // 现在安全地更新
                if let Some(m) = self.monitors.get_mut(mon_key) {
                    if let Some(p) = m.pertag.as_mut() {
                        // 应用更新
                        for (i, client_key) in updates {
                            if i < p.sel.len() {
                                p.sel[i] = client_key;
                            }
                        }
                        let cur = p.cur_tag;
                        m.sel = p.sel.get(cur).copied().flatten().or(next_visible);
                    }
                }
            }
        }

        // 4) 恢复 sel_mon 与 bar monitor
        if let Some(id) = snap.sel_monitor_num {
            self.sel_mon = self.get_monitor_by_id(id);
        }
        if let Some(id) = snap.current_bar_monitor_id {
            self.current_bar_monitor_id = Some(id);
            let _ = self.position_statusbar_on_monitor(id);
        }

        // 5) 一次性更新“可见性 + 叠放 + 焦点”，不要触发布局计算以免改动几何
        // self.arrange(None);
        for &mon_key in self.monitor_order.clone().iter() {
            self.showhide_monitor(mon_key); // 只根据 tag 显示/隐藏，不改变尺寸
        }
        let _ = self.restack(self.sel_mon);
        let _ = self.focus(None);
        self.mark_bar_update_needed_if_visible(None);
    }

    // 尾插：保持快照顺序
    fn attach_to_monitor_end(&mut self, ck: ClientKey, mon: MonitorKey) {
        if let Some(v) = self.monitor_clients.get_mut(mon) {
            if !v.iter().any(|&k| k == ck) {
                v.push(ck);
            }
        }
        if let Some(c) = self.clients.get_mut(ck) {
            c.mon = Some(mon);
        }
    }
    fn attach_to_monitor_stack_end(&mut self, ck: ClientKey, mon: MonitorKey) {
        if let Some(v) = self.monitor_stack.get_mut(mon) {
            if !v.iter().any(|&k| k == ck) {
                v.push(ck);
            }
        }
    }

    // 创建新的客户端
    fn insert_client(&mut self, client: WMClient) -> ClientKey {
        let key = self.clients.insert(client);
        self.client_order.push(key);
        key
    }

    // 创建新的监视器
    fn insert_monitor(&mut self, monitor: WMMonitor) -> MonitorKey {
        let key = self.monitors.insert(monitor);
        self.monitor_order.push(key);
        self.monitor_clients.insert(key, Vec::new());
        self.monitor_stack.insert(key, Vec::new());
        key
    }

    /// 检查客户端是否是当前选中的客户端
    fn is_client_selected(&self, client_key: ClientKey) -> bool {
        self.sel_mon
            .and_then(|sel_mon_key| self.monitors.get(sel_mon_key))
            .and_then(|monitor| monitor.sel)
            .map(|sel_client| sel_client == client_key)
            .unwrap_or(false)
    }

    // 获取监视器的所有客户端
    fn get_monitor_clients(&self, mon_key: MonitorKey) -> &[ClientKey] {
        self.monitor_clients
            .get(mon_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    // 获取监视器的堆栈顺序
    fn get_monitor_stack(&self, mon_key: MonitorKey) -> &[ClientKey] {
        self.monitor_stack
            .get(mon_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn get_sel_mon(&self) -> Option<&WMMonitor> {
        self.sel_mon
            .and_then(|sel_mon_key| self.monitors.get(sel_mon_key))
            .and_then(|monitor| Some(monitor))
    }

    fn get_selected_client_key(&self) -> Option<ClientKey> {
        self.sel_mon
            .and_then(|sel_mon_key| self.monitors.get(sel_mon_key))
            .and_then(|monitor| monitor.sel)
    }

    fn attach(&mut self, client_key: ClientKey) {
        if let Some(client) = self.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
                    // 插入到列表开头（模拟链表头插入）
                    client_list.insert(0, client_key);
                }
            }
        }
    }

    fn detach(&mut self, client_key: ClientKey) {
        if let Some(client) = self.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
                    if let Some(pos) = client_list.iter().position(|&k| k == client_key) {
                        client_list.remove(pos);
                    }
                }
            }
        }
    }

    fn attachstack(&mut self, client_key: ClientKey) {
        if let Some(client) = self.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(stack_list) = self.monitor_stack.get_mut(mon_key) {
                    stack_list.insert(0, client_key);
                }
            }
        }
    }

    /// 从指定监视器移除客户端
    fn detach_from_monitor(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
            client_list.retain(|&k| k != client_key);
        }
        if let Some(stack_list) = self.monitor_stack.get_mut(mon_key) {
            stack_list.retain(|&k| k != client_key);
        }
    }

    /// 将客户端添加到指定监视器
    fn attach_to_monitor(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
            client_list.insert(0, client_key);
        }
        if let Some(stack_list) = self.monitor_stack.get_mut(mon_key) {
            stack_list.insert(0, client_key);
        }
    }

    fn detachstack(&mut self, client_key: ClientKey) {
        if let Some(client) = self.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(stack_list) = self.monitor_stack.get_mut(mon_key) {
                    if let Some(pos) = stack_list.iter().position(|&k| k == client_key) {
                        stack_list.remove(pos);
                    }
                }
                // 更新选中客户端
                let next_visible_client = self.find_next_visible_client_by_mon(mon_key);
                if let Some(monitor) = self.monitors.get_mut(mon_key) {
                    if monitor.sel == Some(client_key) {
                        // 找到下一个可见客户端
                        monitor.sel = next_visible_client;
                    }
                }
            }
        }
    }

    // 查找下一个可见客户端
    fn find_next_visible_client_by_mon(&self, mon_key: MonitorKey) -> Option<ClientKey> {
        if let Some(stack_list) = self.monitor_stack.get(mon_key) {
            for &client_key in stack_list {
                if let Some(_) = self.clients.get(client_key) {
                    if self.is_client_visible_on_monitor(client_key, mon_key) {
                        return Some(client_key);
                    }
                }
            }
        }
        None
    }

    fn is_client_visible_on_monitor(&self, client_key: ClientKey, mon_key: MonitorKey) -> bool {
        if let (Some(client), Some(monitor)) =
            (self.clients.get(client_key), self.monitors.get(mon_key))
        {
            (client.state.tags & monitor.tag_set[monitor.sel_tags]) > 0
        } else {
            false
        }
    }

    /// 检查客户端是否可见（使用 ClientKey）
    fn is_client_visible_by_key(&self, client_key: ClientKey) -> bool {
        if let Some(client) = self.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    return (client.state.tags & monitor.tag_set[monitor.sel_tags]) > 0;
                }
            }
        }

        false
    }

    fn nexttiled(&self, mon_key: MonitorKey, start_from: Option<ClientKey>) -> Option<ClientKey> {
        let client_list = self.get_monitor_clients(mon_key);
        let start_index = if let Some(start_key) = start_from {
            client_list
                .iter()
                .position(|&k| k == start_key)
                .map(|i| i + 1)
                .unwrap_or(0)
        } else {
            0
        };

        for &client_key in &client_list[start_index..] {
            if let Some(client) = self.clients.get(client_key) {
                if !client.state.is_floating
                    && self.is_client_visible_on_monitor(client_key, mon_key)
                {
                    return Some(client_key);
                }
            }
        }
        None
    }

    fn pop(&mut self, client_key: ClientKey) {
        // info!("[pop]");
        let mon_key = if let Some(client) = self.clients.get(client_key) {
            client.mon
        } else {
            return;
        };

        self.detach(client_key);
        self.attach(client_key);
        let _ = self.focus(Some(client_key));

        if let Some(mon_key) = mon_key {
            self.arrange(Some(mon_key));
        }
    }

    fn wintoclient(&self, win: u32) -> Option<ClientKey> {
        // 先检查是否为单实例状态栏窗口
        if let Some(bar_win) = self.status_bar_window {
            if bar_win == win {
                return self.status_bar_client;
            }
        }

        // 再查找常规客户端
        self.clients
            .iter()
            .find(|(_, client)| client.win == win)
            .map(|(key, _)| key)
    }

    /// 记录 X11 环境信息用于调试
    fn log_x11_environment() {
        info!("[X11 Environment Debug]");
        info!("DISPLAY: {:?}", env::var("DISPLAY"));
        info!("XAUTHORITY: {:?}", env::var("XAUTHORITY"));
        info!("XDG_SESSION_TYPE: {:?}", env::var("XDG_SESSION_TYPE"));
        info!("USER: {:?}", env::var("USER"));
        info!("HOME: {:?}", env::var("HOME"));

        // 检查 X11 socket 文件
        if let Ok(display) = env::var("DISPLAY") {
            let socket_path = format!("/tmp/.X11-unix/X{}", display.trim_start_matches(":"));
            info!("X11 socket path: {}", socket_path);
            info!(
                "X11 socket exists: {}",
                std::path::Path::new(&socket_path).exists()
            );
        }

        // 检查 X 服务器是否在运行
        let x_running = std::process::Command::new("pgrep")
            .arg("-f")
            .arg("X|Xorg")
            .output()
            .map(|output| !output.stdout.is_empty())
            .unwrap_or(false);
        info!("X server running: {}", x_running);
    }

    pub fn restart(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[restart] Preparing seamless restart");
        // 先保存快照
        if let Err(e) = self.save_restart_snapshot() {
            warn!("[restart] save_restart_snapshot failed: {:?}", e);
        }
        // 标记重启，退出主循环
        self.running.store(false, Ordering::SeqCst);
        self.is_restarting.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn is_bar_visible_on_mon(&self, mon_key: MonitorKey) -> bool {
        if let Some(m) = self.monitors.get(mon_key) {
            if let Some(p) = m.pertag.as_ref() {
                if let Some(&show) = p.show_bars.get(p.cur_tag) {
                    return show;
                }
            }
        }
        // 没有 pertag 或越界时，保守返回 true（与现有默认行为一致）
        true
    }
    fn mark_bar_update_needed_if_visible(&mut self, monitor_id: Option<i32>) {
        match monitor_id {
            Some(id) => {
                if let Some(mon_key) = self.get_monitor_by_id(id) {
                    if self.is_bar_visible_on_mon(mon_key) {
                        self.pending_bar_updates.insert(id);
                    }
                }
            }
            None => {
                for (key, m) in self.monitors.iter() {
                    if self.is_bar_visible_on_mon(key) {
                        self.pending_bar_updates.insert(m.num);
                    }
                }
            }
        }
    }

    /// 获取窗口的 WM_CLASS（即类名和实例名）
    fn get_wm_class(&self, window: u32) -> Option<(String, String)> {
        self.backend
            .property_ops()
            .get_wm_class(WindowId(window.into()))
    }

    fn applysizehints(
        &mut self,
        client_key: ClientKey,
        x: &mut i32,
        y: &mut i32,
        w: &mut i32,
        h: &mut i32,
        interact: bool,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // 设置最小可能的客户端区域大小
        *w = (*w).max(1);
        *h = (*h).max(1);

        // 获取当前几何信息用于后续比较
        let original_geometry = if let Some(client) = self.clients.get(client_key) {
            (
                client.geometry.x,
                client.geometry.y,
                client.geometry.w,
                client.geometry.h,
            )
        } else {
            return Err("Client not found".into());
        };

        // 边界检查
        self.apply_boundary_constraints(client_key, x, y, w, h, interact)?;

        // 尺寸提示处理
        let geometry_changed = self.apply_size_hints_constraints(client_key, w, h)?;

        // 检查最终几何形状是否与客户端当前几何形状不同
        Ok(geometry_changed
            || *x != original_geometry.0
            || *y != original_geometry.1
            || *w != original_geometry.2
            || *h != original_geometry.3)
    }

    /// 应用边界约束
    fn apply_boundary_constraints(
        &self,
        client_key: ClientKey,
        x: &mut i32,
        y: &mut i32,
        w: &i32,
        h: &i32,
        interact: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (client_total_width, client_total_height, mon_key) =
            if let Some(client) = self.clients.get(client_key) {
                (
                    *w + 2 * client.geometry.border_w,
                    *h + 2 * client.geometry.border_w,
                    client.mon,
                )
            } else {
                return Err("Client not found".into());
            };

        if interact {
            // 屏幕边界约束
            self.constrain_to_screen(x, y, client_total_width, client_total_height);
        } else {
            // 监视器边界约束
            if let Some(mon_key) = mon_key {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    self.constrain_to_monitor(
                        x,
                        y,
                        client_total_width,
                        client_total_height,
                        &monitor.geometry,
                    );
                }
            }
        }

        Ok(())
    }

    /// 约束到屏幕边界
    fn constrain_to_screen(&self, x: &mut i32, y: &mut i32, total_width: i32, total_height: i32) {
        // 防止窗口完全离开屏幕
        *x = (*x).clamp(-(total_width - 1), self.s_w - 1);
        *y = (*y).clamp(-(total_height - 1), self.s_h - 1);
    }

    /// 约束到监视器边界
    fn constrain_to_monitor(
        &self,
        x: &mut i32,
        y: &mut i32,
        total_width: i32,
        total_height: i32,
        monitor_geometry: &MonitorGeometry,
    ) {
        let MonitorGeometry {
            w_x: wx,
            w_y: wy,
            w_w: ww,
            w_h: wh,
            ..
        } = *monitor_geometry;

        // 防止窗口完全离开监视器
        *x = (*x).clamp(wx - total_width + 1, wx + ww - 1);
        *y = (*y).clamp(wy - total_height + 1, wy + wh - 1);
    }

    /// 应用尺寸提示约束
    fn apply_size_hints_constraints(
        &mut self,
        client_key: ClientKey,
        w: &mut i32,
        h: &mut i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let is_floating = self
            .clients
            .get(client_key)
            .map(|client| client.state.is_floating)
            .unwrap_or(false);

        // 只有在需要时才应用尺寸提示
        if !CONFIG.behavior().resize_hints && !is_floating {
            return Ok(false);
        }

        // 确保尺寸提示有效
        self.ensure_size_hints_valid(client_key)?;

        // 获取尺寸提示
        let hints = if let Some(client) = self.clients.get(client_key) {
            client.size_hints.clone()
        } else {
            return Err("Client not found".into());
        };

        // 应用所有尺寸约束
        let (new_w, new_h) = self.calculate_constrained_size(*w, *h, &hints);
        let changed = *w != new_w || *h != new_h;
        *w = new_w;
        *h = new_h;

        Ok(changed)
    }

    /// 确保尺寸提示有效
    fn ensure_size_hints_valid(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hints_valid = self
            .clients
            .get(client_key)
            .map(|client| client.size_hints.hints_valid)
            .unwrap_or(false);

        if !hints_valid {
            self.updatesizehints(client_key)?;
        }

        Ok(())
    }

    /// 计算受约束的尺寸
    fn calculate_constrained_size(&self, mut w: i32, mut h: i32, hints: &SizeHints) -> (i32, i32) {
        // 1. 应用基础尺寸和增量
        w = self.apply_increments(w - hints.base_w, hints.inc_w) + hints.base_w;
        h = self.apply_increments(h - hints.base_h, hints.inc_h) + hints.base_h;

        // 2. 应用长宽比限制
        (w, h) = self.apply_aspect_ratio_constraints(w, h, hints);

        // 3. 应用最小/最大尺寸限制
        w = w.max(hints.min_w);
        h = h.max(hints.min_h);

        if hints.max_w > 0 {
            w = w.min(hints.max_w);
        }
        if hints.max_h > 0 {
            h = h.min(hints.max_h);
        }

        (w, h)
    }

    /// 应用增量约束
    fn apply_increments(&self, size: i32, increment: i32) -> i32 {
        if increment > 0 {
            (size / increment) * increment
        } else {
            size
        }
    }

    /// 应用长宽比约束
    fn apply_aspect_ratio_constraints(
        &self,
        mut w: i32,
        mut h: i32,
        hints: &SizeHints,
    ) -> (i32, i32) {
        if hints.min_aspect > 0.0 && hints.max_aspect > 0.0 {
            if hints.min_aspect > 0.0 && hints.max_aspect > 0.0 {
                let ratio = w as f32 / h as f32;
                if ratio < hints.min_aspect {
                    w = (h as f32 * hints.min_aspect + 0.5) as i32;
                } else if ratio > hints.max_aspect {
                    h = (w as f32 / hints.max_aspect + 0.5) as i32;
                }
            }
        }
        (w, h)
    }

    fn updatesizehints(&mut self, client_key: ClientKey) -> Result<(), Box<dyn std::error::Error>> {
        let win = self
            .clients
            .get(client_key)
            .map(|c| c.win)
            .ok_or("Client not found")?;
        match self
            .backend
            .property_ops()
            .fetch_normal_hints(WindowId(win.into()))?
        {
            Some(h) => {
                let c = self.clients.get_mut(client_key).ok_or("Client not found")?;
                c.size_hints.base_w = h.base_w;
                c.size_hints.base_h = h.base_h;
                c.size_hints.inc_w = h.inc_w;
                c.size_hints.inc_h = h.inc_h;
                c.size_hints.max_w = h.max_w;
                c.size_hints.max_h = h.max_h;
                c.size_hints.min_w = h.min_w;
                c.size_hints.min_h = h.min_h;
                c.size_hints.min_aspect = h.min_aspect;
                c.size_hints.max_aspect = h.max_aspect;
                c.state.is_fixed =
                    (h.max_w > 0) && (h.max_h > 0) && (h.max_w == h.min_w) && (h.max_h == h.min_h);
                c.size_hints.hints_valid = true;
            }
            None => {
                if let Some(c) = self.clients.get_mut(client_key) {
                    c.size_hints.hints_valid = false;
                }
            }
        }
        Ok(())
    }

    /// 优化后的清理函数 - 只处理必须手动清理的资源
    pub fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup] Starting essential cleanup (letting Rust handle memory)");

        // 2. 清理 X11 相关资源（必须手动处理）
        self.cleanup_x11_resources()?;

        // 3. 清理系统资源（必须手动处理）
        self.cleanup_system_resources()?;

        self.backend.color_allocator().free_all_theme_pixels()?;

        // 4. 同步所有 X11 操作
        self.backend.window_ops().flush()?;

        info!("[cleanup] Essential cleanup completed (Rust will handle the rest)");
        Ok(())
    }

    /// 清理 X11 相关资源
    fn cleanup_x11_resources(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup_x11_resources] Cleaning X11 resources");

        // 清理所有客户端的 X11 状态（恢复窗口到合理状态）
        self.cleanup_all_clients_x11_state()?;

        // 释放按键抓取
        self.cleanup_key_grabs()?;

        // 重置输入焦点到根窗口
        self.reset_input_focus()?;

        // 清理 EWMH 属性
        self.cleanup_ewmh_properties()?;

        info!("[cleanup_x11_resources] X11 resources cleaned");
        Ok(())
    }

    /// 清理系统资源
    fn cleanup_system_resources(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup_system_resources] Cleaning system resources");

        // 终止状态栏进程
        self.cleanup_statusbar_processes()?;

        // 清理共享内存（如果需要显式清理）
        self.cleanup_shared_memory_resources()?;

        info!("[cleanup_system_resources] System resources cleaned");
        Ok(())
    }

    fn cleanup_all_clients_x11_state(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup_all_clients_x11_state]");
        let restarting = self.is_restarting.load(Ordering::SeqCst);

        // 先收集所有需要处理的客户端信息
        let mut clients_to_process = Vec::new();
        for &mon_key in &self.monitor_order {
            if let Some(stack) = self.monitor_stack.get(mon_key) {
                for &ck in stack {
                    if let Some(c) = self.clients.get(ck) {
                        // 收集需要的信息而不是直接操作
                        clients_to_process.push((c.win, c.geometry.old_border_w, ck));
                    }
                }
            }
        }
        // 现在可以安全地进行操作
        for (win, old_border_w, ck) in clients_to_process {
            if let Some(_) = self.clients.get(ck) {
                if restarting {
                    self.backend
                        .window_ops()
                        .ungrab_all_buttons(WindowId(win.into()))?;
                    let mask = EventMaskBits::NONE.bits();
                    self.backend
                        .window_ops()
                        .change_event_mask(WindowId(win.into()), mask)?;
                } else {
                    // 抓取服务器确保操作原子性
                    self.backend.window_ops().grab_server()?;

                    // 正常退出：执行完整恢复
                    let _ = self.restore_client_x11_state(win, old_border_w);

                    // 无论成功失败都要释放服务器
                    let _ = self.backend.window_ops().ungrab_server();
                }
            }
        }

        Ok(())
    }

    fn restore_client_x11_state(
        &mut self,
        win: u32,
        old_border_w: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 清空事件掩码
        if let Err(e) = self
            .backend
            .window_ops()
            .change_event_mask(WindowId(win.into()), EventMaskBits::NONE.bits())
        {
            warn!("Failed to clear events for {}: {:?}", win, e);
        }
        // 恢复边框宽度
        if let Err(e) = self
            .backend
            .window_ops()
            .set_border_width(WindowId(win.into()), old_border_w as u32)
        {
            warn!("Failed to restore border for {}: {:?}", win, e);
        }
        // 取消按钮抓取
        if let Err(e) = self
            .backend
            .window_ops()
            .ungrab_all_buttons(WindowId(win.into()))
        {
            warn!("Failed to ungrab buttons for {}: {:?}", win, e);
        }
        // 设置 Withdrawn 状态（保留原封装）
        if let Err(e) = self.setclientstate(win, WITHDRAWN_STATE as i64) {
            warn!("Failed to set withdrawn state for {}: {:?}", win, e);
        }
        Ok(())
    }

    /// 清理状态栏进程
    fn cleanup_statusbar_processes(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut child = if let Some(child) = self.status_bar_child.take() {
            child
        } else {
            return Ok(());
        };
        // 获取进程 ID
        let pid = child.id();
        let nix_pid = Pid::from_raw(pid as i32);
        // 检查进程是否存在
        match signal::kill(nix_pid, None) {
            Err(_) => {
                // 进程已经不存在
                info!("Process already terminated",);
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
                        info!("Process exited gracefully: {:?}", status);
                        return Ok(());
                    }
                    Ok(None) => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(_) => {
                        return Err("Error waiting".into());
                    }
                }
            }
            // 超时后强制终止
            warn!("Graceful termination timeout, forcing kill");
        }
        // 强制终止
        self.status_bar_pid = None;
        signal::kill(nix_pid, Signal::SIGKILL)?;

        Ok(())
    }

    /// 清理共享内存资源
    fn cleanup_shared_memory_resources(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(rb) = self.status_bar_shmem.take() {
            drop(rb);
        }
        #[cfg(unix)]
        {
            if std::path::Path::new(&SHARED_PATH).exists() {
                if let Err(e) = std::fs::remove_file(&SHARED_PATH) {
                    warn!("Failed to remove {}: {}", SHARED_PATH, e);
                }
            }
        }
        Ok(())
    }

    fn cleanup_key_grabs(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Err(e) = self
            .backend
            .key_ops()
            .clear_key_grabs(self.backend.root_window())
        {
            warn!("[cleanup_key_grabs] Failed to ungrab keys: {:?}", e);
        }
        Ok(())
    }

    fn reset_input_focus(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.backend
            .window_ops()
            .set_input_focus_root(self.backend.root_window())?;
        self.backend.window_ops().flush()?;
        Ok(())
    }

    fn cleanup_ewmh_properties(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(facade) = self.backend.ewmh_facade().as_ref() {
            let _ = facade.reset_root_properties(); // 后端内部清理 _NET_ACTIVE_WINDOW/_NET_CLIENT_LIST/_NET_SUPPORTED
        }
        Ok(())
    }

    fn configurenotify(
        &mut self,
        window: u32,
        _x: i16,
        _y: i16,
        w: u16,
        h: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 检查是否是根窗口的配置变更
        if window == self.backend.root_window().0 as u32 {
            let dirty = self.s_w != w as i32 || self.s_h != h as i32;
            self.s_w = w as i32;
            self.s_h = h as i32;
            if self.updategeom() || dirty {
                self.handle_screen_geometry_change()?;
            }
        }

        Ok(())
    }

    fn handle_screen_geometry_change(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[handle_screen_geometry_change]");
        // 遍历所有显示器
        for &mon_key in self.monitor_order.clone().iter() {
            self.update_fullscreen_clients_on_monitor(mon_key)?;
        }
        // 重新聚焦和排列
        self.focus(None)?;
        self.arrange(None);
        Ok(())
    }

    fn update_fullscreen_clients_on_monitor(
        &mut self,
        mon_key: MonitorKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 获取监视器几何信息
        let monitor_geometry = if let Some(monitor) = self.monitors.get(mon_key) {
            (
                monitor.geometry.m_x,
                monitor.geometry.m_y,
                monitor.geometry.m_w,
                monitor.geometry.m_h,
            )
        } else {
            warn!(
                "[update_fullscreen_clients_on_monitor] Monitor {:?} not found",
                mon_key
            );
            return Ok(());
        };

        // 收集该监视器上的全屏客户端
        let fullscreen_clients: Vec<ClientKey> =
            if let Some(client_keys) = self.monitor_clients.get(mon_key) {
                client_keys
                    .iter()
                    .filter(|&&client_key| {
                        self.clients
                            .get(client_key)
                            .map(|client| client.state.is_fullscreen)
                            .unwrap_or(false)
                    })
                    .copied()
                    .collect()
            } else {
                Vec::new()
            };

        // 调整全屏客户端到新的显示器尺寸
        for client_key in fullscreen_clients {
            let _ = self.resizeclient(
                client_key,
                monitor_geometry.0,
                monitor_geometry.1,
                monitor_geometry.2,
                monitor_geometry.3,
            );
        }
        Ok(())
    }

    fn set_window_border_width(
        &self,
        window: u32,
        border_width: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.backend
            .window_ops()
            .set_border_width(WindowId(window.into()), border_width)?;
        Ok(())
    }

    fn set_window_border_color(
        &mut self,
        window: u32,
        selected: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let scheme_type = if selected {
            SchemeType::Sel
        } else {
            SchemeType::Norm
        };
        if let Ok(pixel) = self
            .backend
            .color_allocator()
            .get_border_pixel_of(scheme_type)
        {
            self.backend
                .window_ops()
                .set_border_pixel(WindowId(window.into()), pixel.0)?;
        }
        Ok(())
    }

    fn grabkeys(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 探测 NumLock（KeyOps）
        self.setup_modifier_masks()?;

        // 清除旧的抓取
        self.backend
            .key_ops()
            .clear_key_grabs(self.backend.root_window())?;

        // 构造绑定列表（通用 Mods + KeySym）
        let bindings: Vec<(Mods, KeySym)> = CONFIG
            .get_keys()
            .iter()
            .map(|k| (k.mask, k.key_sym))
            .collect();

        // 抓取
        self.backend.key_ops().grab_keys(
            self.backend.root_window(),
            &bindings,
            self.numlock_mask_bits,
        )?;

        Ok(())
    }

    fn setfullscreen(
        &mut self,
        client_key: ClientKey,
        fullscreen: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        let is_fullscreen = self
            .clients
            .get(client_key)
            .map(|client| client.state.is_fullscreen)
            .unwrap_or(false);

        if fullscreen && !is_fullscreen {
            // 设置全屏逻辑
            self.backend
                .property_ops()
                .set_fullscreen_state(WindowId(win.into()), fullscreen)?;

            // 更新客户端状态
            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.is_fullscreen = true;
                client.state.old_state = client.state.is_floating;
                client.geometry.old_border_w = client.geometry.border_w;
                client.geometry.border_w = 0;
                client.state.is_floating = true;
            }

            // 获取监视器信息并调整窗口大小
            if let Some(mon_key) = self.clients.get(client_key).and_then(|c| c.mon) {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    let (mx, my, mw, mh) = (
                        monitor.geometry.m_x,
                        monitor.geometry.m_y,
                        monitor.geometry.m_w,
                        monitor.geometry.m_h,
                    );
                    self.resizeclient(client_key, mx, my, mw, mh)?;
                }
            }

            // 提升窗口到顶层
            self.backend
                .window_ops()
                .configure_stack_above(WindowId(win.into()), None)?;
            self.backend.window_ops().flush()?;
        } else if !fullscreen && is_fullscreen {
            // 取消全屏逻辑
            self.backend
                .property_ops()
                .set_fullscreen_state(WindowId(win.into()), fullscreen)?;

            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.is_fullscreen = false;
                client.state.is_floating = client.state.old_state;
                client.geometry.border_w = client.geometry.old_border_w;
                client.geometry.x = client.geometry.old_x;
                client.geometry.y = client.geometry.old_y;
                client.geometry.w = client.geometry.old_w;
                client.geometry.h = client.geometry.old_h;
            }

            // 恢复窗口大小
            let (x, y, w, h) = if let Some(client) = self.clients.get(client_key) {
                (
                    client.geometry.x,
                    client.geometry.y,
                    client.geometry.w,
                    client.geometry.h,
                )
            } else {
                return Ok(());
            };

            self.resizeclient(client_key, x, y, w, h)?;

            // 重新排列
            if let Some(mon_key) = self.clients.get(client_key).and_then(|c| c.mon) {
                self.arrange(Some(mon_key));
            }
        }

        Ok(())
    }

    /// 更新 seturgent 方法签名
    fn seturgent(
        &mut self,
        client_key: ClientKey,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 更新客户端状态
        if let Some(client) = self.clients.get_mut(client_key) {
            client.state.is_urgent = urgent;
        } else {
            return Err("Client not found".into());
        }

        // 获取窗口ID
        let win = self
            .clients
            .get(client_key)
            .map(|client| client.win)
            .ok_or("Client not found after update")?;

        self.set_urgent_flag(win, urgent)?;

        Ok(())
    }

    fn set_urgent_flag(&self, win: u32, urgent: bool) -> Result<(), Box<dyn std::error::Error>> {
        self.backend
            .property_ops()
            .set_urgent_hint(WindowId(win as u64), urgent)
    }

    /// 显示/隐藏指定监视器上的窗口
    fn showhide_monitor(&mut self, mon_key: MonitorKey) {
        // 获取该监视器的堆栈顺序客户端列表
        if let Some(stack_clients) = self.monitor_stack.get(mon_key).cloned() {
            for client_key in stack_clients {
                self.showhide_client(client_key, mon_key);
            }
        }
    }

    /// 显示/隐藏单个客户端
    fn showhide_client(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        let is_visible = self.is_client_visible_on_monitor(client_key, mon_key);

        if is_visible {
            self.show_client(client_key);
        } else {
            self.hide_client(client_key);
        }
    }

    /// 显示客户端（SlotMap版本）
    fn show_client(&mut self, client_key: ClientKey) {
        let (win, x, y, is_floating, is_fullscreen) =
            if let Some(client) = self.clients.get(client_key) {
                (
                    client.win,
                    client.geometry.x,
                    client.geometry.y,
                    client.state.is_floating,
                    client.state.is_fullscreen,
                )
            } else {
                warn!("[show_client] Client {:?} not found", client_key);
                return;
            };

        // 移动窗口到可见位置
        if let Err(e) = self.move_window(win, x, y) {
            warn!("[show_client] Failed to move window {}: {:?}", win, e);
        }

        // 如果是浮动窗口且非全屏，调整大小
        if is_floating && !is_fullscreen {
            let (w, h) = if let Some(client) = self.clients.get(client_key) {
                (client.geometry.w, client.geometry.h)
            } else {
                return;
            };
            self.resize_client(client_key, x, y, w, h, false);
        }
    }

    /// 隐藏客户端（SlotMap版本）
    fn hide_client(&mut self, client_key: ClientKey) {
        let (win, y, width) = if let Some(client) = self.clients.get(client_key) {
            (client.win, client.geometry.y, client.total_width())
        } else {
            warn!("[hide_client] Client {:?} not found", client_key);
            return;
        };

        // 将窗口移动到屏幕外隐藏
        let hidden_x = width * -2;
        if let Err(e) = self.move_window(win, hidden_x, y) {
            warn!("[hide_client] Failed to hide window {}: {:?}", win, e);
        }
    }

    fn resize_client(
        &mut self,
        client_key: ClientKey,
        mut x: i32,
        mut y: i32,
        mut w: i32,
        mut h: i32,
        interact: bool,
    ) {
        if self
            .applysizehints(client_key, &mut x, &mut y, &mut w, &mut h, interact)
            .is_ok()
        {
            let _ = self.resizeclient(client_key, x, y, w, h);
        }
    }

    fn resizeclient(
        &mut self,
        client_key: ClientKey,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get_mut(client_key) {
            client.geometry.old_x = client.geometry.x;
            client.geometry.old_y = client.geometry.y;
            client.geometry.old_w = client.geometry.w;
            client.geometry.old_h = client.geometry.h;

            client.geometry.x = x;
            client.geometry.y = y;
            client.geometry.w = w;
            client.geometry.h = h;

            self.backend.window_ops().configure_xywh_border(
                WindowId(client.win.into()),
                Some(x),
                Some(y),
                Some(w as u32),
                Some(h as u32),
                Some(client.geometry.border_w as u32),
            )?;
            self.configure_client(client_key)?;
            self.backend.window_ops().flush()?;
        }
        Ok(())
    }

    fn configure_client(&self, client_key: ClientKey) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            self.backend.window_ops().send_configure_notify(
                WindowId(client.win.into()),
                client.geometry.x as i16,
                client.geometry.y as i16,
                client.geometry.w as u16,
                client.geometry.h as u16,
                client.geometry.border_w as u16,
            )?;
        }
        Ok(())
    }

    fn move_window(&mut self, win: u32, x: i32, y: i32) -> Result<(), Box<dyn std::error::Error>> {
        self.backend.window_ops().configure_xywh_border(
            WindowId(win.into()),
            Some(x),
            Some(y),
            None,
            None,
            None,
        )?;
        self.backend.window_ops().flush()?;
        Ok(())
    }

    fn createmon(&mut self, show_bar: bool) -> WMMonitor {
        // info!("[createmon]");
        let mut m: WMMonitor = WMMonitor::new();
        m.tag_set[0] = 1;
        m.tag_set[1] = 1;
        m.layout.m_fact = CONFIG.m_fact();
        m.layout.n_master = CONFIG.n_master();
        m.lt[0] = Rc::new(LayoutEnum::TILE);
        m.lt[1] = Rc::new(LayoutEnum::FLOAT);
        m.lt_symbol = m.lt[0].symbol().to_string();
        m.pertag = Some(Pertag::new(show_bar));
        let ref_pertag = m.pertag.as_mut().unwrap();
        ref_pertag.cur_tag = 1;
        ref_pertag.prev_tag = 1;
        let default_layout_0 = m.lt[0].clone();
        let default_layout_1 = m.lt[1].clone();
        for i in 0..=CONFIG.tags_length() {
            ref_pertag.n_masters[i] = m.layout.n_master;
            ref_pertag.m_facts[i] = m.layout.m_fact;

            ref_pertag.lt_idxs[i][0] = Some(default_layout_0.clone());
            ref_pertag.lt_idxs[i][1] = Some(default_layout_1.clone());
            ref_pertag.sel_lts[i] = m.sel_lt;
        }
        info!("[createmon]: {}", m);
        return m;
    }

    fn enter_notify(
        &mut self,
        _root: u32,
        event_window: u32,
        mode: u8,
        detail: u8,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // NOTE: X11 语义：mode=0(NORMAL), detail=2(INFERIOR)
        if (mode != 0 || detail == 2) && event_window != self.backend.root_window().0 as u32 {
            return Ok(());
        }
        // 检查是否进入状态栏
        if self.handle_statusbar_enter_generic(event_window)? {
            return Ok(());
        }
        self.handle_regular_enter_generic(event_window)?;
        Ok(())
    }

    fn handle_statusbar_enter_generic(
        &mut self,
        event_window: u32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if Some(event_window) == self.status_bar_window {
            if let Some(cur_bar_mon_id) = self.current_bar_monitor_id {
                if let Some(target_monitor_key) = self.get_monitor_by_id(cur_bar_mon_id) {
                    if Some(target_monitor_key) != self.sel_mon {
                        let current_sel = self.get_selected_client_key();
                        self.unfocus_client_opt(current_sel, true)?;
                        self.sel_mon = Some(target_monitor_key);
                        self.focus(None)?;
                    }
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn handle_regular_enter_generic(
        &mut self,
        event_window: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_key_opt = self.wintoclient(event_window);
        let monitor_key_opt = if let Some(client_key) = client_key_opt {
            self.clients.get(client_key).and_then(|client| client.mon)
        } else {
            self.wintomon(event_window)
        };
        let current_event_monitor_key = match monitor_key_opt {
            Some(monitor_key) => monitor_key,
            None => return Ok(()),
        };
        let is_on_selected_monitor = Some(current_event_monitor_key) == self.sel_mon;
        if !is_on_selected_monitor {
            self.switch_to_monitor(current_event_monitor_key)?;
        }
        if self.should_focus_client(client_key_opt, is_on_selected_monitor) {
            self.focus(client_key_opt)?;
        }
        Ok(())
    }

    fn destroynotify(&mut self, window: u32) -> Result<(), Box<dyn std::error::Error>> {
        let c = self.wintoclient(window);
        if c.is_some() {
            self.unmanage(c, true)?;
        }
        Ok(())
    }

    fn arrangemon(&mut self, mon_key: MonitorKey) {
        info!("[arrangemon]");

        // 获取布局类型和更新符号
        let (layout_type, layout_symbol) = if let Some(monitor) = self.monitors.get(mon_key) {
            let sel_lt = monitor.sel_lt;
            let layout = &monitor.lt[sel_lt];
            (layout.clone(), layout.symbol().to_string())
        } else {
            warn!("[arrangemon] Monitor {:?} not found", mon_key);
            return;
        };

        // 更新布局符号
        if let Some(monitor) = self.monitors.get_mut(mon_key) {
            monitor.lt_symbol = layout_symbol;
            info!(
                "[arrangemon] sel_lt: {}, ltsymbol: {:?}",
                monitor.sel_lt, monitor.lt_symbol
            );
        }

        // 应用布局
        match *layout_type {
            LayoutEnum::TILE => self.tile(mon_key),
            LayoutEnum::MONOCLE => self.monocle(mon_key),
            LayoutEnum::FLOAT | _ => {}
        }
    }

    fn dirtomon(&mut self, dir: &i32) -> Option<MonitorKey> {
        let selected_monitor_key = self.sel_mon?; // Return None if sel_mon is None
        if self.monitor_order.is_empty() {
            return None;
        }
        // 找到当前选中监视器在顺序列表中的位置
        let current_index = self
            .monitor_order
            .iter()
            .position(|&key| key == selected_monitor_key)?;
        if *dir > 0 {
            // Next monitor (向前)
            let next_index = (current_index + 1) % self.monitor_order.len();
            Some(self.monitor_order[next_index])
        } else {
            // Previous monitor (向后)
            let prev_index = if current_index == 0 {
                self.monitor_order.len() - 1 // 循环到最后一个
            } else {
                current_index - 1
            };
            Some(self.monitor_order[prev_index])
        }
    }

    fn ensure_bar_is_running(&mut self, shared_path: &str) {
        if let Some(child) = self.status_bar_child.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                return; // 仍在运行
            }
            self.status_bar_child = None;
            self.status_bar_pid = None;
        }

        let mut command = if cfg!(feature = "nixgl") {
            let mut cmd = Command::new("nixGL");
            cmd.arg(CONFIG.status_bar_name());
            cmd
        } else {
            Command::new(CONFIG.status_bar_name())
        };
        command.arg(shared_path);

        if let Ok(child) = command.spawn() {
            self.status_bar_pid = Some(child.id());
            self.status_bar_child = Some(child);
        }
    }

    fn restack(
        &mut self,
        mon_key_opt: Option<MonitorKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[restack]");

        let mon_key = mon_key_opt.ok_or("Monitor is required for restack operation")?;
        let monitor = self.monitors.get(mon_key).ok_or("Monitor not found")?;
        let monitor_num = monitor.num;

        // 1) 从顶部到下的栈
        let stack = self.get_monitor_stack(mon_key);

        // 2) 分离 tiled 与 floating（仅可见）
        let mut tiled_bottom_to_top: Vec<u32> = Vec::new();
        let mut floating_bottom_to_top: Vec<u32> = Vec::new();

        for &ck in stack.iter().rev() {
            if let Some(c) = self.clients.get(ck) {
                if !self.is_client_visible_on_monitor(ck, mon_key) {
                    continue;
                }
                if c.state.is_floating {
                    floating_bottom_to_top.push(c.win);
                } else {
                    tiled_bottom_to_top.push(c.win);
                }
            }
        }

        // 3) 选中的浮动窗口置顶
        if let Some(sel_ck) = monitor.sel {
            if let Some(sel_c) = self.clients.get(sel_ck) {
                if sel_c.state.is_floating {
                    if let Some(idx) = floating_bottom_to_top.iter().position(|&w| w == sel_c.win) {
                        let w = floating_bottom_to_top.remove(idx);
                        floating_bottom_to_top.push(w);
                    }
                }
            }
        }

        // 4) 最终顺序（底->顶）
        let mut final_bottom_to_top: Vec<u32> =
            Vec::with_capacity(tiled_bottom_to_top.len() + floating_bottom_to_top.len());
        final_bottom_to_top.extend(tiled_bottom_to_top.into_iter());
        final_bottom_to_top.extend(floating_bottom_to_top.into_iter());

        // 5) 如果顺序未变化，跳过
        let need_restack_windows = match self.last_stacking.get(mon_key) {
            Some(prev) => prev.as_slice() != final_bottom_to_top.as_slice(),
            None => true,
        };

        if need_restack_windows {
            for i in 0..final_bottom_to_top.len() {
                let win = final_bottom_to_top[i];
                let sibling = if i > 0 {
                    Some(WindowId(final_bottom_to_top[i - 1].into()))
                } else {
                    None
                };
                self.backend
                    .window_ops()
                    .configure_stack_above(WindowId(win.into()), sibling)?;
            }
            self.last_stacking
                .insert(mon_key, final_bottom_to_top.clone());
        }

        // 6) bar 置顶（若显示）
        if self.current_bar_monitor_id == Some(monitor_num) {
            if let Some(bar_key) = self.status_bar_client {
                if let Some(bar_client) = self.clients.get(bar_key) {
                    let show_bar = monitor
                        .pertag
                        .as_ref()
                        .and_then(|p| p.show_bars.get(p.cur_tag))
                        .copied()
                        .unwrap_or(true);
                    if show_bar {
                        self.backend
                            .window_ops()
                            .configure_stack_above(WindowId(bar_client.win.into()), None)?;
                    }
                }
            }
        }

        self.backend.window_ops().flush()?;
        self.mark_bar_update_needed_if_visible(Some(monitor_num));

        info!("[restack] finish");
        Ok(())
    }

    fn flush_pending_bar_updates(&mut self) {
        if self.pending_bar_updates.is_empty() {
            return;
        }

        // 选择目标 monitor
        let target_mon_id = self
            .current_bar_monitor_id
            .or_else(|| {
                self.sel_mon
                    .and_then(|k| self.monitors.get(k))
                    .map(|m| m.num)
            })
            .or_else(|| self.pending_bar_updates.iter().copied().next());

        if let Some(mon_id) = target_mon_id {
            if let Some(mon_key) = self.get_monitor_by_id(mon_id) {
                if !self.is_bar_visible_on_mon(mon_key) {
                    self.pending_bar_updates.clear();
                    return;
                }

                // 1) 构造消息（更新 self.message）
                self.update_bar_message_for_monitor(Some(mon_key));

                // 2) 序列化用于差异比较
                let payload = match bincode::encode_to_vec(&self.message, standard()) {
                    Ok(v) => v,
                    Err(_) => {
                        self.pending_bar_updates.clear();
                        return;
                    }
                };

                // 3) 去抖：时间间隔
                let now = std::time::Instant::now();
                if let Some(last) = self.last_bar_update_at {
                    if now.duration_since(last) < self.bar_min_interval {
                        // 未到发送间隔，先保留 pending，下个 tick 再发
                        return;
                    }
                }

                // 4) 差异比较：相同则跳过
                if self.last_bar_payload.as_ref().map(|p| &**p) == Some(&payload[..]) {
                    self.pending_bar_updates.clear();
                    return;
                }

                // 5) 确保 ring buffer 与进程
                if self.status_bar_shmem.is_none() {
                    let ring_buffer = SharedRingBuffer::create_aux(SHARED_PATH, None, None)
                        .expect("Create bar shmem failed");
                    info!("Create bar shmem");
                    self.status_bar_shmem = Some(ring_buffer);
                }
                self.ensure_bar_is_running(SHARED_PATH);

                // 6) 写消息
                if let Some(rb) = self.status_bar_shmem.as_mut() {
                    let _ = rb.try_write_message(&self.message);
                }

                // 7) 记录发送状态
                self.last_bar_payload = Some(payload);
                self.last_bar_update_at = Some(now);
            }
        }

        self.pending_bar_updates.clear();
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 选择运行模式
        if env::var("JWM_USE_SYNC").is_ok() {
            self.run_sync()
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(self.run_async())
        }
    }

    pub async fn run_async(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 后端 flush，确保挂起请求发出
        self.backend.event_source().flush()?;
        let mut event_count: u64 = 0;

        // 定时器用于节拍处理（状态栏等）
        let mut update_timer = tokio::time::interval(std::time::Duration::from_millis(10));

        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            // 抽干所有可用事件
            while let Some(ev) = self.backend.event_source().poll_event()? {
                event_count = event_count.wrapping_add(1);
                let _ = self.handle_backend_event(ev);
            }

            // 处理状态栏命令与待更新
            self.process_commands_from_status_bar();
            if !self.pending_bar_updates.is_empty() {
                self.flush_pending_bar_updates();
            }

            // 等待下一个 tick
            tokio::select! {
                _ = update_timer.tick() => {
                    self.process_commands_from_status_bar();
                    if !self.pending_bar_updates.is_empty() {
                        self.flush_pending_bar_updates();
                    }
                }
            }
        }
        Ok(())
    }

    fn run_sync(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 使用简化的阻塞循环 + 小睡眠，完全走 backend 事件源
        self.backend.event_source().flush()?;
        let mut event_count: u64 = 0;

        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            // 抽干所有可用事件
            while let Some(ev) = self.backend.event_source().poll_event()? {
                event_count = event_count.wrapping_add(1);
                let _ = self.handle_backend_event(ev);
            }

            // 处理状态栏命令与待更新
            self.process_commands_from_status_bar();
            if !self.pending_bar_updates.is_empty() {
                self.flush_pending_bar_updates();
            }

            // 轻微退避，避免 busy loop
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(())
    }

    fn process_commands_from_status_bar(&mut self) {
        // 创建一个临时向量来收集所有命令
        let mut commands_to_process: Vec<SharedCommand> = Vec::new();
        // 第一步：遍历共享内存缓冲区并收集命令
        if let Some(buffer) = self.status_bar_shmem.as_mut() {
            while let Some(cmd) = buffer.receive_command() {
                commands_to_process.push(cmd);
            }
        }
        // 第二步：处理收集到的命令
        for cmd in commands_to_process {
            match cmd.cmd_type.into() {
                CommandType::ViewTag => {
                    // 切换到指定标签
                    info!(
                        "[process_commands] ViewTag command received: {}",
                        cmd.parameter
                    );
                    let arg = WMArgEnum::UInt(cmd.parameter);
                    let _ = self.view(&arg);
                }
                CommandType::ToggleTag => {
                    // 切换标签
                    info!(
                        "[process_commands] ToggleTag command received: {}",
                        cmd.parameter
                    );
                    let arg = WMArgEnum::UInt(cmd.parameter);
                    let _ = self.toggletag(&arg);
                }
                CommandType::SetLayout => {
                    // 设置布局
                    info!(
                        "[process_commands] SetLayout command received: {}",
                        cmd.parameter
                    );
                    let arg = WMArgEnum::Layout(Rc::new(LayoutEnum::from(cmd.parameter)));
                    let _ = self.setlayout(&arg);
                }
                CommandType::None => {}
            }
        }
    }

    pub fn scan(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let tree_child = self
            .backend
            .window_ops()
            .get_tree_child(self.backend.root_window())?;
        let mut cookies = Vec::with_capacity(tree_child.len());
        for win in tree_child {
            let attr = self.backend.window_ops().get_window_attributes(win)?;
            let geom = self.backend.window_ops().get_geometry_translated(win)?;
            let trans = self.get_transient_for(win.0 as u32);
            cookies.push((win, attr, geom, trans));
        }
        for (win, attr, geom, trans) in &cookies {
            if attr.override_redirect || trans.is_some() {
                continue;
            }
            if attr.map_state_viewable
                || self
                    .backend
                    .property_ops()
                    .get_wm_state(*win)
                    .map_or(false, |s| s == ICONIC_STATE.into())
            {
                self.manage(win.0 as u32, geom)?;
            }
        }
        for (win, attr, geom, trans) in &cookies {
            if trans.is_some() {
                if attr.map_state_viewable
                    || self
                        .backend
                        .property_ops()
                        .get_wm_state(*win)
                        .map_or(false, |s| s == ICONIC_STATE.into())
                {
                    self.manage(win.0 as u32, geom)?;
                }
            }
        }
        Ok(())
    }

    fn arrange(&mut self, m_target: Option<MonitorKey>) {
        info!("[arrange]");

        // 确定要操作的监视器
        let monitors_to_process: Vec<MonitorKey> = match m_target {
            Some(monitor_key) => vec![monitor_key], // 操作单个监视器
            None => self.monitor_order.clone(),     // 操作所有监视器
        };

        // Phase 1: Show/Hide windows for each targeted monitor
        for &mon_key in &monitors_to_process {
            self.showhide_monitor(mon_key);
        }

        // Phase 2: Arrange layout and restack for each targeted monitor
        for &mon_key in &monitors_to_process {
            self.arrangemon(mon_key);
            let _ = self.restack(Some(mon_key));
        }
    }

    fn getrootptr(&mut self) -> Result<(i32, i32), Box<dyn std::error::Error>> {
        let (x, y, _mask, _unused) = self.backend.input_ops().query_pointer_root()?;
        Ok((x, y))
    }

    fn recttomon(&mut self, x: i32, y: i32, w: i32, h: i32) -> Option<MonitorKey> {
        // info!("[recttomon]");
        let mut max_area = 0;
        let mut result_monitor = self.sel_mon;

        for &mon_key in &self.monitor_order {
            if let Some(monitor) = self.monitors.get(mon_key) {
                let area = monitor.intersect_area(x, y, w, h);
                if area > max_area {
                    max_area = area;
                    result_monitor = Some(mon_key);
                }
            }
        }

        result_monitor
    }

    fn wintomon(&mut self, w: u32) -> Option<MonitorKey> {
        // 处理根窗口
        if w == self.backend.root_window().0 as u32 {
            match self.getrootptr() {
                Ok((x, y)) => return self.recttomon(x, y, 1, 1),
                Err(e) => {
                    warn!("[wintomon] Failed to get root pointer: {:?}", e);
                    return self.sel_mon;
                }
            }
        }

        // 查找客户端对应的监视器
        match self.wintoclient(w) {
            Some(client_key) => match self.clients.get(client_key) {
                Some(client) => client.mon.or(self.sel_mon),
                None => {
                    warn!(
                        "[wintomon] Client key {:?} not found in clients",
                        client_key
                    );
                    self.sel_mon
                }
            },
            None => {
                info!(
                    "[wintomon] Window {} not managed, returning selected monitor",
                    w
                );
                self.sel_mon
            }
        }
    }

    pub fn checkotherwm(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mask_bits = EventMaskBits::SUBSTRUCTURE_REDIRECT.bits();
        let root = self.backend.root_window();
        match self.backend.window_ops().change_event_mask(root, mask_bits) {
            Ok(_) => {
                info!("[checkotherwm] SubstructureRedirect acquired, no other WM running");
                Ok(())
            }
            Err(e) => {
                error!(
                    "[checkotherwm] Failed to acquire SubstructureRedirect: {:?}",
                    e
                );
                eprintln!("jwm: another window manager may already be running");
                std::process::exit(1);
            }
        }
    }

    pub fn spawn(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[spawn]");

        let mut mut_arg: WMArgEnum = arg.clone();
        if let WMArgEnum::StringVec(ref mut v) = mut_arg {
            // 处理 dmenu 命令的特殊情况
            if *v == *CONFIG.get_dmenucmd() {
                let monitor_num = self.get_sel_mon().unwrap().num;
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
            use std::os::unix::process::CommandExt;

            unsafe {
                command.pre_exec(move || {
                    setsid();
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

    fn tile(&mut self, mon_key: MonitorKey) {
        info!("[tile]");

        // 获取监视器基本信息
        let (wx, wy, ww, wh, mfact, nmaster, monitor_num, client_y_offset) =
            self.get_monitor_info(mon_key);

        // 收集所有可平铺的客户端
        let clients = self.collect_tileable_clients(mon_key);

        if clients.is_empty() {
            return;
        }

        info!(
            "[tile] monitor_num: {}, clients: {}",
            monitor_num,
            clients.len()
        );

        // 计算布局参数
        let (mw, mfacts, sfacts) = self.calculate_layout_params(&clients, ww, mfact, nmaster);

        // 安排客户端位置
        self.arrange_clients(
            &clients,
            wx,
            wy,
            ww,
            wh,
            mw,
            mfacts,
            sfacts,
            nmaster,
            client_y_offset,
        );
    }

    // 获取监视器基本信息
    fn get_monitor_info(&self, mon_key: MonitorKey) -> (i32, i32, i32, i32, f32, u32, i32, i32) {
        if let Some(monitor) = self.monitors.get(mon_key) {
            let client_y_offset = self.get_client_y_offset(monitor);
            (
                monitor.geometry.w_x,
                monitor.geometry.w_y,
                monitor.geometry.w_w,
                monitor.geometry.w_h,
                monitor.layout.m_fact,
                monitor.layout.n_master,
                monitor.num,
                client_y_offset,
            )
        } else {
            warn!("[get_monitor_info] Monitor {:?} not found", mon_key);
            (0, 0, 0, 0, 0.55, 1, 0, 0) // 默认值
        }
    }

    // 收集所有可平铺的客户端
    fn collect_tileable_clients(&self, mon_key: MonitorKey) -> Vec<(ClientKey, f32, i32)> {
        let mut clients = Vec::new();
        let mut current_client = self.nexttiled(mon_key, None);

        while let Some(client_key) = current_client {
            if let Some(client) = self.clients.get(client_key) {
                let client_fact = client.state.client_fact;
                let border_w = client.geometry.border_w;

                clients.push((client_key, client_fact, border_w));

                // 找下一个平铺客户端
                current_client = self.nexttiled(mon_key, Some(client_key));
            } else {
                break;
            }
        }

        clients
    }

    // 计算布局参数
    fn calculate_layout_params(
        &self,
        clients: &[(ClientKey, f32, i32)],
        ww: i32,
        mfact: f32,
        nmaster: u32,
    ) -> (i32, f32, f32) {
        let n = clients.len() as u32;

        // 计算主区域和堆栈区域的cfact总和
        let (mfacts, sfacts) = clients.iter().enumerate().fold(
            (0.0, 0.0),
            |(mfacts, sfacts), (i, (_, client_fact, _))| {
                if i < nmaster as usize {
                    (mfacts + client_fact, sfacts)
                } else {
                    (mfacts, sfacts + client_fact)
                }
            },
        );

        // 计算主区域宽度
        let mw = if n > nmaster && nmaster > 0 {
            (ww as f32 * mfact) as i32
        } else {
            ww
        };

        (mw, mfacts, sfacts)
    }

    // 安排客户端位置
    fn arrange_clients(
        &mut self,
        clients: &[(ClientKey, f32, i32)],
        wx: i32,
        wy: i32,
        ww: i32,
        wh: i32,
        mw: i32,
        mfacts: f32,
        sfacts: f32,
        nmaster: u32,
        client_y_offset: i32,
    ) {
        let available_height = wh - client_y_offset;
        let mut my = 0i32; // 主区域Y偏移
        let mut ty = 0i32; // 堆栈区域Y偏移
        let mut remaining_mfacts = mfacts;
        let mut remaining_sfacts = sfacts;

        for (i, &(client_key, client_fact, border_w)) in clients.iter().enumerate() {
            let is_master = i < nmaster as usize;

            let (x, y, w, h) = if is_master {
                self.calculate_master_geometry(
                    wx,
                    wy,
                    mw,
                    available_height,
                    client_y_offset,
                    client_fact,
                    border_w,
                    i,
                    nmaster,
                    &mut my,
                    &mut remaining_mfacts,
                )
            } else {
                self.calculate_stack_geometry(
                    wx,
                    wy,
                    ww,
                    mw,
                    available_height,
                    client_y_offset,
                    client_fact,
                    border_w,
                    i,
                    nmaster,
                    clients.len(),
                    &mut ty,
                    &mut remaining_sfacts,
                )
            };

            // 调整客户端大小
            self.resize_client(client_key, x, y, w, h, false);
        }
    }

    // 计算主区域窗口几何形状（保持不变）
    fn calculate_master_geometry(
        &self,
        wx: i32,
        wy: i32,
        mw: i32,
        available_height: i32,
        client_y_offset: i32,
        client_fact: f32,
        border_w: i32,
        index: usize,
        nmaster: u32,
        my: &mut i32,
        remaining_mfacts: &mut f32,
    ) -> (i32, i32, i32, i32) {
        let remaining_masters = nmaster - index as u32;
        let remaining_height = (available_height - *my).max(0);

        let height = if *remaining_mfacts > 0.001 {
            (remaining_height as f32 * (client_fact / *remaining_mfacts)) as i32
        } else if remaining_masters > 0 {
            remaining_height / remaining_masters as i32
        } else {
            remaining_height
        };

        *my += height;
        *remaining_mfacts -= client_fact;

        (
            wx,
            wy + *my - height + client_y_offset,
            mw - 2 * border_w,
            height - 2 * border_w,
        )
    }

    // 计算堆栈区域窗口几何形状（保持不变）
    fn calculate_stack_geometry(
        &self,
        wx: i32,
        wy: i32,
        ww: i32,
        mw: i32,
        available_height: i32,
        client_y_offset: i32,
        client_fact: f32,
        border_w: i32,
        index: usize,
        nmaster: u32,
        total_clients: usize,
        ty: &mut i32,
        remaining_sfacts: &mut f32,
    ) -> (i32, i32, i32, i32) {
        let stack_index = index - nmaster as usize;
        let stack_count = total_clients - nmaster as usize;
        let remaining_stacks = stack_count - stack_index;
        let remaining_height = (available_height - *ty).max(0);

        let height = if *remaining_sfacts > 0.001 {
            (remaining_height as f32 * (client_fact / *remaining_sfacts)) as i32
        } else if remaining_stacks > 0 {
            remaining_height / remaining_stacks as i32
        } else {
            remaining_height
        };

        *ty += height;
        *remaining_sfacts -= client_fact;

        (
            wx + mw,
            wy + *ty - height + client_y_offset,
            ww - mw - 2 * border_w,
            height - 2 * border_w,
        )
    }

    fn get_client_y_offset(&self, monitor: &WMMonitor) -> i32 {
        // 只按该 monitor 当前 tag 的 show_bars 决定是否保留顶部 gap
        let show_bar = monitor
            .pertag
            .as_ref()
            .and_then(|p| p.show_bars.get(p.cur_tag))
            .copied()
            .unwrap_or(true);

        if show_bar {
            CONFIG.status_bar_height() + CONFIG.status_bar_padding() * 2
        } else {
            0
        }
    }

    pub fn togglefloating(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[togglefloating]");
        let sel_mon_key = match self.sel_mon {
            Some(key) => key,
            None => return Ok(()),
        };

        // 获取当前选中的客户端
        let sel_client_key = if let Some(monitor) = self.monitors.get(sel_mon_key) {
            monitor.sel
        } else {
            return Ok(());
        };

        let sel_client_key = match sel_client_key {
            Some(key) => key,
            None => return Ok(()), // 没有选中的客户端
        };

        // 检查是否为全屏窗口（全屏窗口不支持切换浮动）
        if let Some(client) = self.clients.get(sel_client_key) {
            if client.state.is_fullscreen {
                return Ok(());
            }
        } else {
            return Ok(());
        }

        // 切换浮动状态
        let (new_floating_state, geometry) =
            if let Some(client) = self.clients.get_mut(sel_client_key) {
                // 计算新的浮动状态
                let new_floating = !client.state.is_floating || client.state.is_fixed;
                client.state.is_floating = new_floating;

                // 如果变为浮动状态，获取当前几何信息用于调整大小
                let geom = if new_floating {
                    Some((
                        client.geometry.x,
                        client.geometry.y,
                        client.geometry.w,
                        client.geometry.h,
                    ))
                } else {
                    None
                };

                (new_floating, geom)
            } else {
                return Ok(());
            };

        // 如果变为浮动状态，调整窗口大小
        if new_floating_state {
            if let Some((x, y, w, h)) = geometry {
                self.resize_client(sel_client_key, x, y, w, h, false);
            }
        }

        // 重新排列布局
        self.arrange(Some(sel_mon_key));

        Ok(())
    }

    fn focusin(&mut self, event: u32) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[focusin]");
        let sel_client_key = self.get_selected_client_key();

        if let Some(client_key) = sel_client_key {
            if let Some(client) = self.clients.get(client_key) {
                if event != client.win {
                    self.setfocus(client_key)?;
                }
            }
        }
        Ok(())
    }

    pub fn focusmon(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        if self.monitor_order.len() <= 1 {
            return Ok(());
        }

        if let WMArgEnum::Int(i) = arg {
            if let Some(target_mon_key) = self.dirtomon(i) {
                // 已经在目标屏则无动作
                if Some(target_mon_key) == self.sel_mon {
                    return Ok(());
                }
                // 统一走切屏逻辑：会更新 current_bar_monitor_id 并移动状态栏
                self.switch_to_monitor(target_mon_key)?;
                // 切屏后在目标屏上重新评估焦点
                self.focus(None)?;
            }
        }
        Ok(())
    }

    pub fn take_screenshot(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        let _ = std::process::Command::new("flameshot").arg("gui").spawn();
        return Ok(());
    }

    pub fn tag(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[tag]");
        if let WMArgEnum::UInt(ui) = *arg {
            let sel_client_key = self.get_selected_client_key();
            let target_tag = ui & CONFIG.tagmask();

            if let Some(client_key) = sel_client_key {
                if target_tag > 0 {
                    // 更新客户端标签
                    if let Some(client) = self.clients.get_mut(client_key) {
                        client.state.tags = target_tag;
                    }

                    // 设置客户端标签属性
                    let _ = self.setclienttagprop(client_key);

                    // 重新聚焦和排列
                    self.focus(None)?;
                    self.arrange(self.sel_mon);
                }
            }
        }
        Ok(())
    }

    pub fn tagmon(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[tagmon]");

        // 检查是否有选中的客户端
        let sel_client_key = self.get_selected_client_key();
        if sel_client_key.is_none() {
            return Ok(());
        }

        // 检查是否只有一个监视器
        if self.monitor_order.len() <= 1 {
            return Ok(());
        }

        if let WMArgEnum::Int(i) = *arg {
            let target_mon = self.dirtomon(&i);
            if let (Some(client_key), Some(target_mon_key)) = (sel_client_key, target_mon) {
                self.sendmon(Some(client_key), Some(target_mon_key));
            }
        }
        Ok(())
    }

    fn sendmon(&mut self, client_key_opt: Option<ClientKey>, target_mon_opt: Option<MonitorKey>) {
        // info!("[sendmon]");

        let client_key = match client_key_opt {
            Some(key) => key,
            None => return,
        };

        let target_mon_key = match target_mon_opt {
            Some(key) => key,
            None => return,
        };

        // 检查客户端当前是否已在目标监视器上
        if let Some(client) = self.clients.get(client_key) {
            if client.mon == Some(target_mon_key) {
                return; // 客户端已在目标监视器上，无需移动
            }
        } else {
            return;
        }

        // 取消客户端焦点
        let _ = self.unfocus(client_key, true);

        // 从当前监视器分离客户端
        self.detach(client_key);
        self.detachstack(client_key);

        // 更新客户端的监视器归属
        if let Some(client) = self.clients.get_mut(client_key) {
            client.mon = Some(target_mon_key);
        }

        // 获取目标监视器的标签集并分配给客户端
        if let Some(target_monitor) = self.monitors.get(target_mon_key) {
            let target_tags = target_monitor.tag_set[target_monitor.sel_tags];

            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.tags = target_tags;
            }
        }

        // 将客户端附加到目标监视器
        self.attach(client_key);
        self.attachstack(client_key);

        // 设置客户端标签属性
        let _ = self.setclienttagprop(client_key);

        // 重新聚焦和排列
        let _ = self.focus(None);
        self.arrange(None);
    }

    pub fn focusstack(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // 提取输入参数
        let direction = match *arg {
            WMArgEnum::Int(i) => i,
            _ => return Ok(()),
        };

        if direction == 0 {
            return Ok(());
        }

        // 检查是否可以切换焦点
        if !self.can_focus_switch()? {
            return Ok(());
        }

        // 根据方向查找目标客户端
        let target_client = if direction > 0 {
            self.find_next_visible_client()?
        } else {
            self.find_previous_visible_client()?
        };

        // 切换焦点
        if let Some(client_key) = target_client {
            self.focus(Some(client_key))?;
            self.restack(self.sel_mon)?;
        }
        Ok(())
    }

    // 辅助方法：检查是否可以切换焦点
    fn can_focus_switch(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let sel_client_key = self.get_selected_client_key().ok_or("No selected client")?;

        if let Some(client) = self.clients.get(sel_client_key) {
            let is_locked_fullscreen =
                client.state.is_fullscreen && CONFIG.behavior().lock_fullscreen;
            Ok(!is_locked_fullscreen)
        } else {
            Err("Selected client not found".into())
        }
    }

    // 辅助方法：查找下一个可见客户端
    fn find_next_visible_client(&self) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.sel_mon.ok_or("No selected monitor")?;
        let current_sel = self.get_selected_client_key().ok_or("No selected client")?;

        // 获取监视器的客户端列表
        if let Some(client_list) = self.monitor_clients.get(sel_mon_key) {
            // 找到当前选中客户端的位置
            if let Some(current_index) = client_list.iter().position(|&k| k == current_sel) {
                // 从下一个位置开始查找
                for &client_key in &client_list[current_index + 1..] {
                    if self.is_client_visible_by_key(client_key) {
                        return Ok(Some(client_key));
                    }
                }

                // 如果没找到，从头开始查找
                for &client_key in &client_list[..current_index] {
                    if self.is_client_visible_by_key(client_key) {
                        return Ok(Some(client_key));
                    }
                }
            }
        }

        Ok(None)
    }

    // 辅助方法：查找上一个可见客户端
    fn find_previous_visible_client(
        &self,
    ) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.sel_mon.ok_or("No selected monitor")?;
        let current_sel = self.get_selected_client_key().ok_or("No selected client")?;

        // 获取监视器的客户端列表
        if let Some(client_list) = self.monitor_clients.get(sel_mon_key) {
            // 找到当前选中客户端的位置
            if let Some(current_index) = client_list.iter().position(|&k| k == current_sel) {
                // 从前一个位置开始向前查找
                for &client_key in client_list[..current_index].iter().rev() {
                    if self.is_client_visible_by_key(client_key) {
                        return Ok(Some(client_key));
                    }
                }

                // 如果没找到，从末尾开始查找
                for &client_key in client_list[current_index + 1..].iter().rev() {
                    if self.is_client_visible_by_key(client_key) {
                        return Ok(Some(client_key));
                    }
                }
            }
        }

        Ok(None)
    }

    pub fn togglebar(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[togglebar]");

        let sel_mon_key = match self.sel_mon {
            Some(key) => key,
            None => return Ok(()),
        };

        // 先在一个小作用域中完成对 pertag.show_bars 的修改，并取出 monitor_num
        let mut monitor_num_opt: Option<i32> = None;
        {
            if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
                if let Some(ref mut pertag) = monitor.pertag {
                    let cur_tag = pertag.cur_tag;
                    if let Some(show_bar) = pertag.show_bars.get_mut(cur_tag) {
                        *show_bar = !*show_bar;
                        info!(
                            "[togglebar] show_bar[mon={}, tag={}] -> {}",
                            monitor.num, cur_tag, show_bar
                        );
                        monitor_num_opt = Some(monitor.num);
                    }
                }
            }
        } // 到这里，monitor 的可变借用生命周期已结束

        // 现在可以安全调用 &mut self 方法
        if let Some(mon_num) = monitor_num_opt {
            if self.current_bar_monitor_id == Some(mon_num) {
                self.position_statusbar_on_monitor(mon_num)?;
                self.arrange(Some(sel_mon_key));
                let _ = self.restack(Some(sel_mon_key));
            }
            self.mark_bar_update_needed_if_visible(Some(mon_num));
        }

        Ok(())
    }

    fn refresh_bar_visibility_on_selected_monitor(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 先读取必要信息并结束借用
        let (sel_mon_key, mon_num) = match self.sel_mon {
            Some(k) => {
                if let Some(m) = self.monitors.get(k) {
                    (k, m.num)
                } else {
                    return Ok(());
                }
            }
            None => return Ok(()),
        };

        // 再调用需要 &mut self 的方法
        if self.current_bar_monitor_id == Some(mon_num) {
            self.position_statusbar_on_monitor(mon_num)?;
            self.arrange(Some(sel_mon_key));
            let _ = self.restack(Some(sel_mon_key));
            self.mark_bar_update_needed_if_visible(Some(mon_num));
        }
        Ok(())
    }

    pub fn incnmaster(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[incnmaster]");

        if let WMArgEnum::Int(i) = *arg {
            let sel_mon_key = self.sel_mon.ok_or("No monitor selected")?;

            if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
                if let Some(ref mut pertag) = monitor.pertag {
                    let cur_tag = pertag.cur_tag;
                    let new_n_master = (monitor.layout.n_master as i32 + i).max(0) as u32;

                    // 更新per-tag的n_master
                    pertag.n_masters[cur_tag] = new_n_master;

                    // 更新当前布局的n_master
                    monitor.layout.n_master = new_n_master;

                    info!(
                        "[incnmaster] Updated n_master to {} for tag {}",
                        new_n_master, cur_tag
                    );
                }
            }

            self.arrange(Some(sel_mon_key));
        }

        Ok(())
    }

    pub fn setcfact(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[setcfact]");

        let sel_client_key = self.get_selected_client_key();
        if sel_client_key.is_none() {
            return Ok(());
        }
        let client_key = sel_client_key.unwrap();

        if let WMArgEnum::Float(f0) = *arg {
            // 获取当前的client_fact
            let current_fact = if let Some(client) = self.clients.get(client_key) {
                client.state.client_fact
            } else {
                return Ok(());
            };

            // 计算新的factor
            let new_fact = if f0.abs() < 0.0001 {
                1.0 // 重置为默认值
            } else {
                f0 + current_fact
            };

            // 限制范围
            if new_fact < 0.25 || new_fact > 4.0 {
                return Ok(());
            }

            // 更新客户端的client_fact
            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.client_fact = new_fact;
                info!(
                    "[setcfact] Updated client_fact to {} for client '{}'",
                    new_fact, client.name
                );
            }

            // 重新排列布局
            self.arrange(self.sel_mon);
        }

        Ok(())
    }

    pub fn movestack(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[movestack]");
        // 提取并验证参数
        let direction = match arg {
            WMArgEnum::Int(i) => *i,
            _ => return Ok(()),
        };

        // 获取当前选中的客户端
        let selected_client_key = self.get_selected_client_key().ok_or("No client selected")?;

        // 根据方向查找目标客户端
        let target_client_key = if direction > 0 {
            self.find_next_tiled_client(selected_client_key)?
        } else {
            self.find_previous_tiled_client(selected_client_key)?
        };

        // 如果找到目标客户端且不是同一个，则交换它们
        if let Some(target_key) = target_client_key {
            if selected_client_key != target_key {
                // 交换客户端在向量中的位置
                self.swap_clients_in_monitor(selected_client_key, target_key)?;

                // 重新排列布局
                self.arrange(self.sel_mon);

                // 短暂屏蔽鼠标抢焦点（比如 150~200ms）
                self.suppress_mouse_focus_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(200));
            }
        }

        Ok(())
    }

    // 辅助方法：检查客户端是否为可见且非浮动的平铺窗口
    fn is_tiled_and_visible(&self, client_key: ClientKey) -> bool {
        if let Some(client) = self.clients.get(client_key) {
            self.is_client_visible_by_key(client_key) && !client.state.is_floating
        } else {
            false
        }
    }

    // 辅助方法：查找下一个平铺客户端
    fn find_next_tiled_client(
        &self,
        current_key: ClientKey,
    ) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.sel_mon.ok_or("No selected monitor")?;
        let client_list = self
            .monitor_clients
            .get(sel_mon_key)
            .ok_or("Monitor client list not found")?;

        // 找到当前客户端的位置
        let current_index = client_list
            .iter()
            .position(|&k| k == current_key)
            .ok_or("Current client not found in monitor list")?;

        // 第一轮：从当前位置向后查找
        for &client_key in &client_list[current_index + 1..] {
            if self.is_tiled_and_visible(client_key) {
                return Ok(Some(client_key));
            }
        }

        // 第二轮：从头开始查找（循环查找）
        for &client_key in &client_list[..current_index] {
            if self.is_tiled_and_visible(client_key) {
                return Ok(Some(client_key));
            }
        }

        Ok(None)
    }

    // 辅助方法：查找上一个平铺客户端
    fn find_previous_tiled_client(
        &self,
        current_key: ClientKey,
    ) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.sel_mon.ok_or("No selected monitor")?;
        let client_list = self
            .monitor_clients
            .get(sel_mon_key)
            .ok_or("Monitor client list not found")?;

        // 找到当前客户端的位置
        let current_index = client_list
            .iter()
            .position(|&k| k == current_key)
            .ok_or("Current client not found in monitor list")?;

        // 第一轮：从当前位置向前查找
        for &client_key in client_list[..current_index].iter().rev() {
            if self.is_tiled_and_visible(client_key) {
                return Ok(Some(client_key));
            }
        }

        // 第二轮：从末尾开始查找（循环查找）
        for &client_key in client_list[current_index + 1..].iter().rev() {
            if self.is_tiled_and_visible(client_key) {
                return Ok(Some(client_key));
            }
        }

        Ok(None)
    }

    // 辅助方法：在监视器的客户端列表中交换两个客户端的位置
    fn swap_clients_in_monitor(
        &mut self,
        client1_key: ClientKey,
        client2_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sel_mon_key = self.sel_mon.ok_or("No selected monitor")?;

        // 在客户端列表中交换位置
        if let Some(client_list) = self.monitor_clients.get_mut(sel_mon_key) {
            let pos1 = client_list
                .iter()
                .position(|&k| k == client1_key)
                .ok_or("Client1 not found in monitor list")?;
            let pos2 = client_list
                .iter()
                .position(|&k| k == client2_key)
                .ok_or("Client2 not found in monitor list")?;

            client_list.swap(pos1, pos2);
        }

        // 在堆栈列表中也交换位置
        if let Some(stack_list) = self.monitor_stack.get_mut(sel_mon_key) {
            if let (Some(pos1), Some(pos2)) = (
                stack_list.iter().position(|&k| k == client1_key),
                stack_list.iter().position(|&k| k == client2_key),
            ) {
                stack_list.swap(pos1, pos2);
            }
        }

        info!(
            "[swap_clients_in_monitor] Swapped clients {:?} and {:?}",
            client1_key, client2_key
        );
        Ok(())
    }

    pub fn setmfact(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[setmfact]");

        if let WMArgEnum::Float(f) = arg {
            let sel_mon_key = self.sel_mon.ok_or("No monitor selected")?;
            if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
                // 计算新的mfact值
                let new_mfact = if f < &1.0 {
                    f + monitor.layout.m_fact
                } else {
                    f - 1.0
                };
                // 检查范围限制
                if new_mfact < 0.05 || new_mfact > 0.95 {
                    return Ok(());
                }
                // 更新per-tag的mfact
                if let Some(ref mut pertag) = monitor.pertag {
                    let cur_tag = pertag.cur_tag;
                    pertag.m_facts[cur_tag] = new_mfact;
                    // 更新当前布局的mfact
                    monitor.layout.m_fact = new_mfact;
                    info!(
                        "[setmfact] Updated m_fact to {} for tag {}",
                        new_mfact, cur_tag
                    );
                }
            }
            self.arrange(Some(sel_mon_key));
            self.suppress_mouse_focus_until =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(200));
        }

        Ok(())
    }

    pub fn setlayout(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[setlayout]");
        let sel_mon_key = self.sel_mon.ok_or("No selected monitor")?;

        // 处理布局设置逻辑
        self.update_layout_selection(sel_mon_key, arg)?;

        // 更新布局符号并检查是否需要重新排列
        let (should_arrange, mon_num) = self.finalize_layout_update(sel_mon_key);

        // 根据情况进行排列或更新状态栏
        if should_arrange {
            self.arrange(Some(sel_mon_key));
        } else {
            self.mark_bar_update_needed_if_visible(mon_num);
        }

        Ok(())
    }

    // 更新布局选择逻辑
    fn update_layout_selection(
        &mut self,
        sel_mon_key: MonitorKey,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match *arg {
            WMArgEnum::Layout(ref lt) => self.handle_specific_layout(sel_mon_key, lt),
            _ => self.toggle_layout_selection(sel_mon_key),
        }
    }

    // 处理指定布局的情况
    fn handle_specific_layout(
        &mut self,
        sel_mon_key: MonitorKey,
        layout: &Rc<LayoutEnum>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitors.get(sel_mon_key).ok_or("Monitor not found")?;

        let current_layout = monitor.lt[monitor.sel_lt].clone();
        let cur_tag = monitor
            .pertag
            .as_ref()
            .ok_or("No pertag information")?
            .cur_tag;

        if **layout == *current_layout {
            // 如果是相同布局，则切换选择
            self.toggle_layout_selection_impl(sel_mon_key, cur_tag);
        } else {
            // 如果是不同布局，则设置新布局
            self.set_new_layout(sel_mon_key, layout, cur_tag);
        }

        Ok(())
    }

    // 切换布局选择（无参数情况）
    fn toggle_layout_selection(
        &mut self,
        sel_mon_key: MonitorKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cur_tag = self
            .monitors
            .get(sel_mon_key)
            .and_then(|m| m.pertag.as_ref())
            .map(|p| p.cur_tag)
            .ok_or("No pertag information available")?;

        self.toggle_layout_selection_impl(sel_mon_key, cur_tag);
        Ok(())
    }

    // 切换布局选择的具体实现
    fn toggle_layout_selection_impl(&mut self, sel_mon_key: MonitorKey, cur_tag: usize) {
        if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
            if let Some(ref mut pertag) = monitor.pertag {
                pertag.sel_lts[cur_tag] ^= 1;
                monitor.sel_lt = pertag.sel_lts[cur_tag];
            }
        }
    }

    // 设置新布局
    fn set_new_layout(&mut self, sel_mon_key: MonitorKey, layout: &Rc<LayoutEnum>, cur_tag: usize) {
        if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
            let sel_lt = monitor.sel_lt;
            if let Some(ref mut pertag) = monitor.pertag {
                pertag.lt_idxs[cur_tag][sel_lt] = Some(layout.clone());
                monitor.lt[sel_lt] = layout.clone();
            }
        }
    }

    // 完成布局更新并返回后续操作信息
    fn finalize_layout_update(&mut self, sel_mon_key: MonitorKey) -> (bool, Option<i32>) {
        if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
            // 更新布局符号
            monitor.lt_symbol = monitor.lt[monitor.sel_lt].symbol().to_string();

            // 检查是否有选中的客户端
            let has_selection = monitor.sel.is_some();
            let mon_num = monitor.num;

            (has_selection, Some(mon_num))
        } else {
            (false, None)
        }
    }

    pub fn zoom(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[zoom]");

        let sel_mon_key = match self.sel_mon {
            Some(key) => key,
            None => return Ok(()),
        };

        // 获取当前选中的客户端
        let selected_client_key = if let Some(monitor) = self.monitors.get(sel_mon_key) {
            monitor.sel
        } else {
            return Ok(());
        };

        let selected_client_key = match selected_client_key {
            Some(key) => key,
            None => return Ok(()), // 没有选中的客户端
        };

        // 检查选中的客户端是否为浮动窗口
        if let Some(client) = self.clients.get(selected_client_key) {
            if client.state.is_floating {
                return Ok(()); // 浮动窗口不参与zoom
            }
        } else {
            return Ok(());
        }

        // 找到第一个平铺窗口
        let first_tiled = self.nexttiled(sel_mon_key, None);

        let target_client_key = if Some(selected_client_key) == first_tiled {
            // 如果选中的客户端就是第一个平铺窗口，找下一个
            self.nexttiled(sel_mon_key, Some(selected_client_key))
        } else {
            // 否则将选中的客户端移到第一位
            Some(selected_client_key)
        };

        // 执行pop操作（将客户端移到第一位）
        if let Some(client_key) = target_client_key {
            self.pop(client_key);
        }

        Ok(())
    }

    pub fn loopview(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[loopview]");

        // 提取并验证参数
        let direction = match arg {
            WMArgEnum::Int(val) => *val,
            _ => return Ok(()),
        };

        if direction == 0 {
            return Ok(());
        }

        // 计算下一个标签
        let next_tag = self.calculate_next_tag(direction);

        // 检查是否需要切换标签
        if self.is_same_tag(next_tag) {
            return Ok(());
        }

        info!(
            "[loopview] next_tag: {}, direction: {}",
            next_tag, direction
        );

        // 执行标签切换
        let cur_tag = self.switch_to_tag(next_tag, next_tag)?;

        // 应用per-tag设置
        let sel_opt = self.apply_pertag_settings(cur_tag)?;

        // 更新焦点和布局
        self.focus(sel_opt)?;
        self.arrange(self.sel_mon.clone());

        self.refresh_bar_visibility_on_selected_monitor()?;

        Ok(())
    }

    // 计算下一个标签的辅助函数
    fn calculate_next_tag(&self, direction: i32) -> u32 {
        let current_tag = if let Some(sel_mon_key) = self.sel_mon {
            if let Some(monitor) = self.monitors.get(sel_mon_key) {
                monitor.tag_set[monitor.sel_tags]
            } else {
                warn!("[calculate_next_tag] Selected monitor not found");
                return 1; // 返回默认的第一个标签
            }
        } else {
            warn!("[calculate_next_tag] No monitor selected");
            return 1; // 返回默认的第一个标签
        };

        // 找到当前tag的位置
        let current_tag_index = if current_tag == 0 {
            0 // 如果当前没有选中的tag，从第一个开始
        } else {
            current_tag.trailing_zeros() as usize
        };

        const MAX_TAGS: usize = 9;
        let next_tag_index = if direction > 0 {
            // 向前循环：1>2>3>...>9>1
            (current_tag_index + 1) % MAX_TAGS
        } else {
            // 向后循环：1>9>8>...>2>1
            if current_tag_index == 0 {
                MAX_TAGS - 1
            } else {
                current_tag_index - 1
            }
        };

        // 将索引转换为tag位掩码
        let next_tag = 1 << next_tag_index;

        info!(
            "[calculate_next_tag] current_tag: {}, next_tag: {}, direction: {}",
            current_tag, next_tag, direction
        );

        next_tag
    }

    pub fn view(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[view]");
        // 提取并验证参数
        let ui = match arg {
            WMArgEnum::UInt(val) => *val,
            _ => return Ok(()),
        };

        let target_tag = ui & CONFIG.tagmask();

        // 检查是否需要切换标签
        if self.is_same_tag(target_tag) {
            return Ok(());
        }

        info!("[view] ui: {}, target_tag: {}", ui, target_tag);

        // 执行标签切换
        let cur_tag = self.switch_to_tag(target_tag, ui)?;

        // 应用per-tag设置
        let sel_opt = self.apply_pertag_settings(cur_tag)?;

        // 更新焦点和布局
        self.focus(sel_opt)?;
        self.arrange(self.sel_mon.clone());

        self.refresh_bar_visibility_on_selected_monitor()?;

        Ok(())
    }

    // 检查是否是相同标签
    fn is_same_tag(&self, target_tag: u32) -> bool {
        if let Some(sel_mon_key) = self.sel_mon {
            if let Some(monitor) = self.monitors.get(sel_mon_key) {
                return target_tag == monitor.tag_set[monitor.sel_tags];
            }
        }
        false
    }

    // 切换到指定标签
    fn switch_to_tag(
        &mut self,
        target_tag: u32,
        ui: u32,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let sel_mon_key = match self.sel_mon {
            Some(k) => k,
            None => return Ok(0),
        };
        let sel_mon_mut = if let Some(sel_mon) = self.monitors.get_mut(sel_mon_key) {
            sel_mon
        } else {
            return Ok(0);
        };

        info!("[switch_to_tag] tag_set: {:?}", sel_mon_mut.tag_set);
        info!("[switch_to_tag] old sel_tags: {}", sel_mon_mut.sel_tags);

        // 切换标签集
        sel_mon_mut.sel_tags ^= 1;
        let new_sel_tags = sel_mon_mut.sel_tags;
        info!("[switch_to_tag] new sel_tags: {}", new_sel_tags);

        // 更新per-tag信息
        let cur_tag = if target_tag > 0 {
            // 设置新标签
            sel_mon_mut.tag_set[new_sel_tags] = target_tag;

            // 计算当前标签索引
            let new_cur_tag = if ui == !0 {
                0 // 显示所有标签
            } else {
                ui.trailing_zeros() as usize + 1
            };

            // 更新 pertag
            if let Some(pertag) = sel_mon_mut.pertag.as_mut() {
                pertag.prev_tag = pertag.cur_tag;
                pertag.cur_tag = new_cur_tag;
            }

            new_cur_tag
        } else {
            // 切换到上一个标签
            if let Some(pertag) = sel_mon_mut.pertag.as_mut() {
                std::mem::swap(&mut pertag.prev_tag, &mut pertag.cur_tag);
                pertag.cur_tag
            } else {
                return Err("No pertag information available".into());
            }
        };

        info!(
            "[switch_to_tag] prev_tag: {}, cur_tag: {}",
            sel_mon_mut.pertag.as_ref().unwrap().prev_tag,
            cur_tag
        );

        Ok(cur_tag)
    }

    fn apply_pertag_settings(
        &mut self,
        cur_tag: usize,
    ) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.sel_mon.ok_or("No monitor selected")?;

        // 先提取所有需要的值，避免借用冲突
        let (n_master, m_fact, sel_lt, layout_0, layout_1, sel_client_key) = {
            let monitor = self
                .monitors
                .get(sel_mon_key)
                .ok_or("Selected monitor not found")?;

            let pertag = monitor
                .pertag
                .as_ref()
                .ok_or("No pertag information available")?;

            let sel_lt = pertag.sel_lts[cur_tag];
            (
                pertag.n_masters[cur_tag],
                pertag.m_facts[cur_tag],
                sel_lt,
                pertag.lt_idxs[cur_tag][sel_lt]
                    .clone()
                    .ok_or("Layout not found")?,
                pertag.lt_idxs[cur_tag][sel_lt ^ 1]
                    .clone()
                    .ok_or("Alternative layout not found")?,
                pertag.sel[cur_tag],
            )
        };

        // 现在安全地应用设置
        if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
            monitor.layout.n_master = n_master;
            monitor.layout.m_fact = m_fact;
            monitor.sel_lt = sel_lt;
            monitor.lt[sel_lt] = layout_0;
            monitor.lt[sel_lt ^ 1] = layout_1;
        } else {
            return Err("Monitor disappeared during operation".into());
        }

        // 记录选中的客户端信息
        if let Some(client_key) = sel_client_key {
            if let Some(client) = self.clients.get(client_key) {
                info!(
                    "[apply_pertag_settings] selected client: {} (key: {:?})",
                    client.name, client_key
                );
            } else {
                warn!(
                    "[apply_pertag_settings] selected client key {:?} not found",
                    client_key
                );
            }
        }

        Ok(sel_client_key)
    }

    pub fn toggleview(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[toggleview]");

        // 提取并验证参数
        let ui = match arg {
            WMArgEnum::UInt(val) => *val,
            _ => return Ok(()),
        };

        let sel_mon_key = match self.sel_mon {
            Some(key) => key,
            None => return Ok(()),
        };

        // 计算新的标签集
        let (sel_tags, newtagset) = if let Some(monitor) = self.monitors.get(sel_mon_key) {
            let sel_tags = monitor.sel_tags;
            let newtagset = monitor.tag_set[sel_tags] ^ (ui & CONFIG.tagmask());
            (sel_tags, newtagset)
        } else {
            return Ok(());
        };

        if newtagset == 0 {
            return Ok(());
        }

        info!("[toggleview] newtagset: {}", newtagset);

        // 更新标签集和per-tag设置
        self.update_tagset_and_pertag(sel_mon_key, sel_tags, newtagset)?;

        // 更新焦点和布局
        self.focus(None)?;
        self.arrange(Some(sel_mon_key));

        self.refresh_bar_visibility_on_selected_monitor()?;

        Ok(())
    }

    // 更新标签集和per-tag设置
    fn update_tagset_and_pertag(
        &mut self,
        mon_key: MonitorKey,
        sel_tags: usize,
        newtagset: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let first_tag = self.find_first_active_tag(newtagset);
        let monitor = self.monitors.get_mut(mon_key).ok_or("Monitor not found")?;

        // 设置新的标签集
        monitor.tag_set[sel_tags] = newtagset;

        // 更新当前标签
        let new_cur_tag = if newtagset == !0 {
            // 显示所有标签
            if let Some(ref mut pertag) = monitor.pertag {
                pertag.prev_tag = pertag.cur_tag;
                pertag.cur_tag = 0;
            }
            0
        } else {
            // 检查当前标签是否还在新的标签集中
            let current_cur_tag = monitor
                .pertag
                .as_ref()
                .ok_or("No pertag information")?
                .cur_tag;

            if current_cur_tag > 0 && (newtagset & (1 << (current_cur_tag - 1))) > 0 {
                // 当前标签仍在新集合中，保持不变
                current_cur_tag
            } else {
                // 当前标签不在新集合中，找到第一个有效标签

                if let Some(ref mut pertag) = monitor.pertag {
                    pertag.prev_tag = current_cur_tag;
                    pertag.cur_tag = first_tag;
                }
                first_tag
            }
        };

        // 应用per-tag设置
        self.apply_pertag_settings_for_monitor(mon_key, new_cur_tag)?;

        Ok(())
    }

    // 查找第一个激活的标签
    fn find_first_active_tag(&self, tagset: u32) -> usize {
        for i in 0..32 {
            if (tagset & (1 << i)) > 0 {
                return i + 1;
            }
        }
        1 // 默认返回第一个标签
    }

    // 为指定监视器应用per-tag设置
    fn apply_pertag_settings_for_monitor(
        &mut self,
        mon_key: MonitorKey,
        cur_tag: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self.monitors.get_mut(mon_key).ok_or("Monitor not found")?;

        // 提取所有需要的值
        let (n_master, m_fact, sel_lt, layout_0, layout_1) = {
            let pertag = monitor
                .pertag
                .as_ref()
                .ok_or("No pertag information available")?;

            let sel_lt = pertag.sel_lts[cur_tag];
            (
                pertag.n_masters[cur_tag],
                pertag.m_facts[cur_tag],
                sel_lt,
                pertag.lt_idxs[cur_tag][sel_lt]
                    .clone()
                    .ok_or("Layout not found")?,
                pertag.lt_idxs[cur_tag][sel_lt ^ 1]
                    .clone()
                    .ok_or("Alternative layout not found")?,
            )
        };

        // 应用设置
        let monitor = self.monitors.get_mut(mon_key).unwrap();
        monitor.layout.n_master = n_master;
        monitor.layout.m_fact = m_fact;
        monitor.sel_lt = sel_lt;
        monitor.lt[sel_lt] = layout_0;
        monitor.lt[sel_lt ^ 1] = layout_1;

        info!(
            "[apply_pertag_settings_for_monitor] Applied settings for tag {}: n_master={}, m_fact={}, sel_lt={}",
            cur_tag, n_master, m_fact, sel_lt
        );

        Ok(())
    }

    pub fn togglefullscr(&mut self, _: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[togglefullscr]");

        let client_key = self.get_selected_client_key();

        if let Some(key) = client_key {
            if let Some(client) = self.clients.get(key) {
                let current_fullscreen = client.state.is_fullscreen;
                let _ = self.setfullscreen(key, !current_fullscreen);
            }
        }

        Ok(())
    }

    pub fn toggletag(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[toggletag]");

        // 获取当前选中的客户端key
        let sel_client_key = if let Some(sel_mon_key) = self.sel_mon {
            if let Some(monitor) = self.monitors.get(sel_mon_key) {
                monitor.sel
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };

        let sel_client_key = match sel_client_key {
            Some(key) => key,
            None => return Ok(()),
        };

        if let WMArgEnum::UInt(ui) = *arg {
            // 获取当前标签并计算新标签
            let current_tags = if let Some(client) = self.clients.get(sel_client_key) {
                client.state.tags
            } else {
                warn!("[toggletag] Selected client {:?} not found", sel_client_key);
                return Ok(());
            };

            let newtags = current_tags ^ (ui & CONFIG.tagmask());

            if newtags > 0 {
                // 更新客户端标签
                if let Some(client) = self.clients.get_mut(sel_client_key) {
                    client.state.tags = newtags;
                } else {
                    return Ok(());
                }

                // 设置客户端标签属性
                self.setclienttagprop(sel_client_key)?;

                // 重新聚焦和排列
                self.focus(None)?;
                self.arrange(self.sel_mon);
            }
        }

        Ok(())
    }

    pub fn quit(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[quit]");
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn setup_ewmh(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(facade) = self.backend.ewmh_facade().as_ref() {
            let _support_win = facade.setup_supporting_wm_check("jwm")?;
            let supported = [
                EwmhFeature::ActiveWindow,
                EwmhFeature::Supported,
                EwmhFeature::WmName,
                EwmhFeature::WmState,
                EwmhFeature::SupportingWmCheck,
                EwmhFeature::WmStateFullscreen,
                EwmhFeature::ClientList,
                EwmhFeature::ClientInfo,
                EwmhFeature::WmWindowType,
                EwmhFeature::WmWindowTypeDialog,
            ];
            facade.declare_supported(&supported)?;
        }
        self.backend.window_ops().flush()?;
        Ok(())
    }

    pub fn setup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[setup]");
        self.backend.init_visual()?;
        let _ = self.updategeom();
        self.setup_ewmh()?;

        let mask = (EventMaskBits::SUBSTRUCTURE_REDIRECT
            | EventMaskBits::STRUCTURE_NOTIFY
            | EventMaskBits::BUTTON_PRESS
            | EventMaskBits::POINTER_MOTION
            | EventMaskBits::ENTER_WINDOW
            | EventMaskBits::LEAVE_WINDOW
            | EventMaskBits::PROPERTY_CHANGE)
            .bits();

        let root = self.backend.root_window();
        self.backend
            .cursor_provider()
            .apply(root.0, StdCursorKind::LeftPtr)?;
        self.backend
            .window_ops()
            .change_event_mask(self.backend.root_window(), mask)?;
        self.grabkeys()?;
        self.focus(None)?;
        self.backend.window_ops().flush()?;

        let snapshot_opt = Self::load_restart_snapshot();

        self.restoring_from_snapshot = snapshot_opt.is_some();

        self.scan()?;

        if let Some(snap) = snapshot_opt {
            info!("[setup] applying snapshot...");
            self.apply_snapshot(&snap);
        } else {
            self.arrange(None);
            let _ = self.restack(self.sel_mon);
            let _ = self.focus(None);
        }
        Ok(())
    }

    pub fn killclient(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[killclient]");

        let sel_client_key = self.get_selected_client_key();
        if sel_client_key.is_none() {
            return Ok(());
        }
        let client_key = sel_client_key.unwrap();

        // 获取客户端信息用于日志
        let (client_name, client_win) = if let Some(client) = self.clients.get(client_key) {
            (client.name.clone(), client.win)
        } else {
            return Ok(());
        };

        info!(
            "[killclient] Attempting to kill client '{}' (window: 0x{:x})",
            client_name, client_win
        );

        // 首先尝试发送 WM_DELETE_WINDOW 协议消息（优雅关闭）
        if self.sendevent_by_window(client_win) {
            info!("[killclient] Sent WM_DELETE_WINDOW protocol message");
            return Ok(());
        }

        // 如果优雅关闭失败，强制终止客户端
        info!("[killclient] WM_DELETE_WINDOW failed, force killing client");
        self.force_kill_client(client_key)?;

        Ok(())
    }

    fn sendevent_by_window(&mut self, window: u32) -> bool {
        let wid = WindowId(window.into());
        if !self.backend.property_ops().supports_delete_window(wid) {
            return false;
        }
        if let Err(e) = self.backend.property_ops().send_delete_window(wid) {
            warn!(
                "[sendevent_by_window] Failed to send WM_DELETE_WINDOW: {}",
                e
            );
            return false;
        }
        let _ = self.backend.window_ops().flush();
        true
    }

    fn force_kill_client(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (win, client_name) = if let Some(client) = self.clients.get(client_key) {
            (client.win, client.name.clone())
        } else {
            return Err("Client not found".into());
        };

        info!(
            "[force_kill_client_by_key] Force killing client '{}' (window: 0x{:x})",
            client_name, win
        );

        self.backend.window_ops().grab_server()?;
        let res = self.backend.window_ops().kill_client(WindowId(win.into()));
        // 无论成功失败，释放 server
        let _ = self.backend.window_ops().ungrab_server();
        self.backend.window_ops().flush()?;

        match res {
            Ok(()) => Ok(()),
            Err(e) => {
                warn!("[force_kill_client_by_key] Kill client failed: {:?}", e);
                Ok(()) // 容错：不让整个流程失败
            }
        }
    }

    fn handle_transient_for_change(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[handle_transient_for_change]");
        let (is_floating, win, client_name) = if let Some(client) = self.clients.get(client_key) {
            (client.state.is_floating, client.win, client.name.clone())
        } else {
            return Ok(());
        };

        if !is_floating {
            // 获取transient_for属性
            let transient_for = self.get_transient_for(win);
            if let Some(parent_window) = transient_for {
                // 检查父窗口是否是我们管理的客户端
                if self.wintoclient(parent_window).is_some() {
                    // 设置为浮动
                    if let Some(client) = self.clients.get_mut(client_key) {
                        client.state.is_floating = true;
                    }

                    debug!(
                        "Window '{}' became floating due to transient_for: 0x{:x}",
                        client_name, parent_window
                    );

                    // 重新排列布局
                    let mon_key = self.clients.get(client_key).and_then(|c| c.mon);
                    self.arrange(mon_key);
                }
            }
        }
        Ok(())
    }

    fn handle_normal_hints_change(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get_mut(client_key) {
            client.size_hints.hints_valid = false;
            debug!(
                "Normal hints changed for window 0x{:x}, invalidating cache",
                client.win
            );
        }
        Ok(())
    }

    fn handle_wm_hints_change(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.updatewmhints(client_key);
        // WM_HINTS 改变可能影响紧急状态，需要重绘状态栏
        self.mark_bar_update_needed_if_visible(None);

        if let Some(client) = self.clients.get(client_key) {
            debug!("WM hints updated for window 0x{:x}", client.win);
        }
        Ok(())
    }

    fn updatetitle_by_key(&mut self, client_key: ClientKey) {
        // 获取窗口ID
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return;
        };
        // 获取新标题
        let new_title = self.fetch_window_title(win);
        // 更新客户端标题
        if let Some(client) = self.clients.get_mut(client_key) {
            client.name = new_title;
            debug!("Updated title for window 0x{:x}: '{}'", win, client.name);
        }
    }

    // 截断到字符数（非字节数）上限
    fn truncate_chars(input: String, max_chars: usize) -> String {
        if input.is_empty() {
            return input;
        }
        let mut count = 0usize;
        let mut truncate_at = input.len();
        for (idx, _) in input.char_indices() {
            if count >= max_chars {
                truncate_at = idx;
                break;
            }
            count += 1;
        }
        let mut s = input;
        s.truncate(truncate_at);
        s
    }

    fn fetch_window_title(&mut self, window: u32) -> String {
        let title = self
            .backend
            .property_ops()
            .get_text_property_best_title(WindowId(window.into()));
        Self::truncate_chars(title, STEXT_MAX_LEN)
    }

    fn handle_title_change(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 更新标题
        self.updatetitle_by_key(client_key);

        // 检查是否需要更新状态栏
        let should_update_bar = self.is_client_selected(client_key);

        if should_update_bar {
            // 获取监视器ID
            let monitor_id = self
                .clients
                .get(client_key)
                .and_then(|client| client.mon)
                .and_then(|mon_key| self.monitors.get(mon_key))
                .map(|monitor| monitor.num);

            if let Some(id) = monitor_id {
                self.mark_bar_update_needed_if_visible(Some(id));

                if let Some(client) = self.clients.get(client_key) {
                    debug!(
                        "Title updated for selected window 0x{:x}, updating status bar",
                        client.win
                    );
                }
            }
        }
        Ok(())
    }

    fn handle_window_type_change(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.updatewindowtype(client_key);

        if let Some(client) = self.clients.get(client_key) {
            debug!("Window type updated for window 0x{:x}", client.win);
        }
        Ok(())
    }

    fn should_move_client(&self, client_key: ClientKey) -> bool {
        if let Some(client) = self.clients.get(client_key) {
            if client.state.is_floating {
                return true;
            }

            if let Some(mon_key) = client.mon {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    return !monitor.lt[monitor.sel_lt].is_tile();
                }
            }
        }
        false
    }

    fn apply_edge_snapping(
        &self,
        client_key: ClientKey,
        new_x: &mut i32,
        new_y: &mut i32,
        mon_wx: i32,
        mon_wy: i32,
        mon_ww: i32,
        mon_wh: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (client_total_width, client_total_height) =
            if let Some(client) = self.clients.get(client_key) {
                (client.total_width(), client.total_height())
            } else {
                return Ok(());
            };

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
        client_key: ClientKey,
        new_x: i32,
        new_y: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (is_floating, current_x, current_y, current_layout_is_tile) =
            if let Some(client) = self.clients.get(client_key) {
                let layout_is_tile = if let Some(mon_key) = client.mon {
                    if let Some(monitor) = self.monitors.get(mon_key) {
                        monitor.lt[monitor.sel_lt].is_tile()
                    } else {
                        false
                    }
                } else {
                    false
                };

                (
                    client.state.is_floating,
                    client.geometry.x,
                    client.geometry.y,
                    layout_is_tile,
                )
            } else {
                return Ok(());
            };

        // 如果窗口不是浮动的且当前是平铺布局，并且移动距离超过阈值
        if !is_floating
            && current_layout_is_tile
            && ((new_x - current_x).abs() > CONFIG.snap() as i32
                || (new_y - current_y).abs() > CONFIG.snap() as i32)
        {
            self.togglefloating(&WMArgEnum::Int(0))?;
        }

        Ok(())
    }

    fn cleanup_move(
        &mut self,
        _window_id: u32,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.backend.input_ops().ungrab_pointer()?;

        let (final_x, final_y, final_w, final_h) =
            if let Some(client) = self.clients.get(client_key) {
                (
                    client.geometry.x,
                    client.geometry.y,
                    client.geometry.w,
                    client.geometry.h,
                )
            } else {
                return Ok(());
            };

        let target_monitor_opt = self.recttomon(final_x, final_y, final_w, final_h);

        if let Some(target_mon_key) = target_monitor_opt {
            if Some(target_mon_key) != self.sel_mon {
                self.sendmon(Some(client_key), Some(target_mon_key));
                self.sel_mon = Some(target_mon_key);
                self.focus(None)?;
            }
        }

        Ok(())
    }

    pub fn movemouse(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        let client_key = match self.get_selected_client_key() {
            Some(k) => k,
            None => return Ok(()),
        };
        if let Some(client) = self.clients.get(client_key) {
            if client.state.is_fullscreen {
                return Ok(());
            }
        } else {
            return Ok(());
        }

        self.restack(self.sel_mon)?;

        let (start_x, start_y, window_id) = {
            let c = self.clients.get(client_key).unwrap();
            (c.geometry.x, c.geometry.y, c.win)
        };
        let (initial_x, initial_y, _mask, _unused) =
            self.backend.input_ops().query_pointer_root()?;
        let (initial_mouse_x, initial_mouse_y) = (initial_x as u16, initial_y as u16);

        let cursor_handle = self.backend.cursor_provider().get(StdCursorKind::Hand)?.0;

        // 关键：先取后端输入句柄（Arc<Mutex<...>>），避免借用 self.backend
        let io = self.backend.input_ops_handle();
        {
            let ops = io.lock().unwrap();
            ops.drag_loop(
                Some(cursor_handle),
                None,
                WindowId(window_id.into()),
                &mut |root_x, root_y, _time| {
                    let mut new_x = start_x + (root_x as i32 - initial_mouse_x as i32);
                    let mut new_y = start_y + (root_y as i32 - initial_mouse_y as i32);

                    let (mon_wx, mon_wy, mon_ww, mon_wh) = {
                        let sel_mon_key = match self.sel_mon {
                            Some(k) => k,
                            None => return Ok(()),
                        };
                        let m = self.monitors.get(sel_mon_key).unwrap();
                        (
                            m.geometry.w_x,
                            m.geometry.w_y,
                            m.geometry.w_w,
                            m.geometry.w_h,
                        )
                    };

                    self.apply_edge_snapping(
                        client_key, &mut new_x, &mut new_y, mon_wx, mon_wy, mon_ww, mon_wh,
                    )?;
                    self.check_and_toggle_floating_for_move(client_key, new_x, new_y)?;
                    if self.should_move_client(client_key) {
                        let (w, h) = {
                            let c = self.clients.get(client_key).unwrap();
                            (c.geometry.w, c.geometry.h)
                        };
                        self.resize_client(client_key, new_x, new_y, w, h, true);
                    }
                    Ok(())
                },
            )?;
        }

        self.cleanup_move(window_id, client_key)?;
        Ok(())
    }
    pub fn resizemouse(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        let client_key = match self.get_selected_client_key() {
            Some(k) => k,
            None => return Ok(()),
        };
        if let Some(client) = self.clients.get(client_key) {
            if client.state.is_fullscreen {
                return Ok(());
            }
        } else {
            return Err("Selected client not found".into());
        }

        self.restack(self.sel_mon)?;

        let (start_x, start_y, border_w, window_id, start_w, start_h) = {
            let c = self.clients.get(client_key).unwrap();
            (
                c.geometry.x,
                c.geometry.y,
                c.geometry.border_w,
                c.win,
                c.geometry.w,
                c.geometry.h,
            )
        };
        let warp_pos = (
            (start_w + border_w - 1) as i16,
            (start_h + border_w - 1) as i16,
        );

        let cursor_handle = self.backend.cursor_provider().get(StdCursorKind::Fleur)?.0;

        let io = self.backend.input_ops_handle();
        {
            let ops = io.lock().unwrap();
            ops.drag_loop(
                Some(cursor_handle),
                Some(warp_pos),
                WindowId(window_id.into()),
                &mut |root_x, root_y, _time| {
                    let new_width =
                        ((root_x as i32 - start_x).max(1 + 2 * border_w) - 2 * border_w).max(1);
                    let new_height =
                        ((root_y as i32 - start_y).max(1 + 2 * border_w) - 2 * border_w).max(1);

                    self.check_and_toggle_floating_for_resize(client_key, new_width, new_height)?;
                    if self.should_resize_client(client_key) {
                        self.resize_client(
                            client_key, start_x, start_y, new_width, new_height, true,
                        );
                    }
                    Ok(())
                },
            )?;
        }

        self.cleanup_resize(window_id, border_w)?;
        Ok(())
    }

    fn check_and_toggle_floating_for_resize(
        &mut self,
        client_key: ClientKey,
        new_width: i32,
        new_height: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (is_floating, current_w, current_h, is_tile_layout) =
            if let Some(client) = self.clients.get(client_key) {
                let is_tile = if let Some(mon_key) = client.mon {
                    if let Some(monitor) = self.monitors.get(mon_key) {
                        monitor.lt[monitor.sel_lt].is_tile()
                    } else {
                        false
                    }
                } else {
                    false
                };

                (
                    client.state.is_floating,
                    client.geometry.w,
                    client.geometry.h,
                    is_tile,
                )
            } else {
                return Err("Client not found".into());
            };

        if !is_floating && is_tile_layout {
            let snap_threshold = CONFIG.snap() as i32;
            if (new_width - current_w).abs() > snap_threshold
                || (new_height - current_h).abs() > snap_threshold
            {
                debug!("Toggling to floating mode due to size change");
                let _ = self.togglefloating(&WMArgEnum::UInt(0));
            }
        }

        Ok(())
    }

    /// 检查客户端是否应该被调整大小
    fn should_resize_client(&self, client_key: ClientKey) -> bool {
        if let Some(client) = self.clients.get(client_key) {
            if client.state.is_floating {
                return true;
            }

            if let Some(mon_key) = client.mon {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    return !monitor.lt[monitor.sel_lt].is_tile();
                }
            }
        }
        false
    }

    fn cleanup_resize(
        &mut self,
        window_id: u32,
        border_width: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(key) = self.get_selected_client_key() {
            if let Some(client) = self.clients.get(key) {
                self.backend.input_ops().warp_pointer_to_window(
                    WindowId(window_id.into()),
                    (client.geometry.w + border_width - 1) as i16,
                    (client.geometry.h + border_width - 1) as i16,
                )?;
            }
        }
        self.backend.input_ops().ungrab_pointer()?;
        self.check_monitor_change_after_resize()?;
        Ok(())
    }

    fn check_monitor_change_after_resize(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let client_key = match self.get_selected_client_key() {
            Some(key) => key,
            None => return Ok(()),
        };

        let (x, y, w, h) = {
            let client = self.clients.get(client_key).unwrap();
            (
                client.geometry.x,
                client.geometry.y,
                client.geometry.w,
                client.geometry.h,
            )
        };

        let target_monitor = self.recttomon(x, y, w, h);

        if let Some(target_mon_key) = target_monitor {
            if Some(target_mon_key) != self.sel_mon {
                debug!("Moving client to different monitor after resize");
                self.sendmon(Some(client_key), Some(target_mon_key));
                self.sel_mon = Some(target_mon_key);
                self.focus(None)?;
            }
        }

        Ok(())
    }

    fn setup_modifier_masks(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Setting up modifier masks (KeyOps)...");
        let (_mods_flag, backend_bits) = self.backend.key_ops_mut().detect_numlock_mask()?;
        self.numlock_mask_bits = backend_bits;
        self.verify_modifier_setup()?;
        Ok(())
    }

    fn verify_modifier_setup(&self) -> Result<(), Box<dyn std::error::Error>> {
        let (_x, _y, mask, _unused) = self.backend.input_ops().query_pointer_root()?;
        info!("Current modifier state: 0x{:04x}", mask);
        if self.numlock_mask_bits != 0 {
            let numlock_active = (mask & self.numlock_mask_bits) != 0;
            info!(
                "NumLock currently {}",
                if numlock_active { "ON" } else { "OFF" }
            );
        }
        Ok(())
    }

    fn setclienttagprop(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let monitor_num = client
                .mon
                .and_then(|mk| self.monitors.get(mk))
                .map(|m| m.num as u32)
                .unwrap_or(0);
            self.backend.property_ops().set_client_info(
                WindowId(client.win.into()),
                client.state.tags,
                monitor_num,
            )?;
            self.backend.window_ops().flush()?;
        }
        Ok(())
    }

    fn mouse_focus_blocked(&mut self) -> bool {
        if let Some(deadline) = self.suppress_mouse_focus_until {
            if std::time::Instant::now() < deadline {
                return true;
            }
            // 超时后清掉标记
            self.suppress_mouse_focus_until = None;
        }
        false
    }

    fn switch_to_monitor(
        &mut self,
        target_monitor_key: MonitorKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 记录旧选中的客户端，但暂不把焦点设到root
        let prev_sel = self.get_selected_client_key();

        // 切换选中显示器
        self.sel_mon = Some(target_monitor_key);

        // 如果新屏有选中客户端，优先直接聚焦它（避免焦点落到 root）
        self.focus(None)?; // 这会在新屏上挑一个可见客户端并 setfocus

        // 此时旧客户端自然失焦了，但它的边框与按钮抓取可能还处于“焦点态”，补一次 UI 状态回退而不改焦点：
        if let Some(old_key) = prev_sel {
            // 仅做边框/按钮抓取退回，不调用 set_input_focus_root（将 setfocus 参数改为 false）
            self.unfocus(old_key, false)?;
        }

        // 状态栏重定位和布局更新（与原逻辑一致）
        let old_id = self.current_bar_monitor_id;
        let new_id = self.monitors.get(target_monitor_key).map(|m| m.num);
        if old_id != new_id {
            if let Some(id) = new_id {
                self.current_bar_monitor_id = Some(id);
                self.position_statusbar_on_monitor(id)?;
            }
            if let Some(old) = old_id.and_then(|oid| self.get_monitor_by_id(oid)) {
                self.arrange(Some(old));
            }
            self.arrange(Some(target_monitor_key));
            self.restack(Some(target_monitor_key))?;
        }

        Ok(())
    }

    fn should_focus_client(
        &self,
        client_key_opt: Option<ClientKey>,
        is_on_selected_monitor: bool,
    ) -> bool {
        // 如果切换了显示器，需要重新聚焦
        if !is_on_selected_monitor {
            return true;
        }

        // 如果鼠标进入了根窗口（没有具体客户端），需要重新聚焦
        if client_key_opt.is_none() {
            return true;
        }

        // 如果进入的客户端与当前选中客户端不同，需要重新聚焦
        let current_selected = self.get_selected_client_key();
        current_selected != client_key_opt
    }

    fn expose(&mut self, window: u32, count: u16) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[expose]");
        // 只处理最后一个expose事件（count为0时）
        if count != 0 {
            return Ok(());
        }

        // 检查窗口所在的显示器并标记状态栏需要更新
        if let Some(monitor_key) = self.wintomon(window) {
            if let Some(monitor) = self.monitors.get(monitor_key) {
                self.mark_bar_update_needed_if_visible(Some(monitor.num));
            }
        }

        Ok(())
    }

    // 辅助方法：取消客户端焦点（可选版本）
    fn unfocus_client_opt(
        &mut self,
        client_key_opt: Option<ClientKey>,
        setfocus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client_key) = client_key_opt {
            self.unfocus_client(client_key, setfocus)?;
        }
        Ok(())
    }

    // 辅助方法：取消单个客户端的焦点
    fn unfocus_client(
        &mut self,
        client_key: ClientKey,
        setfocus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let win = client.win;

            // 抓取按钮（设为非焦点状态）
            self.grabbuttons(client_key, false)?;

            // 设置边框颜色为非选中状态
            self.set_window_border_color(win, false)?;

            if setfocus {
                self.backend
                    .window_ops()
                    .set_input_focus_root(self.backend.root_window())?;
                if let Some(facade) = self.backend.ewmh_facade().as_ref() {
                    let _ = facade.clear_active_window();
                }
            }

            self.backend.window_ops().flush()?;
        }

        Ok(())
    }

    fn grabbuttons(
        &mut self,
        client_key: ClientKey,
        focused: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_win_id = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        self.backend
            .window_ops()
            .ungrab_all_buttons(WindowId(client_win_id.into()))?;

        if !focused {
            self.backend
                .window_ops()
                .grab_button_any_anymod(WindowId(client_win_id.into()), BUTTONMASK.bits())?;
        }

        for button_config in CONFIG.get_buttons().iter() {
            if button_config.click_type == WMClickType::ClickClientWin {
                let base = button_config.mask;
                let combos = [
                    base,
                    base | Mods::CAPS,
                    base | Mods::NUMLOCK,
                    base | Mods::CAPS | Mods::NUMLOCK,
                ];
                let btn_u8 = button_config.button.to_u8();
                for mm in combos {
                    let mods_bits = self
                        .backend
                        .key_ops()
                        .backend_mods_mask_for_grab(mm, self.numlock_mask_bits);
                    self.backend.window_ops().grab_button(
                        WindowId(client_win_id.into()),
                        btn_u8,
                        BUTTONMASK.bits(),
                        mods_bits,
                    )?;
                }
            }
        }

        self.backend.window_ops().flush()?;
        Ok(())
    }

    fn focus(
        &mut self,
        mut client_key_opt: Option<ClientKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[focus]");

        // 如果传入的是状态栏客户端，忽略并寻找合适的替代
        if let Some(client_key) = client_key_opt {
            if let Some(client) = self.clients.get(client_key) {
                info!("[focus] {}", client);
                if Some(client.win) == self.status_bar_window {
                    client_key_opt = None; // 忽略状态栏
                }
            }
        }

        // 检查客户端是否可见，如果不可见则寻找可见的客户端
        let is_visible = match client_key_opt {
            Some(client_key) => self.is_client_visible_by_key(client_key),
            None => false,
        };

        if !is_visible {
            client_key_opt = self.find_visible_client();
        }

        // 处理焦点切换
        self.handle_focus_change_by_key(&client_key_opt)?;

        // 设置新的焦点客户端
        if let Some(client_key) = client_key_opt {
            self.set_client_focus_by_key(client_key)?;
        } else {
            self.set_root_focus()?;
        }

        // 更新选中监视器的状态
        self.update_monitor_selection_by_key(client_key_opt);

        // 标记状态栏需要更新
        self.mark_bar_update_needed_if_visible(None);

        Ok(())
    }

    fn find_visible_client(&self) -> Option<ClientKey> {
        let sel_mon_key = self.sel_mon?;

        // 从监视器的堆栈顺序中查找可见客户端
        if let Some(stack_clients) = self.monitor_stack.get(sel_mon_key) {
            for &client_key in stack_clients {
                if self.is_client_visible_by_key(client_key) {
                    return Some(client_key);
                }
            }
        }

        None
    }

    fn handle_focus_change_by_key(
        &mut self,
        new_focus: &Option<ClientKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let current_sel = self.get_selected_client_key();

        if current_sel.is_some() && current_sel != *new_focus {
            if let Some(current_key) = current_sel {
                self.unfocus(current_key, false)?;
            }
        }

        Ok(())
    }

    fn set_client_focus_by_key(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 检查客户端是否在当前选中的监视器上
        let client_monitor_key = if let Some(client) = self.clients.get(client_key) {
            client.mon
        } else {
            return Err("Client not found".into());
        };

        if let Some(client_mon_key) = client_monitor_key {
            if Some(client_mon_key) != self.sel_mon {
                self.sel_mon = Some(client_mon_key);
            }
        }

        // 清除紧急状态
        if let Some(client) = self.clients.get_mut(client_key) {
            if client.state.is_urgent {
                client.state.is_urgent = false;
                let _ = self.seturgent(client_key, false);
            }
        }

        // 重新排列堆栈顺序
        self.detachstack(client_key);
        self.attachstack(client_key);

        // 抓取按钮事件
        self.grabbuttons(client_key, true)?;

        // 设置边框颜色为选中状态
        if let Some(client) = self.clients.get(client_key) {
            self.set_window_border_color(client.win, true)?;
        }

        // 设置焦点
        self.setfocus(client_key)?;

        Ok(())
    }

    fn update_monitor_selection_by_key(&mut self, client_key_opt: Option<ClientKey>) {
        if let Some(sel_mon_key) = self.sel_mon {
            if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
                monitor.sel = client_key_opt;

                if let Some(ref mut pertag) = monitor.pertag {
                    let cur_tag = pertag.cur_tag;
                    if cur_tag < pertag.sel.len() {
                        pertag.sel[cur_tag] = client_key_opt;
                    }
                }
            }
        }
    }

    fn unfocus(
        &mut self,
        client_key: ClientKey,
        setfocus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let win = client.win;
            // 抓取按钮（设为非焦点状态）
            self.grabbuttons(client_key, false)?;
            // 设置边框颜色为非选中状态
            self.set_window_border_color(win, false)?;
            if setfocus {
                self.backend
                    .window_ops()
                    .set_input_focus_root(self.backend.root_window())?;
                if let Some(facade) = self.backend.ewmh_facade().as_ref() {
                    let _ = facade.clear_active_window();
                }
            }
            self.backend.window_ops().flush()?;
        }
        Ok(())
    }

    fn setfocus(&mut self, client_key: ClientKey) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let wid = WindowId(client.win as u64);
            self.backend.window_ops().set_input_focus_window(wid)?;
            if let Some(facade) = self.backend.ewmh_facade().as_ref() {
                let _ = facade.set_active_window(wid);
            }
            self.backend.window_ops().flush()?;
        }
        Ok(())
    }

    fn set_root_focus(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.backend
            .window_ops()
            .set_input_focus_root(self.backend.root_window())?;
        if let Some(facade) = self.backend.ewmh_facade().as_ref() {
            let _ = facade.clear_active_window();
        }
        self.backend.window_ops().flush()?;
        Ok(())
    }

    fn update_net_client_list(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut ordered: Vec<WindowId> = Vec::with_capacity(self.client_order.len());
        for &key in &self.client_order {
            if let Some(client) = self.clients.get(key) {
                ordered.push(WindowId(client.win as u64));
            }
        }

        let mut stacking: Vec<WindowId> = Vec::new();
        for &mon_key in &self.monitor_order {
            if let Some(stack) = self.monitor_stack.get(mon_key) {
                for &ck in stack.iter().rev() {
                    if let Some(c) = self.clients.get(ck) {
                        stacking.push(WindowId(c.win as u64));
                    }
                }
            }
        }

        if let Some(facade) = self.backend.ewmh_facade().as_ref() {
            facade.set_client_list(&ordered)?;
            facade.set_client_list_stacking(&stacking)?;
        }
        Ok(())
    }

    fn setclientstate(&self, win: u32, state: i64) -> Result<(), Box<dyn std::error::Error>> {
        self.backend
            .property_ops()
            .set_wm_state(WindowId(win as u64), state)
    }

    fn manage(&mut self, win: u32, geom: &Geometry) -> Result<(), Box<dyn std::error::Error>> {
        info!("[manage] Managing window 0x{:x}", win);
        // 检查窗口是否已被管理
        if self.wintoclient(win).is_some() {
            warn!("[manage] Window 0x{:x} already managed", win);
            return Ok(());
        }
        // 创建新的客户端对象
        let mut client = WMClient::new();
        // 设置窗口ID
        client.win = win;
        // 从几何信息中设置初始属性
        client.geometry.x = geom.x as i32;
        client.geometry.old_x = geom.x as i32;
        client.geometry.y = geom.y as i32;
        client.geometry.old_y = geom.y as i32;
        client.geometry.w = geom.w as i32;
        client.geometry.old_w = geom.w as i32;
        client.geometry.h = geom.h as i32;
        client.geometry.old_h = geom.h as i32;
        client.geometry.old_border_w = geom.border as i32;
        client.state.client_fact = 1.0;
        client.name = self.fetch_window_title(client.win);
        self.update_class_info(&mut client);

        info!("[manage] {}", client);
        // 检查是否是状态栏
        if client.is_status_bar() {
            info!("[manage] Detected status bar, managing as statusbar");
            // 插入到SlotMap
            let client_key = self.insert_client(client);
            // 绑定到当前聚焦显示器
            let current_mon_id = self.get_sel_mon().map(|m| m.num).unwrap_or(0);
            self.status_bar_client = Some(client_key);
            self.status_bar_window = Some(win);
            self.current_bar_monitor_id = Some(current_mon_id);

            return self.manage_statusbar(client_key, win, current_mon_id);
        }

        // 插入到SlotMap
        let client_key = self.insert_client(client);
        // 常规客户端管理流程
        self.manage_regular_client(client_key)
    }

    fn setup_client_window(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_popup_like(client_key) {
            if let Some(client) = self.clients.get_mut(client_key) {
                client.geometry.border_w = 0;
            }
            let win = self.clients.get(client_key).unwrap().win;
            self.set_window_border_width(win, 0)?;
            // 不设置选中边框色
            self.configure_client(client_key)?;
            self.setclientstate(win, NORMAL_STATE as i64)?;
            self.backend.window_ops().flush()?;
            return Ok(());
        }

        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        info!("[setup_client_window] Setting up window 0x{:x}", win);

        if let Some(client) = self.clients.get_mut(client_key) {
            client.geometry.border_w = CONFIG.border_px() as i32;
        }
        let border_w = self.clients.get(client_key).unwrap().geometry.border_w;
        self.set_window_border_width(win, border_w as u32)?;

        self.set_window_border_color(win, true)?;

        self.configure_client(client_key)?;

        if !self.restoring_from_snapshot {
            // 原来的“屏幕外临时位置”逻辑，仅在非恢复模式执行
            let (x, y, w, h) = if let Some(client) = self.clients.get(client_key) {
                let offscreen_x = client.geometry.x + 2 * self.s_w;
                (
                    offscreen_x,
                    client.geometry.y,
                    client.geometry.w,
                    client.geometry.h,
                )
            } else {
                return Err("Client not found".into());
            };
            self.backend.window_ops().configure_xywh_border(
                WindowId(win.into()),
                Some(x),
                Some(y),
                Some(w as u32),
                Some(h as u32),
                None,
            )?;
            self.backend.window_ops().flush()?;
        }

        if let Some(client) = self.clients.get(client_key) {
            self.setclientstate(client.win, NORMAL_STATE as i64)?;
        }

        self.backend.window_ops().flush()?;
        Ok(())
    }

    fn parent_client_of(&self, child_key: ClientKey) -> Option<ClientKey> {
        let child_win = self.clients.get(child_key).map(|c| c.win)?;
        let parent_win = self.get_transient_for(child_win)?;
        self.wintoclient(parent_win)
    }

    fn handle_new_client_focus(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 预取必要信息，避免后续借用冲突
        let (client_win, client_mon_key, is_never_focus) =
            if let Some(c) = self.clients.get(client_key) {
                (c.win, c.mon, c.state.never_focus)
            } else {
                return Err("Client not found".into());
            };
        let current_sel = self.get_selected_client_key();
        let current_sel_mon = self.sel_mon;

        // 1) popup-like（菜单/提示/小尺寸 transient 等）
        if self.is_popup_like(client_key) {
            // 叠放到父窗口之上（如可用），否则顶层
            let parent_key_opt = self.parent_client_of(client_key);
            let sibling = parent_key_opt
                .and_then(|pk| self.clients.get(pk))
                .map(|pc| WindowId(pc.win.into()));
            self.backend
                .window_ops()
                .configure_stack_above(WindowId(client_win.into()), sibling)?;
            self.backend.window_ops().flush()?;

            // 明确保持焦点：优先父窗口 -> 之前选中 -> 根焦点
            if let Some(pk) = parent_key_opt {
                // 若父窗口在不同屏，可选是否切屏，这里不切屏，仅保持父焦点
                let _ = self.set_client_focus_by_key(pk);
            } else if let Some(prev_sel) = current_sel {
                let _ = self.set_client_focus_by_key(prev_sel);
            } else {
                let _ = self.set_root_focus();
            }
            // 不修改 monitor.sel，不抢焦点
            return Ok(());
        }

        // 2) 非 popup-like，新窗口属于当前选中屏
        let is_on_selected_monitor = client_mon_key.is_some() && client_mon_key == current_sel_mon;
        if is_on_selected_monitor {
            // 设置该屏选中为新窗口
            if let Some(mon_key) = client_mon_key {
                if let Some(monitor) = self.monitors.get_mut(mon_key) {
                    monitor.sel = Some(client_key);
                }
                // 重排该屏
                self.arrange(Some(mon_key));
            }

            // 焦点策略：只有在允许抢焦点时才抢（非 never_focus）
            if !is_never_focus {
                // 先取消旧焦点（如果与新焦点不同），避免闪烁
                if let Some(prev_sel) = current_sel {
                    if prev_sel != client_key {
                        self.unfocus(prev_sel, false)?;
                    }
                }
                self.focus(Some(client_key))?;
            } else {
                // 不抢焦点：明确保持之前焦点（若不存在则设根焦点）
                if let Some(prev_sel) = current_sel {
                    let _ = self.set_client_focus_by_key(prev_sel);
                } else {
                    let _ = self.set_root_focus();
                }
            }
            return Ok(());
        }

        // 3) 非 popup-like，新窗口处于非选中屏
        if let Some(target_mon_key) = client_mon_key {
            // 该屏选中设为新窗口，并排列该屏
            if let Some(monitor) = self.monitors.get_mut(target_mon_key) {
                monitor.sel = Some(client_key);
            }
            self.arrange(Some(target_mon_key));
        }

        // 根据配置决定是否切屏并抢焦点
        if CONFIG.behavior().focus_follows_new_window && !is_never_focus {
            if let Some(target_mon_key) = client_mon_key {
                // 使用统一的切屏逻辑（会处理状态栏与 restack）
                self.switch_to_monitor(target_mon_key)?;
                // 切屏后设置焦点到新窗口
                self.focus(Some(client_key))?;
            }
        } else {
            // 不切屏：保持当前屏焦点（优先之前选中，否则根焦点）
            if let Some(prev_sel) = current_sel {
                let _ = self.set_client_focus_by_key(prev_sel);
            } else {
                let _ = self.set_root_focus();
            }
        }

        Ok(())
    }

    // 常规客户端管理
    fn manage_regular_client(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 处理 WM_TRANSIENT_FOR
        self.handle_transient_for(client_key)?;

        // 调整窗口位置
        self.adjust_client_position(client_key);

        // 设置窗口属性
        self.setup_client_window(client_key)?;

        // 更新各种提示
        self.updatewindowtype(client_key);
        self.updatesizehints(client_key)?;
        self.updatewmhints(client_key);

        // 添加到管理结构
        self.attach(client_key);
        self.attachstack(client_key);

        // 注册事件和抓取按钮
        self.register_client_events(client_key)?;

        // 映射窗口
        // 已映射窗口避免再次 map
        let already_mapped = {
            let win = self.clients.get(client_key).unwrap().win;
            self.backend
                .window_ops()
                .get_window_attributes(WindowId(win.into()))
                .map(|a| a.map_state_viewable)
                .unwrap_or(false)
        };
        if !already_mapped {
            self.map_client_window(client_key)?;
        }

        // 更新客户端列表
        self.update_net_client_list()?;

        // 处理焦点
        self.handle_new_client_focus(client_key)?;

        Ok(())
    }

    fn handle_transient_for(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        match self.get_transient_for(win) {
            Some(transient_for_win) => {
                // 找到 transient_for 窗口对应的客户端
                if let Some(parent_client_key) = self.wintoclient(transient_for_win) {
                    let (parent_mon, parent_tags) =
                        if let Some(parent) = self.clients.get(parent_client_key) {
                            (parent.mon, parent.state.tags)
                        } else {
                            return Err("Parent client not found".into());
                        };

                    if let Some(client) = self.clients.get_mut(client_key) {
                        client.mon = parent_mon;
                        client.state.tags = parent_tags;
                        // 总是设置为floating
                        client.state.is_floating = true;
                        warn!(
                            "[handle_transient_for] Client {} is transient for parent",
                            client.name
                        );
                    }
                } else {
                    info!("[handle_transient_for] parent client is None");
                    // 父窗口不是我们管理的客户端
                    if let Some(client) = self.clients.get_mut(client_key) {
                        client.mon = self.sel_mon;
                    }
                    self.applyrules_by_key(client_key);
                }
            }
            None => {
                info!("no WM_TRANSIENT_FOR property");
                // 没有 WM_TRANSIENT_FOR 属性
                if let Some(client) = self.clients.get_mut(client_key) {
                    client.mon = self.sel_mon;
                }
                self.applyrules_by_key(client_key);
            }
        }
        Ok(())
    }

    fn update_class_info(&mut self, client: &mut WMClient) {
        if let Some((inst, cls)) = self.get_wm_class(client.win) {
            client.instance = inst;
            client.class = cls;
        }
    }

    /// 检查规则是否匹配客户端
    fn rule_matches(&self, rule: &WMRule, name: &str, class: &str, instance: &str) -> bool {
        // 如果规则的所有字段都为空，则不匹配
        if rule.name.is_empty() && rule.class.is_empty() && rule.instance.is_empty() {
            return false;
        }
        // 检查每个字段是否匹配（空字符串表示忽略该字段）
        let name_matches = rule.name.is_empty() || name.contains(&rule.name);
        let class_matches = rule.class.is_empty() || class.contains(&rule.class);
        let instance_matches = rule.instance.is_empty() || instance.contains(&rule.instance);
        name_matches && class_matches && instance_matches
    }

    /// 应用单个规则到客户端
    fn apply_single_rule(&mut self, client_key: ClientKey, rule: &WMRule) {
        if let Some(client) = self.clients.get_mut(client_key) {
            info!("[apply_single_rule] Applying rule: {:?}", rule);
            // 设置浮动状态
            client.state.is_floating = rule.is_floating;
            // 设置标签
            if rule.tags > 0 {
                client.state.tags |= rule.tags as u32;
            }
            // 设置监视器
            if rule.monitor >= 0 {
                // 查找指定的监视器
                let target_monitor = self
                    .monitor_order
                    .iter()
                    .find(|&&mon_key| {
                        if let Some(monitor) = self.monitors.get(mon_key) {
                            monitor.num == rule.monitor
                        } else {
                            false
                        }
                    })
                    .copied();
                if let Some(mon_key) = target_monitor {
                    client.mon = Some(mon_key);
                    info!(
                        "[apply_single_rule] Assigned client to monitor {}",
                        rule.monitor
                    );
                }
            }
            info!(
                "[apply_single_rule] Applied - floating: {}, tags: {}, monitor: {}",
                client.state.is_floating, client.state.tags, rule.monitor
            );
        }
    }

    /// 为客户端设置默认标签
    fn set_default_tags(&mut self, client_key: ClientKey) {
        if let Some(client) = self.clients.get_mut(client_key) {
            let condition = client.state.tags & CONFIG.tagmask();
            if condition > 0 {
                // 如果客户端已有有效标签，保持现有标签
                client.state.tags = condition;
            } else {
                // 如果没有有效标签，使用当前监视器的选中标签
                if let Some(mon_key) = client.mon {
                    if let Some(monitor) = self.monitors.get(mon_key) {
                        client.state.tags = monitor.tag_set[monitor.sel_tags];
                    }
                } else {
                    // 如果没有监视器，使用第一个标签作为默认
                    client.state.tags = 1;
                }
            }
            info!(
                "[set_default_tags] Set tags to {} for client 0x{:x}",
                client.state.tags, client.win
            );
        }
    }

    fn applyrules_by_key(&mut self, client_key: ClientKey) {
        let (win, name, mut class, mut instance) =
            if let Some(client) = self.clients.get(client_key) {
                (
                    client.win,
                    client.name.clone(),
                    client.class.clone(),
                    client.instance.clone(),
                )
            } else {
                return;
            };
        // 如果类信息为空，尝试从 X11 获取
        if class.is_empty() && instance.is_empty() {
            if let Some((inst, cls)) = self.get_wm_class(win) {
                instance = inst;
                class = cls;

                // 更新客户端的类信息
                if let Some(client) = self.clients.get_mut(client_key) {
                    client.instance = instance.clone();
                    client.class = class.clone();
                }
            }
        }
        info!(
            "[applyrules_by_key] win: 0x{:x}, name: '{}', instance: '{}', class: '{}'",
            win, name, instance, class
        );
        // 重置浮动状态
        if let Some(client) = self.clients.get_mut(client_key) {
            client.state.is_floating = false;
        }
        if name.is_empty() && class.is_empty() && instance.is_empty() {
            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.is_floating = true;
            }
            info!("[applyrules_by_key] No window info available, setting as floating");
        }
        // 应用配置规则
        let mut rule_applied = false;
        for rule in &CONFIG.get_rules() {
            if self.rule_matches(rule, &name, &class, &instance) {
                self.apply_single_rule(client_key, rule);
                rule_applied = true;
                break;
            }
        }
        if !rule_applied {
            info!("[applyrules_by_key] No matching rule found, using defaults");
            // 设置默认标签
            self.set_default_tags(client_key);
        }
        // 最终日志
        if let Some(client) = self.clients.get(client_key) {
            info!(
                "[applyrules_by_key] Final state - class: '{}', instance: '{}', name: '{}', tags: {}, floating: {}",
                client.class, client.instance, client.name, client.state.tags, client.state.is_floating
            );
        }
    }

    fn register_client_events(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        let mask = (EventMaskBits::ENTER_WINDOW
            | EventMaskBits::FOCUS_CHANGE
            | EventMaskBits::PROPERTY_CHANGE
            | EventMaskBits::STRUCTURE_NOTIFY)
            .bits();
        // haha
        self.backend
            .window_ops()
            .change_event_mask(WindowId(win.into()), mask)?;
        self.grabbuttons(client_key, false)?;
        info!(
            "[register_client_events] Events registered for window 0x{:x}",
            win
        );
        Ok(())
    }

    fn map_client_window(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        self.backend.window_ops().map_window(WindowId(win.into()))?;
        self.backend.window_ops().flush()?;
        info!("[map_client_window] Successfully mapped window 0x{:x}", win);
        Ok(())
    }

    fn get_transient_for(&self, window: u32) -> Option<u32> {
        self.backend
            .property_ops()
            .transient_for(WindowId(window.into()))
            .map(|w| w.0 as u32)
    }

    fn manage_statusbar(
        &mut self,
        client_key: ClientKey,
        win: u32,
        current_mon_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 配置状态栏客户端
        let mon_key = self.get_monitor_by_id(current_mon_id);
        if let Some(client) = self.clients.get_mut(client_key) {
            client.mon = mon_key;
            client.state.never_focus = true;
            client.state.is_floating = true;
            client.state.tags = CONFIG.tagmask();
            client.geometry.border_w = CONFIG.border_px() as i32;
        }

        // 调整状态栏位置（通常在顶部）
        self.position_statusbar_on_monitor(current_mon_id)?;

        // 设置状态栏特有的窗口属性
        self.setup_statusbar_window_by_key(client_key)?;

        // 映射状态栏窗口
        self.backend.window_ops().map_window(WindowId(win.into()))?;
        self.backend.window_ops().flush()?;
        Ok(())
    }

    fn set_bar_strut(
        &self,
        bar_win: u32,
        mon: &WMMonitor,
        bar_height: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let top_amount = bar_height.max(0) as u32;
        let top_start_x = mon.geometry.m_x.max(0) as u32;
        let top_end_x = (mon.geometry.m_x + mon.geometry.m_w - 1).max(0) as u32;
        self.backend.property_ops().set_window_strut_top(
            WindowId(bar_win.into()),
            top_amount,
            top_start_x,
            top_end_x,
        )
    }

    fn remove_bar_strut(&self, bar_win: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.backend
            .property_ops()
            .clear_window_strut(WindowId(bar_win.into()))
    }

    fn position_statusbar_on_monitor(
        &mut self,
        monitor_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_key = match self.status_bar_client {
            Some(k) => k,
            None => return Ok(()),
        };
        let mon_key = match self.get_monitor_by_id(monitor_id) {
            Some(k) => k,
            None => return Ok(()),
        };
        let monitor = self.monitors.get(mon_key).unwrap();

        let show_bar = monitor
            .pertag
            .as_ref()
            .and_then(|p| p.show_bars.get(p.cur_tag))
            .copied()
            .unwrap_or(true);

        let (client_win, client_height) = if let Some(client) = self.clients.get_mut(client_key) {
            if show_bar {
                let pad = CONFIG.status_bar_padding();
                client.geometry.x = monitor.geometry.m_x + pad;
                client.geometry.y = monitor.geometry.m_y + pad;
                client.geometry.w = monitor.geometry.m_w - 2 * pad;
                client.geometry.h = CONFIG.status_bar_height();

                self.backend.window_ops().configure_xywh_border(
                    WindowId(client.win.into()),
                    Some(client.geometry.x),
                    Some(client.geometry.y),
                    Some(client.geometry.w as u32),
                    Some(client.geometry.h as u32),
                    None,
                )?;
                (client.win, Some(client.geometry.h))
            } else {
                self.backend.window_ops().configure_xywh_border(
                    WindowId(client.win.into()),
                    Some(-1000),
                    Some(-1000),
                    None,
                    None,
                    None,
                )?;
                (client.win, None)
            }
        } else {
            self.backend.window_ops().flush()?;
            return Ok(());
        };

        if let Some(height) = client_height {
            self.set_bar_strut(client_win, monitor, height)?;
        } else {
            self.remove_bar_strut(client_win)?;
        }
        self.backend.window_ops().flush()?;
        Ok(())
    }

    fn setup_statusbar_window_by_key(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };
        info!(
            "[setup_statusbar_window_by_key] Setting up statusbar window 0x{:x}",
            win
        );

        let mask_bits = (EventMaskBits::STRUCTURE_NOTIFY
            | EventMaskBits::PROPERTY_CHANGE
            | EventMaskBits::ENTER_WINDOW)
            .bits();
        self.backend
            .window_ops()
            .change_event_mask(WindowId(win.into()), mask_bits)?;
        self.configure_client(client_key)?;
        self.backend.window_ops().flush()?;
        info!(
            "[setup_statusbar_window_by_key] Statusbar window setup completed for 0x{:x}",
            win
        );
        Ok(())
    }

    // 辅助函数：根据ID获取显示器
    fn get_monitor_by_id(&self, monitor_id: i32) -> Option<MonitorKey> {
        self.monitors
            .iter()
            .find(|(_, monitor)| monitor.num == monitor_id)
            .map(|(key, _)| key)
    }

    fn maprequest(&mut self, window: u32) -> Result<(), Box<dyn std::error::Error>> {
        let window_attr = self
            .backend
            .window_ops()
            .get_window_attributes(WindowId(window.into()))?;
        if window_attr.override_redirect {
            debug!(
                "Ignoring map request for override_redirect window: {}",
                window
            );
            return Ok(());
        }
        if self.wintoclient(window).is_none() {
            let geom = self
                .backend
                .window_ops()
                .get_geometry_translated(WindowId(window.into()))?;
            self.manage(window, &geom)?;
        } else {
            debug!("Window {} is already managed, ignoring map request", window);
        }
        Ok(())
    }

    fn monocle(&mut self, mon_key: MonitorKey) {
        info!("[monocle]");

        // 获取监视器信息
        let (wx, wy, ww, wh, monitor_num) = if let Some(monitor) = self.monitors.get(mon_key) {
            (
                monitor.geometry.w_x,
                monitor.geometry.w_y,
                monitor.geometry.w_w,
                monitor.geometry.w_h,
                monitor.num,
            )
        } else {
            warn!("[monocle] Monitor {:?} not found", mon_key);
            return;
        };

        // 统计可见客户端数量并收集平铺客户端
        let mut visible_count = 0u32;
        let mut tiled_clients = Vec::new();

        // 获取监视器的客户端列表
        if let Some(client_keys) = self.monitor_clients.get(mon_key) {
            for &client_key in client_keys {
                if let Some(client) = self.clients.get(client_key) {
                    let is_visible = self.is_client_visible_on_monitor(client_key, mon_key);

                    if is_visible {
                        visible_count += 1;
                        // 收集平铺客户端（可见且非浮动）
                        if !client.state.is_floating {
                            tiled_clients.push((client_key, client.geometry.border_w));
                        }
                    }
                }
            }
        }

        // 更新布局符号
        if visible_count > 0 {
            let formatted_string = format!("[{}]", visible_count);
            if let Some(monitor) = self.monitors.get_mut(mon_key) {
                monitor.lt_symbol = formatted_string.clone();
            }
            info!(
                "[monocle] formatted_string: {}, monitor_num: {}",
                formatted_string, monitor_num
            );
        }

        // 如果没有平铺客户端，直接返回
        if tiled_clients.is_empty() {
            return;
        }

        // 获取Y轴偏移
        let client_y_offset = if let Some(monitor) = self.monitors.get(mon_key) {
            self.get_client_y_offset(monitor)
        } else {
            0
        };
        info!("[monocle] client_y_offset: {}", client_y_offset);

        // 调整所有平铺客户端为全屏大小
        for (client_key, border_w) in tiled_clients {
            self.resize_client(
                client_key,
                wx,
                wy + client_y_offset,
                ww - 2 * border_w,
                wh - 2 * border_w - client_y_offset,
                false,
            );
        }
    }

    fn handle_monitor_switch_by_key(
        &mut self,
        new_monitor_key: Option<MonitorKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 从当前选中显示器的选中客户端上移除焦点
        let current_sel = self.get_selected_client_key();
        if let Some(sel_key) = current_sel {
            self.unfocus(sel_key, true)?;
        }

        // 切换到新显示器
        self.sel_mon = new_monitor_key;

        // 在新显示器上设置焦点
        self.focus(None)?;

        if let Some(monitor_key) = new_monitor_key {
            if let Some(monitor) = self.monitors.get(monitor_key) {
                debug!("Switched to monitor {} via mouse motion", monitor.num);
            }
        }

        Ok(())
    }

    fn unmanage(
        &mut self,
        client_key: Option<ClientKey>,
        destroyed: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_key = match client_key {
            Some(key) => key,
            None => return Ok(()),
        };

        // 获取窗口ID
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            warn!("[unmanage] Client {:?} not found", client_key);
            return Ok(());
        };

        // 检查是否是状态栏
        if Some(win) == self.status_bar_window {
            self.unmanage_statusbar(destroyed)?;
            return Ok(());
        }

        // 常规客户端的 unmanage 逻辑
        self.unmanage_regular_client(client_key, destroyed)?;
        Ok(())
    }

    fn unmanage_statusbar(&mut self, destroyed: bool) -> Result<(), Box<dyn std::error::Error>> {
        if !destroyed {
            self.cleanup_statusbar_window(self.status_bar_window.unwrap())?;
        }
        let cleanup_results = [
            ("terminate_process", self.cleanup_statusbar_processes()),
            ("cleanup_shared_memory", self.cleanup_shared_memory_safe()),
        ];
        for (operation, result) in cleanup_results.iter() {
            if let Err(ref e) = result {
                error!("[unmanage_statusbar] {} failed for {}", operation, e);
            }
        }
        info!("[unmanage_statusbar] Successfully removed statusbar",);
        Ok(())
    }

    fn cleanup_statusbar_window(&mut self, win: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.backend
            .window_ops()
            .change_event_mask(WindowId(win.into()), EventMaskBits::NONE.bits())?;
        self.backend.window_ops().flush()?;
        debug!(
            "[cleanup_statusbar_window] Cleared events for statusbar window {}",
            win
        );
        Ok(())
    }

    /// 安全的共享内存清理方法
    fn cleanup_shared_memory_safe(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(shmem) = self.status_bar_shmem.take() {
            info!("[cleanup_shared_memory_safe] Cleaning up shared memory",);
            drop(shmem);
            #[cfg(unix)]
            {
                if let Ok(c_name) = std::ffi::CString::new(SHARED_PATH) {
                    unsafe {
                        let result = libc::shm_unlink(c_name.as_ptr());
                        if result != 0 {
                            let errno = *libc::__errno_location();
                            if errno != libc::ENOENT {
                                return Ok(());
                            }
                        }
                    }
                }
            }
            info!("[cleanup_shared_memory_safe] Shared memory cleaned successfully",);
            Ok(())
        } else {
            info!("[cleanup_shared_memory_safe] No shared memory found",);
            Ok(())
        }
    }

    fn is_popup_like(&self, client_key: ClientKey) -> bool {
        let c = if let Some(c) = self.clients.get(client_key) {
            c
        } else {
            return false;
        };
        if self
            .backend
            .property_ops()
            .is_popup_type(WindowId(c.win.into()))
        {
            return true;
        }
        if self
            .backend
            .property_ops()
            .transient_for(WindowId(c.win.into()))
            .is_some()
            && (c.geometry.w <= 700 && c.geometry.h <= 700)
        {
            return true;
        }
        false
    }

    fn adjust_client_position(&mut self, client_key: ClientKey) {
        if self.is_popup_like(client_key) {
            // 对弹出式窗口完全不做位置修正，让应用自己控制锚点/偏移
            return;
        }

        let (client_total_width, client_mon_key_opt, win) =
            if let Some(client) = self.clients.get(client_key) {
                (client.total_width(), client.mon, client.win)
            } else {
                error!("[adjust_client_position] Client {:?} not found", client_key);
                return;
            };

        let client_mon_key = if let Some(mon_key) = client_mon_key_opt {
            mon_key
        } else {
            error!("[adjust_client_position] Client has no monitor assigned!");
            return;
        };

        let (mon_wx, mon_wy, mon_ww, mon_wh) =
            if let Some(monitor) = self.monitors.get(client_mon_key) {
                (
                    monitor.geometry.w_x,
                    monitor.geometry.w_y,
                    monitor.geometry.w_w,
                    monitor.geometry.w_h,
                )
            } else {
                error!(
                    "[adjust_client_position] Monitor {:?} not found",
                    client_mon_key
                );
                return;
            };

        info!("[adjust_client_position] 0x{:x}", win);

        // 获取当前客户端的几何信息
        let (mut client_x, mut client_y, client_w, client_h) =
            if let Some(client) = self.clients.get(client_key) {
                (
                    client.geometry.x,
                    client.geometry.y,
                    client.geometry.w,
                    client.geometry.h,
                )
            } else {
                return;
            };

        // 确保窗口的右边界不超过显示器工作区的右边界
        if client_x + client_total_width > mon_wx + mon_ww {
            client_x = mon_wx + mon_ww - client_total_width;
            info!(
                "[adjust_client_position] Adjusted X to prevent overflow: {}",
                client_x
            );
        }

        // 计算客户端总高度
        let client_total_height = if let Some(client) = self.clients.get(client_key) {
            client.total_height()
        } else {
            return;
        };

        // 确保窗口的下边界不超过显示器工作区的下边界
        if client_y + client_total_height > mon_wy + mon_wh {
            client_y = mon_wy + mon_wh - client_total_height;
            info!(
                "[adjust_client_position] Adjusted Y to prevent overflow: {}",
                client_y
            );
        }

        // 确保窗口的左边界不小于显示器工作区的左边界
        if client_x < mon_wx {
            client_x = mon_wx;
            info!(
                "[adjust_client_position] Adjusted X to workarea left: {}",
                client_x
            );
        }

        // 确保窗口的上边界不小于显示器工作区的上边界
        if client_y < mon_wy {
            client_y = mon_wy;
            info!(
                "[adjust_client_position] Adjusted Y to workarea top: {}",
                client_y
            );
        }

        // 确保窗口上边界要低于状态栏高度
        let client_y_offset = if let Some(monitor) = self.monitors.get(client_mon_key) {
            self.get_client_y_offset(monitor)
        } else {
            0
        };

        if client_y < client_y_offset {
            client_y = client_y_offset;
            info!(
                "[adjust_client_position] Adjusted Y to avoid status bar: {}",
                client_y
            );
        }

        // 对于小窗口，居中显示
        if client_w < mon_ww / 3 && client_h < mon_wh / 3 {
            client_x = mon_wx + (mon_ww - client_total_width) / 2;
            client_y = mon_wy + (mon_wh - client_total_height) / 2;
            info!(
                "[adjust_client_position] Centered small window at ({}, {})",
                client_x, client_y
            );
        }

        // 应用调整后的位置
        if let Some(client) = self.clients.get_mut(client_key) {
            client.geometry.x = client_x;
            client.geometry.y = client_y;

            info!(
                "[adjust_client_position] Final position: ({}, {}) {}x{}",
                client.geometry.x, client.geometry.y, client.geometry.w, client.geometry.h
            );
        }
    }

    fn unmanage_regular_client(
        &mut self,
        client_key: ClientKey,
        destroyed: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            info!("[unmanage_regular_client] Removing client {:?}", client);
        }

        // 获取客户端的监视器信息
        let mon_key = self.clients.get(client_key).and_then(|client| client.mon);

        // 清理 pertag 中的选中客户端引用
        if let Some(mon_key) = mon_key {
            self.clear_pertag_references(client_key, mon_key);
        }

        // 从链表中移除客户端
        self.detach(client_key);
        self.detachstack(client_key);

        // 如果窗口没有被销毁，需要清理窗口状态
        if !destroyed {
            self.cleanup_window_state(client_key)?;
        }

        // 从 SlotMap 中移除客户端
        self.clients.remove(client_key);

        // 从顺序列表中移除
        self.client_order.retain(|&k| k != client_key);
        self.client_stack_order.retain(|&k| k != client_key);

        // 重新聚焦和排列
        self.focus(None)?;
        self.update_net_client_list()?;
        if let Some(mon_key) = mon_key {
            self.arrange(Some(mon_key));
        }

        Ok(())
    }

    fn clear_pertag_references(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        if let Some(monitor) = self.monitors.get_mut(mon_key) {
            if let Some(ref mut pertag) = monitor.pertag {
                for i in 0..=CONFIG.tags_length() {
                    if pertag.sel[i] == Some(client_key) {
                        pertag.sel[i] = None;
                    }
                }
            }
        }
    }

    fn cleanup_window_state(
        &self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 获取客户端与必要信息
        let client = if let Some(client) = self.clients.get(client_key) {
            client
        } else {
            return Err("Client not found".into());
        };
        let win = client.win;
        let old_border_w = client.geometry.old_border_w;

        // 抓取服务器，保证接下来的更改原子性
        self.backend.window_ops().grab_server()?;

        // 执行清理操作（单独捕获错误并记录日志，不中断整个流程）
        {
            // 取消事件监听
            if let Err(e) = self
                .backend
                .window_ops()
                .change_event_mask(WindowId(win.into()), EventMaskBits::NONE.bits())
            {
                warn!("[cleanup_window_state] Failed to clear event mask: {:?}", e);
            }

            // 恢复原始边框宽度
            if let Err(e) = self
                .backend
                .window_ops()
                .set_border_width(WindowId(win.into()), old_border_w as u32)
            {
                warn!(
                    "[cleanup_window_state] Failed to restore border width: {:?}",
                    e
                );
            }

            // 取消所有按钮抓取
            if let Err(e) = self
                .backend
                .window_ops()
                .ungrab_all_buttons(WindowId(win.into()))
            {
                warn!("[cleanup_window_state] Failed to ungrab buttons: {:?}", e);
            }

            // 设置客户端状态为 WithdrawnState
            if let Err(e) = self.setclientstate(win, WITHDRAWN_STATE as i64) {
                warn!("[cleanup_window_state] Failed to set client state: {:?}", e);
            }

            // 同步所有 X11 操作
            if let Err(e) = self.backend.window_ops().flush() {
                warn!("[cleanup_window_state] Flush failed: {:?}", e);
            }
        }

        // 释放服务器（无论前面的操作是否成功）
        if let Err(e) = self.backend.window_ops().ungrab_server() {
            warn!("[cleanup_window_state] Ungrab server failed: {:?}", e);
        }
        if let Err(e) = self.backend.window_ops().flush() {
            warn!("[cleanup_window_state] Final flush failed: {:?}", e);
        }

        info!(
            "[cleanup_window_state] Window cleanup completed for 0x{:x}",
            win
        );
        Ok(())
    }

    fn unmapnotify(
        &mut self,
        window: u32,
        from_configure: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[unmapnotify]");
        if let Some(client_key) = self.wintoclient(window) {
            if from_configure {
                debug!("Unmap from configure for window 0x{:x}", window);
                let client = if let Some(client) = self.clients.get(client_key) {
                    client
                } else {
                    return Ok(());
                };
                self.setclientstate(client.win, WITHDRAWN_STATE as i64)?;
            } else {
                debug!("Real unmap for window 0x{:x}, unmanaging", window);
                self.unmanage(Some(client_key), false)?;
            }
        } else {
            debug!("Unmap event for unmanaged window: 0x{:x}", window);
        }
        Ok(())
    }

    fn updategeom(&mut self) -> bool {
        info!("[updategeom]");
        let outputs = self.backend.output_ops().enumerate_outputs();

        let dirty = if outputs.len() <= 1 {
            self.setup_single_monitor()
        } else {
            // 把 outputs 转换为 (x,y,w,h)
            let mons: Vec<(i32, i32, i32, i32)> = outputs
                .iter()
                .map(|o| (o.x, o.y, o.width, o.height))
                .collect();
            self.setup_multiple_monitors(mons)
        };

        if dirty {
            self.sel_mon = self.wintomon(self.backend.root_window().0 as u32);
            if self.sel_mon.is_none() && !self.monitor_order.is_empty() {
                self.sel_mon = self.monitor_order.first().copied();
            }
        }
        dirty
    }

    fn setup_single_monitor(&mut self) -> bool {
        let mut dirty = false;

        if self.monitor_order.is_empty() {
            let new_monitor = self.createmon(CONFIG.show_bar());
            let mon_key = self.insert_monitor(new_monitor);
            self.sel_mon = Some(mon_key);
            dirty = true;
        }

        if let Some(&mon_key) = self.monitor_order.first() {
            if let Some(monitor) = self.monitors.get_mut(mon_key) {
                if monitor.geometry.m_w != self.s_w || monitor.geometry.m_h != self.s_h {
                    dirty = true;
                    monitor.num = 0;
                    monitor.geometry.m_x = 0;
                    monitor.geometry.w_x = 0;
                    monitor.geometry.m_y = 0;
                    monitor.geometry.w_y = 0;
                    monitor.geometry.m_w = self.s_w;
                    monitor.geometry.w_w = self.s_w;
                    monitor.geometry.m_h = self.s_h;
                    monitor.geometry.w_h = self.s_h;
                }
            }
        }

        dirty
    }

    fn setup_multiple_monitors(&mut self, monitors: Vec<(i32, i32, i32, i32)>) -> bool {
        let mut dirty = false;
        let num_detected_monitors = monitors.len();
        let current_num_monitors = self.monitor_order.len();

        // 如果检测到的显示器数量多于当前管理的数量，创建新的显示器
        if num_detected_monitors > current_num_monitors {
            dirty = true;
            for _ in current_num_monitors..num_detected_monitors {
                let new_monitor = self.createmon(CONFIG.show_bar());
                let mon_key = self.insert_monitor(new_monitor);
                info!(
                    "[setup_multiple_monitors] Created new monitor {:?}",
                    mon_key
                );
            }
        }

        // 更新现有显示器的几何信息
        for (i, &(x, y, w, h)) in monitors.iter().enumerate() {
            if let Some(&mon_key) = self.monitor_order.get(i) {
                if let Some(monitor) = self.monitors.get_mut(mon_key) {
                    // 检查几何信息是否需要更新
                    if monitor.geometry.m_x != x
                        || monitor.geometry.m_y != y
                        || monitor.geometry.m_w != w
                        || monitor.geometry.m_h != h
                    {
                        dirty = true;
                        monitor.num = i as i32;
                        monitor.geometry.m_x = x;
                        monitor.geometry.w_x = x;
                        monitor.geometry.m_y = y;
                        monitor.geometry.w_y = y;
                        monitor.geometry.m_w = w;
                        monitor.geometry.w_w = w;
                        monitor.geometry.m_h = h;
                        monitor.geometry.w_h = h;
                    }
                }
            }
        }

        // 如果当前显示器数量多于检测到的数量，移除多余的显示器
        if num_detected_monitors < current_num_monitors {
            dirty = true;
            self.remove_excess_monitors(num_detected_monitors);
        }

        dirty
    }

    fn remove_excess_monitors(&mut self, target_count: usize) {
        // 从后往前移除多余的显示器
        while self.monitor_order.len() > target_count {
            if let Some(mon_key_to_remove) = self.monitor_order.pop() {
                // 将该显示器上的客户端移动到第一个显示器
                self.move_clients_to_first_monitor(mon_key_to_remove);

                // 如果被移除的是当前选中的显示器，切换到第一个
                if self.sel_mon == Some(mon_key_to_remove) {
                    self.sel_mon = self.monitor_order.first().copied();
                }

                // 从所有相关数据结构中移除
                self.monitors.remove(mon_key_to_remove);
                self.monitor_clients.remove(mon_key_to_remove);
                self.monitor_stack.remove(mon_key_to_remove);

                info!(
                    "[remove_excess_monitors] Removed monitor {:?}",
                    mon_key_to_remove
                );
            }
        }
    }

    fn move_clients_to_first_monitor(&mut self, from_monitor_key: MonitorKey) {
        let target_monitor_key = if let Some(&first_mon_key) = self.monitor_order.first() {
            first_mon_key
        } else {
            warn!("[move_clients_to_first_monitor] No target monitor available");
            return;
        };

        // 获取需要移动的客户端
        let clients_to_move: Vec<ClientKey> = self
            .monitor_clients
            .get(from_monitor_key)
            .cloned()
            .unwrap_or_default();

        // 获取目标监视器的标签集
        let target_tags = if let Some(target_monitor) = self.monitors.get(target_monitor_key) {
            target_monitor.tag_set[target_monitor.sel_tags]
        } else {
            1 // 默认标签
        };

        // 移动所有客户端
        for client_key in clients_to_move {
            // 更新客户端的监视器和标签
            if let Some(client) = self.clients.get_mut(client_key) {
                client.mon = Some(target_monitor_key);
                client.state.tags = target_tags;
            }

            // 从原监视器移除
            self.detach_from_monitor(client_key, from_monitor_key);

            // 添加到目标监视器
            self.attach_to_monitor(client_key, target_monitor_key);

            info!(
                "[move_clients_to_first_monitor] Moved client {:?} from monitor {:?} to {:?}",
                client_key, from_monitor_key, target_monitor_key
            );
        }
    }

    fn updatewindowtype(&mut self, client_key: ClientKey) {
        if let Some(client) = self.clients.get(client_key) {
            let win_id = WindowId(client.win.into());
            if let Ok(true) = self.backend.property_ops().is_fullscreen(win_id) {
                let _ = self.setfullscreen(client_key, true);
            }
            if self.backend.property_ops().is_popup_type(win_id) {
                if let Some(c) = self.clients.get_mut(client_key) {
                    c.state.is_floating = true;
                }
            }
        }
    }

    fn updatewmhints(&mut self, client_key: ClientKey) {
        let win = match self.clients.get(client_key) {
            Some(c) => c.win,
            None => return,
        };
        let wid = WindowId(win.into());
        if let Some(hints) = self.backend.property_ops().get_wm_hints(wid) {
            // 处理紧急状态
            if hints.urgent {
                let is_focused = self.is_client_selected(client_key);
                if is_focused {
                    let _ = self.backend.property_ops().set_urgent_hint(wid, false);
                } else {
                    if let Some(c) = self.clients.get_mut(client_key) {
                        c.state.is_urgent = true;
                    }
                }
            } else {
                if let Some(c) = self.clients.get_mut(client_key) {
                    c.state.is_urgent = false;
                }
            }
            // 处理 InputHint
            if let Some(input_ok) = hints.input {
                if let Some(c) = self.clients.get_mut(client_key) {
                    c.state.never_focus = !input_ok;
                }
            } else {
                if let Some(c) = self.clients.get_mut(client_key) {
                    c.state.never_focus = false;
                }
            }
        }
    }

    fn update_bar_message_for_monitor(&mut self, mon_key_opt: Option<MonitorKey>) {
        // info!("[update_bar_message_for_monitor]");

        let mon_key = match mon_key_opt {
            Some(key) => key,
            None => {
                error!("[update_bar_message_for_monitor] Monitor key is None, cannot update bar message.");
                return;
            }
        };

        // 检查监视器是否存在
        let monitor = if let Some(monitor) = self.monitors.get(mon_key) {
            monitor
        } else {
            error!(
                "[update_bar_message_for_monitor] Monitor {:?} not found",
                mon_key
            );
            return;
        };

        self.message = SharedMessage::default();
        let mut monitor_info_for_message = MonitorInfo::default();

        // 设置监视器基本信息
        monitor_info_for_message.monitor_x = monitor.geometry.w_x;
        monitor_info_for_message.monitor_y = monitor.geometry.w_y;
        monitor_info_for_message.monitor_width = monitor.geometry.w_w;
        monitor_info_for_message.monitor_height = monitor.geometry.w_h;
        monitor_info_for_message.monitor_num = monitor.num;
        monitor_info_for_message.set_ltsymbol(&monitor.lt_symbol);

        // 计算标签掩码
        let (occupied_tags_mask, urgent_tags_mask) = self.calculate_tag_masks(mon_key);

        // 处理标签状态
        for i in 0..CONFIG.tags_length() {
            let tag_bit = 1 << i;

            // 计算是否为填充标签（当前选中客户端是否在此标签上）
            let is_filled_tag = self.is_filled_tag(mon_key, tag_bit);

            // 获取监视器信息（重新借用）
            let monitor = self.monitors.get(mon_key).unwrap();
            let active_tagset = monitor.tag_set[monitor.sel_tags];

            let is_selected_tag = (active_tagset & tag_bit) != 0;
            let is_urgent_tag = (urgent_tags_mask & tag_bit) != 0;
            let is_occupied_tag = (occupied_tags_mask & tag_bit) != 0;

            let tag_status = TagStatus::new(
                is_selected_tag,
                is_urgent_tag,
                is_filled_tag,
                is_occupied_tag,
            );

            monitor_info_for_message.set_tag_status(i, tag_status);
        }

        // 设置选中客户端名称
        let selected_client_name = self.get_selected_client_name(mon_key);
        monitor_info_for_message.set_client_name(&selected_client_name);

        self.message.monitor_info = monitor_info_for_message;
    }

    /// 计算标签掩码（占用和紧急）
    fn calculate_tag_masks(&self, mon_key: MonitorKey) -> (u32, u32) {
        let mut occupied_tags_mask = 0u32;
        let mut urgent_tags_mask = 0u32;

        // 遍历该监视器的所有客户端
        if let Some(client_keys) = self.monitor_clients.get(mon_key) {
            for &client_key in client_keys {
                if let Some(client) = self.clients.get(client_key) {
                    occupied_tags_mask |= client.state.tags;
                    if client.state.is_urgent {
                        urgent_tags_mask |= client.state.tags;
                    }
                }
            }
        }

        (occupied_tags_mask, urgent_tags_mask)
    }

    /// 检查指定标签是否为"填充"状态（选中客户端在此标签上）
    fn is_filled_tag(&self, mon_key: MonitorKey, tag_bit: u32) -> bool {
        // 检查是否为全局选中的监视器
        if self.sel_mon != Some(mon_key) {
            return false;
        }

        // 获取选中的客户端
        if let Some(monitor) = self.monitors.get(mon_key) {
            if let Some(sel_client_key) = monitor.sel {
                if let Some(client) = self.clients.get(sel_client_key) {
                    return (client.state.tags & tag_bit) != 0;
                }
            }
        }

        false
    }

    /// 获取选中客户端的名称
    fn get_selected_client_name(&self, mon_key: MonitorKey) -> String {
        if let Some(monitor) = self.monitors.get(mon_key) {
            if let Some(sel_client_key) = monitor.sel {
                if let Some(client) = self.clients.get(sel_client_key) {
                    return client.name.clone();
                }
            }
        }
        String::new()
    }
}
