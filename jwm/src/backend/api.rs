// src/backend/api.rs

use crate::backend::common_define::OutputId;
use crate::backend::common_define::{
    ColorScheme, CursorHandle, KeySym, Mods, Pixel, SchemeType, StdCursorKind, WindowId,
};
use crate::backend::error::BackendError;
use std::any::Any;
use std::fmt::Debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    Surface(WindowId),
    Background { output: Option<OutputId> },
}

#[derive(Clone, Debug)]
pub struct OutputInfo {
    pub id: OutputId,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f32,
    pub refresh_rate: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ScreenInfo {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    pub can_warp_pointer: bool,
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
    Strut,
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
    Graceful,
    Forced,
}

#[derive(Debug, Clone)]
pub struct WindowAttributes {
    pub override_redirect: bool,
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
    MoveResizeRequest {
        window: WindowId,
        direction: u32,
        button: u32,
    },
    MappingNotify,
    DamageNotify { drawable: WindowId },
}

pub trait WindowOps: Send {
    fn set_position(&self, win: WindowId, x: i32, y: i32) -> Result<(), BackendError>;
    fn configure(
        &self,
        win: WindowId,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        border: u32,
    ) -> Result<(), BackendError>;
    fn set_decoration_style(
        &self,
        win: WindowId,
        border_width: u32,
        border_color: Pixel,
    ) -> Result<(), BackendError>;
    fn raise_window(&self, win: WindowId) -> Result<(), BackendError>;
    fn map_window(&self, win: WindowId) -> Result<(), BackendError>;
    fn unmap_window(&self, win: WindowId) -> Result<(), BackendError>;
    fn close_window(&self, win: WindowId) -> Result<CloseResult, BackendError>;
    fn set_input_focus(&self, win: WindowId) -> Result<(), BackendError>;
    fn set_input_focus_root(&self) -> Result<(), BackendError>;
    fn get_window_attributes(&self, win: WindowId) -> Result<WindowAttributes, BackendError>;
    fn get_geometry(&self, win: WindowId) -> Result<Geometry, BackendError>;
    fn scan_windows(&self) -> Result<Vec<WindowId>, BackendError>;

    fn flush(&self) -> Result<(), BackendError>;

    fn kill_client(&self, win: WindowId) -> Result<(), BackendError>;

    fn apply_window_changes(
        &self,
        win: WindowId,
        changes: WindowChanges,
    ) -> Result<(), BackendError>;

    fn ungrab_all_buttons(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn grab_button_any_anymod(&self, _win: WindowId, _mask: u32) -> Result<(), BackendError> {
        Ok(())
    }
    fn grab_button(
        &self,
        _win: WindowId,
        _btn: u8,
        _mask: u32,
        _mods: Mods,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn change_event_mask(&self, _win: WindowId, _mask: u32) -> Result<(), BackendError> {
        Ok(())
    }
    fn get_tree_child(&self, _win: WindowId) -> Result<Vec<WindowId>, BackendError> {
        Ok(vec![])
    }
    /// Send WM_TAKE_FOCUS client message if the window supports it.
    /// Returns true if the message was sent.
    fn send_take_focus(&self, _win: WindowId) -> Result<bool, BackendError> {
        Ok(false)
    }

    /// Restack windows in order (first = bottom, last = top).
    /// Uses sibling stacking for fewer X11 round-trips.
    /// Default implementation falls back to sequential raise_window.
    fn restack_windows(&self, windows: &[WindowId]) -> Result<(), BackendError> {
        for &win in windows {
            self.raise_window(win)?;
        }
        Ok(())
    }
}

pub trait InputOps: Send {
    fn set_cursor(&self, kind: StdCursorKind) -> Result<(), BackendError>;

    fn get_pointer_position(&self) -> Result<(f64, f64), BackendError>;

    fn grab_pointer(&self, mask: u32, cursor: Option<u64>) -> Result<bool, BackendError>;

    fn ungrab_pointer(&self) -> Result<(), BackendError>;

    fn warp_pointer(&self, _x: f64, _y: f64) -> Result<(), BackendError> {
        Ok(())
    }

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), BackendError>;
    fn warp_pointer_to_window(&self, _win: WindowId, _x: i16, _y: i16) -> Result<(), BackendError> {
        Ok(())
    }
    fn allow_events(
        &self,
        _mode: crate::backend::api::AllowMode,
        _time: u32,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayerSurfaceInfo {
    /// wlr-layer-shell exclusive zone semantics.
    /// - `0`: does not reserve space
    /// - `-1`: reserve the full surface dimension along the anchored edge
    /// - `>0`: reserve that many logical pixels
    pub exclusive_zone: i32,
    pub anchor_top: bool,
    pub anchor_bottom: bool,
    pub anchor_left: bool,
    pub anchor_right: bool,
}

pub trait PropertyOps: Send {
    fn get_title(&self, win: WindowId) -> String;
    fn get_class(&self, win: WindowId) -> (String, String); // (instance, class)
    fn get_window_types(&self, win: WindowId) -> Vec<WindowType>;

    fn is_fullscreen(&self, win: WindowId) -> bool;
    fn set_fullscreen_state(&self, win: WindowId, on: bool) -> Result<(), BackendError>;

    fn transient_for(&self, win: WindowId) -> Option<WindowId>;

    // Hints
    fn get_wm_hints(&self, win: WindowId) -> Option<crate::backend::api::WmHints>;
    fn set_urgent_hint(&self, win: WindowId, urgent: bool) -> Result<(), BackendError>;
    fn fetch_normal_hints(
        &self,
        win: WindowId,
    ) -> Result<Option<crate::backend::api::NormalHints>, BackendError>;

    fn set_window_strut_top(
        &self,
        win: WindowId,
        top: u32,
        start_x: u32,
        end_x: u32,
    ) -> Result<(), BackendError>;
    fn clear_window_strut(&self, win: WindowId) -> Result<(), BackendError>;

    fn get_wm_state(&self, win: WindowId) -> Result<i64, BackendError>;
    fn set_wm_state(&self, win: WindowId, state: i64) -> Result<(), BackendError>;

    fn set_client_info_props(
        &self,
        win: WindowId,
        tags: u32,
        monitor_num: u32,
    ) -> Result<(), BackendError>;

    fn get_window_strut_partial(&self, _win: WindowId) -> Option<StrutPartial> {
        None
    }

    fn get_layer_surface_info(&self, _win: WindowId) -> Option<LayerSurfaceInfo> {
        None
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StrutPartial {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
    pub left_start_y: u32,
    pub left_end_y: u32,
    pub right_start_y: u32,
    pub right_end_y: u32,
    pub top_start_x: u32,
    pub top_end_x: u32,
    pub bottom_start_x: u32,
    pub bottom_end_x: u32,
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

    /// Invalidate cached output layout (no-op for backends that don't cache)
    fn invalidate_output_cache(&self) {}
}

pub trait KeyOps: Send {
    // 注册全局快捷键
    fn grab_keys(&self, root: WindowId, bindings: &[(Mods, KeySym)]) -> Result<(), BackendError>;
    fn clear_key_grabs(&self, root: WindowId) -> Result<(), BackendError>;

    // 辅助转换
    fn clean_mods(&self, raw_state: u16) -> Mods;
    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, BackendError>;
    fn clear_cache(&mut self);
}

pub trait EwmhFacade: Send {
    fn set_active_window(&self, win: WindowId) -> Result<(), BackendError>;
    fn clear_active_window(&self) -> Result<(), BackendError>;
    fn set_client_list(&self, list: &[WindowId]) -> Result<(), BackendError>;
    fn set_client_list_stacking(&self, list: &[WindowId]) -> Result<(), BackendError>;
    fn setup_supporting_wm_check(&self, wm_name: &str) -> Result<WindowId, BackendError>;
    fn declare_supported(&self, features: &[EwmhFeature]) -> Result<(), BackendError>;
    fn reset_root_properties(&self) -> Result<(), BackendError>;
    fn set_desktop_info(&self, current: u32, total: u32, names: &[&str]) -> Result<(), BackendError>;
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
    CurrentDesktop,
    NumberOfDesktops,
    DesktopNames,
    DesktopViewport,
    WmMoveResize,
}

pub trait ColorAllocator: Send {
    fn set_scheme(&mut self, t: SchemeType, s: ColorScheme);
    fn allocate_schemes_pixels(&mut self) -> Result<(), BackendError>;
    fn get_border_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, BackendError>;
    fn free_all_theme_pixels(&mut self) -> Result<(), BackendError>;
}

pub trait CursorProvider: Send {
    fn preload_common(&mut self) -> Result<(), BackendError>;
    fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, BackendError>;
    fn apply(&mut self, window_id: WindowId, kind: StdCursorKind) -> Result<(), BackendError>;
    fn cleanup(&mut self) -> Result<(), BackendError>;
}

pub trait EventHandler {
    fn handle_event(
        &mut self,
        backend: &mut dyn Backend,
        event: BackendEvent,
    ) -> Result<(), BackendError>;

    fn update(&mut self, backend: &mut dyn Backend) -> Result<(), BackendError>;

    fn should_exit(&self) -> bool;

    /// Returns true when the handler has active animations and needs
    /// the event loop to keep ticking (non-blocking dispatch).
    fn needs_tick(&self) -> bool {
        false
    }
}

pub trait Backend: Send {
    fn capabilities(&self) -> Capabilities;
    fn root_window(&self) -> Option<WindowId>;
    fn as_any(&self) -> &dyn Any;
    fn check_existing_wm(&self) -> Result<(), BackendError>;

    // Ops Getters
    fn window_ops(&self) -> &dyn WindowOps;
    fn input_ops(&self) -> &dyn InputOps;
    fn property_ops(&self) -> &dyn PropertyOps;
    fn output_ops(&self) -> &dyn OutputOps;
    fn key_ops(&self) -> &dyn KeyOps;
    fn key_ops_mut(&mut self) -> &mut dyn KeyOps;
    fn cursor_provider(&mut self) -> &mut dyn CursorProvider;
    fn color_allocator(&mut self) -> &mut dyn ColorAllocator;

    fn register_wm(&self, _name: &str) -> Result<(), BackendError> {
        Ok(())
    }

    // 通用清理接口
    fn cleanup(&mut self) -> Result<(), BackendError> {
        Ok(())
    }

    fn on_focused_client_changed(&mut self, _win: Option<WindowId>) -> Result<(), BackendError> {
        Ok(())
    }
    fn on_client_list_changed(
        &mut self,
        _clients: &[WindowId],
        _stack: &[WindowId],
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn on_desktop_changed(
        &mut self,
        _current: u32,
        _total: u32,
        _names: &[&str],
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn begin_move(&mut self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }

    fn begin_resize(&mut self, _win: WindowId, _edge: ResizeEdge) -> Result<(), BackendError> {
        Ok(())
    }

    fn handle_motion(&mut self, _x: f64, _y: f64, _time: u32) -> Result<bool, BackendError> {
        Ok(false)
    }

    fn handle_button_release(&mut self, _time: u32) -> Result<bool, BackendError> {
        Ok(false)
    }

    fn run(&mut self, handler: &mut dyn EventHandler) -> Result<(), BackendError>;

    fn request_render(&mut self) {}

    fn has_compositor(&self) -> bool {
        false
    }

    fn compositor_render_frame(
        &mut self,
        _scene: &[(u64, i32, i32, u32, u32)],
    ) -> Result<bool, BackendError> {
        Ok(false)
    }

    /// Request a compositor-level screenshot.
    ///
    /// On backends that own the framebuffer (udev/KMS) this captures the
    /// rendered output directly and saves it as a PNG file.  Other backends
    /// return `Ok(false)` to signal that the caller should fall back to an
    /// external tool.
    fn take_screenshot_to_file(&mut self, _path: &std::path::Path) -> Result<bool, BackendError> {
        Ok(false)
    }
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
