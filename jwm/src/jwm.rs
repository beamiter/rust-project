use libc::{close, setsid, sigaction, sigemptyset, SIGCHLD, SIG_DFL};

use log::info;
use log::warn;
use log::{debug, error};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;

use serde::{Deserialize, Serialize};
use slotmap::{DefaultKey, SecondaryMap, SlotMap};
use std::cell::RefCell;
use std::cell::RefMut;
use std::cmp::{max, min};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::usize;

use x11rb::connection::Connection;
use x11rb::errors::{ReplyError, ReplyOrIdError};
use x11rb::properties::WmSizeHints;
use x11rb::protocol::render::Pictforminfo;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::COPY_DEPTH_FROM_PARENT;

use crate::config::CONFIG;
use crate::xcb_util::SchemeType;
use crate::xcb_util::{test_all_cursors, Atoms, CursorManager, ThemeManager};
use shared_structures::CommandType;
use shared_structures::SharedCommand;
use shared_structures::{MonitorInfo, SharedMessage, SharedRingBuffer, TagStatus};

// definitions for initial window state.
pub const WITHDRAWN_STATE: u8 = 0;
pub const NORMAL_STATE: u8 = 1;
pub const ICONIC_STATE: u8 = 2;
pub const CLIENT_STORAGE_PATH: &str = "/tmp/jwm/client_storage.bin";

pub type ClientKey = DefaultKey;
pub type MonitorKey = DefaultKey;

lazy_static::lazy_static! {
    pub static ref BUTTONMASK: EventMask  = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE;
    pub static ref MOUSEMASK: EventMask  = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WMClientRestore {
    pub name: String,
    pub class: String,
    pub instance: String,
    pub win: Window,
    pub geometry: ClientGeometry,
    pub size_hints: SizeHints,
    pub state: ClientState,
    pub monitor_num: u32,
}

impl WMClientRestore {
    /// 从 WMClient 创建可序列化的版本
    pub fn from_client(client: &WMClient) -> Self {
        // let monitor_num = client.mon.as_ref().map_or(0, |v| v.borrow().num as u32);
        Self {
            name: client.name.clone(),
            class: client.class.clone(),
            instance: client.instance.clone(),
            win: client.win,
            geometry: client.geometry.clone(),
            size_hints: client.size_hints.clone(),
            state: client.state.clone(),
            monitor_num: 0,
        }
    }
    pub fn to_client(&self) -> WMClient {
        WMClient {
            name: self.name.clone(),
            class: self.class.clone(),
            instance: self.instance.clone(),
            win: self.win,
            geometry: self.geometry.clone(),
            size_hints: self.size_hints.clone(),
            state: self.state.clone(),
            mon: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WMClientCollection {
    pub clients: HashMap<Window, WMClientRestore>, // 以 Window ID 为键
    pub timestamp: u64,                            // 保存时间戳
}

impl WMClientCollection {
    /// 创建新的客户端集合
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// 从客户端向量创建集合
    pub fn from_clients(clients: Vec<WMClientRestore>) -> Self {
        let mut client_map = HashMap::new();
        for client in clients {
            client_map.insert(client.win, client);
        }

        Self {
            clients: client_map,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// 添加客户端
    pub fn add_client(&mut self, client: WMClientRestore) {
        self.clients.insert(client.win, client);
        self.update_timestamp();
    }

    /// 根据 Window ID 获取客户端
    pub fn get_client(&self, win_id: Window) -> Option<&WMClientRestore> {
        self.clients.get(&win_id)
    }

    /// 根据 Window ID 获取可变客户端引用
    pub fn get_client_mut(&mut self, win_id: Window) -> Option<&mut WMClientRestore> {
        self.clients.get_mut(&win_id)
    }

    /// 移除客户端
    pub fn remove_client(&mut self, win_id: Window) -> Option<WMClientRestore> {
        let result = self.clients.remove(&win_id);
        if result.is_some() {
            self.update_timestamp();
        }
        result
    }

    /// 检查是否包含指定的窗口
    pub fn contains_window(&self, win_id: Window) -> bool {
        self.clients.contains_key(&win_id)
    }

    /// 获取所有客户端的引用
    pub fn get_all_clients(&self) -> impl Iterator<Item = &WMClientRestore> {
        self.clients.values()
    }

    /// 获取所有窗口 ID
    pub fn get_all_window_ids(&self) -> impl Iterator<Item = &Window> {
        self.clients.keys()
    }

    /// 获取客户端数量
    pub fn len(&self) -> usize {
        self.clients.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    /// 更新时间戳
    pub fn update_timestamp(&mut self) {
        self.timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// 清空所有客户端
    pub fn clear(&mut self) {
        self.clients.clear();
        self.update_timestamp();
    }

    /// 保存到文件
    pub fn save_to_file<P: AsRef<std::path::Path>>(
        &self,
        path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let encoded = bincode::serialize(self)?;
        std::fs::write(path, encoded)?;
        Ok(())
    }

    /// 从文件加载
    pub fn load_from_file<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let data = std::fs::read(path)?;
        let decoded = bincode::deserialize(&data)?;
        Ok(decoded)
    }

    /// 静态方法：从多个客户端保存到文件
    pub fn save_clients_to_file<P: AsRef<std::path::Path>>(
        clients: &[WMClientRestore],
        path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let collection = Self::from_clients(clients.to_vec());
        collection.save_to_file(path)
    }

    /// 静态方法：从文件加载并返回客户端向量
    pub fn load_clients_from_file<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<Vec<WMClientRestore>, Box<dyn std::error::Error>> {
        let collection = Self::load_from_file(path)?;
        Ok(collection.clients.into_values().collect())
    }

    /// 静态方法：从文件加载并返回 HashMap
    pub fn load_clients_as_map<P: AsRef<std::path::Path>>(
        path: P,
    ) -> Result<HashMap<Window, WMClientRestore>, Box<dyn std::error::Error>> {
        let collection = Self::load_from_file(path)?;
        Ok(collection.clients)
    }

    /// 根据类名查找客户端
    pub fn find_by_class(&self, class: &str) -> Vec<&WMClientRestore> {
        self.clients
            .values()
            .filter(|client| client.class == class)
            .collect()
    }

    /// 根据实例名查找客户端
    pub fn find_by_instance(&self, instance: &str) -> Vec<&WMClientRestore> {
        self.clients
            .values()
            .filter(|client| client.instance == instance)
            .collect()
    }

    /// 根据窗口名称查找客户端
    pub fn find_by_name(&self, name: &str) -> Vec<&WMClientRestore> {
        self.clients
            .values()
            .filter(|client| client.name.contains(name))
            .collect()
    }

    /// 根据状态过滤客户端
    pub fn filter_by_state(&self, state: &ClientState) -> Vec<&WMClientRestore> {
        self.clients
            .values()
            .filter(|client| &client.state == state)
            .collect()
    }

    /// 批量更新客户端状态
    pub fn batch_update_state(&mut self, win_ids: &[Window], new_state: ClientState) {
        for &win_id in win_ids {
            if let Some(client) = self.clients.get_mut(&win_id) {
                client.state = new_state.clone();
            }
        }
        self.update_timestamp();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WMClient {
    // === 基本信息 ===
    pub name: String,
    pub class: String,
    pub instance: String,
    pub win: Window,

    // === 几何信息 ===
    pub geometry: ClientGeometry,
    pub size_hints: SizeHints,

    // === 状态信息 ===
    pub state: ClientState,

    // === 链表和关联 ===
    pub mon: Option<MonitorKey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    /// 检查是否为状态栏（需要CONFIG常量）
    pub fn is_status_bar(&self) -> bool {
        // 这里需要根据你的CONFIG实现来调整
        // 示例实现：
        self.name.contains("bar") || self.class.contains("bar")
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

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum WMShowBarEnum {
    Keep(bool),
    Toggle(bool),
}
impl WMShowBarEnum {
    pub fn show_bar(&self) -> &bool {
        match self {
            Self::Keep(val) => val,
            Self::Toggle(val) => val,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WMButton {
    pub click_type: WMClickType,
    pub mask: KeyButMask,
    pub button: ButtonIndex,
    pub func: Option<WMFuncType>,
    pub arg: WMArgEnum,
}
impl WMButton {
    pub fn new(
        click_type: WMClickType,
        mask: KeyButMask,
        button: ButtonIndex,
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
    pub mask: KeyButMask,
    pub key_sym: Keysym,
    pub func_opt: Option<WMFuncType>,
    pub arg: WMArgEnum,
}
impl WMKey {
    pub fn new(mod0: KeyButMask, keysym: Keysym, func: Option<WMFuncType>, arg: WMArgEnum) -> Self {
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

/// 表示一个窗口重排操作
#[derive(Debug, Clone)]
struct RestackOperation {
    window: Window,
    stack_mode: StackMode,
    sibling: Option<Window>,
}

pub struct Jwm {
    pub stext_max_len: usize,
    pub s_w: i32,
    pub s_h: i32,
    pub numlock_mask: KeyButMask,
    pub running: AtomicBool,
    pub is_restarting: AtomicBool,
    pub cursor_manager: CursorManager,
    pub theme_manager: ThemeManager,
    pub visual_id: Visualid,
    pub depth: u8,
    pub color_map: Colormap,
    pub status_bar_shmem: HashMap<i32, SharedRingBuffer>,
    pub status_bar_child: HashMap<i32, Child>,
    pub message: SharedMessage,

    // 新的SlotMap存储结构
    pub clients: SlotMap<ClientKey, WMClient>,
    pub monitors: SlotMap<MonitorKey, WMMonitor>,
    // 维护顺序的向量
    pub client_order: Vec<ClientKey>, // 客户端顺序（替代next链表）
    pub client_stack_order: Vec<ClientKey>, // 堆栈顺序（替代stack_next链表）
    pub monitor_order: Vec<MonitorKey>, // 监视器顺序
    // 当前选中的监视器
    pub sel_mon: Option<MonitorKey>,
    pub motion_mon: Option<MonitorKey>,
    // 每个监视器的客户端列表
    pub monitor_clients: SecondaryMap<MonitorKey, Vec<ClientKey>>,
    pub monitor_stack: SecondaryMap<MonitorKey, Vec<ClientKey>>,

    // 状态栏专用管理
    pub status_bar_flags: HashMap<i32, WMShowBarEnum>, // monitor_id -> show_bar_enum
    pub status_bar_clients: HashMap<i32, Rc<RefCell<WMClient>>>, // monitor_id -> statusbar_client
    pub status_bar_windows: HashMap<Window, i32>,      // window_id -> monitor_id (快速查找)
    pub pending_bar_updates: HashSet<i32>,

    pub x11rb_conn: RustConnection,
    pub x11rb_root: Window,
    pub x11rb_screen: Screen,
    pub atoms: Atoms,
    keycode_cache: HashMap<u8, u32>,
    pub enable_move_cursor_to_client_center: bool,
    pub restored_clients_info: WMClientCollection,
}

impl Jwm {
    fn handler(&mut self, event: Event) -> Result<(), Box<dyn std::error::Error>> {
        match event {
            // Event::ButtonPress(e) => self.buttonpress(&e)?,
            // Event::ClientMessage(e) => self.clientmessage(&e)?,
            // Event::ConfigureRequest(e) => self.configurerequest(&e)?,
            // Event::ConfigureNotify(e) => self.configurenotify(&e)?,
            // Event::DestroyNotify(e) => self.destroynotify(&e)?,
            // Event::EnterNotify(e) => self.enternotify(&e)?,
            // Event::Expose(e) => self.expose(&e)?,
            // Event::FocusIn(e) => self.focusin(&e)?,
            // Event::KeyPress(e) => self.keypress(&e)?,
            // Event::MappingNotify(e) => self.mappingnotify(&e)?,
            // Event::MapRequest(e) => self.maprequest(&e)?,
            // Event::MotionNotify(e) => self.motionnotify(&e)?,
            // Event::PropertyNotify(e) => self.propertynotify(&e)?,
            // Event::UnmapNotify(e) => self.unmapnotify(&e)?,
            _ => {
                debug!("Unsupported event type: {:?}", event);
            }
        }
        Ok(())
    }

    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        info!("[new] Starting JWM initialization");

        // 显示当前的 X11 环境信息
        // Self::log_x11_environment();

        // 尝试连接到 X11 服务器，添加错误处理
        info!("[new] Connecting to X11 server");
        let (x11rb_conn, x11rb_screen_num) =
            match x11rb::rust_connection::RustConnection::connect(None) {
                Ok(conn) => {
                    info!("[new] X11 connection established");
                    conn
                }
                Err(e) => {
                    error!("[new] Failed to connect to X11 server: {}", e);
                    return Err(format!("X11 connection failed: {}", e).into());
                }
            };

        info!("[new] Getting atoms");
        let atoms = match Atoms::new(&x11rb_conn) {
            Ok(cookie) => match cookie.reply() {
                Ok(atoms) => {
                    info!("[new] Atoms retrieved successfully");
                    atoms
                }
                Err(e) => {
                    error!("[new] Failed to get atoms reply: {}", e);
                    return Err(format!("Atoms reply failed: {}", e).into());
                }
            },
            Err(e) => {
                error!("[new] Failed to request atoms: {}", e);
                return Err(format!("Atoms request failed: {}", e).into());
            }
        };

        info!("[new] Testing cursors");
        let _ = test_all_cursors(&x11rb_conn);

        let x11rb_screen = x11rb_conn.setup().roots[x11rb_screen_num].clone();
        let s_w = x11rb_screen.width_in_pixels.into();
        let s_h = x11rb_screen.height_in_pixels.into();
        let x11rb_root = x11rb_screen.root;

        info!(
            "[new] Screen info - screen_num: {}, resolution: {}x{}, root: 0x{:x}",
            x11rb_screen_num, s_w, s_h, x11rb_root
        );

        info!("[new] Creating cursor manager");
        let cursor_manager = match CursorManager::new(&x11rb_conn) {
            Ok(cm) => {
                info!("[new] Cursor manager created");
                cm
            }
            Err(e) => {
                error!("[new] Failed to create cursor manager: {}", e);
                return Err(format!("Cursor manager creation failed: {}", e).into());
            }
        };

        info!("[new] Creating theme manager");
        let theme_manager = match ThemeManager::create_default(&x11rb_conn, &x11rb_screen.clone()) {
            Ok(tm) => {
                info!("[new] Theme manager created");
                tm
            }
            Err(e) => {
                error!("[new] Failed to create theme manager: {}", e);
                return Err(format!("Theme manager creation failed: {}", e).into());
            }
        };

        info!("[new] JWM initialization completed successfully");

        Ok(Jwm {
            stext_max_len: 512,
            s_w,
            s_h,
            numlock_mask: KeyButMask::default(),
            running: AtomicBool::new(true),
            is_restarting: AtomicBool::new(false),
            theme_manager,
            cursor_manager,

            clients: SlotMap::new(),
            monitors: SlotMap::new(),
            client_order: Vec::new(),
            client_stack_order: Vec::new(),
            monitor_order: Vec::new(),
            sel_mon: None,
            motion_mon: None,
            monitor_clients: SecondaryMap::new(),
            monitor_stack: SecondaryMap::new(),

            visual_id: 0,
            depth: 0,
            color_map: 0,
            status_bar_shmem: HashMap::new(),
            status_bar_child: HashMap::new(),
            message: SharedMessage::default(),
            status_bar_flags: HashMap::new(),
            status_bar_clients: HashMap::new(),
            status_bar_windows: HashMap::new(),
            pending_bar_updates: HashSet::new(),
            x11rb_conn,
            x11rb_root,
            x11rb_screen,
            atoms,
            keycode_cache: HashMap::new(),
            enable_move_cursor_to_client_center: false,
            restored_clients_info: WMClientCollection::new(),
        })
    }

    // 创建新的客户端
    pub fn insert_client(&mut self, client: WMClient) -> ClientKey {
        let key = self.clients.insert(client);
        self.client_order.push(key);
        key
    }

    // 创建新的监视器
    pub fn insert_monitor(&mut self, monitor: WMMonitor) -> MonitorKey {
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

    /// 获取当前选中的客户端键
    fn get_selected_client(&self) -> Option<ClientKey> {
        self.sel_mon
            .and_then(|sel_mon_key| self.monitors.get(sel_mon_key))
            .and_then(|monitor| monitor.sel)
    }

    // 获取监视器的所有客户端
    pub fn get_monitor_clients(&self, mon_key: MonitorKey) -> &[ClientKey] {
        self.monitor_clients
            .get(mon_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    // 获取监视器的堆栈顺序
    pub fn get_monitor_stack(&self, mon_key: MonitorKey) -> &[ClientKey] {
        self.monitor_stack
            .get(mon_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn get_sel_mon(&self) -> Option<&WMMonitor> {
        self.sel_mon
            .and_then(|sel_mon_key| self.monitors.get(sel_mon_key))
            .and_then(|monitor| Some(monitor))
    }

    fn get_selected_client_key(&self) -> Option<ClientKey> {
        self.sel_mon
            .and_then(|sel_mon_key| self.monitors.get(sel_mon_key))
            .and_then(|monitor| monitor.sel)
    }

    pub fn attach(&mut self, client_key: ClientKey) {
        if let Some(client) = self.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
                    // 插入到列表开头（模拟链表头插入）
                    client_list.insert(0, client_key);
                }
            }
        }
    }

    pub fn detach(&mut self, client_key: ClientKey) {
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

    pub fn attachstack(&mut self, client_key: ClientKey) {
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

    pub fn detachstack(&mut self, client_key: ClientKey) {
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

    pub fn nexttiled(
        &self,
        mon_key: MonitorKey,
        start_from: Option<ClientKey>,
    ) -> Option<ClientKey> {
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

    pub fn pop(&mut self, client_key: ClientKey) {
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

    pub fn find_client_key(&self, target_client: &WMClient) -> Option<ClientKey> {
        self.wintoclient(target_client.win)
    }

    pub fn wintoclient(&self, win: Window) -> Option<ClientKey> {
        // 首先检查状态栏
        if let Some(&monitor_id) = self.status_bar_windows.get(&win) {
            return self.status_bar_clients.get(&monitor_id).and_then(|_| {
                // 需要在SlotMap中查找对应的key
                self.clients
                    .iter()
                    .find(|(_, client)| client.win == win)
                    .map(|(key, _)| key)
            });
        }
        // 查找常规客户端
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

    /// 设置 X11 环境变量
    pub fn set_x11_environment(env_vars: &HashMap<String, String>) {
        for (key, value) in env_vars {
            env::set_var(key, value);
            info!("[set_x11_environment] Set {}: {}", key, value);
        }
    }

    /// 简化的重启方法 - 使用信号机制
    pub fn restart(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[restart] Preparing for restart via signal");
        self.running.store(false, Ordering::SeqCst);
        self.is_restarting.store(true, Ordering::SeqCst);
        info!("[restart] Restart prepared, main loop will exit");
        Ok(())
    }

    fn mark_bar_update_needed(&mut self, monitor_id: Option<i32>) {
        if let Some(id) = monitor_id {
            self.pending_bar_updates.insert(id);
        } else {
            for val in self.monitors.values() {
                self.pending_bar_updates.insert(val.num);
            }
        }
    }

    fn are_equal_rc<T>(a: &Option<Rc<RefCell<T>>>, b: &Option<Rc<RefCell<T>>>) -> bool {
        match (a, b) {
            (Some(rc_a), Some(rc_b)) => Rc::ptr_eq(rc_a, rc_b),
            _ => false,
        }
    }

    fn clean_mask(&self, mask: u16) -> KeyButMask {
        // 第一步：移除NumLock和CapsLock
        let mask_without_locks = mask & !(self.numlock_mask.bits() | KeyButMask::LOCK.bits());
        // 第二步：只保留真正的修饰键
        let modifier_mask = KeyButMask::SHIFT
            | KeyButMask::CONTROL
            | KeyButMask::MOD1
            | KeyButMask::MOD2
            | KeyButMask::MOD3
            | KeyButMask::MOD4
            | KeyButMask::MOD5;
        KeyButMask::from(mask_without_locks) & modifier_mask
    }

    /// 获取窗口的 WM_CLASS（即类名和实例名）
    pub fn get_wm_class<C: Connection>(conn: &C, window: Window) -> Option<(String, String)> {
        // Get the WM_NAME property of the window
        let cookie = conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .unwrap();
        if let Ok(prop) = cookie.reply() {
            // 2. 检查属性是否存在且格式正确
            if prop.type_ != u32::from(AtomEnum::STRING) || prop.format != 8 {
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
    pub fn applyrules(&mut self, c: &Rc<RefCell<WMClient>>) {
        info!("[applyrules]");
        // rule matching
        let mut c = c.borrow_mut();
        c.state.is_floating = false;
        if let Some((inst, cls)) = Self::get_wm_class(&self.x11rb_conn, c.win as u32) {
            c.instance = inst;
            c.class = cls;
            info!(
                "win: 0x{}, name: {}, instance: {}, class: {}",
                c.win, c.name, c.instance, c.class
            );
        } else {
            // 对于这种完全未命名的，直接设置为floating
            c.state.is_floating = true;
            info!("[applyrules] special case");
        }
        for r in &CONFIG.get_rules() {
            if r.name.is_empty() && r.class.is_empty() && r.instance.is_empty() {
                continue;
            }
            if (r.name.is_empty() || c.name.find(&r.name).is_some())
                && (r.class.is_empty() || c.class.find(&r.class).is_some())
                && (r.instance.is_empty() || c.instance.find(&r.instance).is_some())
            {
                info!("[applyrules] rule: {:?}", r);
                c.state.is_floating = r.is_floating;
                c.state.tags |= r.tags as u32;
                for (key, value) in &self.monitors {
                    if value.num == r.monitor {
                        c.mon = Some(key);
                        break;
                    }
                }
            }
        }
        let condition = c.state.tags & CONFIG.tagmask();
        if condition > 0 {
            c.state.tags = condition;
        } else {
            if let Some(client_monitor) = self.monitors.get(c.mon.unwrap()) {
                c.state.tags = client_monitor.tag_set[client_monitor.sel_tags];
            }
        };
        info!(
            "[applyrules] class: {}, instance: {}, name: {}, tags: {}, floating: {}",
            c.class, c.instance, c.name, c.state.tags, c.state.is_floating
        );
    }

    pub fn applysizehints(
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
            let current_ratio = w as f32 / h as f32;

            if current_ratio > hints.max_aspect {
                // 太宽，调整宽度
                w = (h as f32 * hints.max_aspect + 0.5) as i32;
            } else if current_ratio < 1.0 / hints.min_aspect {
                // 太高，调整高度
                h = (w as f32 * hints.min_aspect + 0.5) as i32;
            }
        }

        (w, h)
    }

    /// 更新 updatesizehints 方法签名
    pub fn updatesizehints(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        // 获取 WM_NORMAL_HINTS
        let reply = match WmSizeHints::get_normal_hints(&self.x11rb_conn, win)?.reply()? {
            Some(reply) => reply,
            None => {
                // 没有 WM_NORMAL_HINTS 属性，使用默认值
                if let Some(client) = self.clients.get_mut(client_key) {
                    client.size_hints.hints_valid = false;
                }
                return Ok(());
            }
        };

        // 更新客户端的尺寸提示
        if let Some(client) = self.clients.get_mut(client_key) {
            if let Some((w, h)) = reply.base_size {
                client.size_hints.base_w = w;
                client.size_hints.base_h = h;
            }
            if let Some((w, h)) = reply.size_increment {
                client.size_hints.inc_w = w;
                client.size_hints.inc_h = h;
            }
            if let Some((w, h)) = reply.max_size {
                client.size_hints.max_w = w;
                client.size_hints.max_h = h;
            }
            if let Some((w, h)) = reply.min_size {
                client.size_hints.min_w = w;
                client.size_hints.min_h = h;
            }
            if let Some((min_aspect, max_aspect)) = reply.aspect {
                client.size_hints.min_aspect =
                    min_aspect.numerator as f32 / min_aspect.denominator as f32;
                client.size_hints.max_aspect =
                    max_aspect.numerator as f32 / max_aspect.denominator as f32;
            }

            client.state.is_fixed = (client.size_hints.max_w > 0)
                && (client.size_hints.max_h > 0)
                && (client.size_hints.max_w == client.size_hints.min_w)
                && (client.size_hints.max_h == client.size_hints.min_h);

            client.size_hints.hints_valid = true;
        }

        Ok(())
    }

    /// 优化后的清理函数 - 只处理必须手动清理的资源
    pub fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup] Starting essential cleanup (letting Rust handle memory)");

        // 1. 保存客户端状态（在清理 X11 资源之前）
        if let Err(e) = self.store_all_clients() {
            warn!("[cleanup] Failed to store client state: {:?}", e);
        }

        // 2. 清理 X11 相关资源（必须手动处理）
        self.cleanup_x11_resources()?;

        // 3. 清理系统资源（必须手动处理）
        self.cleanup_system_resources()?;

        // 4. 同步所有 X11 操作
        self.x11rb_conn.flush()?;

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

    /// 清理所有客户端的 X11 状态（不是为了释放内存，而是为了恢复窗口状态）
    fn cleanup_all_clients_x11_state(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup_all_clients_x11_state] Restoring client window states");
        let mut clients_to_cleanup = Vec::new();
        // 收集所有需要清理的客户端
        for &mon_key in &self.monitor_order {
            if let Some(stack_clients) = self.monitor_stack.get(mon_key) {
                for &client_key in stack_clients {
                    clients_to_cleanup.push(client_key);
                }
            }
        }

        // 批量清理客户端 X11 状态
        for client_key in clients_to_cleanup {
            let client_opt = self.clients.get(client_key).cloned();
            if let Some(client) = client_opt {
                if let Err(e) = self.cleanup_single_client_x11_state(&client) {
                    warn!(
                        "[cleanup_all_clients_x11_state] Failed to cleanup client {}: {:?}",
                        client.win, e
                    );
                    // 继续处理其他客户端，不要因为一个失败就停止
                }
            }
        }
        Ok(())
    }

    /// 清理单个客户端的 X11 状态
    fn cleanup_single_client_x11_state(
        &mut self,
        client: &WMClient,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (win, old_border_w) = (client.win, client.geometry.old_border_w);

        // 抓取服务器确保操作原子性
        self.x11rb_conn.grab_server()?;

        let result = self.restore_client_x11_state(win, old_border_w, client);

        // 无论成功失败都要释放服务器
        let _ = self.x11rb_conn.ungrab_server();

        result
    }

    /// 恢复客户端的 X11 状态
    fn restore_client_x11_state(
        &mut self,
        win: Window,
        old_border_w: i32,
        client: &WMClient,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 取消事件选择
        let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT);
        if let Err(e) = self.x11rb_conn.change_window_attributes(win, &aux) {
            warn!(
                "[restore_client_x11_state] Failed to clear events for {}: {:?}",
                win, e
            );
        }
        // 恢复原始边框宽度
        if let Err(e) = self.set_window_border_width(win, old_border_w as u32) {
            warn!(
                "[restore_client_x11_state] Failed to restore border for {}: {:?}",
                win, e
            );
        }
        // 取消按钮抓取
        if let Err(e) = self
            .x11rb_conn
            .ungrab_button(ButtonIndex::ANY, win, ModMask::ANY.into())
        {
            warn!(
                "[restore_client_x11_state] Failed to ungrab buttons for {}: {:?}",
                win, e
            );
        }
        // 设置客户端状态为 WithdrawnState
        if let Err(e) = self.setclientstate(client, WITHDRAWN_STATE as i64) {
            warn!(
                "[restore_client_x11_state] Failed to set withdrawn state for {}: {:?}",
                win, e
            );
        }
        Ok(())
    }

    /// 清理状态栏进程
    fn cleanup_statusbar_processes(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let statusbar_monitor_ids: Vec<i32> = self.status_bar_child.keys().cloned().collect();
        for monitor_id in statusbar_monitor_ids {
            if let Err(e) = self.terminate_status_bar_process_safe(monitor_id) {
                warn!(
                    "[cleanup_statusbar_processes] Failed to terminate statusbar {}: {}",
                    monitor_id, e
                );
            }
        }
        // 清理状态栏客户端映射（让 Drop 处理实际的内存释放）
        self.status_bar_clients.clear();
        self.status_bar_windows.clear();
        self.status_bar_flags.clear();

        Ok(())
    }

    /// 清理共享内存资源
    fn cleanup_shared_memory_resources(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 只清理需要显式处理的共享内存资源
        let monitor_ids: Vec<i32> = self.status_bar_shmem.keys().cloned().collect();
        for monitor_id in monitor_ids {
            // 显式删除系统共享内存对象（如果需要）
            self.cleanup_system_shared_memory(monitor_id)?;
        }
        // 清理映射表（Rust 会自动释放 SharedRingBuffer 对象）
        self.status_bar_shmem.clear();
        Ok(())
    }

    /// 清理系统级共享内存
    fn cleanup_system_shared_memory(
        &self,
        monitor_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(unix)]
        {
            let shared_path = format!("/dev/shm/monitor_{}", monitor_id);
            if std::path::Path::new(&shared_path).exists() {
                if let Err(e) = std::fs::remove_file(&shared_path) {
                    warn!(
                        "[cleanup_system_shared_memory] Failed to remove {}: {}",
                        shared_path, e
                    );
                }
            }
        }
        Ok(())
    }

    fn cleanup_key_grabs(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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
                warn!("[cleanup_key_grabs] Failed to send ungrab request: {:?}", e);
            }
        }
        Ok(())
    }

    fn reset_input_focus(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        match self
            .x11rb_conn
            .set_input_focus(InputFocus::POINTER_ROOT, self.x11rb_root, 0u32)
        {
            Ok(cookie) => {
                if let Err(e) = cookie.check() {
                    warn!("[reset_input_focus] Failed to reset focus: {:?}", e);
                }
            }
            Err(e) => {
                warn!("[reset_input_focus] Failed to send focus request: {:?}", e);
            }
        }
        Ok(())
    }

    fn cleanup_ewmh_properties(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let properties_to_clean = [
            self.atoms._NET_ACTIVE_WINDOW,
            self.atoms._NET_CLIENT_LIST,
            self.atoms._NET_SUPPORTED,
        ];

        for &property in &properties_to_clean {
            if let Err(e) = self.x11rb_conn.delete_property(self.x11rb_root, property) {
                warn!(
                    "[cleanup_ewmh_properties] Failed to delete property {:?}: {:?}",
                    property, e
                );
            }
        }
        Ok(())
    }

    fn restore_all_clients(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.restored_clients_info = match WMClientCollection::load_from_file(CLIENT_STORAGE_PATH) {
            Ok(val) => val,
            _ => return Ok(()),
        };
        info!("[restore_all_clients] {:?}", self.restored_clients_info);
        Ok(())
    }

    fn store_all_clients(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[store_all_clients]");
        let client_restores: Vec<WMClientRestore> = self
            .monitor_order
            .iter()
            .filter_map(|&mon_key| self.monitor_stack.get(mon_key))
            .flat_map(|stack_clients| stack_clients.iter())
            .filter_map(|&client_key| self.clients.get(client_key))
            .map(|client| WMClientRestore::from_client(client))
            .collect();
        let client_store = WMClientCollection::from_clients(client_restores);
        client_store.save_to_file(CLIENT_STORAGE_PATH)?;
        Ok(())
    }

    pub fn clientmessage(
        &mut self,
        e: &ClientMessageEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[clientmessage]");
        let client_key = if let Some(key) = self.wintoclient(e.window) {
            key
        } else {
            return Ok(());
        };

        // 检查客户端是否存在
        if !self.clients.contains_key(client_key) {
            return Ok(());
        }

        // 检查是否是窗口状态消息
        if e.type_ == self.atoms._NET_WM_STATE {
            // 检查是否是全屏状态变更
            if self.is_fullscreen_state_message(e) {
                // 获取当前全屏状态（不持有借用）
                let is_fullscreen = self
                    .clients
                    .get(client_key)
                    .map(|client| client.state.is_fullscreen)
                    .unwrap_or(false);

                // 解析操作类型
                let action = self.get_client_message_long(e, 0)?;
                let fullscreen = match action {
                    1 => true,           // NET_WM_STATE_ADD
                    0 => false,          // NET_WM_STATE_REMOVE
                    2 => !is_fullscreen, // NET_WM_STATE_TOGGLE
                    _ => return Ok(()),  // 未知操作
                };

                self.setfullscreen(client_key, fullscreen)?;
            }
        }
        // 检查是否是激活窗口消息
        else if e.type_ == self.atoms._NET_ACTIVE_WINDOW {
            // 获取紧急状态（不持有借用）
            let is_urgent = self
                .clients
                .get(client_key)
                .map(|client| client.state.is_urgent)
                .unwrap_or(false);

            if !self.is_client_selected(client_key) && !is_urgent {
                self.seturgent(client_key, true)?;
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
        // 检查是否是根窗口的配置变更
        if e.window == self.x11rb_root {
            info!("[configurenotify] e: {:?}", e);
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

    pub fn configure(&mut self, client_key: ClientKey) -> Result<(), Box<dyn std::error::Error>> {
        let client = if let Some(client) = self.clients.get_mut(client_key) {
            client
        } else {
            return Err("Client not found".into());
        };
        info!("[configure] {}", client);
        let event = ConfigureNotifyEvent {
            event: client.win,
            window: client.win,
            x: client.geometry.x as i16,
            y: client.geometry.y as i16,
            width: client.geometry.w as u16,
            height: client.geometry.h as u16,
            border_width: client.geometry.border_w as u16,
            above_sibling: 0,
            override_redirect: false,
            response_type: CONFIGURE_NOTIFY_EVENT,
            sequence: 0,
        };
        self.x11rb_conn
            .send_event(false, client.win, EventMask::STRUCTURE_NOTIFY, event)?;
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

    pub fn set_window_border_color(
        &self,
        window: Window,
        selected: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let scheme_type = if selected {
            SchemeType::Sel
        } else {
            SchemeType::Norm
        };
        if let Some(border_color) = self.theme_manager.get_border(scheme_type) {
            if let Some(pixel) = self.theme_manager.get_x11_pixel(border_color) {
                self.x11rb_conn.change_window_attributes(
                    window,
                    &ChangeWindowAttributesAux::new().border_pixel(pixel),
                )?;
            }
        }
        Ok(())
    }

    pub fn grabkeys(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[grabkeys]");
        self.setup_modifier_masks()?;

        let modifiers_to_try = [
            KeyButMask::default(),
            KeyButMask::LOCK,
            self.numlock_mask,
            self.numlock_mask | KeyButMask::LOCK,
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
                    if key_config.key_sym == u32::from(keysym) {
                        for &modifier_combo in modifiers_to_try.iter() {
                            self.x11rb_conn.grab_key(
                                false, // owner_events
                                self.x11rb_root,
                                ModMask::from(key_config.mask.bits() | modifier_combo.bits()),
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
        client_opt: Option<Rc<RefCell<WMClient>>>,
        focused: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_win_id = match client_opt.as_ref() {
            Some(c_rc) => c_rc.borrow().win,
            None => return Ok(()),
        };

        let modifiers_to_try = [
            KeyButMask::default(),
            KeyButMask::LOCK,
            self.numlock_mask,
            self.numlock_mask | KeyButMask::LOCK,
        ];

        // 取消之前的按钮抓取
        self.x11rb_conn
            .ungrab_button(ButtonIndex::ANY, client_win_id, ModMask::ANY.into())?;

        if !focused {
            self.x11rb_conn.grab_button(
                false, // owner_events
                client_win_id,
                *BUTTONMASK,
                GrabMode::SYNC,
                GrabMode::SYNC,
                0u32, // confine_to
                0u32, // cursor
                ButtonIndex::ANY,
                ModMask::ANY.into(),
            )?;
        }

        for button_config in CONFIG.get_buttons().iter() {
            if button_config.click_type == WMClickType::ClickClientWin {
                for &modifier_combo in modifiers_to_try.iter() {
                    self.x11rb_conn.grab_button(
                        false,
                        client_win_id,
                        *BUTTONMASK,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                        0u32,
                        0u32,
                        button_config.button,
                        ModMask::from(button_config.mask.bits() | modifier_combo.bits()),
                    )?;
                }
            }
        }

        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn setfullscreen(
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
            self.set_x11_fullscreen_property(win, true)?;

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
            let config = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
            self.x11rb_conn.configure_window(win, &config)?;
            self.x11rb_conn.flush()?;
        } else if !fullscreen && is_fullscreen {
            // 取消全屏逻辑
            self.set_x11_fullscreen_property(win, false)?;

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
    pub fn seturgent(
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

        // 设置X11 urgent hint
        self.set_x11_urgent_hint(win, urgent)?;

        Ok(())
    }

    /// 辅助方法：设置X11全屏属性
    fn set_x11_fullscreen_property(
        &mut self,
        win: Window,
        fullscreen: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;

        if fullscreen {
            self.x11rb_conn.change_property32(
                PropMode::REPLACE,
                win,
                self.atoms._NET_WM_STATE,
                AtomEnum::ATOM,
                &[self.atoms._NET_WM_STATE_FULLSCREEN],
            )?;
        } else {
            self.x11rb_conn.change_property32(
                PropMode::REPLACE,
                win,
                self.atoms._NET_WM_STATE,
                AtomEnum::ATOM,
                &[],
            )?;
        }

        Ok(())
    }

    /// 辅助方法：设置X11 urgent hint
    fn set_x11_urgent_hint(
        &self,
        win: Window,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
                return Ok(());
            }
        };
        let reply = match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => {
                // 属性不存在，我们视为 flags = 0
                debug!("WM_HINTS not set, treating as zero");
                self.send_wm_hints_with_flags(win, if urgent { 256 } else { 0 });
                // 256 = XUrgencyHint
                return Ok(());
            }
        };
        // 2. 解析 flags（第一个 u32）
        let mut values = if let Some(values) = reply.value32() {
            values
        } else {
            return Ok(());
        };
        let mut flags = match values.next() {
            Some(f) => f,
            None => {
                debug!("WM_HINTS has no data");
                self.send_wm_hints_with_flags(win, if urgent { 256 } else { 0 });
                return Ok(());
            }
        };

        // 3. 修改 XUrgencyHint 位（第 9 位，值为 256）
        const X_URGENCY_HINT: u32 = 1 << 8; // 256
        if urgent {
            flags |= X_URGENCY_HINT;
        } else {
            flags &= !X_URGENCY_HINT;
        }
        // 4. 重新设置 WM_HINTS 属性
        self.send_wm_hints_with_flags(win, flags);
        Ok(())
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

    /// 递归显示/隐藏监视器上的所有客户端（保持原有逻辑）
    fn showhide_monitor_recursive(&mut self, mon_key: MonitorKey) {
        // 获取堆栈中的第一个客户端
        let first_client = self
            .monitor_stack
            .get(mon_key)
            .and_then(|stack| stack.first().copied());

        if let Some(client_key) = first_client {
            self.showhide_client_recursive(client_key, mon_key);
        }
    }

    /// 递归显示/隐藏客户端（保持原有的递归逻辑）
    fn showhide_client_recursive(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        let is_visible = self.is_client_visible_on_monitor(client_key, mon_key);

        if is_visible {
            // 显示客户端 - 从上到下
            self.show_client_recursive_top_down(client_key, mon_key);
        } else {
            // 隐藏客户端 - 从下到上
            self.hide_client_recursive_bottom_up(client_key, mon_key);
        }
    }

    /// 从上到下递归显示客户端
    fn show_client_recursive_top_down(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        // 先显示当前客户端
        self.show_client(client_key);

        // 然后递归处理下一个客户端
        let next_client = self.find_next_client_in_stack(client_key, mon_key);
        if let Some(next_key) = next_client {
            self.showhide_client_recursive(next_key, mon_key);
        }
    }

    /// 从下到上递归隐藏客户端
    fn hide_client_recursive_bottom_up(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        // 先递归处理下一个客户端（底部优先）
        let next_client = self.find_next_client_in_stack(client_key, mon_key);
        if let Some(next_key) = next_client {
            self.showhide_client_recursive(next_key, mon_key);
        }

        // 然后隐藏当前客户端
        self.hide_client(client_key);
    }

    /// 在堆栈中查找下一个客户端
    fn find_next_client_in_stack(
        &self,
        current_key: ClientKey,
        mon_key: MonitorKey,
    ) -> Option<ClientKey> {
        if let Some(stack) = self.monitor_stack.get(mon_key) {
            if let Some(pos) = stack.iter().position(|&key| key == current_key) {
                // 返回下一个客户端（如果存在）
                stack.get(pos + 1).copied()
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 更新 resize 方法签名以使用 ClientKey
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

    /// 更新 resizeclient 方法签名
    pub fn resizeclient(
        &mut self,
        client_key: ClientKey,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get_mut(client_key) {
            // 保存旧的位置和大小
            client.geometry.old_x = client.geometry.x;
            client.geometry.old_y = client.geometry.y;
            client.geometry.old_w = client.geometry.w;
            client.geometry.old_h = client.geometry.h;

            // 更新新的位置和大小
            client.geometry.x = x;
            client.geometry.y = y;
            client.geometry.w = w;
            client.geometry.h = h;

            // 构建配置值
            let values = ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(w as u32)
                .height(h as u32)
                .border_width(client.geometry.border_w as u32);

            // 发送配置窗口请求
            self.x11rb_conn.configure_window(client.win, &values)?;

            // 调用configure方法
            self.configure_client(client_key)?;

            // 同步连接
            self.x11rb_conn.flush()?;
        }

        Ok(())
    }

    /// 更新 configure 方法签名
    pub fn configure_client(
        &self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let event = ConfigureNotifyEvent {
                event: client.win,
                window: client.win,
                x: client.geometry.x as i16,
                y: client.geometry.y as i16,
                width: client.geometry.w as u16,
                height: client.geometry.h as u16,
                border_width: client.geometry.border_w as u16,
                above_sibling: 0,
                override_redirect: false,
                response_type: CONFIGURE_NOTIFY_EVENT,
                sequence: 0,
            };

            self.x11rb_conn
                .send_event(false, client.win, EventMask::STRUCTURE_NOTIFY, event)?;
            self.x11rb_conn.flush()?;
        }

        Ok(())
    }

    fn move_window(
        &mut self,
        win: Window,
        x: i32,
        y: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ConfigureWindowAux::new().x(x).y(y);
        self.x11rb_conn.configure_window(win, &aux)?;
        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn configurerequest(
        &mut self,
        e: &ConfigureRequestEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_key = self.wintoclient(e.window);
        if let Some(client_key) = client_key {
            // 检查是否是状态栏
            if let Some(&monitor_id) = self.status_bar_windows.get(&e.window) {
                info!("[configurerequest] statusbar on monitor {}", monitor_id);
                self.handle_statusbar_configure_request(monitor_id, e)?;
            } else {
                // 常规客户端的配置请求处理
                self.handle_regular_configure_request(client_key, e)?;
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
            "[handle_statusbar_configure_request] StatusBar resize request for monitor {}",
            monitor_id
        );

        // 检查状态栏是否存在并获取基本信息
        let statusbar_exists = self.status_bar_clients.contains_key(&monitor_id);
        if !statusbar_exists {
            error!(
                "[handle_statusbar_configure_request] StatusBar not found for monitor {}",
                monitor_id
            );
            return self.handle_unmanaged_configure_request(e);
        }

        // 更新状态栏几何信息（限制借用范围）
        {
            if let Some(statusbar) = self.status_bar_clients.get(&monitor_id) {
                let mut statusbar_mut = statusbar.borrow_mut();

                // 更新几何信息
                if e.value_mask.contains(ConfigWindow::X) {
                    statusbar_mut.geometry.x = e.x as i32;
                }
                if e.value_mask.contains(ConfigWindow::Y) {
                    statusbar_mut.geometry.y = e.y as i32;
                }
                if e.value_mask.contains(ConfigWindow::HEIGHT) {
                    statusbar_mut.geometry.h =
                        e.height.max(CONFIG.status_bar_height() as u16) as i32;
                }

                // 应用配置
                let values = ConfigureWindowAux::new()
                    .x(statusbar_mut.geometry.x)
                    .y(statusbar_mut.geometry.y)
                    .width(statusbar_mut.geometry.w as u32)
                    .height(statusbar_mut.geometry.h as u32);

                self.x11rb_conn.configure_window(e.window, &values)?;
                self.x11rb_conn.flush()?;
            }
        } // 确保 statusbar_mut 在这里被释放

        // 现在可以安全地进行其他操作
        let client_key_opt = self.wintoclient(e.window);

        if let Some(client_key) = client_key_opt {
            self.configure_client(client_key)?;
        }

        // 重新排列
        let monitor_key = self.get_monitor_by_id(monitor_id);
        self.arrange(monitor_key);

        Ok(())
    }

    fn handle_regular_configure_request(
        &mut self,
        client_key: ClientKey,
        e: &ConfigureRequestEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[handle_regular_configure_request]");

        // 获取客户端基本信息
        let (is_floating, mon_key, win) = if let Some(client) = self.clients.get(client_key) {
            (client.state.is_floating, client.mon, client.win)
        } else {
            return Err("Client not found".into());
        };

        // 更新边框宽度
        if e.value_mask.contains(ConfigWindow::BORDER_WIDTH) {
            if let Some(client) = self.clients.get_mut(client_key) {
                client.geometry.border_w = e.border_width as i32;
            }
        }

        if is_floating {
            // 获取监视器几何信息
            let (mx, my, mw, mh) = if let Some(mon_key) = mon_key {
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

            // 更新客户端几何信息
            if let Some(client) = self.clients.get_mut(client_key) {
                if e.value_mask.contains(ConfigWindow::X) {
                    client.geometry.old_x = client.geometry.x;
                    client.geometry.x = mx + e.x as i32;
                }
                if e.value_mask.contains(ConfigWindow::Y) {
                    client.geometry.old_y = client.geometry.y;
                    client.geometry.y = my + e.y as i32;
                }
                if e.value_mask.contains(ConfigWindow::WIDTH) {
                    client.geometry.old_w = client.geometry.w;
                    client.geometry.w = e.width as i32;
                }
                if e.value_mask.contains(ConfigWindow::HEIGHT) {
                    client.geometry.old_h = client.geometry.h;
                    client.geometry.h = e.height as i32;
                }

                // 确保窗口不超出显示器边界
                if (client.geometry.x + client.geometry.w) > mx + mw && client.state.is_floating {
                    client.geometry.x = mx + (mw / 2 - client.total_width() / 2);
                }
                if (client.geometry.y + client.geometry.h) > my + mh && client.state.is_floating {
                    client.geometry.y = my + (mh / 2 - client.total_height() / 2);
                }
            }

            // 如果只是位置变化，发送配置确认
            if e.value_mask.contains(ConfigWindow::X | ConfigWindow::Y)
                && !e
                    .value_mask
                    .contains(ConfigWindow::WIDTH | ConfigWindow::HEIGHT)
            {
                self.configure_client(client_key)?;
            }

            // 检查可见性并更新窗口
            let is_visible = self.is_client_visible_by_key(client_key);
            if is_visible {
                if let Some(client) = self.clients.get(client_key) {
                    self.x11rb_conn.configure_window(
                        client.win,
                        &ConfigureWindowAux::new()
                            .x(client.geometry.x)
                            .y(client.geometry.y)
                            .width(client.geometry.w as u32)
                            .height(client.geometry.h as u32),
                    )?;
                    self.x11rb_conn.flush()?;
                }
            }
        } else {
            // 平铺布局中的窗口，只允许有限的配置更改
            self.configure_client(client_key)?;
        }

        Ok(())
    }

    fn handle_unmanaged_configure_request(
        &mut self,
        e: &ConfigureRequestEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[handle_unmanaged_configure_request] e: {:?}", e);
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
        self.x11rb_conn.flush()?;
        Ok(())
    }

    pub fn createmon(&mut self) -> WMMonitor {
        // info!("[createmon]");
        let mut m: WMMonitor = WMMonitor::new();
        m.tag_set[0] = 1;
        m.tag_set[1] = 1;
        m.layout.m_fact = CONFIG.m_fact();
        m.layout.n_master = CONFIG.n_master();
        m.lt[0] = Rc::new(LayoutEnum::TILE);
        m.lt[1] = Rc::new(LayoutEnum::FLOAT);
        m.lt_symbol = m.lt[0].symbol().to_string();
        m.pertag = Some(Pertag::new());
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

    pub fn destroynotify(
        &mut self,
        e: &DestroyNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[destroynotify]");
        let c = self.wintoclient(e.window);
        if c.is_some() {
            self.unmanage(c, true)?;
        }
        Ok(())
    }

    pub fn arrangemon(&mut self, mon_key: MonitorKey) {
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

    pub fn dirtomon(&mut self, dir: &i32) -> Option<MonitorKey> {
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

    fn write_message(&mut self, num: i32, message: &SharedMessage) -> std::io::Result<()> {
        if let Some(ring_buffer) = self.status_bar_shmem.get_mut(&num) {
            match ring_buffer.try_write_message(&message) {
                Ok(true) => {
                    if let Some(_statusbar) = self.status_bar_clients.get(&num) {
                        // info!("statusbar: {}", statusbar.borrow());
                    }
                    // info!("[write_message] {:?}", message);
                    Ok(()) // Message written successfully
                }
                Ok(false) => {
                    println!("缓冲区已满，等待空间...");
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
            0 => CONFIG.status_bar_instance_0().to_string(),
            1 => CONFIG.status_bar_instance_1().to_string(),
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
                command.arg(CONFIG.status_bar_base_name());
            }
            // 这段代码只有在编译时 *没有* 启用 nixgl feature 时才会存在
            #[cfg(not(feature = "nixgl"))]
            {
                info!("--> [feature=nixgl] disabled. Launching status bar directly.");
                command = Command::new(CONFIG.status_bar_base_name());
            }
            if let Ok(child) = command
                .arg0(&Self::monitor_to_bar_name(num))
                .arg(shared_path)
                .spawn()
            {
                // insert 会自动处理新增和覆盖两种情况
                self.status_bar_child.insert(num, child);
                info!(
                    "--> spawned: Successfully started/restarted status bar for monitor {}.",
                    num
                );
            }
        }
    }

    pub fn update_bar_message(&mut self, mon_key_opt: Option<MonitorKey>) {
        self.update_bar_message_for_monitor(mon_key_opt);
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
        mon_key_opt: Option<MonitorKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[restack]");

        let mon_key = mon_key_opt.ok_or("Monitor is required for restack operation")?;

        // 检查监视器是否存在
        let monitor_num = if let Some(monitor) = self.monitors.get(mon_key) {
            monitor.num
        } else {
            return Err("Monitor not found".into());
        };
        self.mark_bar_update_needed(Some(monitor_num));

        // 收集并批量处理所有窗口重排操作
        let restack_operations = self.collect_restack_operations(mon_key, monitor_num)?;
        self.execute_restack_operations(restack_operations)?;

        // 移动光标到选中客户端中心
        if let Some(monitor) = self.monitors.get(mon_key) {
            if let Some(sel_client_key) = monitor.sel {
                self.move_cursor_to_client_center(sel_client_key)?;
            }
        }

        info!("[restack] finish");
        Ok(())
    }

    /// 收集所有需要重新排列的窗口操作
    fn collect_restack_operations(
        &self,
        mon_key: MonitorKey,
        monitor_num: i32,
    ) -> Result<Vec<RestackOperation>, Box<dyn std::error::Error>> {
        let mut operations = Vec::new();

        // 1. 收集非浮动窗口（底层）
        let non_floating_windows = self.collect_non_floating_windows(mon_key)?;
        operations.extend(self.create_non_floating_operations(&non_floating_windows));

        // 2. 添加选中的浮动窗口（中层）
        if let Some(floating_op) = self.create_selected_floating_operation(mon_key)? {
            operations.push(floating_op);
        }

        // 3. 添加状态栏（顶层）
        if let Some(statusbar_op) = self.create_statusbar_operation(monitor_num)? {
            operations.push(statusbar_op);
        }

        Ok(operations)
    }

    /// 收集所有非浮动可见窗口
    fn collect_non_floating_windows(
        &self,
        mon_key: MonitorKey,
    ) -> Result<Vec<Window>, Box<dyn std::error::Error>> {
        let mut windows = Vec::new();

        // 获取监视器的堆栈顺序客户端
        if let Some(stack_clients) = self.monitor_stack.get(mon_key) {
            for &client_key in stack_clients {
                if let Some(client) = self.clients.get(client_key) {
                    if !client.state.is_floating
                        && self.is_client_visible_on_monitor(client_key, mon_key)
                    {
                        windows.push(client.win);
                    }
                }
            }
        }

        Ok(windows)
    }

    /// 为非浮动窗口创建重排操作
    fn create_non_floating_operations(&self, windows: &[Window]) -> Vec<RestackOperation> {
        windows
            .iter()
            .enumerate()
            .map(|(i, &win)| {
                let sibling = if i == 0 { None } else { Some(windows[i - 1]) };
                RestackOperation {
                    window: win,
                    stack_mode: StackMode::BELOW,
                    sibling,
                }
            })
            .collect()
    }

    /// 为选中的浮动窗口创建重排操作
    fn create_selected_floating_operation(
        &self,
        mon_key: MonitorKey,
    ) -> Result<Option<RestackOperation>, Box<dyn std::error::Error>> {
        if let Some(monitor) = self.monitors.get(mon_key) {
            if let Some(sel_client_key) = monitor.sel {
                if let Some(client) = self.clients.get(sel_client_key) {
                    if client.state.is_floating {
                        return Ok(Some(RestackOperation {
                            window: client.win,
                            stack_mode: StackMode::ABOVE,
                            sibling: None,
                        }));
                    }
                }
            }
        }
        Ok(None)
    }

    /// 为状态栏创建重排操作
    fn create_statusbar_operation(
        &self,
        monitor_num: i32,
    ) -> Result<Option<RestackOperation>, Box<dyn std::error::Error>> {
        if let Some(statusbar) = self.status_bar_clients.get(&monitor_num) {
            let statusbar_win = statusbar.borrow().win;
            return Ok(Some(RestackOperation {
                window: statusbar_win,
                stack_mode: StackMode::ABOVE,
                sibling: None,
            }));
        }
        Ok(None)
    }

    /// 执行所有重排操作
    fn execute_restack_operations(
        &mut self,
        operations: Vec<RestackOperation>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if operations.is_empty() {
            return Ok(());
        }

        // 批量执行所有配置更改
        for op in operations {
            let mut config = ConfigureWindowAux::new().stack_mode(op.stack_mode);

            if let Some(sibling_win) = op.sibling {
                config = config.sibling(sibling_win);
            }

            self.x11rb_conn.configure_window(op.window, &config)?;
        }

        // 单次同步所有操作
        self.x11rb_conn.flush()?;
        Ok(())
    }

    /// 将鼠标指针移动到客户端窗口的中心
    fn move_cursor_to_client_center(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.enable_move_cursor_to_client_center {
            return Ok(());
        }

        // 获取当前鼠标位置
        let query_cookie = self.x11rb_conn.query_pointer(self.x11rb_root)?;
        let query_reply = query_cookie.reply()?;
        let (initial_mouse_x, initial_mouse_y) = (query_reply.root_x, query_reply.root_y);

        // 检查鼠标是否已经在客户端内
        if let Some(client) = self.clients.get(client_key) {
            if client.contains_point(initial_mouse_x.into(), initial_mouse_y.into()) {
                return Ok(());
            }

            let (win, center_x, center_y) = {
                let center_x = client.geometry.w / 2;
                let center_y = client.geometry.h / 2;
                (client.win, center_x, center_y)
            };

            // 使用 warp_pointer 将鼠标移动到窗口中心
            self.x11rb_conn.warp_pointer(
                0u32,            // src_window (0 = None)
                win,             // dst_window (目标窗口)
                0,               // src_x
                0,               // src_y
                0,               // src_width
                0,               // src_height
                center_x as i16, // dst_x (相对于目标窗口的X坐标)
                center_y as i16, // dst_y (相对于目标窗口的Y坐标)
            )?;

            // 刷新连接确保请求被发送
            self.x11rb_conn.flush()?;
            debug!(
                "[move_cursor_to_client_center] Moved cursor to center of window {}: ({}, {})",
                win, center_x, center_y
            );
        }

        Ok(())
    }

    fn flush_pending_bar_updates(&mut self) {
        if self.pending_bar_updates.is_empty() {
            return;
        }
        // info!(
        //     "[flush_pending_bar_updates] Updating {} monitors",
        //     self.pending_bar_updates.len()
        // );
        for monitor_id in self.pending_bar_updates.clone() {
            if let Some(monitor) = self.get_monitor_by_id(monitor_id) {
                self.update_bar_message(Some(monitor));
            }
        }

        self.pending_bar_updates.clear();

        // Show or hide status bar
        let status_bar_flags = self.status_bar_flags.clone();
        for (&mon_id, &show_bar_enum) in status_bar_flags.iter() {
            match show_bar_enum {
                WMShowBarEnum::Toggle(show_bar) => {
                    let client_mut = self.status_bar_clients.get_mut(&mon_id).unwrap().clone();
                    if show_bar == true {
                        info!("[flush_pending_bar_updates] show bar");
                        let _ = self.show_statusbar(&mut client_mut.as_ref().borrow_mut(), mon_id);
                    } else {
                        info!("[flush_pending_bar_updates] hide bar");
                        let _ = self.hide_statusbar(&mut client_mut.as_ref().borrow_mut(), mon_id);
                    }
                    // 发送确认配置事件给 status bar
                    if let Some(client_key) =
                        self.find_client_key(&mut client_mut.as_ref().borrow_mut())
                    {
                        let _ = self.configure(client_key);
                    }
                    info!("[flush_pending_bar_updates] Updating workarea due to statusbar geometry change");
                    // 重新排列该显示器上的其他客户端
                    if let Some(monitor) = self.get_monitor_by_id(mon_id) {
                        self.arrange(Some(monitor));
                    }
                    self.status_bar_flags
                        .insert(mon_id, WMShowBarEnum::Keep(show_bar));
                }
                _ => {}
            }
        }
    }

    fn show_statusbar(
        &mut self,
        client_mut: &mut WMClient,
        monitor_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(_monitor) = self.get_monitor_by_id(monitor_id) {
            // 将状态栏放在显示器顶部
            let x = client_mut.geometry.x;
            let y = client_mut.geometry.y;
            info!("[show_statusbar] Show at ({}, {})", x, y,);
            self.move_window(client_mut.win, x, y)?;
        }
        Ok(())
    }

    fn hide_statusbar(
        &mut self,
        client_mut: &mut WMClient,
        monitor_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(_monitor) = self.get_monitor_by_id(monitor_id) {
            let hidden_x = -1000;
            let hidden_y = -1000;
            info!("[hide_statusbar] Hide at ({}, {})", hidden_x, hidden_y,);
            self.move_window(client_mut.win, hidden_x, hidden_y)?;
        }
        Ok(())
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
        self.x11rb_conn.flush()?;
        let mut event_count: u64 = 0;
        let mut update_timer = tokio::time::interval(Duration::from_millis(10));
        // 🔧 创建一次性的 AsyncFd
        let async_fd = {
            use std::os::unix::io::AsRawFd;
            use tokio::io::unix::AsyncFd;
            let stream = self.x11rb_conn.stream();
            let fd = stream.as_raw_fd();
            AsyncFd::new(fd)?
        };
        info!("Starting async event loop");
        while self.running.load(Ordering::SeqCst) {
            // 🔧 一次性处理所有事件
            let events_processed = self.process_all_x11_events(&mut event_count)?;
            self.process_commands_from_status_bar();
            if events_processed || !self.pending_bar_updates.is_empty() {
                self.flush_pending_bar_updates();
            }
            // 🔧 修复的 select 逻辑
            tokio::select! {
                _ = update_timer.tick() => {
                    if !self.pending_bar_updates.is_empty() {
                        self.flush_pending_bar_updates();
                    }
                }
                // 替换方案
                // _ = tokio::time::sleep(Duration::from_millis(1)) => {
                // }
                result = self.wait_for_x11_ready_fixed(&async_fd) => {
                    if let Err(e) = result {
                        warn!("X11 ready wait error: {}", e);
                    }
                    // 下次循环会处理新事件
                }
            }
        }
        Ok(())
    }

    pub fn run_sync(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.x11rb_conn.flush()?;
        let mut event_count: u64 = 0;
        info!("Starting sync event loop");
        while self.running.load(Ordering::SeqCst) {
            // 处理所有待处理的 X11 事件
            while let Some(event) = self.x11rb_conn.poll_for_event()? {
                event_count = event_count.wrapping_add(1);
                info!(
                    "[run_sync] event_count: {}, event: {:?}",
                    event_count, event
                );
                let _ = self.handler(event);
            }
            // 处理状态栏命令
            self.process_commands_from_status_bar();
            // 更新状态栏
            if !self.pending_bar_updates.is_empty() {
                self.flush_pending_bar_updates();
            }
            // 等待下一个事件
            if let Some(event) = self.x11rb_conn.wait_for_event().ok() {
                event_count = event_count.wrapping_add(1);
                info!(
                    "[run_sync] event_count: {}, event: {:?}",
                    event_count, event
                );
                let _ = self.handler(event);
            }
        }
        Ok(())
    }

    // 🔧 统一的事件处理函数
    fn process_all_x11_events(
        &mut self,
        event_count: &mut u64,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let mut events_processed = false;
        while let Some(event) = self.x11rb_conn.poll_for_event()? {
            *event_count = event_count.wrapping_add(1);
            // info!(
            //     "[run_async] event_count: {}, event: {:?}",
            //     event_count, event
            // );
            let _ = self.handler(event);
            events_processed = true;
        }

        Ok(events_processed)
    }

    // 🔧 修复的 wait_for_x11_ready
    async fn wait_for_x11_ready_fixed(
        &self,
        async_fd: &tokio::io::unix::AsyncFd<std::os::unix::io::RawFd>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 添加超时保护
        tokio::time::timeout(Duration::from_millis(100), async {
            let mut guard = async_fd.readable().await?;
            guard.clear_ready();
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .await??;
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
            self.enable_move_cursor_to_client_center = false;
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

    fn get_transient_for(&self, window: Window) -> Option<Window> {
        match self.get_transient_for_hint(window) {
            Ok(trans) => trans,
            Err(_) => None,
        }
    }

    pub fn scan(&mut self) -> Result<(), ReplyOrIdError> {
        // info!("[scan]");
        let tree_reply = self.x11rb_conn.query_tree(self.x11rb_root)?.reply()?;
        let mut cookies = Vec::with_capacity(tree_reply.children.len());
        for win in tree_reply.children {
            let restored_client = self.restored_clients_info.get_client(win).cloned();
            if let Some(restored_client) = restored_client {
                self.manage_restored(&restored_client);
                continue;
            }
            let attr = self.get_window_attributes(win)?;
            let geom = Self::get_and_query_window_geom(&self.x11rb_conn, win)?;
            let trans = self.get_transient_for(win);
            cookies.push((win, attr, geom, trans));
        }
        for (win, attr, geom, trans) in &cookies {
            if attr.override_redirect || trans.is_some() {
                continue;
            }
            if attr.map_state == MapState::VIEWABLE
                || self.get_wm_state(*win) == ICONIC_STATE as i64
            {
                self.manage(*win, geom);
            }
        }
        for (win, attr, geom, trans) in &cookies {
            {
                if trans.is_some() {
                    if attr.map_state == MapState::VIEWABLE
                        || self.get_wm_state(*win) == ICONIC_STATE as i64
                    {
                        self.manage(*win, geom);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn arrange(&mut self, m_target: Option<MonitorKey>) {
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

    pub fn recttomon(&mut self, x: i32, y: i32, w: i32, h: i32) -> Option<MonitorKey> {
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

    pub fn wintomon(&mut self, w: Window) -> Option<MonitorKey> {
        // 处理根窗口
        if w == self.x11rb_root {
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
                debug!(
                    "[wintomon] Window {} not managed, returning selected monitor",
                    w
                );
                self.sel_mon
            }
        }
    }

    pub fn buttonpress(&mut self, e: &ButtonPressEvent) -> Result<(), Box<dyn std::error::Error>> {
        let mut click_type = WMClickType::ClickRootWin;
        let window = e.event as u32;

        // 处理监视器切换
        if let Some(target_mon_key) = self.wintomon(window) {
            if Some(target_mon_key) != self.sel_mon {
                let current_sel = self.get_selected_client_key();
                self.unfocus(current_sel.unwrap(), true)?;
                self.sel_mon = Some(target_mon_key);
                self.focus(None)?;
            }
        }

        // 处理客户端点击
        if let Some(client_key) = self.wintoclient(window) {
            self.focus(Some(client_key))?;
            let _ = self.restack(self.sel_mon);
            self.x11rb_conn
                .allow_events(Allow::REPLAY_POINTER, e.time)?;
            click_type = WMClickType::ClickClientWin;
        }

        // 处理按钮配置
        let event_mask = self.clean_mask(e.state.bits());
        for config in CONFIG.get_buttons().iter() {
            if config.click_type == click_type
                && config.func.is_some()
                && config.button == ButtonIndex::from(e.detail)
                && self.clean_mask(config.mask.bits()) == event_mask
            {
                if let Some(ref func) = config.func {
                    info!("[buttonpress] Executing button action");
                    let _ = func(self, &config.arg);
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
        // 首先尝试找到支持 alpha 通道的 32 位视觉效果
        for depth in self.x11rb_screen.allowed_depths.clone() {
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
        self.visual_id = self.x11rb_screen.root_visual;
        self.depth = self.x11rb_screen.root_depth;
        self.color_map = self.x11rb_screen.default_colormap.into();

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

    pub fn tile(&mut self, mon_key: MonitorKey) {
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
        let monitor_id = monitor.num;
        if self.status_bar_clients.get(&monitor_id).is_some() {
            let offset = if *self
                .status_bar_flags
                .get(&monitor_id)
                .unwrap_or(&WMShowBarEnum::Keep(true))
                .show_bar()
            {
                CONFIG.status_bar_height() + CONFIG.status_bar_padding() * 2
            } else {
                0
            };
            info!(
                "[get_client_y_offset] Monitor {}: offset = {} (height: {} + pad: {} x 2)",
                monitor_id,
                offset,
                CONFIG.status_bar_height(),
                CONFIG.status_bar_padding()
            );
            return offset.max(0);
        }
        CONFIG.status_bar_height() + CONFIG.status_bar_padding() * 2
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

    pub fn focusin(&mut self, e: &FocusInEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[focusin]");
        let sel_client_key = self.get_selected_client_key();

        if let Some(client_key) = sel_client_key {
            if let Some(client) = self.clients.get(client_key) {
                if e.event != client.win {
                    self.setfocus(client_key)?;
                }
            }
        }
        Ok(())
    }

    pub fn setfocus(&mut self, client_key: ClientKey) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let win = client.win;
            let never_focus = client.state.never_focus;

            if !never_focus {
                self.x11rb_conn.set_input_focus(
                    InputFocus::POINTER_ROOT,
                    win,
                    0u32, // time
                )?;

                use x11rb::wrapper::ConnectionExt;
                self.x11rb_conn.change_property32(
                    PropMode::REPLACE,
                    self.x11rb_root,
                    self.atoms._NET_ACTIVE_WINDOW,
                    AtomEnum::WINDOW,
                    &[win],
                )?;
            }

            self.sendevent_by_window(win, self.atoms.WM_TAKE_FOCUS);
            self.x11rb_conn.flush()?;
        }
        Ok(())
    }

    pub fn focusmon(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[focusmon]");
        // 检查是否只有一个监视器
        if self.monitor_order.len() <= 1 {
            return Ok(());
        }

        if let WMArgEnum::Int(i) = arg {
            let target_mon = self.dirtomon(i);

            if let Some(target_mon_key) = target_mon {
                if Some(target_mon_key) == self.sel_mon {
                    return Ok(());
                }

                // 取消当前监视器上选中客户端的焦点
                let current_sel = self.get_selected_client_key();
                self.unfocus(current_sel.unwrap(), false)?;

                // 切换到目标监视器
                self.sel_mon = Some(target_mon_key);
                self.focus(None)?;
            }
        }
        Ok(())
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

    pub fn sendmon(
        &mut self,
        client_key_opt: Option<ClientKey>,
        target_mon_opt: Option<MonitorKey>,
    ) {
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
        // let _ = self.unfocus(Some(client_key), true);

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
        // let _ = self.focus(None);
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
            // self.focus(Some(client_key))?;
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

    pub fn togglebar(&mut self, arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[togglebar]");

        if let WMArgEnum::Int(_) = arg {
            let sel_mon_key = match self.sel_mon {
                Some(key) => key,
                None => return Ok(()),
            };

            let monitor_num = if let Some(monitor) = self.monitors.get_mut(sel_mon_key) {
                if let Some(ref mut pertag) = monitor.pertag {
                    let cur_tag = pertag.cur_tag;
                    if let Some(show_bar) = pertag.show_bars.get_mut(cur_tag) {
                        *show_bar = !*show_bar;
                        info!("[togglebar] show_bar: {}", show_bar);
                        Some(monitor.num)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(num) = monitor_num {
                self.mark_bar_update_needed(Some(num));
            }
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
            self.mark_bar_update_needed(mon_num);
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
        let sel_mon_mut = if let Some(sel_mon) = self.monitors.get_mut(self.sel_mon.unwrap()) {
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
        // self.focus(None)?;
        self.arrange(Some(sel_mon_key));

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

    pub fn setup_ewmh(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // --- 1. 创建 _NET_SUPPORTING_WM_CHECK 窗口 ---
        let frame_win = self.x11rb_conn.generate_id()?;
        let win_aux = CreateWindowAux::new()
            .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS)
            .background_pixel(self.x11rb_screen.white_pixel);
        self.x11rb_conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            frame_win,
            self.x11rb_screen.root,
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

        // _NET_SUPPORTING_WM_CHECK = frame_win (Atom 类型 WINDOW)
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
            self.x11rb_screen.root,
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
            self.x11rb_screen.root,
            self.atoms._NET_SUPPORTED,
            AtomEnum::ATOM,
            &supported_atoms,
        )?;

        // --- 5. 清除 _NET_CLIENT_LIST 和 _NET_CLIENT_INFO ---
        let _ = self
            .x11rb_conn
            .delete_property(self.x11rb_screen.root, self.atoms._NET_CLIENT_LIST);
        let _ = self
            .x11rb_conn
            .delete_property(self.x11rb_screen.root, self.atoms._NET_CLIENT_INFO);

        // --- 6. 刷新请求 ---
        let _ = self.x11rb_conn.flush();
        Ok(())
    }

    pub fn setup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Err(_) = Command::new("pkill")
            .arg("-9")
            .arg(CONFIG.status_bar_base_name())
            .spawn()
        {
            error!("[new] Clear status bar failed");
        }
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
        // self.focus(None)?;

        self.x11rb_conn.flush()?;

        self.restore_all_clients()?;
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
        if self.sendevent_by_window(client_win, self.atoms.WM_DELETE_WINDOW) {
            info!("[killclient] Sent WM_DELETE_WINDOW protocol message");
            return Ok(());
        }

        // 如果优雅关闭失败，强制终止客户端
        info!("[killclient] WM_DELETE_WINDOW failed, force killing client");
        self.force_kill_client_by_key(client_key)?;

        Ok(())
    }

    /// 通过窗口ID发送事件（从原来的sendevent方法改造）
    fn sendevent_by_window(&mut self, window: Window, proto: Atom) -> bool {
        info!(
            "[sendevent_by_window] Sending protocol {:?} to window 0x{:x}",
            proto, window
        );

        // 1. 获取 WM_PROTOCOLS 属性
        let cookie = match self.x11rb_conn.get_property(
            false,                   // delete: 不删除
            window,                  // window
            self.atoms.WM_PROTOCOLS, // Atom for WM_PROTOCOLS
            AtomEnum::ATOM,
            0,    // long_offset
            1024, // 足够大的长度
        ) {
            Ok(cookie) => cookie,
            Err(_) => {
                warn!("[sendevent_by_window] Failed to send get_property request");
                return false;
            }
        };

        let reply = match cookie.reply() {
            Ok(reply) => reply,
            Err(_) => {
                warn!(
                    "[sendevent_by_window] Failed to get WM_PROTOCOLS for window 0x{:x}",
                    window
                );
                return false;
            }
        };

        // 2. 检查属性值中是否包含目标 proto
        let protocols: Vec<Atom> = reply.value32().into_iter().flatten().collect();

        if !protocols.contains(&proto) {
            info!(
                "[sendevent_by_window] Protocol {:?} not supported by window 0x{:x}",
                proto, window
            );
            return false;
        }

        // 3. 构造 ClientMessageEvent
        let event = ClientMessageEvent::new(
            32,                      // format: 32 位
            window,                  // window
            self.atoms.WM_PROTOCOLS, // message_type
            [proto, 0, 0, 0, 0],     // data.l[0] = protocol atom
        );

        // 4. 发送事件
        use x11rb::x11_utils::Serialize;
        let buffer = event.serialize();
        let result = self.x11rb_conn.send_event(
            false,
            window,
            EventMask::NO_EVENT, // 不需要事件掩码（由接收方决定）
            buffer,
        );

        if let Err(e) = result {
            warn!("[sendevent_by_window] Failed to send event: {}", e);
            return false;
        }

        // 5. flush
        if let Err(e) = self.x11rb_conn.flush() {
            warn!("[sendevent_by_window] Failed to flush connection: {}", e);
            return false;
        }

        info!(
            "[sendevent_by_window] Successfully sent protocol {:?} to window 0x{:x}",
            proto, window
        );
        true
    }

    /// 通过ClientKey强制终止客户端
    fn force_kill_client_by_key(
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

        // 抓取服务器以确保操作的原子性
        self.x11rb_conn.grab_server()?;

        // 设置关闭模式为销毁所有资源
        self.x11rb_conn
            .set_close_down_mode(CloseDown::DESTROY_ALL)?;

        // 强制终止客户端
        let result = match self.x11rb_conn.kill_client(win) {
            Ok(cookie) => {
                // 同步并检查结果
                self.x11rb_conn.flush()?;
                match cookie.check() {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        warn!("[force_kill_client_by_key] Kill client failed: {:?}", e);
                    }
                }
            }
            Err(e) => {
                warn!(
                    "[force_kill_client_by_key] Failed to send kill_client request: {:?}",
                    e
                );
            }
        };

        // 释放服务器（无论成功失败）
        self.x11rb_conn.ungrab_server()?;
        self.x11rb_conn.flush()?;

        Ok(())
    }

    pub fn gettextprop(&mut self, w: Window, atom: Atom, text: &mut String) -> bool {
        text.clear();

        let property = match self.get_window_property(w, atom) {
            Ok(prop) => prop,
            Err(_) => return false,
        };

        if property.value.is_empty() {
            debug!("[gettextprop] Property value is empty");
            return false;
        }

        // 只处理 8 位格式的属性
        if property.format != 8 {
            debug!(
                "[gettextprop] Unsupported property format: {}",
                property.format
            );
            return false;
        }

        // 根据属性类型解析文本
        let parsed_text = match property.type_ {
            type_ if type_ == self.atoms.UTF8_STRING => self.parse_utf8_string(&property.value),
            type_ if type_ == u32::from(AtomEnum::STRING) => {
                Some(self.parse_latin1_string(&property.value))
            }
            type_ if type_ == self.atoms.COMPOUND_TEXT => self.parse_compound_text(&property.value),
            _ => self.parse_fallback_text(&property.value),
        };

        match parsed_text {
            Some(parsed) => {
                *text = self.truncate_text(parsed);
                true
            }
            None => false,
        }
    }

    // 获取窗口属性
    fn get_window_property(
        &mut self,
        w: Window,
        atom: Atom,
    ) -> Result<GetPropertyReply, Box<dyn std::error::Error>> {
        let cookie = self.x11rb_conn.get_property(
            false,         // delete: 不删除属性
            w,             // window
            atom,          // property
            AtomEnum::ANY, // type: 接受任何类型
            0,             // long_offset
            u32::MAX,      // long_length: 读取全部内容
        )?;

        let property = cookie.reply()?;
        Ok(property)
    }

    // 解析 UTF-8 字符串
    fn parse_utf8_string(&self, value: &[u8]) -> Option<String> {
        match String::from_utf8(value.to_vec()) {
            Ok(utf8_string) => {
                debug!("[gettextprop] Successfully parsed UTF8_STRING");
                Some(utf8_string)
            }
            Err(e) => {
                debug!("[gettextprop] Invalid UTF-8 in UTF8_STRING: {:?}", e);
                None
            }
        }
    }

    // 解析 Latin-1 字符串
    fn parse_latin1_string(&self, value: &[u8]) -> String {
        debug!("[gettextprop] Parsing as STRING (Latin-1)");
        value.iter().map(|&b| b as char).collect()
    }

    // 解析 COMPOUND_TEXT
    fn parse_compound_text(&self, value: &[u8]) -> Option<String> {
        debug!("[gettextprop] Parsing as COMPOUND_TEXT");

        // 首先尝试 UTF-8 解析
        match String::from_utf8(value.to_vec()) {
            Ok(utf8_string) => Some(utf8_string),
            Err(_) => {
                debug!("[gettextprop] COMPOUND_TEXT UTF-8 failed, falling back to Latin-1");
                Some(self.parse_latin1_string(value))
            }
        }
    }

    // 回退文本解析
    fn parse_fallback_text(&self, value: &[u8]) -> Option<String> {
        debug!("[gettextprop] Using fallback text parsing");

        // 首先尝试 UTF-8
        match String::from_utf8(value.to_vec()) {
            Ok(utf8_string) => Some(utf8_string),
            Err(_) => {
                debug!("[gettextprop] Fallback UTF-8 failed, using Latin-1");
                Some(self.parse_latin1_string(value))
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

    pub fn propertynotify(
        &mut self,
        e: &PropertyNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[propertynotify]");

        // 处理根窗口属性变更
        if e.window == self.x11rb_root && e.atom == u32::from(AtomEnum::WM_NAME) {
            debug!("Root window name property changed");
            return Ok(());
        }

        // 忽略属性删除事件
        if e.state == Property::DELETE {
            debug!("Ignoring property delete event for window {}", e.window);
            return Ok(());
        }

        // 处理客户端窗口属性变更
        if let Some(client_key) = self.wintoclient(e.window) {
            self.handle_client_property_change(client_key, e)?;
        } else {
            debug!("Property change for unmanaged window: {}", e.window);
        }

        Ok(())
    }

    fn handle_client_property_change(
        &mut self,
        client_key: ClientKey,
        e: &PropertyNotifyEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match e.atom {
            atom if atom == self.atoms.WM_TRANSIENT_FOR => {
                self.handle_transient_for_change(client_key)?;
            }
            atom if atom == u32::from(AtomEnum::WM_NORMAL_HINTS) => {
                self.handle_normal_hints_change(client_key)?;
            }
            atom if atom == u32::from(AtomEnum::WM_HINTS) => {
                self.handle_wm_hints_change(client_key)?;
            }
            atom if atom == u32::from(AtomEnum::WM_NAME) || atom == self.atoms._NET_WM_NAME => {
                self.handle_title_change(client_key)?;
            }
            atom if atom == self.atoms._NET_WM_WINDOW_TYPE => {
                self.handle_window_type_change(client_key)?;
            }
            _ => {
                debug!("Unhandled property change: atom {}", e.atom);
            }
        }

        Ok(())
    }

    fn handle_transient_for_change(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (is_floating, win, client_name) = if let Some(client) = self.clients.get(client_key) {
            (client.state.is_floating, client.win, client.name.clone())
        } else {
            return Ok(());
        };

        if !is_floating {
            // 获取transient_for属性
            let transient_for = self.get_transient_for_hint(win)?;
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
        self.mark_bar_update_needed(None);

        if let Some(client) = self.clients.get(client_key) {
            debug!("WM hints updated for window 0x{:x}", client.win);
        }
        Ok(())
    }

    fn gettextprop_by_window(&mut self, window: Window, atom: Atom, text: &mut String) -> bool {
        text.clear();

        let property = match self.get_window_property(window, atom) {
            Ok(prop) => prop,
            Err(_) => return false,
        };

        if property.value.is_empty() {
            debug!("[gettextprop_by_window] Property value is empty");
            return false;
        }

        // 只处理 8 位格式的属性
        if property.format != 8 {
            debug!(
                "[gettextprop_by_window] Unsupported property format: {}",
                property.format
            );
            return false;
        }

        // 根据属性类型解析文本
        let parsed_text = match property.type_ {
            type_ if type_ == self.atoms.UTF8_STRING => self.parse_utf8_string(&property.value),
            type_ if type_ == u32::from(AtomEnum::STRING) => {
                Some(self.parse_latin1_string(&property.value))
            }
            type_ if type_ == self.atoms.COMPOUND_TEXT => self.parse_compound_text(&property.value),
            _ => self.parse_fallback_text(&property.value),
        };

        match parsed_text {
            Some(parsed) => {
                *text = self.truncate_text(parsed);
                true
            }
            None => false,
        }
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

    fn fetch_window_title(&mut self, window: Window) -> String {
        // 尝试获取 _NET_WM_NAME (UTF-8)
        if let Some(title) = self.get_text_property(window, self.atoms._NET_WM_NAME) {
            return title;
        }

        // 如果失败，尝试 WM_NAME (Latin-1)
        if let Some(title) = self.get_text_property(window, AtomEnum::WM_NAME.into()) {
            return title;
        }

        // 如果都失败，返回默认值
        format!("Window 0x{:x}", window)
    }

    fn get_text_property(&mut self, window: Window, atom: Atom) -> Option<String> {
        let property = self.get_window_property(window, atom).ok()?;

        if property.value.is_empty() || property.format != 8 {
            return None;
        }

        // 根据属性类型解析文本
        let parsed_text = match property.type_ {
            type_ if type_ == self.atoms.UTF8_STRING => self.parse_utf8_string(&property.value),
            type_ if type_ == u32::from(AtomEnum::STRING) => {
                Some(self.parse_latin1_string(&property.value))
            }
            type_ if type_ == self.atoms.COMPOUND_TEXT => self.parse_compound_text(&property.value),
            _ => self.parse_fallback_text(&property.value),
        }?;

        Some(self.truncate_text(parsed_text))
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
                self.mark_bar_update_needed(Some(id));

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

    pub fn movemouse(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[movemouse]");

        // 1. 获取当前选中的客户端
        let client_key = match self.get_selected_client_key() {
            Some(key) => key,
            None => {
                debug!("No selected client for move");
                return Ok(());
            }
        };

        // 2. 全屏检查
        if let Some(client) = self.clients.get(client_key) {
            if client.state.is_fullscreen {
                debug!("Cannot move fullscreen window");
                return Ok(());
            }
        } else {
            return Ok(());
        }

        // 3. 准备工作
        self.restack(self.sel_mon)?;

        // 保存窗口开始移动时的信息
        let (original_x, original_y, window_id) = if let Some(client) = self.clients.get(client_key)
        {
            (client.geometry.x, client.geometry.y, client.win)
        } else {
            return Ok(());
        };

        // 4. 抓取鼠标指针
        let cursor = self
            .cursor_manager
            .get_cursor(&self.x11rb_conn, crate::xcb_util::StandardCursor::Hand1)?;

        let grab_reply = self
            .x11rb_conn
            .grab_pointer(
                false,           // owner_events
                self.x11rb_root, // grab_window
                *MOUSEMASK,      // event_mask
                GrabMode::ASYNC, // pointer_mode
                GrabMode::ASYNC, // keyboard_mode
                0u32,            // confine_to
                cursor,          // cursor
                0u32,            // time
            )?
            .reply()?;

        if grab_reply.status != GrabStatus::SUCCESS {
            let status_str = match grab_reply.status {
                GrabStatus::ALREADY_GRABBED => "AlreadyGrabbed",
                GrabStatus::FROZEN => "Frozen",
                GrabStatus::INVALID_TIME => "InvalidTime",
                GrabStatus::NOT_VIEWABLE => "NotViewable",
                _ => "Unknown",
            };
            return Err(format!("Failed to grab pointer: {}", status_str).into());
        }

        // 5. 获取鼠标初始位置
        let query_reply = self.x11rb_conn.query_pointer(self.x11rb_root)?.reply()?;
        let (initial_mouse_x, initial_mouse_y) = (query_reply.root_x, query_reply.root_y);

        info!(
            "[movemouse] initial mouse (root): x={}, y={}",
            initial_mouse_x, initial_mouse_y
        );

        // 6. 进入移动循环
        let result = self.move_loop(
            client_key,
            original_x,
            original_y,
            initial_mouse_x as u16,
            initial_mouse_y as u16,
        );

        // 7. 清理工作
        if let Err(e) = self.x11rb_conn.ungrab_pointer(0u32) {
            error!("[movemouse] Failed to ungrab pointer: {}", e);
        }
        self.cleanup_move(window_id, client_key)?;

        info!("[movemouse] completed");
        result
    }

    fn move_loop(
        &mut self,
        client_key: ClientKey,
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
                        continue;
                    }
                    last_motion_time = e.time;

                    self.handle_move_motion(
                        client_key,
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
        client_key: ClientKey,
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
        let (mon_wx, mon_wy, mon_ww, mon_wh) = if let Some(sel_mon_key) = self.sel_mon {
            if let Some(monitor) = self.monitors.get(sel_mon_key) {
                (
                    monitor.geometry.w_x,
                    monitor.geometry.w_y,
                    monitor.geometry.w_w,
                    monitor.geometry.w_h,
                )
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };

        // 应用边缘吸附
        self.apply_edge_snapping(
            client_key, &mut new_x, &mut new_y, mon_wx, mon_wy, mon_ww, mon_wh,
        )?;

        // 检查是否需要切换到浮动模式
        self.check_and_toggle_floating_for_move(client_key, new_x, new_y)?;

        // 如果是浮动窗口或浮动布局，执行移动
        let should_move = self.should_move_client(client_key);

        if should_move {
            let (window_w, window_h) = if let Some(client) = self.clients.get(client_key) {
                (client.geometry.w, client.geometry.h)
            } else {
                return Ok(());
            };

            self.resize_client(client_key, new_x, new_y, window_w, window_h, true);
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
        _window_id: Window,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 释放鼠标抓取
        self.x11rb_conn.ungrab_pointer(0u32)?;
        self.x11rb_conn.flush()?;

        // 检查窗口移动后是否跨越了显示器边界
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

    pub fn resizemouse(&mut self, _arg: &WMArgEnum) -> Result<(), Box<dyn std::error::Error>> {
        info!("[resizemouse]");

        // 1. 获取当前选中的客户端
        let client_key = match self.get_selected_client_key() {
            Some(key) => key,
            None => {
                debug!("No selected client for resize");
                return Ok(());
            }
        };

        // 2. 全屏检查
        if let Some(client) = self.clients.get(client_key) {
            if client.state.is_fullscreen {
                debug!("Cannot resize fullscreen window");
                return Ok(());
            }
        } else {
            return Err("Selected client not found".into());
        }

        // 3. 准备工作
        self.restack(self.sel_mon)?;

        // 保存窗口开始调整大小时的信息
        let (original_x, original_y, border_width, window_id, current_w, current_h) = {
            let client = self.clients.get(client_key).unwrap();
            (
                client.geometry.x,
                client.geometry.y,
                client.geometry.border_w,
                client.win,
                client.geometry.w,
                client.geometry.h,
            )
        };

        // 4. 抓取鼠标指针
        let cursor = self
            .cursor_manager
            .get_cursor(&self.x11rb_conn, crate::xcb_util::StandardCursor::Fleur)?;

        let grab_reply = self
            .x11rb_conn
            .grab_pointer(
                false,
                self.x11rb_root,
                *MOUSEMASK,
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
        let result = self.resize_loop(client_key, original_x, original_y, border_width);

        // 7. 清理工作
        self.cleanup_resize(window_id, border_width)?;

        result
    }

    fn resize_loop(
        &mut self,
        client_key: ClientKey,
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
                    // self.expose(&e)?;
                }
                Event::MapRequest(e) => {
                    // self.maprequest(&e)?;
                }
                Event::MotionNotify(e) => {
                    // 节流处理
                    if e.time.wrapping_sub(last_motion_time) <= 16 {
                        // ~60 FPS
                        continue;
                    }
                    last_motion_time = e.time;

                    self.handle_resize_motion(
                        client_key,
                        &e,
                        original_x,
                        original_y,
                        border_width,
                    )?;
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
        client_key: ClientKey,
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
        self.check_and_toggle_floating_for_resize(client_key, new_width, new_height)?;

        // 如果是浮动窗口或浮动布局，执行调整大小
        let should_resize = self.should_resize_client(client_key);

        if should_resize {
            self.resize_client(
                client_key, original_x, original_y, new_width, new_height, true,
            );
        }

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
        window_id: Window,
        border_width: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 将鼠标定位到最终位置
        let (final_w, final_h) = {
            let client_key = self.get_selected_client_key();
            if let Some(key) = client_key {
                if let Some(client) = self.clients.get(key) {
                    (client.geometry.w, client.geometry.h)
                } else {
                    return Ok(());
                }
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

        // 检查是否需要移动到不同的显示器
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
                // 注意：这里需要实现focus方法的SlotMap版本
                // self.focus(None)?;
            }
        }

        Ok(())
    }

    pub fn setup_modifier_masks(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Setting up modifier masks...");
        // 1. 获取NumLock的keycode
        let numlock_keycode = self.find_numlock_keycode()?;
        if numlock_keycode == 0 {
            warn!("NumLock key not found, using default mask");
            self.numlock_mask = KeyButMask::MOD2; // 默认Mod2
            return Ok(());
        }
        // 2. 获取修饰键映射
        let modifier_mapping = self.x11rb_conn.get_modifier_mapping()?.reply()?;
        // 3. 查找NumLock对应的修饰键位
        let numlock_mask = self.find_modifier_mask(numlock_keycode, &modifier_mapping);
        self.numlock_mask = KeyButMask::from(numlock_mask);
        info!(
            "NumLock detection: keycode={}, {:?}",
            numlock_keycode, self.numlock_mask,
        );
        // 4. 验证结果
        self.verify_modifier_setup()?;
        Ok(())
    }

    fn find_numlock_keycode(&self) -> Result<u8, Box<dyn std::error::Error>> {
        // NumLock的keysym值
        const XK_NUM_LOCK: u32 = 0xFF7F;
        // 获取键盘映射
        let setup = self.x11rb_conn.setup();
        let min_keycode = setup.min_keycode;
        let max_keycode = setup.max_keycode;
        let keyboard_mapping = self
            .x11rb_conn
            .get_keyboard_mapping(min_keycode, max_keycode - min_keycode + 1)?
            .reply()?;
        let keysyms_per_keycode = keyboard_mapping.keysyms_per_keycode as usize;
        // 遍历所有keycode，查找NumLock
        for keycode in min_keycode..=max_keycode {
            let keycode_index = (keycode - min_keycode) as usize;
            let base_index = keycode_index * keysyms_per_keycode;
            // 检查这个keycode的所有keysym
            for i in 0..keysyms_per_keycode {
                let keysym_index = base_index + i;
                if keysym_index < keyboard_mapping.keysyms.len() {
                    if keyboard_mapping.keysyms[keysym_index] == XK_NUM_LOCK {
                        info!("Found NumLock at keycode {}", keycode);
                        return Ok(keycode);
                    }
                }
            }
        }
        warn!("NumLock keycode not found in keyboard mapping");
        Ok(0)
    }

    fn find_modifier_mask(&self, target_keycode: u8, modifier_map: &GetModifierMappingReply) -> u8 {
        let keycodes_per_modifier = modifier_map.keycodes_per_modifier() as usize;
        // 遍历8个修饰键位 (Shift, Lock, Control, Mod1-Mod5)
        for mod_index in 0..8 {
            let start_index = mod_index * keycodes_per_modifier;
            let end_index = start_index + keycodes_per_modifier;
            // 检查这个修饰键位的所有keycode
            if end_index <= modifier_map.keycodes.len() {
                for &keycode in &modifier_map.keycodes[start_index..end_index] {
                    if keycode == target_keycode && keycode != 0 {
                        let mask = 1 << mod_index;
                        info!(
                            "NumLock found at modifier index {} ({}), mask=0x{:02x}",
                            mod_index,
                            self.modifier_index_to_name(mod_index),
                            mask
                        );
                        return mask;
                    }
                }
            }
        }
        warn!(
            "NumLock keycode {} not found in modifier mapping",
            target_keycode
        );
        0
    }

    fn modifier_index_to_name(&self, index: usize) -> &'static str {
        match index {
            0 => "Shift",
            1 => "Lock",
            2 => "Control",
            3 => "Mod1",
            4 => "Mod2",
            5 => "Mod3",
            6 => "Mod4",
            7 => "Mod5",
            _ => "Unknown",
        }
    }

    fn verify_modifier_setup(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 获取当前修饰键状态来验证设置
        let pointer_query = self.x11rb_conn.query_pointer(self.x11rb_root)?.reply()?;

        info!("Current modifier state: {:?}", pointer_query.mask);
        if self.numlock_mask != KeyButMask::default() {
            let numlock_active = pointer_query.mask & self.numlock_mask != 0u16.into();
            info!(
                "NumLock currently {}",
                if numlock_active { "ON" } else { "OFF" }
            );
        }

        Ok(())
    }

    pub fn setclienttagprop(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let monitor_num = if let Some(mon_key) = client.mon {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    monitor.num as u32
                } else {
                    0
                }
            } else {
                0
            };

            let data: [u32; 2] = [client.state.tags, monitor_num];

            use x11rb::wrapper::ConnectionExt;
            self.x11rb_conn.change_property32(
                PropMode::REPLACE,
                client.win,
                self.atoms._NET_CLIENT_INFO,
                AtomEnum::CARDINAL,
                &data,
            )?;
        }
        Ok(())
    }

    pub fn sendevent(&mut self, client_mut: &mut WMClient, proto: Atom) -> bool {
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
        use x11rb::x11_utils::Serialize;
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
            if let Some(target_monitor_key) = self.get_monitor_by_id(monitor_id) {
                if Some(target_monitor_key) != self.sel_mon {
                    // 取消当前选中客户端的焦点
                    let current_sel = self.get_selected_client_key();
                    self.unfocus_client_opt(current_sel, true)?;

                    // 切换到目标监视器
                    self.sel_mon = Some(target_monitor_key);

                    // 重新设置焦点
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
        let client_key_opt = self.wintoclient(e.event);
        let monitor_key_opt = if let Some(client_key) = client_key_opt {
            // 如果是已管理的客户端，获取其所在监视器
            self.clients.get(client_key).and_then(|client| client.mon)
        } else {
            // 如果事件窗口不是已管理的客户端，尝试根据窗口ID确定显示器
            self.wintomon(e.event)
        };

        // 如果无法确定显示器，则不处理
        let current_event_monitor_key = match monitor_key_opt {
            Some(monitor_key) => monitor_key,
            None => return Ok(()),
        };

        // 处理显示器焦点切换
        let is_on_selected_monitor = Some(current_event_monitor_key) == self.sel_mon;

        if !is_on_selected_monitor {
            self.switch_to_monitor(current_event_monitor_key)?;
        }

        // 处理客户端焦点切换
        if self.should_focus_client_slotmap(client_key_opt, is_on_selected_monitor) {
            self.focus(client_key_opt)?;
        }

        Ok(())
    }

    fn switch_to_monitor(
        &mut self,
        target_monitor_key: MonitorKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 获取旧选中显示器上的选中客户端
        let previously_selected_client_opt = self.get_selected_client_key();

        // 从旧显示器的选中客户端上移除焦点，并将X焦点设回根
        self.unfocus_client_opt(previously_selected_client_opt, true)?;

        // 更新选中显示器为当前事件发生的显示器
        self.sel_mon = Some(target_monitor_key);

        if let Some(monitor) = self.monitors.get(target_monitor_key) {
            debug!("Switched to monitor {}", monitor.num);
        }

        Ok(())
    }

    fn should_focus_client_slotmap(
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

    pub fn expose(&mut self, e: &ExposeEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[expose]");
        // 只处理最后一个expose事件（count为0时）
        if e.count != 0 {
            return Ok(());
        }

        // 检查窗口所在的显示器并标记状态栏需要更新
        if let Some(monitor_key) = self.wintomon(e.window) {
            if let Some(monitor) = self.monitors.get(monitor_key) {
                self.mark_bar_update_needed(Some(monitor.num));
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
            self.grabbuttons_for_client(client_key, false)?;

            // 设置边框颜色为非选中状态
            self.set_window_border_color(win, false)?;

            if setfocus {
                // 将焦点设置到根窗口
                self.x11rb_conn
                    .set_input_focus(InputFocus::POINTER_ROOT, self.x11rb_root, 0u32)?;

                // 清除 _NET_ACTIVE_WINDOW 属性
                self.x11rb_conn
                    .delete_property(self.x11rb_root, self.atoms._NET_ACTIVE_WINDOW)?;
            }

            self.x11rb_conn.flush()?;
        }

        Ok(())
    }

    // 为客户端抓取按钮的SlotMap版本
    fn grabbuttons_for_client(
        &mut self,
        client_key: ClientKey,
        focused: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let client_win_id = client.win;

            let modifiers_to_try = [
                KeyButMask::default(),
                KeyButMask::LOCK,
                self.numlock_mask,
                self.numlock_mask | KeyButMask::LOCK,
            ];

            // 取消之前的按钮抓取
            self.x11rb_conn
                .ungrab_button(ButtonIndex::ANY, client_win_id, ModMask::ANY.into())?;

            if !focused {
                self.x11rb_conn.grab_button(
                    false, // owner_events
                    client_win_id,
                    *BUTTONMASK,
                    GrabMode::SYNC,
                    GrabMode::SYNC,
                    0u32, // confine_to
                    0u32, // cursor
                    ButtonIndex::ANY,
                    ModMask::ANY.into(),
                )?;
            }

            for button_config in CONFIG.get_buttons().iter() {
                if button_config.click_type == WMClickType::ClickClientWin {
                    for &modifier_combo in modifiers_to_try.iter() {
                        self.x11rb_conn.grab_button(
                            false,
                            client_win_id,
                            *BUTTONMASK,
                            GrabMode::ASYNC,
                            GrabMode::ASYNC,
                            0u32,
                            0u32,
                            button_config.button,
                            ModMask::from(button_config.mask.bits() | modifier_combo.bits()),
                        )?;
                    }
                }
            }

            self.x11rb_conn.flush()?;
        }

        Ok(())
    }

    pub fn focus(
        &mut self,
        mut client_key_opt: Option<ClientKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[focus]");

        // 如果传入的是状态栏客户端，忽略并寻找合适的替代
        if let Some(client_key) = client_key_opt {
            if let Some(client) = self.clients.get(client_key) {
                if self.status_bar_windows.contains_key(&client.win) {
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
        self.mark_bar_update_needed(None);

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
        self.grabbuttons_by_key(client_key, true)?;

        // 设置边框颜色为选中状态
        if let Some(client) = self.clients.get(client_key) {
            self.set_window_border_color(client.win, true)?;
        }

        // 设置焦点
        self.setfocus_by_key(client_key)?;

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
            self.grabbuttons_by_key(client_key, false)?;

            // 设置边框颜色为非选中状态
            self.set_window_border_color(win, false)?;

            if setfocus {
                self.x11rb_conn
                    .set_input_focus(InputFocus::POINTER_ROOT, self.x11rb_root, 0u32)?;

                self.x11rb_conn
                    .delete_property(self.x11rb_root, self.atoms._NET_ACTIVE_WINDOW)?;
            }

            self.x11rb_conn.flush()?;
        }

        Ok(())
    }

    // grabbuttons的SlotMap版本
    fn grabbuttons_by_key(
        &mut self,
        client_key: ClientKey,
        focused: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_win_id = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        let modifiers_to_try = [
            KeyButMask::default(),
            KeyButMask::LOCK,
            self.numlock_mask,
            self.numlock_mask | KeyButMask::LOCK,
        ];

        // 取消之前的按钮抓取
        self.x11rb_conn
            .ungrab_button(ButtonIndex::ANY, client_win_id, ModMask::ANY.into())?;

        if !focused {
            self.x11rb_conn.grab_button(
                false, // owner_events
                client_win_id,
                *BUTTONMASK,
                GrabMode::SYNC,
                GrabMode::SYNC,
                0u32, // confine_to
                0u32, // cursor
                ButtonIndex::ANY,
                ModMask::ANY.into(),
            )?;
        }

        for button_config in CONFIG.get_buttons().iter() {
            if button_config.click_type == WMClickType::ClickClientWin {
                for &modifier_combo in modifiers_to_try.iter() {
                    self.x11rb_conn.grab_button(
                        false,
                        client_win_id,
                        *BUTTONMASK,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                        0u32,
                        0u32,
                        button_config.button,
                        ModMask::from(button_config.mask.bits() | modifier_combo.bits()),
                    )?;
                }
            }
        }

        self.x11rb_conn.flush()?;
        Ok(())
    }

    // setfocus的SlotMap版本
    fn setfocus_by_key(&mut self, client_key: ClientKey) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(client_key) {
            let win = client.win;
            let never_focus = client.state.never_focus;

            if !never_focus {
                self.x11rb_conn.set_input_focus(
                    InputFocus::POINTER_ROOT,
                    win,
                    0u32, // time
                )?;

                use x11rb::wrapper::ConnectionExt;
                self.x11rb_conn.change_property32(
                    PropMode::REPLACE,
                    self.x11rb_root,
                    self.atoms._NET_ACTIVE_WINDOW,
                    AtomEnum::WINDOW,
                    &[win],
                )?;
            }

            self.sendevent_by_window(win, self.atoms.WM_TAKE_FOCUS);
            self.x11rb_conn.flush()?;
        }

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

    fn update_net_client_list(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;
        // 清空现有列表
        let _ = self
            .x11rb_conn
            .delete_property(self.x11rb_root, self.atoms._NET_CLIENT_LIST);
        // 收集所有客户端窗口ID
        let mut all_windows = Vec::new();
        for &mon_key in &self.monitor_order {
            if let Some(client_keys) = self.monitor_clients.get(mon_key) {
                for &client_key in client_keys {
                    if let Some(client) = self.clients.get(client_key) {
                        all_windows.push(client.win);
                    }
                }
            }
        }
        // 一次性设置所有窗口
        if !all_windows.is_empty() {
            self.x11rb_conn.change_property32(
                PropMode::REPLACE,
                self.x11rb_root,
                self.atoms._NET_CLIENT_LIST,
                AtomEnum::WINDOW,
                &all_windows,
            )?;
        }
        info!(
            "[update_net_client_list] Updated _NET_CLIENT_LIST with {} windows",
            all_windows.len()
        );
        Ok(())
    }

    pub fn setclientstate(
        &self,
        client: &WMClient,
        state: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[setclientstate]");
        let data_to_set: [u32; 2] = [state as u32, 0]; // 0 代表 None (无图标窗口)
        let win = client.win;
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
        self.enable_move_cursor_to_client_center = false;
        // 使用缓存的键盘映射转换keycode到keysym
        let keysym = self.get_keysym_from_keycode(e.detail)?;
        debug!(
            "[keypress] keycode: {}, keysym: 0x{:x}, raw_state: {:?}, clean_state: {:?}",
            e.detail,
            keysym,
            e.state,
            self.clean_mask(e.state.bits())
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
            if self.is_key_match(key_config, keysym, clean_state) {
                info!(
                    "[keypress] executing binding {}: keysym=0x{:x}, mod={:?}, arg={:?}",
                    i, key_config.key_sym, key_config.mask, key_config.arg,
                );
                if let Some(func) = key_config.func_opt {
                    let _ = func(self, &key_config.arg);
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn is_key_match(&self, key_config: &WMKey, keysym: u32, clean_state: KeyButMask) -> bool {
        keysym == key_config.key_sym as u32
            && self.clean_mask(key_config.mask.bits()) == clean_state
            && key_config.func_opt.is_some()
    }

    /// 清除键盘映射缓存（在键盘映射变更时调用）
    pub fn clear_keycode_cache(&mut self) {
        self.keycode_cache.clear();
        info!("Keycode cache cleared");
    }

    pub fn manage_restored(
        &mut self,
        restored_client: &WMClientRestore,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[manage_restored]");

        // 创建新的客户端
        let mut client = WMClient::new();
        client.win = restored_client.win;
        client.name = restored_client.name.clone();
        client.instance = restored_client.instance.clone();
        client.class = restored_client.class.clone();
        client.geometry = restored_client.geometry.clone();
        client.state = restored_client.state.clone();
        client.size_hints = restored_client.size_hints.clone();

        info!("[manage_restored] {}", client);

        // 插入到SlotMap并获取key
        let client_key = self.insert_client(client);

        // 管理恢复的客户端
        self.manage_restored_client(client_key, restored_client)
    }

    pub fn manage(
        &mut self,
        w: Window,
        geom: &GetGeometryReply,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[manage] Managing window 0x{:x}", w);

        // 检查窗口是否已被管理
        if self.wintoclient(w).is_some() {
            warn!("[manage] Window 0x{:x} already managed", w);
            return Ok(());
        }

        // 创建新的客户端对象
        let mut client = WMClient::new();

        // 设置窗口ID
        client.win = w;

        // 从几何信息中设置初始属性
        client.geometry.x = geom.x as i32;
        client.geometry.old_x = geom.x as i32;
        client.geometry.y = geom.y as i32;
        client.geometry.old_y = geom.y as i32;
        client.geometry.w = geom.width as i32;
        client.geometry.old_w = geom.width as i32;
        client.geometry.h = geom.height as i32;
        client.geometry.old_h = geom.height as i32;
        client.geometry.old_border_w = geom.border_width as i32;
        client.state.client_fact = 1.0;

        // 获取并设置窗口标题
        self.updatetitle_by_window(&mut client);

        #[cfg(any(feature = "nixgl", feature = "tauri_bar"))]
        {
            if client.name == CONFIG.status_bar_base_name() {
                let mut instance_name = String::new();
                for &tmp_num in self.status_bar_child.keys() {
                    if !self.status_bar_clients.contains_key(&tmp_num) {
                        instance_name = match tmp_num {
                            0 => CONFIG.status_bar_instance_0().to_string(),
                            1 => CONFIG.status_bar_instance_1().to_string(),
                            _ => CONFIG.status_bar_base_name().to_string(),
                        };
                        break;
                    }
                }
                if !instance_name.is_empty() {
                    let _ = self.set_class_info(
                        &mut client,
                        instance_name.as_str(),
                        instance_name.as_str(),
                    );
                    // 重新获取类信息
                    self.update_class_info_by_window(&mut client);
                }
            }
        }

        self.update_class_info_by_window(&mut client);
        info!("[manage] {}", client);

        // 检查是否是状态栏
        if client.is_status_bar() {
            info!("[manage] Detected status bar, managing as statusbar");
            let client_key = self.insert_client(client);
            return self.manage_statusbar(client_key);
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
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        info!("[setup_client_window] Setting up window 0x{:x}", win);

        // 1. 设置边框宽度
        if let Some(client) = self.clients.get_mut(client_key) {
            client.geometry.border_w = CONFIG.border_px() as i32;
        }

        let border_w = self.clients.get(client_key).unwrap().geometry.border_w;
        self.set_window_border_width(win, border_w as u32)?;

        // 2. 设置边框颜色为"正常"状态的颜色
        self.set_window_border_color(win, true)?;

        // 3. 发送 ConfigureNotify 事件给客户端
        self.configure_client(client_key)?;

        // 4. 设置窗口在屏幕外的临时位置（避免闪烁）
        let (x, y, w, h) = if let Some(client) = self.clients.get(client_key) {
            let offscreen_x = client.geometry.x + 2 * self.s_w; // 移到屏幕外
            (
                offscreen_x,
                client.geometry.y,
                client.geometry.w,
                client.geometry.h,
            )
        } else {
            return Err("Client not found".into());
        };

        let aux = ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .width(w as u32)
            .height(h as u32);
        self.x11rb_conn.configure_window(win, &aux)?;
        self.x11rb_conn.flush()?;

        // 5. 设置客户端的 WM_STATE 为 NormalState
        if let Some(client) = self.clients.get(client_key) {
            self.setclientstate(client, NORMAL_STATE as i64)?;
        }

        // 6. 同步所有操作
        self.x11rb_conn.flush()?;
        info!(
            "[setup_client_window] Window setup completed for 0x{:x}",
            win
        );
        Ok(())
    }

    fn handle_new_client_focus(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 检查新窗口所在的显示器是否是当前选中的显示器
        let (client_mon_key, is_never_focus) = if let Some(client) = self.clients.get(client_key) {
            (client.mon, client.state.never_focus)
        } else {
            return Err("Client not found".into());
        };

        let current_client_monitor_is_selected_monitor = client_mon_key == self.sel_mon;

        if current_client_monitor_is_selected_monitor {
            // 取消当前选中窗口的焦点
            let prev_sel_opt = self.get_selected_client_key();
            if let Some(prev_key) = prev_sel_opt {
                self.unfocus(prev_key, false)?; // false: 不立即设置根窗口焦点
                info!("[handle_new_client_focus] Unfocused previous client");
            }

            // 将新窗口设为其所在显示器的选中窗口
            if let Some(mon_key) = client_mon_key {
                if let Some(monitor) = self.monitors.get_mut(mon_key) {
                    monitor.sel = Some(client_key);
                }
            }

            // 重新排列该显示器的窗口
            if let Some(mon_key) = client_mon_key {
                self.arrange(Some(mon_key));
            }

            // 设置焦点到新窗口（如果它不是 never_focus）
            if !is_never_focus {
                self.focus(Some(client_key))?;
                if let Some(client) = self.clients.get(client_key) {
                    info!(
                        "[handle_new_client_focus] Focused new client: {}",
                        client.name
                    );
                }
            } else {
                // 如果新窗口是 never_focus，重新评估焦点
                self.focus(None)?;
                info!("[handle_new_client_focus] New client is never_focus, re-evaluated focus");
            }
        } else {
            // 如果新窗口不在当前选中的显示器上
            // 将新窗口设为其所在显示器的选中窗口
            if let Some(mon_key) = client_mon_key {
                if let Some(monitor) = self.monitors.get_mut(mon_key) {
                    monitor.sel = Some(client_key);
                }

                // 只重新排列该显示器，不改变全局焦点
                self.arrange(Some(mon_key));
            }
            info!("[handle_new_client_focus] New client on non-selected monitor, arranged only");
        }

        // 根据配置决定是否自动切换到新窗口的显示器
        if CONFIG.behavior().focus_follows_new_window {
            if let Some(new_mon_key) = client_mon_key {
                if Some(new_mon_key) != self.sel_mon {
                    // 切换到新窗口的显示器
                    let old_sel = self.get_selected_client_key();
                    if let Some(old_key) = old_sel {
                        self.unfocus(old_key, true)?;
                    }
                    self.sel_mon = Some(new_mon_key);
                    self.focus(Some(client_key))?;
                    info!("[handle_new_client_focus] Switched to new window's monitor");
                }
            }
        }

        Ok(())
    }

    fn manage_restored_client(
        &mut self,
        client_key: ClientKey,
        restored_client: &WMClientRestore,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[manage_restored_client]");

        // 查找匹配的监视器
        let target_monitor_key = self
            .monitor_order
            .iter()
            .find(|&&mon_key| {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    monitor.num == restored_client.monitor_num as i32
                } else {
                    false
                }
            })
            .copied();

        if let Some(mon_key) = target_monitor_key {
            if let Some(client) = self.clients.get_mut(client_key) {
                client.mon = Some(mon_key);
                info!(
                    "[manage_restored_client] set monitor number: {}",
                    restored_client.monitor_num
                );
            }
        }

        // 调整窗口位置
        self.adjust_client_position(client_key);

        // 设置窗口属性
        self.setup_client_window(client_key)?;

        // 更新窗口提示
        self.updatewmhints(client_key);

        // 添加到管理结构
        self.attach(client_key);
        self.attachstack(client_key);

        // 注册事件和抓取按钮
        self.register_client_events(client_key)?;

        // 更新客户端列表
        self.update_net_client_list()?;

        // 映射窗口
        self.map_client_window(client_key)?;

        // 处理焦点
        self.handle_new_client_focus(client_key)?;

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

        // 更新客户端列表
        self.update_net_client_list()?;

        // 映射窗口
        self.map_client_window(client_key)?;

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

        // 使用 x11rb 获取 WM_TRANSIENT_FOR 属性
        match self.get_transient_for_hint(win) {
            Ok(Some(transient_for_win)) => {
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
            Ok(None) => {
                info!("no WM_TRANSIENT_FOR property");
                // 没有 WM_TRANSIENT_FOR 属性
                if let Some(client) = self.clients.get_mut(client_key) {
                    client.mon = self.sel_mon;
                }
                self.applyrules_by_key(client_key);
            }
            Err(e) => {
                warn!("Failed to get transient_for hint: {:?}", e);
                // 失败时使用默认行为
                if let Some(client) = self.clients.get_mut(client_key) {
                    client.mon = self.sel_mon;
                }
                self.applyrules_by_key(client_key);
            }
        }
        Ok(())
    }

    // 辅助方法
    fn updatetitle_by_window(&mut self, client: &mut WMClient) {
        if !self.gettextprop(client.win, self.atoms._NET_WM_NAME, &mut client.name) {
            self.gettextprop(client.win, AtomEnum::WM_NAME.into(), &mut client.name);
        }
    }

    fn update_class_info_by_window(&mut self, client: &mut WMClient) {
        if let Some((inst, cls)) = Self::get_wm_class(&self.x11rb_conn, client.win) {
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

    /// 应用所有规则到客户端（完整版本）
    fn applyrules_by_key(&mut self, client_key: ClientKey) {
        let (win, mut name, mut class, mut instance) =
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
            if let Some((inst, cls)) = Self::get_wm_class(&self.x11rb_conn, win) {
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

        // 特殊处理：如果所有信息都为空，设置为浮动
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
        }

        // 设置默认标签
        self.set_default_tags(client_key);

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

        // 选择窗口事件
        let aux = ChangeWindowAttributesAux::new().event_mask(
            EventMask::ENTER_WINDOW
                | EventMask::FOCUS_CHANGE
                | EventMask::PROPERTY_CHANGE
                | EventMask::STRUCTURE_NOTIFY,
        );
        self.x11rb_conn.change_window_attributes(win, &aux)?;

        // 抓取按钮
        self.grabbuttons_by_key(client_key, false)?;

        // 更新 EWMH _NET_CLIENT_LIST
        use x11rb::wrapper::ConnectionExt;
        self.x11rb_conn.change_property32(
            PropMode::APPEND,
            self.x11rb_root,
            self.atoms._NET_CLIENT_LIST,
            AtomEnum::WINDOW,
            &[win],
        )?;

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

        match self.x11rb_conn.map_window(win) {
            Ok(cookie) => {
                if let Err(e) = cookie.check() {
                    error!(
                        "[map_client_window] Failed to map window 0x{:x}: {:?}",
                        win, e
                    );
                    return Err(e.into());
                }
            }
            Err(e) => {
                error!(
                    "[map_client_window] Failed to send map_window request for 0x{:x}: {:?}",
                    win, e
                );
                return Err(e.into());
            }
        }

        self.x11rb_conn.flush()?;
        info!("[map_client_window] Successfully mapped window 0x{:x}", win);
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

    /// 状态栏管理的SlotMap版本（保持与现有系统兼容）
    fn manage_statusbar(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 确定状态栏所属的显示器
        let monitor_id = if let Some(client) = self.clients.get(client_key) {
            self.determine_statusbar_monitor(client)
        } else {
            return Err("Client not found".into());
        };

        info!("[manage_statusbar] monitor_id: {}", monitor_id);

        // 配置状态栏客户端
        let mon_key_opt = self.get_monitor_by_id(monitor_id);
        if let Some(client) = self.clients.get_mut(client_key) {
            client.mon = mon_key_opt;
            client.state.never_focus = true;
            client.state.is_floating = true;
            client.state.tags = CONFIG.tagmask(); // 在所有标签可见
            client.geometry.border_w = CONFIG.border_px() as i32;
        }

        // 调整状态栏位置（通常在顶部）
        self.position_statusbar_by_key(client_key, monitor_id)?;

        // 设置状态栏特有的窗口属性
        self.setup_statusbar_window_by_key(client_key)?;

        // 为了保持与现有系统的兼容性，创建Rc<RefCell<WMClient>>
        let client_rc = if let Some(client) = self.clients.get(client_key) {
            Rc::new(RefCell::new(client.clone()))
        } else {
            return Err("Client not found after configuration".into());
        };

        let win = client_rc.borrow().win;

        // 注册状态栏到管理映射中
        self.status_bar_clients
            .insert(monitor_id, client_rc.clone());
        self.status_bar_windows.insert(win, monitor_id);
        self.status_bar_flags
            .insert(monitor_id, WMShowBarEnum::Keep(true));

        // 映射状态栏窗口
        if let Err(e) = self.x11rb_conn.map_window(win) {
            error!(
                "[manage_statusbar] Failed to map statusbar window 0x{:x}: {:?}",
                win, e
            );
        } else {
            debug!("[manage_statusbar] Mapped statusbar window 0x{:x}", win);
        }

        info!(
            "[manage_statusbar] Successfully managed statusbar on monitor {}",
            monitor_id
        );
        Ok(())
    }

    /// 确定状态栏应该在哪个显示器（SlotMap适配版本）
    fn determine_statusbar_monitor(&self, client: &WMClient) -> i32 {
        info!("[determine_statusbar_monitor]: {}", client);

        // 尝试从窗口名称中解析监视器ID
        if let Some(suffix) = client
            .name
            .strip_prefix(&format!("{}_", CONFIG.status_bar_base_name()))
        {
            if let Ok(monitor_id) = suffix.parse::<i32>() {
                return monitor_id;
            }
        }

        // 尝试从类名中解析监视器ID
        if let Some(suffix) = client
            .class
            .strip_prefix(&format!("{}_", CONFIG.status_bar_base_name()))
        {
            if let Ok(monitor_id) = suffix.parse::<i32>() {
                return monitor_id;
            }
        }

        // 尝试从实例名中解析监视器ID
        if let Some(suffix) = client
            .instance
            .strip_prefix(&format!("{}_", CONFIG.status_bar_base_name()))
        {
            if let Ok(monitor_id) = suffix.parse::<i32>() {
                return monitor_id;
            }
        }

        // 默认使用当前选中的监视器
        self.sel_mon
            .and_then(|mon_key| self.monitors.get(mon_key))
            .map(|monitor| monitor.num)
            .unwrap_or(0)
    }

    /// 定位状态栏（SlotMap版本）
    fn position_statusbar_by_key(
        &mut self,
        client_key: ClientKey,
        monitor_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(monitor_key) = self.get_monitor_by_id(monitor_id) {
            if let Some(monitor) = self.monitors.get(monitor_key) {
                let bar_padding = CONFIG.status_bar_padding();

                // 计算状态栏位置
                let x = monitor.geometry.m_x + bar_padding;
                let y = monitor.geometry.m_y + bar_padding;
                let w = monitor.geometry.m_w - 2 * bar_padding;
                let h = CONFIG.status_bar_height();

                // 更新客户端几何信息
                if let Some(client) = self.clients.get_mut(client_key) {
                    client.geometry.x = x;
                    client.geometry.y = y;
                    client.geometry.w = w;
                    client.geometry.h = h;

                    info!(
                        "[position_statusbar_by_key] Positioned at ({}, {}) {}x{}",
                        x, y, w, h
                    );

                    // 配置X11窗口
                    let aux = ConfigureWindowAux::new()
                        .x(x)
                        .y(y)
                        .width(w as u32)
                        .height(h as u32);
                    self.x11rb_conn.configure_window(client.win, &aux)?;
                    self.x11rb_conn.flush()?;
                }
            }
        }
        Ok(())
    }

    /// 设置状态栏窗口属性（SlotMap版本）
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

        // 设置状态栏窗口的事件监听
        let aux = ChangeWindowAttributesAux::new().event_mask(
            EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE | EventMask::ENTER_WINDOW,
        );
        self.x11rb_conn.change_window_attributes(win, &aux)?;

        // 发送配置通知
        self.configure_client(client_key)?;

        // 同步操作
        self.x11rb_conn.flush()?;
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

    #[cfg(any(feature = "nixgl", feature = "tauri_bar"))]
    fn set_class_info(
        &mut self,
        client_mut: &mut WMClient,
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

    #[cfg(any(feature = "nixgl", feature = "tauri_bar"))]
    fn verify_class_info_set(
        &mut self,
        client: &WMClient,
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
    fn update_class_info(&mut self, client_mut: &mut WMClient) {
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

    pub fn monocle(&mut self, mon_key: MonitorKey) {
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
        let new_monitor_key = self.recttomon(e.root_x as i32, e.root_y as i32, 1, 1);

        // 检查是否切换了显示器
        if new_monitor_key != self.motion_mon {
            self.handle_monitor_switch_by_key(new_monitor_key)?;
        }

        // 更新motion_mon
        self.motion_mon = new_monitor_key;
        Ok(())
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

    pub fn unmanage(
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
        if let Some(&monitor_id) = self.status_bar_windows.get(&win) {
            self.unmanage_statusbar(monitor_id, destroyed)?;
            return Ok(());
        }

        // 常规客户端的 unmanage 逻辑
        self.unmanage_regular_client(client_key, destroyed)?;
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
        if !destroyed {
            self.cleanup_statusbar_window(win)?;
        }
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
        for (operation, result) in cleanup_results.iter() {
            if let Err(ref e) = result {
                error!(
                    "[unmanage_statusbar] {} failed for monitor {}: {}",
                    operation, monitor_id, e
                );
            }
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
                let shmem_name = format!("{}_{}", CONFIG.status_bar_base_name(), monitor_id);
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

    fn adjust_client_position(&mut self, client_key: ClientKey) {
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

    // 如果需要批量调整客户端位置的优化版本
    fn adjust_multiple_clients_positions(&mut self, client_keys: &[ClientKey]) {
        for &client_key in client_keys {
            self.adjust_client_position(client_key);
        }
    }

    // 针对特定监视器调整所有客户端位置
    fn adjust_all_clients_on_monitor(&mut self, mon_key: MonitorKey) {
        let client_keys: Vec<ClientKey> =
            if let Some(client_list) = self.monitor_clients.get(mon_key) {
                client_list.clone()
            } else {
                return;
            };

        for client_key in client_keys {
            self.adjust_client_position(client_key);
        }
    }

    // 智能调整：只调整超出边界的客户端
    fn adjust_client_position_smart(&mut self, client_key: ClientKey) {
        let needs_adjustment = if let Some(client) = self.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(monitor) = self.monitors.get(mon_key) {
                    let (mon_wx, mon_wy, mon_ww, mon_wh) = (
                        monitor.geometry.w_x,
                        monitor.geometry.w_y,
                        monitor.geometry.w_w,
                        monitor.geometry.w_h,
                    );

                    let client_total_width = client.total_width();
                    let client_total_height = client.total_height();

                    // 检查是否需要调整
                    client.geometry.x < mon_wx
                        || client.geometry.y < mon_wy
                        || client.geometry.x + client_total_width > mon_wx + mon_ww
                        || client.geometry.y + client_total_height > mon_wy + mon_wh
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if needs_adjustment {
            self.adjust_client_position(client_key);
        }
    }

    pub fn unmanage_regular_client(
        &mut self,
        client_key: ClientKey,
        destroyed: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[unmanage_regular_client] Removing client {:?}", client_key);

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
        let client = if let Some(client) = self.clients.get(client_key) {
            client
        } else {
            return Err("Client not found".into());
        };
        let (win, old_border_w) = (client.win, client.geometry.old_border_w);

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
            if let Err(e) = self.setclientstate(client, WITHDRAWN_STATE as i64) {
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
            "[cleanup_window_state] Window cleanup completed for 0x{}",
            win
        );
        Ok(())
    }

    pub fn unmapnotify(&mut self, e: &UnmapNotifyEvent) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[unmapnotify]");
        if let Some(client_key) = self.wintoclient(e.window) {
            if e.from_configure {
                // 这是由于配置请求导致的unmap（通常是合成窗口管理器）
                debug!("Unmap from configure for window {}", e.window);
                let client = if let Some(client) = self.clients.get(client_key) {
                    client
                } else {
                    return Ok(());
                };
                self.setclientstate(client, WITHDRAWN_STATE as i64)?;
            } else {
                // 这是真正的窗口销毁或隐藏
                debug!("Real unmap for window {}, unmanaging", e.window);
                self.unmanage(Some(client_key), false)?;
            }
        } else {
            debug!("Unmap event for unmanaged window: {}", e.window);
        }
        Ok(())
    }

    pub fn updategeom(&mut self) -> bool {
        info!("[updategeom]");

        let dirty = match self.get_monitors_randr() {
            Ok(monitors) => {
                info!("[updategeom] monitors: {:?}", monitors);
                if monitors.is_empty() {
                    self.setup_single_monitor()
                } else {
                    self.setup_multiple_monitors(monitors)
                }
            }
            Err(_) => {
                // RandR 不可用，使用单显示器模式
                self.setup_single_monitor()
            }
        };

        if dirty {
            // 更新选中的监视器
            self.sel_mon = self.wintomon(self.x11rb_root);
            if self.sel_mon.is_none() && !self.monitor_order.is_empty() {
                self.sel_mon = self.monitor_order.first().copied();
            }
        }

        dirty
    }

    fn setup_single_monitor(&mut self) -> bool {
        let mut dirty = false;

        if self.monitor_order.is_empty() {
            let new_monitor = self.createmon();
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
                let new_monitor = self.createmon();
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
        monitors.dedup();

        Ok(monitors)
    }

    pub fn updatewindowtype(&mut self, client_key: ClientKey) {
        // info!("[updatewindowtype]");
        // 获取窗口ID
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            warn!("[updatewindowtype] Client {:?} not found", client_key);
            return;
        };
        // 获取窗口属性
        let state = self.getatomprop_by_window(win, self.atoms._NET_WM_STATE.into());
        let wtype = self.getatomprop_by_window(win, self.atoms._NET_WM_WINDOW_TYPE.into());
        // 处理全屏状态
        if state == self.atoms._NET_WM_STATE_FULLSCREEN {
            let _ = self.setfullscreen(client_key, true);
        }
        // 处理对话框类型
        if wtype == self.atoms._NET_WM_WINDOW_TYPE_DIALOG {
            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.is_floating = true;
            }
        }
    }

    /// 根据窗口ID获取原子属性
    pub fn getatomprop_by_window(&self, window: Window, prop: Atom) -> Atom {
        // 发送 GetProperty 请求
        let cookie = match self.x11rb_conn.get_property(
            false,          // delete: 是否删除属性（false）
            window,         // window
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

    pub fn updatewmhints(&mut self, client_key: ClientKey) {
        // 获取窗口ID
        let win = if let Some(client) = self.clients.get(client_key) {
            client.win
        } else {
            warn!("[updatewmhints] Client {:?} not found", client_key);
            return;
        };

        // 1. 读取 WM_HINTS 属性
        use x11rb::wrapper::ConnectionExt;
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
        let is_focused = self.is_client_selected(client_key);

        const X_URGENCY_HINT: u32 = 1 << 8;
        const INPUT_HINT: u32 = 1 << 0;

        // 4. 处理 XUrgencyHint
        if (flags & X_URGENCY_HINT) != 0 {
            if is_focused {
                // 如果是当前选中窗口，清除 urgency hint
                let new_flags = flags & !X_URGENCY_HINT;
                let mut data: Vec<u32> = vec![new_flags];
                data.extend(&mut values); // 保留其余字段

                let _ = self
                    .x11rb_conn
                    .change_property32(
                        PropMode::REPLACE,
                        win,
                        AtomEnum::WM_HINTS,
                        AtomEnum::CARDINAL,
                        &data,
                    )
                    .and_then(|_| self.x11rb_conn.flush());
            } else {
                // 否则标记为 urgent
                if let Some(client) = self.clients.get_mut(client_key) {
                    client.state.is_urgent = true;
                }
            }
        } else {
            // 没有 urgency hint
            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.is_urgent = false;
            }
        }

        // 5. 处理 InputHint
        if (flags & INPUT_HINT) != 0 {
            // InputHint 存在，检查 input 字段
            let input = match values.next() {
                Some(i) => i as i32,
                None => return,
            };

            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.never_focus = input <= 0;
            }
        } else {
            // InputHint 不存在，可聚焦
            if let Some(client) = self.clients.get_mut(client_key) {
                client.state.never_focus = false;
            }
        }
    }

    pub fn updatetitle(&mut self, c: &mut WMClient) {
        // info!("[updatetitle]");
        if !self.gettextprop(c.win, self.atoms._NET_WM_NAME.into(), &mut c.name) {
            self.gettextprop(c.win, AtomEnum::WM_NAME.into(), &mut c.name);
        }
    }

    pub fn update_bar_message_for_monitor(&mut self, mon_key_opt: Option<MonitorKey>) {
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
        let mut current_tag_index = 0;
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

            if is_selected_tag {
                current_tag_index = i + 1;
            }

            monitor_info_for_message.set_tag_status(i, tag_status);
        }

        // 处理状态栏显示状态
        self.update_status_bar_visibility(mon_key, current_tag_index);

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

    /// 更新状态栏可见性
    fn update_status_bar_visibility(&mut self, mon_key: MonitorKey, current_tag_index: usize) {
        let monitor_num = if let Some(monitor) = self.monitors.get(mon_key) {
            monitor.num
        } else {
            return;
        };

        let current_show_bar = if let Some(monitor) = self.monitors.get(mon_key) {
            monitor
                .pertag
                .as_ref()
                .and_then(|pertag| pertag.show_bars.get(current_tag_index))
                .copied()
                .unwrap_or(true)
        } else {
            true
        };

        if let Some(show_bar_enum) = self.status_bar_flags.get_mut(&monitor_num) {
            let prev_show_bar = *show_bar_enum.show_bar();
            if current_show_bar != prev_show_bar {
                *show_bar_enum = WMShowBarEnum::Toggle(current_show_bar);
            }
        }
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
