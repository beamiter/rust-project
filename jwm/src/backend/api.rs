// src/backend/api.rs
use crate::backend::common_define::{ArgbColor, ColorScheme, SchemeType};
pub use crate::backend::common_define::{
    CursorHandle, KeySym, Mods, Pixel, StdCursorKind, WindowId,
};
use std::any::Any;
use std::fmt::Debug;

#[derive(Clone, Copy, Debug)]
pub struct ScreenInfo {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct OutputInfo {
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    pub can_warp_pointer: bool,
    // Wayland 通常不支持客户端列表查询，而是 Compositor 维护
    pub supports_client_list: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum NetWmState {
    Fullscreen,
}

#[derive(Debug, Clone, Default)]
pub struct WindowChanges {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub border_width: Option<u32>,
    pub sibling: Option<WindowId>,
    pub stack_mode: Option<StackMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackMode {
    Above,
    Below,
    TopIf,
    BottomIf,
    Opposite,
}

#[derive(Debug, Clone, Copy)]
pub enum NetWmAction {
    Add,
    Remove,
    Toggle,
}

/// 属性变更类型抽象
#[derive(Debug, Clone, Copy)]
pub enum PropertyKind {
    Title,        // 标题变更 (WM_NAME / _NET_WM_NAME / XDG Toplevel title)
    Class,        // 类型变更 (WM_CLASS / AppID)
    TransientFor, // 父窗口关系
    SizeHints,    // 尺寸限制
    Urgency,      // 紧急状态
    WindowType,   // 窗口类型 (Dialog, Dock etc.)
    Protocols,    // 支持的协议 (Delete window etc.)
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowType {
    Normal,
    Desktop,
    Dock,
    Toolbar,
    Menu,
    Utility,
    Splash,
    Dialog,
    DropdownMenu,
    PopupMenu,
    Tooltip,
    Notification,
    Combo,
    Dnd,
    Unknown,
}

/// 核心事件定义：混合了 X11 原生事件和抽象事件
#[derive(Debug, Clone)]
pub enum BackendEvent {
    // --- 通用高层事件 ---
    /// 窗口创建 (X11 MapRequest / Wayland NewToplevel)
    WindowCreated(WindowId),
    /// 窗口销毁 (X11 DestroyNotify / Wayland SurfaceDestroy)
    WindowDestroyed(WindowId),
    /// 窗口映射 (X11 MapNotify / Wayland SurfaceCommit with buffer)
    WindowMapped(WindowId),
    /// 窗口取消映射
    WindowUnmapped(WindowId),

    WindowConfigured {
        window: WindowId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    },

    // --- 输入事件 ---
    ButtonPress {
        window: WindowId,
        state: u16,
        detail: u8,
        time: u32,
    },
    ButtonRelease {
        window: WindowId,
        time: u32,
    },
    MotionNotify {
        window: WindowId,
        root_x: i16,
        root_y: i16,
        time: u32,
    },
    KeyPress {
        keycode: u8,
        state: u16,
    },

    // --- 窗口管理事件 ---
    EnterNotify {
        window: WindowId,
        subwindow: Option<WindowId>,
    },
    LeaveNotify {
        window: WindowId,
    },
    FocusIn {
        window: WindowId,
    },
    FocusOut {
        window: WindowId,
    },

    /// 客户端请求配置 (X11 ConfigureRequest / Wayland xdg_toplevel.request_bounds)
    ConfigureRequest {
        window: WindowId,
        changes: WindowChanges, // 使用结构体封装参数
        mask_bits: u16,         // 仅 X11 需要，Wayland 可忽略
    },

    /// 属性变更
    PropertyChanged {
        window: WindowId,
        kind: PropertyKind,
    },

    /// 状态变更请求 (Fullscreen, Minimize etc.)
    WindowStateRequest {
        window: WindowId,
        action: NetWmAction,
        state: NetWmState,
    },

    // --- 传统 X11 特定事件保留 (为了兼容现有代码) ---
    ActiveWindowMessage {
        window: WindowId,
    },
    ClientMessage {
        window: WindowId,
        type_: u32,
        data: [u32; 5],
        format: u8,
    },
    MappingNotify,
    Expose {
        window: WindowId,
    },

    // --- 自定义快捷键 ---
    WmKeyboardShortcut {
        keysym: KeySym,
        mods: Mods,
    },
}

#[derive(Debug, Clone)]
pub struct WindowAttributes {
    pub override_redirect: bool, // Wayland 下对应非受管 Surface
    pub map_state_viewable: bool,
}

#[derive(Debug, Clone)]
pub struct Geometry {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    pub border: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllowMode {
    AsyncPointer,
    ReplayPointer,
    SyncPointer,
    AsyncKeyboard,
    SyncKeyboard,
    ReplayKeyboard,
    AsyncBoth,
    SyncBoth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseResult {
    Graceful,
    Forced,
}

// --- Traits 定义 ---

pub trait KeyOps: Send {
    fn grab_keys(
        &self,
        root: WindowId,
        bindings: &[(Mods, KeySym)],
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn clean_mods(&self, raw_state: u16) -> Mods;
    fn clear_key_grabs(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, Box<dyn std::error::Error>>;
    fn clear_cache(&mut self);
}

pub trait InputOps: Send {
    fn set_cursor(&self, kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>>;

    /// 抓取指针：在 Wayland 中通常是隐式的或通过 Serial 处理
    fn grab_pointer(
        &self,
        mask: u32,
        cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>>;
    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// 允许事件继续：主要用于 X11 的 Sync Grab 模式
    fn allow_events(&self, mode: AllowMode, time: u32) -> Result<(), Box<dyn std::error::Error>>;

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>>;

    /// 警告：Wayland 协议通常禁止强制移动鼠标
    fn warp_pointer_to_window(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait OutputOps: Send {
    fn screen_info(&self) -> ScreenInfo;
    fn enumerate_outputs(&self) -> Vec<OutputInfo>;
}

/// 事件源抽象
/// X11: 主动 Poll
/// Wayland: Smithay 驱动，这里可能只需要处理内部消息队列
pub trait EventSource: Send {
    fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>>;
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

pub trait WindowOps: Send {
    fn get_tree_child(&self, win: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>>;

    /// 设置服务端装饰 (Wayland 下可能需要绘制 SSD)
    fn set_decoration_style(
        &self,
        win: WindowId,
        border_width: u32,
        border_color: Pixel,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn change_event_mask(&self, win: WindowId, mask: u32)
    -> Result<(), Box<dyn std::error::Error>>;
    fn close_window(&self, win: WindowId) -> Result<CloseResult, Box<dyn std::error::Error>>;

    /// 将窗口显示在屏幕上 (X11 Map / Wayland Commit)
    fn map_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    /// 应用位置、大小、堆叠顺序变更
    /// Wayland 下，位置变更只影响 SSD 或 Popup，Toplevel 位置由 Compositor 渲染时决定
    fn apply_window_changes(
        &self,
        win: WindowId,
        changes: WindowChanges,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn set_input_focus_root(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn set_input_focus_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    fn send_client_message(
        &self,
        win: WindowId,
        type_atom: u32,
        data: [u32; 5],
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn flush(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn kill_client(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    // X11 特定，Wayland 实现为空即可
    fn grab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn ungrab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get_window_attributes(
        &self,
        win: WindowId,
    ) -> Result<WindowAttributes, Box<dyn std::error::Error>>;
    fn get_geometry_translated(
        &self,
        win: WindowId,
    ) -> Result<Geometry, Box<dyn std::error::Error>>;

    fn ungrab_all_buttons(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn grab_button(
        &self,
        win: WindowId,
        button: u8,
        event_mask_bits: u32,
        mods_bits: Mods,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn grab_button_any_anymod(
        &self,
        win: WindowId,
        event_mask_bits: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// 通知客户端尺寸变更 (Wayland configure event)
    fn send_configure_notify(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        border: u16,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NormalHints {
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
}

#[derive(Debug, Clone, Copy)]
pub struct WmHints {
    pub urgent: bool,
    pub input: Option<bool>,
}

pub trait PropertyOps: Send {
    fn get_title(&self, win: WindowId) -> String;
    fn get_class(&self, win: WindowId) -> (String, String); // (instance, class)
    fn get_window_types(&self, win: WindowId) -> Vec<WindowType>;

    fn is_fullscreen(&self, win: WindowId) -> bool;
    fn set_fullscreen_state(
        &self,
        win: WindowId,
        on: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn get_wm_hints(&self, win: WindowId) -> Option<WmHints>;
    fn set_urgent_hint(
        &self,
        win: WindowId,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn transient_for(&self, win: WindowId) -> Option<WindowId>;
    fn fetch_normal_hints(
        &self,
        win: WindowId,
    ) -> Result<Option<NormalHints>, Box<dyn std::error::Error>>;

    fn supports_delete_window(&self, win: WindowId) -> bool;
    // Wayland 下没有 send_delete_window 概念，通常是在 WindowOps::close_window 处理
    fn send_delete_window(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn set_window_strut_top(
        &self,
        win: WindowId,
        top: u32,
        start_x: u32,
        end_x: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn clear_window_strut(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    // EWMH client info, Wayland 无需实现
    fn set_client_info_props(
        &self,
        win: WindowId,
        tags: u32,
        monitor_num: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn get_wm_state(&self, win: WindowId) -> Result<i64, Box<dyn std::error::Error>>;
    fn set_wm_state(&self, win: WindowId, state: i64) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EwmhFeature {
    ActiveWindow,
    Supported,
    WmName,
    WmState,
    SupportingWmCheck,
    WmStateFullscreen,
    ClientList,
    ClientInfo,
    WmWindowType,
    WmWindowTypeDialog,
}

pub trait EwmhFacade: Send {
    fn set_active_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn clear_active_window(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn set_client_list(&self, list: &[WindowId]) -> Result<(), Box<dyn std::error::Error>>;
    fn set_client_list_stacking(&self, list: &[WindowId])
    -> Result<(), Box<dyn std::error::Error>>;
    fn setup_supporting_wm_check(
        &self,
        wm_name: &str,
    ) -> Result<WindowId, Box<dyn std::error::Error>>;
    fn set_supported_atoms(&self, supported: &[u32]) -> Result<(), Box<dyn std::error::Error>>;
    fn declare_supported(&self, features: &[EwmhFeature])
    -> Result<(), Box<dyn std::error::Error>>;
    fn reset_root_properties(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

pub trait ColorAllocator: Send {
    fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, Box<dyn std::error::Error>>;
    fn free_pixels(&mut self, pixels: &[Pixel]) -> Result<(), Box<dyn std::error::Error>>;
    fn set_scheme(&mut self, t: SchemeType, s: ColorScheme);
    fn get_scheme(&self, t: SchemeType) -> Option<ColorScheme>;
    fn ensure_pixel(&mut self, color: ArgbColor) -> Result<Pixel, Box<dyn std::error::Error>>;
    fn get_pixel_cached(&self, color: ArgbColor) -> Option<Pixel>;
    fn allocate_schemes_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn free_all_theme_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    // Helper methods with default impls
    fn get_border_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        self.get_scheme(t)
            .ok_or("scheme not found".into())
            .and_then(|s| self.ensure_pixel(s.border))
    }
    fn get_fg_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        self.get_scheme(t)
            .ok_or("scheme not found".into())
            .and_then(|s| self.ensure_pixel(s.fg))
    }
    fn get_bg_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        self.get_scheme(t)
            .ok_or("scheme not found".into())
            .and_then(|s| self.ensure_pixel(s.bg))
    }
}

pub trait CursorProvider: Send {
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>>;
    fn apply(
        &mut self,
        window_id: u64,
        kind: StdCursorKind,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait Backend: Send {
    fn capabilities(&self) -> Capabilities;
    fn window_ops(&self) -> &dyn WindowOps;
    fn input_ops(&self) -> &dyn InputOps;
    fn property_ops(&self) -> &dyn PropertyOps;
    fn output_ops(&self) -> &dyn OutputOps;
    fn key_ops(&self) -> &dyn KeyOps;
    fn key_ops_mut(&mut self) -> &mut dyn KeyOps;
    fn ewmh_facade(&self) -> Option<&dyn EwmhFacade>;
    fn cursor_provider(&mut self) -> &mut dyn CursorProvider;
    fn color_allocator(&mut self) -> &mut dyn ColorAllocator;
    fn event_source(&mut self) -> &mut dyn EventSource;
    fn root_window(&self) -> WindowId;
    fn as_any(&self) -> &dyn Any;
}
