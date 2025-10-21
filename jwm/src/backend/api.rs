// src/backend/api.rs
use std::fmt::Debug;

// 通用后端窗口ID（X11: Window; Wayland: 自定义句柄）
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowId(pub u64);

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

// 统一事件（只保留 JWM 需要的字段）
#[derive(Debug, Clone)]
pub enum BackendEvent {
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

// src/backend/api.rs

use crate::backend::common_input::{KeySym, Mods};

pub trait KeyOps: Send {
    // 探测 NumLock 掩码，返回 (通用 Mods 标记, X11/后端的掩码位 bits)
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
}

// 输入接口
pub trait InputOps: Send {
    fn grab_pointer(
        &self,
        mask: u32,
        cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>>;

    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>>;

    fn allow_events(&self, mode: u8, time: u32) -> Result<(), Box<dyn std::error::Error>>;

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>>;

    fn warp_pointer_to_window(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>>;

    // 重要：使用 trait object 作为回调，保证对象安全
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

// 属性接口
pub trait PropertyOps: Send {
    fn get_text_property_best_title(&self, win: WindowId) -> String;
    fn get_wm_class(&self, win: WindowId) -> Option<(String, String)>;
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
}

// EWMH 门面（Wayland 可 no-op）
pub trait EwmhFacade: Send {
    fn set_active_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;
    fn clear_active_window(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn set_client_list(&self, list: &[WindowId]) -> Result<(), Box<dyn std::error::Error>>;
    fn set_client_list_stacking(&self, list: &[WindowId])
        -> Result<(), Box<dyn std::error::Error>>;
}

// 后端总接口（聚合各子服务）
pub trait Backend: Send {
    fn capabilities(&self) -> Capabilities;
    fn window_ops(&self) -> &dyn WindowOps;
    fn input_ops(&self) -> &dyn InputOps;
    fn property_ops(&self) -> &dyn PropertyOps;
    fn output_ops(&self) -> &dyn OutputOps;
    fn ewmh(&self) -> Option<&dyn EwmhFacade>;

    fn cursor_provider(&mut self) -> &mut dyn crate::backend::traits::CursorProvider;
    fn color_allocator(&mut self) -> &mut dyn crate::backend::traits::ColorAllocator;

    fn event_source(&mut self) -> &mut dyn EventSource;
    fn root_window(&self) -> WindowId;
}
