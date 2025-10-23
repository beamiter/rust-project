// src/backend/api.rs
use crate::backend::common_define::{ArgbColor, ColorScheme, SchemeType};
pub use crate::backend::common_define::{
    CursorHandle, KeySym, Mods, Pixel, StdCursorKind, WindowId,
};
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
    pub has_active_window_prop: bool,
    pub supports_client_list: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum NetWmState {
    Fullscreen, /* 后续可扩充 */
}
#[derive(Debug, Clone, Copy)]
pub enum NetWmAction {
    Add,
    Remove,
    Toggle,
}

#[derive(Debug, Clone, Copy)]
pub enum PropertyKind {
    WmTransientFor,
    WmNormalHints,
    WmHints,
    WmName,
    NetWmName,
    NetWmWindowType,
    Other, // 后端无法识别
}

#[derive(Debug, Clone)]
pub enum BackendEvent {
    EwmhState {
        window: WindowId,
        action: NetWmAction,
        states: [Option<NetWmState>; 2], // 最多两个
    },
    ActiveWindowMessage {
        window: WindowId,
    },
    PropertyChanged {
        window: WindowId,
        kind: PropertyKind,
        deleted: bool,
    },
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
    MappingNotify {
        request: u8, // 与 X11 Mapping 枚举值一致
    },
    ClientMessage {
        window: WindowId,
        type_: u32,
        data: [u32; 5],
        format: u8,
    },
    ConfigureRequest {
        window: WindowId,
        mask: u16,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        border: u16,
        sibling: Option<WindowId>,
        stack_mode: u8,
    },
    ConfigureNotify {
        window: WindowId,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
    },
    DestroyNotify {
        window: WindowId,
    },
    EnterNotify {
        window: WindowId,
        event: WindowId,
        mode: u8,
        detail: u8,
    },
    Expose {
        window: WindowId,
        count: u16,
    },
    FocusIn {
        event: WindowId,
    },
    MapRequest {
        window: WindowId,
    },
    PropertyNotify {
        window: WindowId,
        atom: u32,
        state: u8,
    },
    UnmapNotify {
        window: WindowId,
        from_configure: bool,
    },
}

// 窗口属性结构
#[derive(Debug, Clone)]
pub struct WindowAttributes {
    pub override_redirect: bool,
    pub map_state_viewable: bool,
}

// 几何（已换算为 root 坐标）
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

pub trait KeyOps: Send {
    // 探测 NumLock 掩码，返回 (通用 Mods 标记, 后端掩码位 bits)
    fn detect_numlock_mask(&mut self) -> Result<(Mods, u16), Box<dyn std::error::Error>>;

    // 清理所有键抓取（针对 root）
    fn clear_key_grabs(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    // 抓取键绑定（通用形式：mods + keysym），numlock_mask_bits 为后端的掩码位
    fn grab_keys(
        &self,
        root: WindowId,
        bindings: &[(Mods, KeySym)],
        numlock_mask_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>>;

    // 依据 keycode 获取 keysym（用于处理 KeyPress）
    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, Box<dyn std::error::Error>>;

    // 清空内部键盘映射缓存（在 MappingNotify 时）
    fn clear_cache(&mut self);
}

pub trait InputOps: Send {
    fn grab_pointer(
        &self,
        mask: u32,
        cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>>;
    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>>;

    fn allow_events(&self, mode: AllowMode, time: u32) -> Result<(), Box<dyn std::error::Error>>;

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>>;
    fn warp_pointer_to_window(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn drag_loop(
        &self,
        cursor: Option<u64>,
        warp_to: Option<(i16, i16)>,
        target: WindowId,
        on_motion: &mut dyn FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

// 输出（屏幕/显示器）接口
pub trait OutputOps: Send {
    fn screen_info(&self) -> ScreenInfo;
    fn enumerate_outputs(&self) -> Vec<OutputInfo>;
}

// 事件源（供 JWM 主循环消费）
pub trait EventSource: Send {
    fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>>;
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

// 窗口接口
pub trait WindowOps: Send {
    fn get_tree_child(&self, win: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>>;

    fn set_border_width(
        &self,
        win: WindowId,
        border: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn set_border_pixel(&self, win: WindowId, pixel: u32)
        -> Result<(), Box<dyn std::error::Error>>;

    fn change_event_mask(&self, win: WindowId, mask: u32)
        -> Result<(), Box<dyn std::error::Error>>;

    fn map_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    fn configure_xywh_border(
        &self,
        win: WindowId,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        border: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn configure_stack_above(
        &self,
        win: WindowId,
        sibling: Option<WindowId>,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn set_input_focus_root(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    fn send_client_message(
        &self,
        win: WindowId,
        type_atom: u32,
        data: [u32; 5],
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn delete_property(&self, win: WindowId, atom: u32) -> Result<(), Box<dyn std::error::Error>>;

    fn change_property32(
        &self,
        win: WindowId,
        property: u32,
        ty: u32,
        data: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>>;

    // 新增：设置 8-bit STRING 属性
    fn change_property8(
        &self,
        win: WindowId,
        property: u32,
        ty: u32,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn flush(&self) -> Result<(), Box<dyn std::error::Error>>;

    fn kill_client(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    fn grab_server(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn ungrab_server(&self) -> Result<(), Box<dyn std::error::Error>>;

    fn get_window_attributes(
        &self,
        win: WindowId,
    ) -> Result<WindowAttributes, Box<dyn std::error::Error>>;

    fn get_geometry_translated(
        &self,
        win: WindowId,
    ) -> Result<Geometry, Box<dyn std::error::Error>>;

    // 便捷：取消所有按钮抓取（X11 需要）
    fn ungrab_all_buttons(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    // 便捷：抓取任何按钮 + 任意修饰（未聚焦时启用）
    fn grab_button_any_anymod(
        &self,
        win: WindowId,
        event_mask_bits: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;

    // 便捷：抓取具体按钮与修饰
    fn grab_button(
        &self,
        win: WindowId,
        button: u8, // MouseButton::to_u8() 映射
        event_mask_bits: u32,
        mods_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn send_configure_notify(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        border: u16,
    ) -> Result<(), Box<dyn std::error::Error>>;

    // 设置焦点到具体窗口（revert_to=POINTER_ROOT）
    fn set_input_focus_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
}

#[derive(Debug, Clone, Copy)]
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
    pub input: Option<bool>, // None 表示未提供 InputHint
}

// 属性接口
pub trait PropertyOps: Send {
    fn set_window_strut_top(
        &self,
        win: WindowId,
        top: u32,
        start_x: u32,
        end_x: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn clear_window_strut(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn get_text_property_best_title(&self, win: WindowId) -> String;
    fn get_wm_class(&self, win: WindowId) -> Option<(String, String)>;

    // 语义化：窗口类型/状态，隐藏 Atom
    fn is_popup_type(&self, win: WindowId) -> bool;
    fn is_fullscreen(&self, win: WindowId) -> Result<bool, Box<dyn std::error::Error>>;
    fn set_fullscreen_state(
        &self,
        win: WindowId,
        on: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;

    // 语义化：ICCCM WM_HINTS
    fn get_wm_hints(&self, win: WindowId) -> Option<WmHints>;
    fn set_urgent_hint(
        &self,
        win: WindowId,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;

    // 语义化：WM_TRANSIENT_FOR
    fn transient_for(&self, win: WindowId) -> Option<WindowId>;

    // 语义化：WM_NORMAL_HINTS（WmSizeHints）
    fn fetch_normal_hints(
        &self,
        win: WindowId,
    ) -> Result<Option<NormalHints>, Box<dyn std::error::Error>>;

    // 语义化：WM_DELETE_WINDOW 协议
    fn supports_delete_window(&self, win: WindowId) -> bool;
    fn send_delete_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    // 语义化：_NET_CLIENT_INFO 设置
    fn set_client_info(
        &self,
        win: WindowId,
        tags: u32,
        monitor_num: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn get_net_wm_state_atoms(&self, win: WindowId)
        -> Result<Vec<u32>, Box<dyn std::error::Error>>;
    fn has_net_wm_state(
        &self,
        win: WindowId,
        state_atom: u32,
    ) -> Result<bool, Box<dyn std::error::Error>>;
    fn get_window_types(&self, win: WindowId) -> Vec<u32>;

    // 新增：设置、添加、删除 _NET_WM_STATE
    fn set_net_wm_state_atoms(
        &self,
        win: WindowId,
        atoms: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn add_net_wm_state_atom(
        &self,
        win: WindowId,
        atom: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn remove_net_wm_state_atom(
        &self,
        win: WindowId,
        atom: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;

    // ICCCM WM_STATE 读写
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

// EWMH 门面（Wayland 可 no-op）
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
    // 可选：退出清理根属性
    fn reset_root_properties(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

// 通用颜色分配接口
pub trait ColorAllocator: Send {
    // 分配一个 RGB 颜色，返回通用 Pixel 句柄
    fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, Box<dyn std::error::Error>>;
    // 释放多个像素（可选）
    fn free_pixels(&mut self, pixels: &[Pixel]) -> Result<(), Box<dyn std::error::Error>>;

    // 主题 API（新增）
    fn set_scheme(&mut self, t: SchemeType, s: ColorScheme);
    fn get_scheme(&self, t: SchemeType) -> Option<ColorScheme>;

    // 获取像素：优先从缓存，不在缓存则分配/缓存后返回
    fn ensure_pixel(&mut self, color: ArgbColor) -> Result<Pixel, Box<dyn std::error::Error>>;

    // 只读查询缓存（不触发分配）
    fn get_pixel_cached(&self, color: ArgbColor) -> Option<Pixel>;

    // 为当前所有方案预分配像素（去重）
    fn allocate_schemes_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    // 释放所有主题缓存像素（实现方可按自身缓存释放）
    fn free_all_theme_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    // 便捷方法（可有默认实现）
    fn get_border_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        if let Some(s) = self.get_scheme(t) {
            self.ensure_pixel(s.border)
        } else {
            Err("scheme not found".into())
        }
    }
    fn get_fg_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        if let Some(s) = self.get_scheme(t) {
            self.ensure_pixel(s.fg)
        } else {
            Err("scheme not found".into())
        }
    }
    fn get_bg_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        if let Some(s) = self.get_scheme(t) {
            self.ensure_pixel(s.bg)
        } else {
            Err("scheme not found".into())
        }
    }
}

pub trait CursorProvider: Send {
    // 预创建常用光标
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    // 获取（或创建）某种标准光标
    fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>>;
    // 应用光标到窗口
    // 这里用通用 WindowId: u64 表示，X11=Window，Wayland=surface 或 seat/cursor surface 逻辑自行映射
    fn apply(
        &mut self,
        window_id: u64,
        kind: StdCursorKind,
    ) -> Result<(), Box<dyn std::error::Error>>;
    // 清理资源
    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}

// 后端总接口（聚合各子服务）
pub trait Backend: Send {
    fn capabilities(&self) -> Capabilities;
    fn window_ops(&self) -> &dyn WindowOps;
    fn input_ops(&self) -> &dyn InputOps;
    fn input_ops_handle(&self) -> std::sync::Arc<std::sync::Mutex<dyn InputOps + Send>>;
    fn property_ops(&self) -> &dyn PropertyOps;
    fn output_ops(&self) -> &dyn OutputOps;
    fn key_ops(&self) -> &dyn KeyOps;
    fn key_ops_mut(&mut self) -> &mut dyn KeyOps;
    fn ewmh_facade(&self) -> Option<&dyn EwmhFacade>;

    fn cursor_provider(&mut self) -> &mut dyn CursorProvider;
    fn color_allocator(&mut self) -> &mut dyn ColorAllocator;

    fn event_source(&mut self) -> &mut dyn EventSource;
    fn root_window(&self) -> WindowId;

    fn init_visual(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}
