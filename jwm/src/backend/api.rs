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
    pub has_active_window_prop: bool,
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

#[derive(Debug, Clone, Copy)]
pub enum PropertyKind {
    WmTransientFor,
    WmNormalHints,
    WmHints,
    WmName,
    NetWmName,
    NetWmWindowType,
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

#[derive(Debug, Clone)]
pub enum BackendEvent {
    EwmhState {
        window: WindowId,
        action: NetWmAction,
        states: [Option<NetWmState>; 2],
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
        request: u8,
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
    WmKeyboardShortcut {
        keysym: KeySym,
        mods: Mods,
    },
}

#[derive(Debug, Clone)]
pub struct WindowAttributes {
    pub override_redirect: bool,
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

pub trait OutputOps: Send {
    fn screen_info(&self) -> ScreenInfo;
    fn enumerate_outputs(&self) -> Vec<OutputInfo>;
}

pub trait EventSource: Send {
    fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>>;
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

pub trait WindowOps: Send {
    fn get_tree_child(&self, win: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>>;

    fn set_decoration_style(
        &self,
        win: WindowId,
        border_width: u32,
        border_color: Pixel,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn change_event_mask(&self, win: WindowId, mask: u32)
    -> Result<(), Box<dyn std::error::Error>>;

    fn close_window(&self, win: WindowId) -> Result<CloseResult, Box<dyn std::error::Error>>;

    fn map_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    fn apply_window_changes(
        &self,
        win: WindowId,
        changes: WindowChanges,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn set_input_focus_root(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    fn send_client_message(
        &self,
        win: WindowId,
        type_atom: u32,
        data: [u32; 5],
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn flush(&self) -> Result<(), Box<dyn std::error::Error>>;

    fn kill_client(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

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

    fn grab_button_any_anymod(
        &self,
        win: WindowId,
        event_mask_bits: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn grab_button(
        &self,
        win: WindowId,
        button: u8,
        event_mask_bits: u32,
        mods_bits: Mods,
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
    fn send_delete_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

    fn set_window_strut_top(
        &self,
        win: WindowId,
        top: u32,
        start_x: u32,
        end_x: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn clear_window_strut(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>>;

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
    fn as_any(&self) -> &dyn Any;
}
