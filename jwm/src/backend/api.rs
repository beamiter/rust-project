// src/backend/api.rs

use crate::backend::common_define::OutputId;
use crate::backend::common_define::{
    ColorScheme, CursorHandle, KeySym, Mods, Pixel, SchemeType, StdCursorKind, WindowId,
};
use std::any::Any;
use std::fmt::Debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    /// 命中某个 surface/window
    Surface(WindowId),
    /// 命中背景/空白区域（可携带 output 信息；X11 下可先为 None）
    Background { output: Option<OutputId> },
}

/// 屏幕/输出信息
#[derive(Clone, Debug)]
pub struct OutputInfo {
    pub id: OutputId,
    pub name: String,
    /// 全局坐标系中的 X 位置
    pub x: i32,
    /// 全局坐标系中的 Y 位置
    pub y: i32,
    /// 物理像素宽度
    pub width: i32,
    /// 物理像素高度
    pub height: i32,
    /// 缩放因子 (X11 通常为 1.0, Wayland HiDPI 可能为 1.5, 2.0 等)
    pub scale: f32,
    /// 刷新率 (mHz)
    pub refresh_rate: u32,
}

/// 简单的屏幕概览 (通常指整个桌面的边界)
#[derive(Clone, Copy, Debug)]
pub struct ScreenInfo {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    /// 后端是否支持强制移动鼠标指针 (Wayland 通常不支持)
    pub can_warp_pointer: bool,
    /// 后端是否支持查询所有客户端列表 (X11 支持，Wayland 不支持需自维护)
    pub supports_client_list: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetWmState {
    Fullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetWmAction {
    Add,
    Remove,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackMode {
    Above,
    Below,
    TopIf,
    BottomIf,
    Opposite,
}

/// 客户端发起的配置请求参数
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    Title,
    Class,
    TransientFor,
    SizeHints,
    Urgency,
    WindowType,
    Protocols,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyMode {
    Normal,
    Grab,
    Ungrab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseResult {
    /// 优雅关闭 (发送 WM_DELETE_WINDOW 或 xdg_toplevel.close)
    Graceful,
    /// 强制销毁 (Kill Client / Destroy Resource)
    Forced,
}

#[derive(Debug, Clone)]
pub struct WindowAttributes {
    /// 是否绕过窗口管理器 (Tooltip, Menu, DND 等)
    /// X11: override_redirect=true
    /// Wayland: 非 xdg_toplevel (如 popup) 或特殊的 layer shell surface
    pub override_redirect: bool,
    /// 窗口是否可见
    pub map_state_viewable: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub border: u32,
}

// --- 事件定义 ---

#[derive(Debug, Clone)]
pub enum BackendEvent {
    // === 硬件与输出 ===
    OutputAdded(OutputInfo),
    OutputRemoved(OutputId),
    OutputChanged(OutputInfo),
    ScreenLayoutChanged,
    ChildProcessExited,

    // === 窗口生命周期 ===
    WindowCreated(WindowId),
    WindowDestroyed(WindowId),
    WindowMapped(WindowId),
    WindowUnmapped(WindowId),
    WindowConfigured {
        window: WindowId,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },

    ButtonPress {
        target: HitTarget,
        state: u16,
        detail: u8,
        time: u32,
        root_x: f64,
        root_y: f64,
    },
    ButtonRelease {
        target: HitTarget,
        time: u32,
    },
    MotionNotify {
        target: HitTarget,
        root_x: f64,
        root_y: f64,
        time: u32,
    },
    KeyPress {
        keycode: u8,
        state: u16,
        time: u32,
    },

    // === 焦点与状态 ===
    EnterNotify {
        window: WindowId,
        subwindow: Option<WindowId>,
        mode: NotifyMode,
        root_x: f64,
        root_y: f64,
    },
    LeaveNotify {
        window: WindowId,
        mode: NotifyMode,
    },
    FocusIn {
        window: WindowId,
    },
    FocusOut {
        window: WindowId,
    },

    // === 客户端请求 (Policy) ===
    ConfigureRequest {
        window: WindowId,
        changes: WindowChanges,
        mask_bits: u16,
    },
    WindowStateRequest {
        window: WindowId,
        action: NetWmAction,
        state: NetWmState,
    },
    PropertyChanged {
        window: WindowId,
        kind: PropertyKind,
    },
    WmKeyboardShortcut {
        keysym: KeySym,
        mods: Mods,
    },
    Expose {
        window: WindowId,
    },
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
}

pub trait WindowOps: Send {
    fn set_position(&self, win: WindowId, x: i32, y: i32)
    -> Result<(), Box<dyn std::error::Error>>;
    fn configure(
        &self,
        win: WindowId,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        border: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn set_decoration_style(
        &self,
        win: WindowId,
        border_width: u32,
        border_color: Pixel,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn raise_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn map_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn unmap_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn close_window(&self, win: WindowId) -> Result<CloseResult, Box<dyn std::error::Error>>;
    fn set_input_focus(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn set_input_focus_root(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn get_window_attributes(
        &self,
        win: WindowId,
    ) -> Result<WindowAttributes, Box<dyn std::error::Error>>;
    fn get_geometry(&self, win: WindowId) -> Result<Geometry, Box<dyn std::error::Error>>;
    fn scan_windows(&self) -> Result<Vec<WindowId>, Box<dyn std::error::Error>>;

    fn flush(&self) -> Result<(), Box<dyn std::error::Error>>;

    fn kill_client(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    fn apply_window_changes(
        &self,
        win: WindowId,
        changes: WindowChanges,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn ungrab_all_buttons(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn grab_button_any_anymod(
        &self,
        _win: WindowId,
        _mask: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn grab_button(
        &self,
        _win: WindowId,
        _btn: u8,
        _mask: u32,
        _mods: Mods,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn change_event_mask(
        &self,
        _win: WindowId,
        _mask: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn get_tree_child(&self, _win: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }
}

/// 输入设备操作接口
pub trait InputOps: Send {
    /// 设置当前光标形状
    fn set_cursor(&self, kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>>;

    /// 获取当前指针绝对坐标
    fn get_pointer_position(&self) -> Result<(f64, f64), Box<dyn std::error::Error>>;

    /// 显式抓取指针 (用于 Interactive Move/Resize)
    /// X11: XGrabPointer
    /// Wayland: 开启内部抓取状态，将后续事件独占发送给 Handler
    fn grab_pointer(
        &self,
        mask: u32,
        cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>>;

    /// 释放指针抓取
    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// 强制移动指针 (X11 only, Wayland 返回 Ok 但不做任何事)
    fn warp_pointer(&self, _x: f64, _y: f64) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    // 兼容旧接口
    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>>;
    fn warp_pointer_to_window(
        &self,
        _win: WindowId,
        _x: i16,
        _y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn allow_events(
        &self,
        _mode: crate::backend::api::AllowMode,
        _time: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

/// 窗口属性读取接口
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

    fn transient_for(&self, win: WindowId) -> Option<WindowId>;

    // Hints
    fn get_wm_hints(&self, win: WindowId) -> Option<crate::backend::api::WmHints>;
    fn set_urgent_hint(
        &self,
        win: WindowId,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn fetch_normal_hints(
        &self,
        win: WindowId,
    ) -> Result<Option<crate::backend::api::NormalHints>, Box<dyn std::error::Error>>;

    // Legacy / EWMH Struts (Wayland 使用 Layer Shell)
    fn set_window_strut_top(
        &self,
        win: WindowId,
        top: u32,
        start_x: u32,
        end_x: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn clear_window_strut(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    // EWMH State (X11 Only)
    fn get_wm_state(&self, win: WindowId) -> Result<i64, Box<dyn std::error::Error>>;
    fn set_wm_state(&self, win: WindowId, state: i64) -> Result<(), Box<dyn std::error::Error>>;

    // Client Info (X11 Only)
    fn set_client_info_props(
        &self,
        win: WindowId,
        tags: u32,
        monitor_num: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct WmHints {
    pub urgent: bool,
    pub input: Option<bool>,
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

pub trait OutputOps: Send {
    /// 获取当前所有连接的输出设备
    fn enumerate_outputs(&self) -> Vec<OutputInfo>;
    /// 获取主屏幕信息 (兼容旧接口)
    fn screen_info(&self) -> ScreenInfo;

    fn output_at(&self, x: i32, y: i32) -> Option<OutputId>;
}

pub trait KeyOps: Send {
    // 注册全局快捷键
    fn grab_keys(
        &self,
        root: WindowId,
        bindings: &[(Mods, KeySym)],
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn clear_key_grabs(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    // 辅助转换
    fn clean_mods(&self, raw_state: u16) -> Mods;
    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, Box<dyn std::error::Error>>;
    fn clear_cache(&mut self);
}

// X11 EWMH 兼容层 (Wayland 下通常为空实现或通过 Xwayland 桥接)
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
    fn declare_supported(&self, features: &[EwmhFeature])
    -> Result<(), Box<dyn std::error::Error>>;
    fn reset_root_properties(&self) -> Result<(), Box<dyn std::error::Error>>;
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

pub trait ColorAllocator: Send {
    fn set_scheme(&mut self, t: SchemeType, s: ColorScheme);
    fn allocate_schemes_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn get_border_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>>;
    fn free_all_theme_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>>;
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

pub trait EventHandler {
    /// 处理具体的后端事件
    fn handle_event(
        &mut self,
        backend: &mut dyn Backend,
        event: BackendEvent,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// 每一轮循环的更新回调 (用于处理定时任务、动画帧等)
    fn update(&mut self, backend: &mut dyn Backend) -> Result<(), Box<dyn std::error::Error>>;

    /// 询问 Handler 是否应该退出主循环
    fn should_exit(&self) -> bool;
}

pub trait Backend: Send {
    fn capabilities(&self) -> Capabilities;
    fn root_window(&self) -> Option<WindowId>;
    fn as_any(&self) -> &dyn Any;
    fn check_existing_wm(&self) -> Result<(), Box<dyn std::error::Error>>;

    // Ops Getters
    fn window_ops(&self) -> &dyn WindowOps;
    fn input_ops(&self) -> &dyn InputOps;
    fn property_ops(&self) -> &dyn PropertyOps;
    fn output_ops(&self) -> &dyn OutputOps;
    fn key_ops(&self) -> &dyn KeyOps;
    fn key_ops_mut(&mut self) -> &mut dyn KeyOps;
    fn cursor_provider(&mut self) -> &mut dyn CursorProvider;
    fn color_allocator(&mut self) -> &mut dyn ColorAllocator;

    fn register_wm(&self, _name: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    // 通用清理接口
    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn on_focused_client_changed(
        &mut self,
        _win: Option<WindowId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn on_client_list_changed(
        &mut self,
        _clients: &[WindowId],
        _stack: &[WindowId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// 开始交互式移动窗口
    /// X11: 后端记录状态，自行抓取指针
    /// Wayland: 触发 xdg_toplevel_move
    fn begin_move(&mut self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// 开始交互式调整窗口大小
    fn begin_resize(
        &mut self,
        _win: WindowId,
        _edge: ResizeEdge,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    // 处理鼠标移动 (用于后端内部的交互逻辑)
    // 返回 true 表示后端已处理该事件，Jwm 不应继续处理
    fn handle_motion(
        &mut self,
        _x: f64,
        _y: f64,
        _time: u32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    // 处理鼠标释放 (结束交互)
    // 返回 true 表示后端已处理该事件
    fn handle_button_release(&mut self, _time: u32) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    fn run(&mut self, handler: &mut dyn EventHandler) -> Result<(), Box<dyn std::error::Error>>;

    fn request_render(&mut self) {}
}

// 兼容性定义
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
