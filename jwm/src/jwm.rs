use libc::{SIG_DFL, SIGCHLD, setsid, sigaction, sigemptyset};

use log::info;
use log::warn;
use log::{debug, error};

use nix::sys::signal::{self, Signal};
use nix::sys::wait::WaitPidFlag;
use nix::sys::wait::WaitStatus;
use nix::sys::wait::waitpid;
use nix::unistd::Pid;

use crate::backend::api::EventHandler;
use crate::backend::api::HitTarget;
use crate::backend::api::ResizeEdge;
use crate::backend::common_define::OutputId;
use crate::backend::common_define::WindowId;
use crate::backend::error::BackendError;
use crate::core::controller::WMController;
use crate::core::models::MonitorGeometry;
use crate::core::state::WMState;
use slotmap::SecondaryMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::process::Stdio;
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::usize;

use crate::backend::api::AllowMode;
use crate::backend::api::Backend;
use crate::backend::api::BackendEvent;
use crate::backend::api::Geometry;
use crate::backend::api::NetWmAction;
use crate::backend::api::NetWmState;
use crate::backend::api::PropertyKind;
use crate::backend::api::StrutPartial;
use crate::backend::api::StackMode;
use crate::backend::api::WindowChanges;
use crate::backend::api::WindowType;
use crate::backend::common_define::ArgbColor;
use crate::backend::common_define::ColorScheme;
use crate::backend::common_define::ConfigWindowBits;
use crate::backend::common_define::EventMaskBits;
use crate::backend::common_define::SchemeType;
use crate::backend::common_define::{KeySym, Mods, MouseButton, StdCursorKind};
use crate::config::CONFIG;
use crate::core::layout::LayoutEnum;
use crate::ipc::{self, IpcEvent, IpcResponse, MonitorInfoIpc, TreeNode, WindowInfo, WorkspaceInfo};
use crate::ipc_server::{IpcServer, IncomingIpc};
use crate::core::models::{ClientKey, MonitorKey, Pertag, SizeHints, WMClient, WMMonitor};

use crate::core::animation::{AnimationKind, AnimationManager};
use crate::core::layout::{self, LayoutClient, LayoutParams, LayoutResult};
use crate::core::types::Rect;
use shared_structures::CommandType;
use shared_structures::SharedCommand;
use shared_structures::{MonitorInfo, SharedMessage, SharedRingBuffer, TagStatus};

// definitions for initial window state.
pub const WITHDRAWN_STATE: u8 = 0;
pub const STEXT_MAX_LEN: usize = 512;
pub const NORMAL_STATE: u8 = 1;
pub const ICONIC_STATE: u8 = 2;
pub const SHARED_PATH: &str = "/dev/shm/jwm_bar_global";
lazy_static::lazy_static! {
    pub static ref BUTTONMASK: EventMaskBits  = EventMaskBits::BUTTON_PRESS | EventMaskBits::BUTTON_RELEASE;
    pub static ref MOUSEMASK: EventMaskBits   = EventMaskBits::BUTTON_PRESS | EventMaskBits::BUTTON_RELEASE | EventMaskBits::POINTER_MOTION;
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

pub type WMFuncType =
    fn(&mut Jwm, &mut dyn Backend, &WMArgEnum) -> Result<(), Box<dyn std::error::Error>>;
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

#[derive(Debug, Clone, Copy)]
pub enum InteractionAction {
    Move,
    Resize(ResizeEdge),
}

#[derive(Debug, Clone)]
pub struct InteractionState {
    pub client_key: ClientKey,
    pub action: InteractionAction,
    pub start_win_geom: Geometry, // 记录开始时的窗口位置/大小
    pub start_mouse_x: i32,       // 记录开始时的鼠标位置
    pub start_mouse_y: i32,
    pub last_update_time: std::time::Instant, // 用于限流
}

pub struct Jwm {
    // 纯状态数据
    pub state: WMState,

    pub s_w: i32,
    pub s_h: i32,
    pub running: AtomicBool,
    pub is_restarting: AtomicBool,
    pub last_mouse_root: (f64, f64),

    pub message: SharedMessage,

    pub status_bar_shmem: Option<SharedRingBuffer>,
    pub status_bar_child: Option<Child>,
    pub status_bar_client: Option<ClientKey>,
    pub status_bar_window: Option<WindowId>,
    pub current_bar_monitor_id: Option<i32>,

    pub status_bar_last_spawn: Option<std::time::Instant>,
    pub status_bar_backoff_until: Option<std::time::Instant>,
    pub status_bar_restart_failures: u32,

    pub last_key_grab_refresh_at: Option<std::time::Instant>,

    pub pending_bar_updates: HashSet<MonitorIndex>,

    pub suppress_mouse_focus_until: Option<std::time::Instant>,

    pub last_stacking: SecondaryMap<MonitorKey, Vec<WindowId>>,

    pub scratchpads: HashMap<String, ClientKey>,
    pub scratchpad_pending_name: Option<String>,

    pub animations: AnimationManager,

    key_bindings: Vec<WMKey>,

    /// Strut reservations from external panels (polybar, trayer, etc.)
    external_struts: HashMap<WindowId, StrutPartial>,

    // IPC
    pub ipc_server: Option<IpcServer>,

    // Config hot-reload
    pub config_last_modified: Option<std::time::SystemTime>,
    pub config_reload_debounce: Option<std::time::Instant>,
}

// =================================================================================
// 1. 实现 WMController
// =================================================================================
impl WMController for Jwm {
    // === 硬件与输出 ===
    fn on_output_added(
        &mut self,
        backend: &mut dyn Backend,
        info: crate::backend::api::OutputInfo,
    ) {
        if let Err(e) = self.handle_output_added(backend, info) {
            error!("Error handling OutputAdded: {:?}", e);
        }
    }

    fn on_output_removed(&mut self, backend: &mut dyn Backend, id: OutputId) {
        if let Err(e) = self.handle_output_removed(backend, id) {
            error!("Error handling OutputRemoved: {:?}", e);
        }
    }

    fn on_output_changed(
        &mut self,
        backend: &mut dyn Backend,
        info: crate::backend::api::OutputInfo,
    ) {
        if let Err(e) = self.handle_output_changed(backend, info) {
            error!("Error handling OutputChanged: {:?}", e);
        }
    }

    fn on_screen_layout_changed(&mut self, backend: &mut dyn Backend) {
        info!("[WMController] Screen Layout Changed (Hotplug detected), refreshing geometry...");
        if self.updategeom(backend) {
            // Re-apply external strut reservations after geometry reset
            if !self.external_struts.is_empty() {
                self.apply_strut_reservations();
            }
            if let Err(e) = self.handle_screen_geometry_change(backend) {
                error!("Error handling ScreenLayoutChanged: {:?}", e);
            }
        }
    }

    fn on_child_process_exited(&mut self, _backend: &mut dyn Backend) {
        debug!("Received SIGCHLD, reaping zombies...");
        self.reap_zombies();
    }

    // === 窗口生命周期 ===
    fn on_map_request(&mut self, backend: &mut dyn Backend, win: WindowId) {
        if let Err(e) = self.maprequest(backend, win) {
            error!("Error handling MapRequest for {:?}: {:?}", win, e);
        }
    }

    fn on_unmap_notify(&mut self, backend: &mut dyn Backend, win: WindowId, from_configure: bool) {
        if let Err(e) = self.unmapnotify(backend, win, from_configure) {
            error!("Error handling UnmapNotify for {:?}: {:?}", win, e);
        }
    }

    fn on_destroy_notify(&mut self, backend: &mut dyn Backend, win: WindowId) {
        if let Err(e) = self.destroynotify(backend, win) {
            error!("Error handling DestroyNotify for {:?}: {:?}", win, e);
        }
    }

    fn on_window_configured(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) {
        if let Err(e) = self.configurenotify(backend, win, x, y, width, height) {
            error!("Error handling ConfigureNotify: {:?}", e);
        }
    }

    fn on_mapping_notify(&mut self, backend: &mut dyn Backend) {
        backend.key_ops_mut().clear_cache();
        if let Err(e) = self.grabkeys(backend) {
            error!("Error refreshing keys on MappingNotify: {:?}", e);
        }
    }

    // === 输入事件 ===
    fn on_key_press(&mut self, backend: &mut dyn Backend, keycode: u8, mods: u16, _time: u32) {
        let debug_keys = std::env::var("JWM_DEBUG_KEYS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        if debug_keys {
            let keysym = backend
                .key_ops_mut()
                .keysym_from_keycode(keycode)
                .unwrap_or(0);
            let mods_clean = backend.key_ops().clean_mods(mods);
            info!(
                "[key] keycode={} keysym=0x{:x} mods_raw=0x{:x} mods_clean=0x{:x}",
                keycode,
                keysym,
                mods,
                mods_clean.bits()
            );
        }
        if let Err(e) = self.on_key_press_internal(backend, keycode, mods) {
            error!("Error handling KeyPress: {:?}", e);
        }
    }

    fn on_button_press(
        &mut self,
        backend: &mut dyn Backend,
        target: crate::backend::api::HitTarget,
        state: u16,
        detail: u8,
        time: u32,
    ) {
        if let Err(e) = self.on_button_press_internal(backend, target, state, detail, time) {
            error!("Error handling ButtonPress: {:?}", e);
        }
    }

    fn on_button_release(&mut self, backend: &mut dyn Backend, _target: HitTarget, _time: u32) {
        match backend.handle_button_release(0) {
            Ok(handled) => {
                if handled {
                    // Sync floating window geometry after drag ends
                    self.sync_focused_floating_geometry(backend);

                    if let Err(e) = self.check_monitor_consistency(backend) {
                        error!(
                            "Error checking monitor consistency after button release: {:?}",
                            e
                        );
                    }
                }
            }
            Err(e) => error!("Error in backend handle_button_release: {:?}", e),
        }
    }

    fn on_motion_notify(
        &mut self,
        backend: &mut dyn Backend,
        target: HitTarget,
        root_x: f64,
        root_y: f64,
        time: u32,
    ) {
        let win_opt = match target {
            HitTarget::Surface(w) => Some(w),
            HitTarget::Background { .. } => None,
        };
        match backend.handle_motion(root_x, root_y, time) {
            Ok(true) => {
                self.last_mouse_root = (root_x, root_y);
                return;
            }
            Ok(false) => {}
            Err(e) => {
                error!("Error in backend handle_motion: {:?}", e);
                return;
            }
        }

        self.last_mouse_root = (root_x, root_y);
        if let Err(e) =
            self.on_motion_notify_internal(backend, win_opt, root_x as i16, root_y as i16, time)
        {
            error!("Error handling MotionNotify: {:?}", e);
        }
    }

    fn on_enter_notify(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        root_x: f64,
        root_y: f64,
        mode: crate::backend::api::NotifyMode,
    ) {
        if mode != crate::backend::api::NotifyMode::Normal {
            return;
        }
        self.last_mouse_root = (root_x, root_y);

        if let Err(e) = self.enter_notify(backend, win) {
            error!("Error handling EnterNotify: {:?}", e);
        }
    }

    fn on_leave_notify(&mut self, _backend: &mut dyn Backend, _win: WindowId) {
        // Jwm 目前对 LeaveNotify 没做特殊处理，预留接口
    }

    fn on_focus_in(&mut self, backend: &mut dyn Backend, win: WindowId) {
        if let Err(e) = self.focusin(backend, win) {
            error!("Error handling FocusIn: {:?}", e);
        }
    }

    fn on_focus_out(&mut self, _backend: &mut dyn Backend, _win: WindowId) {
        // Jwm 目前主要处理 FocusIn
    }

    fn on_expose(&mut self, backend: &mut dyn Backend, win: WindowId) {
        if let Err(e) = self.expose(backend, win, 0) {
            error!("Error handling Expose: {:?}", e);
        }
    }

    // === 客户端请求 / 协议 ===
    fn on_configure_request(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        mask_bits: u16,
        changes: WindowChanges,
    ) {
        if let Err(e) = self.on_configure_request_internal(backend, win, mask_bits, changes) {
            error!("Error handling ConfigureRequest: {:?}", e);
        }
    }

    fn on_property_changed(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        kind: PropertyKind,
    ) {
        // Handle external strut changes (polybar, trayer, etc.) — works for
        // both managed and unmanaged (override-redirect) windows.
        if kind == PropertyKind::Strut {
            // Skip our own bar window
            if Some(win) != self.status_bar_window {
                if let Some(strut) = backend.property_ops().get_window_strut_partial(win) {
                    if strut.left > 0 || strut.right > 0 || strut.top > 0 || strut.bottom > 0 {
                        let changed = self.external_struts.get(&win) != Some(&strut);
                        self.external_struts.insert(win, strut);
                        if changed {
                            info!("[strut] Updated external strut for {:?}: top={} bottom={} left={} right={}",
                                win, strut.top, strut.bottom, strut.left, strut.right);
                            self.apply_strut_reservations();
                            self.arrange(backend, None);
                        }
                    } else {
                        // All edges zero — remove
                        if self.external_struts.remove(&win).is_some() {
                            info!("[strut] Removed external strut for {:?}", win);
                            self.apply_strut_reservations();
                            self.arrange(backend, None);
                        }
                    }
                } else if self.external_struts.remove(&win).is_some() {
                    info!("[strut] Property deleted for {:?}", win);
                    self.apply_strut_reservations();
                    self.arrange(backend, None);
                }
            }
        }

        if let Some(client_key) = self.wintoclient(win) {
            let res = match kind {
                PropertyKind::TransientFor => self.handle_transient_for_change(backend, client_key),
                PropertyKind::SizeHints => self.handle_normal_hints_change(client_key),
                PropertyKind::Urgency => self.handle_wm_hints_change(backend, client_key),
                PropertyKind::Title => self.handle_title_change(backend, client_key),
                PropertyKind::Class => self.handle_class_change(backend, client_key),
                PropertyKind::WindowType => self.handle_window_type_change(backend, client_key),
                _ => Ok(()),
            };
            if let Err(e) = res {
                error!("Error handling PropertyChanged {:?}: {:?}", kind, e);
            }

            // Wayland clients (including gtk_bar) may only become identifiable after they
            // set title/app_id. Promote to status bar as soon as it matches.
            if self.status_bar_client.is_none() {
                let is_bar = self
                    .state
                    .clients
                    .get(client_key)
                    .map(|c| c.is_status_bar(CONFIG.load().status_bar_name()))
                    .unwrap_or(false);

                if is_bar {
                    info!("Detected status bar via property update, promoting client");
                    self.status_bar_client = Some(client_key);
                    self.status_bar_window = Some(win);

                    let current_mon_id = self.get_sel_mon().map(|m| m.num).unwrap_or(0);
                    self.current_bar_monitor_id = Some(current_mon_id);

                    if let Err(e) = self.manage_statusbar(backend, client_key, win, current_mon_id)
                    {
                        error!("Error promoting status bar: {e}");
                    }
                }
            }
        }
    }

    fn on_client_message(&mut self, backend: &mut dyn Backend, win: WindowId) {
        // 对应 ActiveWindowMessage
        if let Some(ck) = self.wintoclient(win) {
            let is_urgent = self
                .state
                .clients
                .get(ck)
                .map(|c| c.state.is_urgent)
                .unwrap_or(false);
            if !self.is_client_selected(ck) && !is_urgent {
                if let Err(e) = self.seturgent(backend, ck, true) {
                    error!("Error setting urgent on client message: {:?}", e);
                }
            }
        }
    }

    fn on_window_state_request(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        action: NetWmAction,
        state: NetWmState,
    ) {
        if matches!(state, NetWmState::Fullscreen) {
            if let Some(ck) = self.wintoclient(win) {
                let is_fullscreen = self
                    .state
                    .clients
                    .get(ck)
                    .map(|c| c.state.is_fullscreen)
                    .unwrap_or(false);
                let fullscreen = match action {
                    NetWmAction::Add => true,
                    NetWmAction::Remove => false,
                    NetWmAction::Toggle => !is_fullscreen,
                };
                if let Err(e) = self.setfullscreen(backend, ck, fullscreen) {
                    error!("Error handling WindowStateRequest: {:?}", e);
                }
            }
        }
    }

    fn on_wm_keyboard_shortcut(&mut self, backend: &mut dyn Backend, keysym: KeySym, mods: Mods) {
        for key_config in self.key_bindings.to_vec().iter() {
            if keysym == key_config.key_sym && mods == key_config.mask {
                if let Some(func) = key_config.func_opt {
                    if let Err(e) = func(self, backend, &key_config.arg) {
                        error!("Error executing keyboard shortcut: {:?}", e);
                    }
                }
                break;
            }
        }
    }
}

// =================================================================================
// _NET_WM_MOVERESIZE & Strut helpers
// =================================================================================
impl Jwm {
    fn on_moveresize_request(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        direction: u32,
    ) {
        const _NET_WM_MOVERESIZE_CANCEL: u32 = 11;
        const _NET_WM_MOVERESIZE_MOVE: u32 = 8;

        if direction == _NET_WM_MOVERESIZE_CANCEL {
            let _ = backend.handle_button_release(0);
            return;
        }

        let client_key = match self.wintoclient(win) {
            Some(ck) => ck,
            None => return,
        };

        if direction == _NET_WM_MOVERESIZE_MOVE {
            if let Err(e) = self.enable_floating_keep_geometry(backend, client_key) {
                error!("Error enabling floating for move-resize move: {:?}", e);
                return;
            }
            if let Err(e) = backend.begin_move(win) {
                error!("Error begin_move for _NET_WM_MOVERESIZE: {:?}", e);
            }
            return;
        }

        if direction <= 7 {
            let edge = match direction {
                0 => ResizeEdge::TopLeft,
                1 => ResizeEdge::Top,
                2 => ResizeEdge::TopRight,
                3 => ResizeEdge::Right,
                4 => ResizeEdge::BottomRight,
                5 => ResizeEdge::Bottom,
                6 => ResizeEdge::BottomLeft,
                7 => ResizeEdge::Left,
                _ => unreachable!(),
            };
            if let Err(e) = self.enable_floating_keep_geometry(backend, client_key) {
                error!("Error enabling floating for move-resize resize: {:?}", e);
                return;
            }
            if let Err(e) = backend.begin_resize(win, edge) {
                error!("Error begin_resize for _NET_WM_MOVERESIZE: {:?}", e);
            }
        }
        // direction 9 (SIZE_KEYBOARD) and 10 (MOVE_KEYBOARD) are ignored
    }

    /// Compute the maximum strut reservation for a given monitor from external panels.
    /// Returns (top, bottom, left, right) in pixels.
    fn get_strut_reserved(&self, mon_key: MonitorKey) -> (i32, i32, i32, i32) {
        let monitor = match self.state.monitors.get(mon_key) {
            Some(m) => m,
            None => return (0, 0, 0, 0),
        };
        let mx = monitor.geometry.m_x;
        let my = monitor.geometry.m_y;
        let mw = monitor.geometry.m_w;
        let mh = monitor.geometry.m_h;
        let mx_end = mx + mw;
        let my_end = my + mh;

        let mut top = 0i32;
        let mut bottom = 0i32;
        let mut left = 0i32;
        let mut right = 0i32;

        for strut in self.external_struts.values() {
            // Top edge: strut applies if the monitor's X range overlaps [top_start_x, top_end_x]
            if strut.top > 0 {
                let sx = strut.top_start_x as i32;
                let ex = strut.top_end_x as i32;
                // If start/end are both 0, the strut applies to all monitors
                if (sx == 0 && ex == 0) || (sx < mx_end && ex >= mx) {
                    top = top.max(strut.top as i32 - my);
                }
            }
            // Bottom edge
            if strut.bottom > 0 {
                let sx = strut.bottom_start_x as i32;
                let ex = strut.bottom_end_x as i32;
                if (sx == 0 && ex == 0) || (sx < mx_end && ex >= mx) {
                    bottom = bottom.max(strut.bottom as i32 - (my_end - mh).max(0));
                }
            }
            // Left edge
            if strut.left > 0 {
                let sy = strut.left_start_y as i32;
                let ey = strut.left_end_y as i32;
                if (sy == 0 && ey == 0) || (sy < my_end && ey >= my) {
                    left = left.max(strut.left as i32 - mx);
                }
            }
            // Right edge
            if strut.right > 0 {
                let sy = strut.right_start_y as i32;
                let ey = strut.right_end_y as i32;
                if (sy == 0 && ey == 0) || (sy < my_end && ey >= my) {
                    right = right.max(strut.right as i32 - (mx_end - mw).max(0));
                }
            }
        }

        (top.max(0), bottom.max(0), left.max(0), right.max(0))
    }

    /// Apply external strut reservations to all monitors' workarea geometry.
    fn apply_strut_reservations(&mut self) {
        let mon_keys: Vec<MonitorKey> = self.state.monitor_order.clone();
        for mon_key in mon_keys {
            let (strut_top, strut_bottom, strut_left, strut_right) =
                self.get_strut_reserved(mon_key);
            if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
                // Reset workarea to monitor area, then subtract struts
                monitor.geometry.w_x = monitor.geometry.m_x + strut_left;
                monitor.geometry.w_y = monitor.geometry.m_y + strut_top;
                monitor.geometry.w_w = monitor.geometry.m_w - strut_left - strut_right;
                monitor.geometry.w_h = monitor.geometry.m_h - strut_top - strut_bottom;
            }
        }
    }

    /// Check and read strut property on newly mapped windows.
    fn check_strut_on_manage(&mut self, backend: &mut dyn Backend, win: WindowId) {
        if Some(win) == self.status_bar_window {
            return;
        }
        if let Some(strut) = backend.property_ops().get_window_strut_partial(win) {
            if strut.left > 0 || strut.right > 0 || strut.top > 0 || strut.bottom > 0 {
                info!(
                    "[strut] New window {:?} has strut: top={} bottom={} left={} right={}",
                    win, strut.top, strut.bottom, strut.left, strut.right
                );
                self.external_struts.insert(win, strut);
                self.apply_strut_reservations();
                self.arrange(backend, None);
            }
        }
    }

    /// Remove strut for a window being unmanaged.
    fn remove_strut_on_unmanage(&mut self, backend: &mut dyn Backend, win: WindowId) {
        if self.external_struts.remove(&win).is_some() {
            info!("[strut] Removed strut on unmanage for {:?}", win);
            self.apply_strut_reservations();
            self.arrange(backend, None);
        }
    }
}

impl EventHandler for Jwm {
    fn handle_event(
        &mut self,
        backend: &mut dyn Backend,
        event: BackendEvent,
    ) -> Result<(), BackendError> {
        match event {
            // === 硬件与输出 ===
            BackendEvent::OutputAdded(info) => self.on_output_added(backend, info),
            BackendEvent::OutputRemoved(id) => self.on_output_removed(backend, id),
            BackendEvent::OutputChanged(info) => self.on_output_changed(backend, info),
            BackendEvent::ScreenLayoutChanged => self.on_screen_layout_changed(backend),
            BackendEvent::ChildProcessExited => self.on_child_process_exited(backend),

            // === 窗口生命周期 ===
            BackendEvent::WindowCreated(win) => self.on_map_request(backend, win),
            BackendEvent::WindowDestroyed(win) => self.on_destroy_notify(backend, win),
            BackendEvent::WindowMapped(win) => {
                // Some X11 notification daemons (e.g. dunst) use override_redirect windows.
                // Those bypass MapRequest, so they won't be managed/clamped via normal paths.
                // Clamp them to the monitor workarea here to avoid being covered by the status bar.
                self.maybe_clamp_override_redirect_notification(backend, win);
            }
            BackendEvent::WindowUnmapped(win) => self.on_unmap_notify(backend, win, false),
            BackendEvent::WindowConfigured {
                window,
                x,
                y,
                width,
                height,
            } => self.on_window_configured(backend, window, x, y, width, height),
            BackendEvent::MappingNotify => self.on_mapping_notify(backend),

            // === 输入事件 ===
            BackendEvent::ButtonPress {
                target,
                state,
                detail,
                time,
                ..
            } => self.on_button_press(backend, target, state, detail, time),
            BackendEvent::ButtonRelease { target, time } => {
                self.on_button_release(backend, target, time)
            }
            BackendEvent::MotionNotify {
                target,
                root_x,
                root_y,
                time,
            } => self.on_motion_notify(backend, target, root_x, root_y, time),
            BackendEvent::KeyPress {
                keycode,
                state,
                time,
            } => self.on_key_press(backend, keycode, state, time),
            BackendEvent::EnterNotify {
                window,
                subwindow: _,
                mode,
                root_x,
                root_y,
            } => self.on_enter_notify(backend, window, root_x, root_y, mode),
            BackendEvent::LeaveNotify { window, mode: _ } => self.on_leave_notify(backend, window),
            BackendEvent::FocusIn { window } => self.on_focus_in(backend, window),
            BackendEvent::FocusOut { window } => self.on_focus_out(backend, window),
            BackendEvent::Expose { window } => self.on_expose(backend, window),

            // === 协议与属性 ===
            BackendEvent::ConfigureRequest {
                window,
                mask_bits,
                changes,
            } => self.on_configure_request(backend, window, mask_bits, changes),
            BackendEvent::PropertyChanged { window, kind } => {
                self.on_property_changed(backend, window, kind)
            }
            BackendEvent::WmKeyboardShortcut { keysym, mods } => {
                self.on_wm_keyboard_shortcut(backend, keysym, mods)
            }
            BackendEvent::WindowStateRequest {
                window,
                action,
                state,
            } => self.on_window_state_request(backend, window, action, state),
            BackendEvent::ActiveWindowMessage { window } => self.on_client_message(backend, window),

            BackendEvent::MoveResizeRequest {
                window,
                direction,
                button: _,
            } => self.on_moveresize_request(backend, window, direction),

            // Compositor: damage events are handled at the backend level
            BackendEvent::DamageNotify { .. } => {}

            // 忽略或不需要显式处理的事件
            BackendEvent::ClientMessage { .. } => { /* ClientMessage Generic */ }
        }

        backend.request_render();
        Ok(())
    }

    fn update(&mut self, backend: &mut dyn Backend) -> Result<(), BackendError> {
        if self.status_bar_shmem.is_none() {
            let ring_buffer = SharedRingBuffer::create_aux(SHARED_PATH, None, None)
                .expect("Create bar shmem failed");
            info!("Create bar shmem");
            self.status_bar_shmem = Some(ring_buffer);
            return Ok(());
        }
        self.ensure_bar_is_running(SHARED_PATH);

        // Some status bar implementations may grab keys after starting.
        // Re-assert our grabs once per (re)spawn to keep WM shortcuts working.
        if let Some(spawned_at) = self.status_bar_last_spawn {
            let need_refresh = self
                .last_key_grab_refresh_at
                .map(|t| t < spawned_at)
                .unwrap_or(true);
            if need_refresh {
                if let Err(e) = self.grabkeys(backend) {
                    warn!("Failed to refresh key grabs after bar spawn: {e}");
                }
                self.last_key_grab_refresh_at = Some(spawned_at);
            }
        }

        self.process_commands_from_status_bar(backend);
        self.process_ipc(backend);
        self.check_config_reload(backend);
        self.flush_pending_bar_updates();
        self.tick_animations(backend);
        backend.window_ops().flush()?;
        Ok(())
    }

    fn should_exit(&self) -> bool {
        // 检查原子布尔值
        !self.running.load(Ordering::SeqCst)
    }

    fn needs_tick(&self) -> bool {
        self.animations.has_active()
    }
}

impl Jwm {
    fn enable_floating_keep_geometry(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(sel_mon_key) = self.state.sel_mon else {
            return Ok(());
        };

        if let Some(client) = self.state.clients.get_mut(client_key) {
            if !client.state.is_floating {
                client.state.is_floating = true;
                client.geometry.floating_x = client.geometry.x;
                client.geometry.floating_y = client.geometry.y;
                client.geometry.floating_w = client.geometry.w;
                client.geometry.floating_h = client.geometry.h;
            }
        }

        self.reorder_client_in_monitor_groups(client_key);

        self.arrange(backend, Some(sel_mon_key));
        Ok(())
    }
    fn debug_drag_enabled() -> bool {
        std::env::var("JWM_DEBUG_DRAG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true)
    }

    fn func_name(func: WMFuncType) -> &'static str {
        macro_rules! eq {
            ($f:path) => {
                std::ptr::fn_addr_eq(func, $f as WMFuncType)
            };
        }

        if eq!(Jwm::spawn) {
            "spawn"
        } else if eq!(Jwm::focusstack) {
            "focusstack"
        } else if eq!(Jwm::focusmon) {
            "focusmon"
        } else if eq!(Jwm::take_screenshot) {
            "take_screenshot"
        } else if eq!(Jwm::quit) {
            "quit"
        } else if eq!(Jwm::restart) {
            "restart"
        } else if eq!(Jwm::killclient) {
            "killclient"
        } else if eq!(Jwm::zoom) {
            "zoom"
        } else if eq!(Jwm::setlayout) {
            "setlayout"
        } else if eq!(Jwm::togglefloating) {
            "togglefloating"
        } else if eq!(Jwm::togglebar) {
            "togglebar"
        } else if eq!(Jwm::setmfact) {
            "setmfact"
        } else if eq!(Jwm::setcfact) {
            "setcfact"
        } else if eq!(Jwm::incnmaster) {
            "incnmaster"
        } else if eq!(Jwm::movestack) {
            "movestack"
        } else if eq!(Jwm::view) {
            "view"
        } else if eq!(Jwm::tag) {
            "tag"
        } else if eq!(Jwm::toggleview) {
            "toggleview"
        } else if eq!(Jwm::toggletag) {
            "toggletag"
        } else if eq!(Jwm::tagmon) {
            "tagmon"
        } else if eq!(Jwm::loopview) {
            "loopview"
        } else if eq!(Jwm::movemouse) {
            "movemouse"
        } else if eq!(Jwm::resizemouse) {
            "resizemouse"
        } else if eq!(Jwm::togglesticky) {
            "togglesticky"
        } else if eq!(Jwm::togglescratchpad) {
            "togglescratchpad"
        } else if eq!(Jwm::togglepip) {
            "togglepip"
        } else {
            "<unknown>"
        }
    }

    fn maybe_clamp_override_redirect_notification(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
    ) {
        let attr = match backend.window_ops().get_window_attributes(win) {
            Ok(a) => a,
            Err(_) => return,
        };
        if !attr.override_redirect {
            return;
        }

        // Avoid meddling with regular menus/tooltips unless we're confident.
        let types = backend.property_ops().get_window_types(win);
        let (inst, cls) = backend.property_ops().get_class(win);
        let title = backend.property_ops().get_title(win);

        let is_dunst = title == "Dunst"
            || inst.eq_ignore_ascii_case("dunst")
            || cls.eq_ignore_ascii_case("dunst")
            || inst.eq_ignore_ascii_case("dunstify")
            || cls.eq_ignore_ascii_case("dunstify");
        let is_notification = types.contains(&WindowType::Notification) || is_dunst;
        if !is_notification {
            return;
        }

        let geom = match backend.window_ops().get_geometry(win) {
            Ok(g) => g,
            Err(_) => return,
        };

        // Find the monitor by window center (fallback to selected monitor).
        let cx = geom.x.saturating_add((geom.w as i32) / 2);
        let cy = geom.y.saturating_add((geom.h as i32) / 2);
        let mon_key = self.recttomon(backend, cx, cy).or(self.state.sel_mon);
        let Some(mon_key) = mon_key else {
            return;
        };

        let work = match self.monitor_work_area(mon_key) {
            Some(r) => r,
            None => return,
        };

        let w = geom.w as i32;
        let h = geom.h as i32;
        let mut new_x = geom.x;
        let mut new_y = geom.y;

        // Clamp to workarea bounds.
        let min_x = work.x;
        let max_x = work.x + work.w - w;
        new_x = if min_x <= max_x {
            new_x.clamp(min_x, max_x)
        } else {
            min_x
        };

        let min_y = work.y;
        let max_y = work.y + work.h - h;
        new_y = if min_y <= max_y {
            new_y.clamp(min_y, max_y)
        } else {
            min_y
        };

        if new_x == geom.x && new_y == geom.y {
            return;
        }

        let changes = WindowChanges {
            x: Some(new_x),
            y: Some(new_y),
            ..Default::default()
        };
        if let Err(e) = backend.window_ops().apply_window_changes(win, changes) {
            debug!(
                "Failed to clamp override_redirect notification win={:?}: {:?}",
                win, e
            );
        }
    }

    pub fn new(backend: &mut dyn Backend) -> Result<Self, Box<dyn std::error::Error>> {
        info!("[new] Starting JWM initialization");
        Self::log_x11_environment();
        backend.cursor_provider().preload_common()?;
        let si = backend.output_ops().screen_info();
        let s_w = si.width;
        let s_h = si.height;
        info!(
            "[new] Screen info - resolution: {}x{}, root: {:?}",
            s_w,
            s_h,
            backend.root_window()
        );
        let alloc = backend.color_allocator();
        let colors = crate::config::CONFIG.load().colors().clone();
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
        backend.color_allocator().allocate_schemes_pixels()?;
        info!("[new] JWM initialization completed successfully");
        let outputs = backend.output_ops().enumerate_outputs();
        let mut jwm = Jwm {
            state: WMState::new(),

            s_w,
            s_h,
            running: AtomicBool::new(true),
            is_restarting: AtomicBool::new(false),

            message: SharedMessage::default(),

            status_bar_shmem: None,
            status_bar_child: None,
            status_bar_client: None,
            status_bar_window: None,
            current_bar_monitor_id: None,

            status_bar_last_spawn: None,
            status_bar_backoff_until: None,
            status_bar_restart_failures: 0,

            last_key_grab_refresh_at: None,
            pending_bar_updates: HashSet::new(),

            suppress_mouse_focus_until: None,

            last_stacking: SecondaryMap::new(),
            scratchpads: HashMap::new(),
            scratchpad_pending_name: None,
            animations: AnimationManager::new(),
            key_bindings: CONFIG.load().get_keys(),
            external_struts: HashMap::new(),
            last_mouse_root: (0.0, 0.0),

            ipc_server: match IpcServer::new() {
                Ok(s) => Some(s),
                Err(e) => {
                    warn!("[ipc] failed to start IPC server: {e}");
                    None
                }
            },
            config_last_modified: crate::config::Config::get_config_modified_time().ok(),
            config_reload_debounce: None,
        };
        if let Ok((x, y)) = backend.input_ops().get_pointer_position() {
            jwm.last_mouse_root = (x, y);
        }
        for out in outputs {
            jwm.add_monitor(out);
        }
        if !jwm.state.monitor_order.is_empty() {
            jwm.state.sel_mon = Some(jwm.state.monitor_order[0]);
        }
        Ok(jwm)
    }

    // --- 热插拔处理逻辑 ---
    fn add_monitor(&mut self, info: crate::backend::api::OutputInfo) {
        info!("[add_monitor] Adding output: {:?}", info);
        let mut m = self.createmon(CONFIG.load().show_bar());

        // 设置 Monitor 几何属性
        m.geometry.m_x = info.x;
        m.geometry.m_y = info.y;
        m.geometry.m_w = info.width;
        m.geometry.m_h = info.height;
        // 工作区通常等于屏幕区，减去 Bar 的计算在 layout 中动态进行
        m.geometry.w_x = info.x;
        m.geometry.w_y = info.y;
        m.geometry.w_w = info.width;
        m.geometry.w_h = info.height;
        m.num = self.state.monitors.len() as i32;

        let key = self.state.monitors.insert(m);
        self.state.monitor_order.push(key);
        self.state.output_map.insert(key, info.id);
        self.state.monitor_clients.insert(key, Vec::new());
        self.state.monitor_stack.insert(key, Vec::new());

        if self.state.sel_mon.is_none() {
            self.state.sel_mon = Some(key);
        }
    }

    fn handle_output_added(
        &mut self,
        backend: &mut dyn Backend,
        info: crate::backend::api::OutputInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.add_monitor(info);

        // Wayland clients can appear before outputs are fully initialized (early autostart).
        // Those clients end up with `mon=None`, meaning JWM will treat them as invisible:
        // - click-to-focus won't stick (focus() falls back to visible clients)
        // - arrange() won't resize them
        // The udev backend still renders them, so they look "stuck" at their initial size.
        self.attach_unassigned_clients_to_selected_monitor();

        self.arrange(backend, None);
        Ok(())
    }

    fn attach_unassigned_clients_to_selected_monitor(&mut self) {
        let target_mon_key = self
            .state
            .sel_mon
            .or_else(|| self.state.monitor_order.first().copied());

        let Some(mon_key) = target_mon_key else {
            return;
        };

        let target_tags = self
            .state
            .monitors
            .get(mon_key)
            .map(|m| m.tag_set[m.sel_tags])
            .unwrap_or(1);

        let unassigned: Vec<ClientKey> = self
            .state
            .clients
            .iter()
            .filter_map(|(k, c)| if c.mon.is_none() { Some(k) } else { None })
            .collect();

        for client_key in unassigned {
            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.mon = Some(mon_key);
                if client.state.tags == 0 {
                    client.state.tags = target_tags;
                }
            }

            // Ensure this client participates in layout/focus stacks.
            self.attach_to_monitor(client_key, mon_key);
        }
    }

    fn handle_output_removed(
        &mut self,
        backend: &mut dyn Backend,
        id: OutputId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[handle_output_removed] Removing output {:?}", id);

        // 查找对应的 MonitorKey
        let mon_key_opt = self
            .state
            .output_map
            .iter()
            .find(|&(_, &oid)| oid == id)
            .map(|(k, _)| k);

        if let Some(mon_key) = mon_key_opt {
            self.move_clients_to_first_monitor(mon_key);

            // 移除数据
            self.state.monitors.remove(mon_key);
            self.state.output_map.remove(mon_key);
            self.state.monitor_clients.remove(mon_key);
            self.state.monitor_stack.remove(mon_key);
            self.state.monitor_order.retain(|&k| k != mon_key);

            // 如果删除了当前选中的 Monitor，重置选中
            if self.state.sel_mon == Some(mon_key) {
                self.state.sel_mon = self.state.monitor_order.first().copied();
                self.focus(backend, None)?;
            }

            self.arrange(backend, None);
        }
        Ok(())
    }

    fn handle_output_changed(
        &mut self,
        backend: &mut dyn Backend,
        info: crate::backend::api::OutputInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mon_key_opt = self
            .state
            .output_map
            .iter()
            .find(|&(_, &oid)| oid == info.id)
            .map(|(k, _)| k);
        if let Some(mon_key) = mon_key_opt {
            if let Some(m) = self.state.monitors.get_mut(mon_key) {
                m.geometry.m_x = info.x;
                m.geometry.m_y = info.y;
                m.geometry.m_w = info.width;
                m.geometry.m_h = info.height;
                m.geometry.w_x = info.x;
                m.geometry.w_y = info.y;
                m.geometry.w_w = info.width;
                m.geometry.w_h = info.height;
            }
            self.arrange(backend, Some(mon_key));
        }
        Ok(())
    }

    pub fn setup_initial_windows(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 只有后端支持扫描才执行 (X11)
        if let Ok(windows) = backend.window_ops().scan_windows() {
            info!("[setup_initial_windows] Scanning {} windows", windows.len());
            for win in windows {
                let attr = backend.window_ops().get_window_attributes(win)?;
                if !attr.override_redirect && attr.map_state_viewable {
                    let geom = backend.window_ops().get_geometry(win)?;
                    self.manage(backend, win, &geom)?;
                }
            }
        }
        Ok(())
    }

    fn clean_mask(&self, backend: &mut dyn Backend, raw: u16) -> Mods {
        let mods_all = backend.key_ops().clean_mods(raw);

        mods_all
            & (Mods::SHIFT
                | Mods::CONTROL
                | Mods::ALT
                | Mods::SUPER
                | Mods::MOD2
                | Mods::MOD3
                | Mods::MOD5)
    }

    fn on_key_press_internal(
        &mut self,
        backend: &mut dyn Backend,
        keycode: u8,
        state_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let debug_keys = std::env::var("JWM_DEBUG_KEYS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let keysym = backend.key_ops_mut().keysym_from_keycode(keycode)?;
        let clean_state = self.clean_mask(backend, state_bits);

        let mut matched = false;
        for key_config in self.key_bindings.to_vec().iter() {
            let kc_mask = key_config.mask
                & (Mods::SHIFT
                    | Mods::CONTROL
                    | Mods::ALT
                    | Mods::SUPER
                    | Mods::MOD2
                    | Mods::MOD3
                    | Mods::MOD5);
            if keysym == key_config.key_sym && kc_mask == clean_state {
                matched = true;
                if debug_keys {
                    let func_name = key_config
                        .func_opt
                        .map(Self::func_name)
                        .unwrap_or("<none>");
                    info!(
                        "[key] matched keysym=0x{:x} mods=0x{:x} func={} arg={:?}",
                        keysym,
                        clean_state.bits(),
                        func_name,
                        key_config.arg
                    );
                }
                if let Some(func) = key_config.func_opt {
                    if let Err(e) = func(self, backend, &key_config.arg) {
                        error!("Error executing keyboard shortcut: {:?}", e);
                    }
                }
                break;
            }
        }

        if debug_keys && !matched {
            info!(
                "[key] no match keysym=0x{:x} mods=0x{:x}",
                keysym,
                clean_state.bits()
            );
        }
        Ok(())
    }

    fn on_button_press_internal(
        &mut self,
        backend: &mut dyn Backend,
        target: crate::backend::api::HitTarget,
        state_bits: u16,
        detail_btn: u8,
        time: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut click_type = WMClickType::ClickRootWin;
        let clicked_win: Option<crate::backend::common_define::WindowId> = match target {
            HitTarget::Surface(wid) => Some(wid),
            HitTarget::Background { .. } => None,
        };
        let target_mon_key = self.target_to_monitor(
            backend,
            target,
            (self.last_mouse_root.0 as i32, self.last_mouse_root.1 as i32),
        );
        if target_mon_key != self.state.sel_mon {
            if let Some(cur) = self.get_selected_client_key() {
                self.unfocus_client(backend, cur, true)?;
            }
            self.state.sel_mon = target_mon_key;
            self.focus(backend, None)?;
        }
        let mut is_client_click = false;
        let mut clicked_client_key: Option<ClientKey> = None;
        if let Some(wid) = clicked_win {
            if Some(wid) != backend.root_window() {
                if let Some(client_key) = self.wintoclient(wid) {
                    is_client_click = true;
                    clicked_client_key = Some(client_key);
                    self.focus(backend, Some(client_key))?;
                    // Invalidate stacking cache so restack always applies the
                    // new z-order when clicking a partially-obscured window.
                    if let Some(mon_key) = self.state.sel_mon {
                        self.last_stacking.remove(mon_key);
                    }
                    let _ = self.restack(backend, self.state.sel_mon);
                    click_type = WMClickType::ClickClientWin;
                }
            }
        }

        let event_mask = self.clean_mask(backend, state_bits);
        let mouse_button = MouseButton::from_u8(detail_btn);

        let mut handled_by_wm = false;
        for config in CONFIG.load().get_buttons().iter() {
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
                    if Self::debug_drag_enabled()
                        && event_mask.contains(Mods::CONTROL)
                        && mouse_button == MouseButton::Left
                        && is_client_click
                    {
                        let (px, py) = backend
                            .input_ops()
                            .get_pointer_position()
                            .unwrap_or((self.last_mouse_root.0, self.last_mouse_root.1));

                        let (win, geom) = clicked_client_key
                            .and_then(|ck| {
                                self.state
                                    .clients
                                    .get(ck)
                                    .map(|c| (c.win, c.geometry.clone()))
                            })
                            .map(|(w, g)| (Some(w), Some(g)))
                            .unwrap_or((clicked_win, None));

                        let func_name = Self::func_name(*func);
                        info!(
                            "[drag] Ctrl+Left ButtonPress: click_type={:?} win={:?} client={:?} func={} mods=0x{:x} pointer=({:.1},{:.1}) geom={:?}",
                            click_type,
                            win,
                            clicked_client_key,
                            func_name,
                            event_mask.bits(),
                            px,
                            py,
                            geom
                        );
                    }
                    let _ = func(self, backend, &config.arg);
                }
                break;
            }
        }

        if is_client_click {
            let _ = if handled_by_wm {
                backend
                    .input_ops()
                    .allow_events(AllowMode::AsyncPointer, time)
            } else {
                backend
                    .input_ops()
                    .allow_events(AllowMode::ReplayPointer, time)
            };
        }
        Ok(())
    }

    fn on_motion_notify_internal(
        &mut self,
        backend: &mut dyn Backend,
        window: Option<WindowId>,
        root_x: i16,
        root_y: i16,
        _time: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 1. 如果因为键盘操作等原因暂时阻塞了鼠标聚焦，直接返回
        if self.mouse_focus_blocked() {
            return Ok(());
        }
        // 2. 尝试聚焦客户端
        if let Some(win) = window {
            let is_already_focused = self
                .get_selected_client_key()
                .and_then(|key| self.state.clients.get(key))
                .map(|c| c.win == win)
                .unwrap_or(false);
            if !is_already_focused {
                if let Some(client_key) = self.wintoclient(win) {
                    if !self.is_client_selected(client_key) {
                        self.focus(backend, Some(client_key))?;
                    }
                }
            }
        }
        // 3. 更新当前鼠标所在的显示器状态
        let new_monitor_key = self.recttomon(backend, root_x as i32, root_y as i32);
        if new_monitor_key != self.state.motion_mon {
            self.handle_monitor_switch_by_key(backend, new_monitor_key)?;
        }
        self.state.motion_mon = new_monitor_key;

        Ok(())
    }

    fn on_configure_request_internal(
        &mut self,
        backend: &mut dyn Backend,
        window: WindowId,
        mask_bits: u16,
        changes: WindowChanges,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if Some(window) == self.status_bar_window {
            return self
                .handle_statusbar_configure_request_params(backend, window, mask_bits, changes);
        }

        if let Some(client_key) = self.wintoclient(window) {
            return self
                .handle_regular_configure_request_params(backend, client_key, mask_bits, changes);
        }

        self.handle_unmanaged_configure_request_params(backend, window, mask_bits, changes)
    }

    fn handle_statusbar_configure_request_params(
        &mut self,
        backend: &mut dyn Backend,
        window: WindowId,
        mask_bits: u16,
        req: WindowChanges,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.status_bar_client.is_none() {
            error!("[handle_statusbar_configure_request] StatusBar not found");
            return self.handle_unmanaged_configure_request_params(backend, window, mask_bits, req);
        }

        let mut changes = WindowChanges::default();
        let mask = ConfigWindowBits::from_bits_truncate(mask_bits);

        let bar_key = match self.status_bar_client {
            Some(k) => k,
            None => return Ok(()),
        };
        let statusbar_mut = match self.state.clients.get_mut(bar_key) {
            Some(c) => c,
            None => return Ok(()),
        };

        if mask.contains(ConfigWindowBits::X) {
            if let Some(x) = req.x {
                statusbar_mut.geometry.x = x;
                changes.x = Some(x);
            }
        }
        if mask.contains(ConfigWindowBits::Y) {
            if let Some(y) = req.y {
                statusbar_mut.geometry.y = y;
                changes.y = Some(y);
            }
        }
        if mask.contains(ConfigWindowBits::HEIGHT) {
            if let Some(h) = req.height {
                let new_h = (h as i32).max(CONFIG.load().status_bar_height());
                statusbar_mut.geometry.h = new_h;
                changes.height = Some(new_h as u32);
            }
        }

        changes.width = Some(statusbar_mut.geometry.w as u32);

        backend.window_ops().apply_window_changes(window, changes)?;

        let monitor_key = self.current_bar_monitor_id.and_then(|id| self.get_monitor_by_id(id));
        self.arrange(backend, monitor_key);
        if let Some(client_key) = self.wintoclient(window) {
            self.configure_client(backend, client_key)?;
        }
        Ok(())
    }

    fn handle_regular_configure_request_params(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        mask_bits: u16,
        req: WindowChanges,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let is_popup = self.is_popup_like(backend, client_key);
        let mask = ConfigWindowBits::from_bits_truncate(mask_bits);

        if mask.contains(ConfigWindowBits::BORDER_WIDTH) {
            if let Some(border) = req.border_width {
                if !is_popup {
                    if let Some(client) = self.state.clients.get_mut(client_key) {
                        client.geometry.border_w = border as i32;
                    }
                }
            }
        }

        let (is_floating, mon_key_opt) = if let Some(client) = self.state.clients.get(client_key) {
            (client.state.is_floating, client.mon)
        } else {
            return Err("Client not found".into());
        };

        if is_floating {
            let (mx, my, mw, mh) = if let Some(mon_key) = mon_key_opt {
                let monitor = self
                    .state
                    .monitors
                    .get(mon_key)
                    .ok_or("Monitor not found")?;
                (
                    monitor.geometry.m_x,
                    monitor.geometry.m_y,
                    monitor.geometry.m_w,
                    monitor.geometry.m_h,
                )
            } else {
                return Err("Client has no monitor assigned".into());
            };

            let mut popup_apply: Option<WindowId> = None;
            let mut popup_clamp_request: Option<(i32, i32, i32, i32)> = None;
            let mut popup_is_dialog = false;

            let mut clamp_request: Option<(i32, i32, i32, i32)> = None;

            if let Some(client) = self.state.clients.get_mut(client_key) {
                if mask.contains(ConfigWindowBits::X) {
                    if let Some(x) = req.x {
                        client.geometry.old_x = client.geometry.x;
                        client.geometry.x = mx + x;
                    }
                }
                if mask.contains(ConfigWindowBits::Y) {
                    if let Some(y) = req.y {
                        client.geometry.old_y = client.geometry.y;
                        client.geometry.y = my + y;
                    }
                }
                if mask.contains(ConfigWindowBits::WIDTH) {
                    if let Some(w) = req.width {
                        client.geometry.old_w = client.geometry.w;
                        client.geometry.w = w as i32;
                    }
                }
                if mask.contains(ConfigWindowBits::HEIGHT) {
                    if let Some(h) = req.height {
                        client.geometry.old_h = client.geometry.h;
                        client.geometry.h = h as i32;
                    }
                }

                if (client.geometry.x + client.geometry.w) > mx + mw && client.state.is_floating {
                    client.geometry.x = mx + (mw / 2 - client.total_width() / 2);
                }
                if (client.geometry.y + client.geometry.h) > my + mh && client.state.is_floating {
                    client.geometry.y = my + (mh / 2 - client.total_height() / 2);
                }

                // Defer workarea clamping until after we release the mutable borrow.
                if client.state.is_floating && !client.state.is_fullscreen {
                    clamp_request = Some((
                        client.geometry.x,
                        client.geometry.y,
                        client.total_width(),
                        client.total_height(),
                    ));
                }

                if is_popup {
                    let types = backend.property_ops().get_window_types(client.win);
                    let should_clamp = types.contains(&WindowType::Notification)
                        || types.contains(&WindowType::Dialog);
                    popup_is_dialog = types.contains(&WindowType::Dialog);

                    if should_clamp {
                        popup_clamp_request = Some((
                            client.geometry.x,
                            client.geometry.y,
                            client.total_width(),
                            client.total_height(),
                        ));
                    }
                    popup_apply = Some(client.win);
                }
            }

            // Popup-like windows: apply workarea clamp for Dialog/Notification, then commit.
            if let Some(win) = popup_apply {
                if let (Some(mon_key), Some((x, y, total_w, total_h))) =
                    (mon_key_opt, popup_clamp_request)
                {
                    let mut clamp = self
                        .monitor_work_area(mon_key)
                        .unwrap_or(Rect::new(mx, my, mw, mh));

                    // For transient dialogs, intersect with parent bounds to avoid jumping
                    // across tiled columns.
                    if popup_is_dialog {
                        if let Some(parent_key) = self.parent_client_of(backend, client_key) {
                            if let Some(parent) = self.state.clients.get(parent_key) {
                                let parent_rect = Rect::new(
                                    parent.geometry.x,
                                    parent.geometry.y,
                                    parent.total_width(),
                                    parent.total_height(),
                                );

                                let left = clamp.x.max(parent_rect.x);
                                let top = clamp.y.max(parent_rect.y);
                                let right = (clamp.x + clamp.w).min(parent_rect.x + parent_rect.w);
                                let bottom =
                                    (clamp.y + clamp.h).min(parent_rect.y + parent_rect.h);
                                let w = (right - left).max(0);
                                let h = (bottom - top).max(0);
                                if w > 0 && h > 0 {
                                    clamp = Rect::new(left, top, w, h);
                                }
                            }
                        }
                    }

                    let min_x = clamp.x;
                    let max_x = clamp.x + clamp.w - total_w;
                    let clamped_x = if min_x <= max_x {
                        x.clamp(min_x, max_x)
                    } else {
                        min_x
                    };

                    let min_y = clamp.y;
                    let max_y = clamp.y + clamp.h - total_h;
                    let clamped_y = if min_y <= max_y {
                        y.clamp(min_y, max_y)
                    } else {
                        min_y
                    };

                    if let Some(client) = self.state.clients.get_mut(client_key) {
                        client.geometry.x = clamped_x;
                        client.geometry.y = clamped_y;
                    }
                }

                if let Some(client) = self.state.clients.get(client_key) {
                    let changes = WindowChanges {
                        x: Some(client.geometry.x),
                        y: Some(client.geometry.y),
                        width: Some(client.geometry.w as u32),
                        height: Some(client.geometry.h as u32),
                        ..Default::default()
                    };
                    backend.window_ops().apply_window_changes(win, changes)?;
                }

                return Ok(());
            }

            // Clamp floating (non-fullscreen) windows to the monitor workarea so they don't end
            // up under dock/statusbar reserved space.
            if let (Some(mon_key), Some((x, y, total_w, total_h))) = (mon_key_opt, clamp_request)
            {
                let clamp = self
                    .monitor_work_area(mon_key)
                    .unwrap_or(Rect::new(mx, my, mw, mh));

                let min_x = clamp.x;
                let max_x = clamp.x + clamp.w - total_w;
                let clamped_x = if min_x <= max_x {
                    x.clamp(min_x, max_x)
                } else {
                    min_x
                };

                let min_y = clamp.y;
                let max_y = clamp.y + clamp.h - total_h;
                let clamped_y = if min_y <= max_y {
                    y.clamp(min_y, max_y)
                } else {
                    min_y
                };

                if let Some(client) = self.state.clients.get_mut(client_key) {
                    if client.state.is_floating && !client.state.is_fullscreen {
                        client.geometry.x = clamped_x;
                        client.geometry.y = clamped_y;
                    }
                }
            }

            if mask.contains(ConfigWindowBits::X | ConfigWindowBits::Y)
                && !mask.contains(ConfigWindowBits::WIDTH | ConfigWindowBits::HEIGHT)
            {
                self.configure_client(backend, client_key)?;
            }

            if self.is_client_visible_by_key(client_key) {
                if let Some(client) = self.state.clients.get(client_key) {
                    let changes = WindowChanges {
                        x: Some(client.geometry.x),
                        y: Some(client.geometry.y),
                        width: Some(client.geometry.w as u32),
                        height: Some(client.geometry.h as u32),
                        ..Default::default()
                    };
                    backend
                        .window_ops()
                        .apply_window_changes(client.win, changes)?;
                }
            }
        } else {
            self.configure_client(backend, client_key)?;
        }

        Ok(())
    }

    fn handle_unmanaged_configure_request_params(
        &mut self,
        backend: &mut dyn Backend,
        window: WindowId,
        mask_bits: u16,
        req: WindowChanges,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "[handle_unmanaged_configure_request] unmanaged window={:?}",
            window
        );

        let mask = ConfigWindowBits::from_bits_truncate(mask_bits);
        let mut changes = WindowChanges::default();

        if mask.contains(ConfigWindowBits::X) {
            changes.x = req.x;
        }
        if mask.contains(ConfigWindowBits::Y) {
            changes.y = req.y;
        }
        if mask.contains(ConfigWindowBits::WIDTH) {
            changes.width = req.width;
        }
        if mask.contains(ConfigWindowBits::HEIGHT) {
            changes.height = req.height;
        }
        if mask.contains(ConfigWindowBits::BORDER_WIDTH) {
            changes.border_width = req.border_width;
        }
        if mask.contains(ConfigWindowBits::SIBLING) {
            changes.sibling = req.sibling;
        }
        if mask.contains(ConfigWindowBits::STACK_MODE) {
            changes.stack_mode = req.stack_mode;
        }

        backend.window_ops().apply_window_changes(window, changes)?;
        Ok(())
    }

    fn target_to_monitor(
        &mut self,
        backend: &mut dyn Backend,
        target: crate::backend::api::HitTarget,
        fallback_pos: (i32, i32),
    ) -> Option<MonitorKey> {
        use crate::backend::api::HitTarget;

        match target {
            HitTarget::Background { output: Some(oid) } => {
                // 直接用 output_map 找 monitor
                for (mon_key, &mapped_oid) in &self.state.output_map {
                    if mapped_oid == oid {
                        return Some(mon_key);
                    }
                }
                self.state.sel_mon
            }
            HitTarget::Background { output: None } => {
                // fallback：用坐标查
                self.recttomon(backend, fallback_pos.0, fallback_pos.1)
            }
            HitTarget::Surface(win) => {
                // 还是按原逻辑：先看 client.mon，否则用 pointer 落点
                if let Some(ck) = self.wintoclient(win) {
                    if let Some(c) = self.state.clients.get(ck) {
                        return c.mon.or(self.state.sel_mon);
                    }
                }
                self.recttomon(backend, fallback_pos.0, fallback_pos.1)
            }
        }
    }

    fn insert_client(&mut self, client: WMClient) -> ClientKey {
        let win = client.win;
        let key = self.state.clients.insert(client);
        self.state.client_order.push(key);
        self.state.win_to_client.insert(win, key);
        key
    }

    fn insert_monitor(&mut self, monitor: WMMonitor) -> MonitorKey {
        let key = self.state.monitors.insert(monitor);
        self.state.monitor_order.push(key);
        self.state.monitor_clients.insert(key, Vec::new());
        self.state.monitor_stack.insert(key, Vec::new());
        key
    }

    fn is_client_selected(&self, client_key: ClientKey) -> bool {
        self.state
            .sel_mon
            .and_then(|sel_mon_key| self.state.monitors.get(sel_mon_key))
            .and_then(|monitor| monitor.sel)
            .map(|sel_client| sel_client == client_key)
            .unwrap_or(false)
    }

    fn get_monitor_clients(&self, mon_key: MonitorKey) -> &[ClientKey] {
        self.state
            .monitor_clients
            .get(mon_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn get_monitor_stack(&self, mon_key: MonitorKey) -> &[ClientKey] {
        self.state
            .monitor_stack
            .get(mon_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn get_sel_mon(&self) -> Option<&WMMonitor> {
        self.state
            .sel_mon
            .and_then(|sel_mon_key| self.state.monitors.get(sel_mon_key))
            .and_then(|monitor| Some(monitor))
    }

    fn get_selected_client_key(&self) -> Option<ClientKey> {
        self.state
            .sel_mon
            .and_then(|sel_mon_key| self.state.monitors.get(sel_mon_key))
            .and_then(|monitor| monitor.sel)
    }

    fn attach_front(&mut self, client_key: ClientKey) {
        if let Some(client) = self.state.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(client_list) = self.state.monitor_clients.get_mut(mon_key) {
                    client_list.insert(0, client_key);
                }
            }
        }
    }

    fn attach_back(&mut self, client_key: ClientKey) {
        if let Some(client) = self.state.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(client_list) = self.state.monitor_clients.get_mut(mon_key) {
                    client_list.push(client_key);
                }
            }
        }
        self.reorder_client_in_monitor_groups(client_key);
    }

    fn detach(&mut self, client_key: ClientKey) {
        if let Some(client) = self.state.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(client_list) = self.state.monitor_clients.get_mut(mon_key) {
                    if let Some(pos) = client_list.iter().position(|&k| k == client_key) {
                        client_list.remove(pos);
                    }
                }
            }
        }
    }

    fn reorder_client_in_monitor_groups(&mut self, client_key: ClientKey) {
        let (Some(mon_key), Some(is_floating)) = (
            self.state.clients.get(client_key).and_then(|c| c.mon),
            self.state.clients.get(client_key).map(|c| c.state.is_floating),
        ) else {
            return;
        };

        let Some(client_list) = self.state.monitor_clients.get_mut(mon_key) else {
            return;
        };

        if let Some(pos) = client_list.iter().position(|&k| k == client_key) {
            client_list.remove(pos);
        }

        if is_floating {
            client_list.push(client_key);
            return;
        }

        let mut insert_pos = client_list.len();
        for (idx, &key) in client_list.iter().enumerate() {
            let other_is_floating = self
                .state
                .clients
                .get(key)
                .map(|c| c.state.is_floating)
                .unwrap_or(false);
            if other_is_floating {
                insert_pos = idx;
                break;
            }
        }

        client_list.insert(insert_pos, client_key);
    }

    fn attachstack(&mut self, client_key: ClientKey) {
        if let Some(client) = self.state.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(stack_list) = self.state.monitor_stack.get_mut(mon_key) {
                    stack_list.insert(0, client_key);
                }
            }
        }
    }

    fn detach_from_monitor(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        if let Some(client_list) = self.state.monitor_clients.get_mut(mon_key) {
            client_list.retain(|&k| k != client_key);
        }
        if let Some(stack_list) = self.state.monitor_stack.get_mut(mon_key) {
            stack_list.retain(|&k| k != client_key);
        }
    }

    fn attach_to_monitor(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        if let Some(client_list) = self.state.monitor_clients.get_mut(mon_key) {
            client_list.push(client_key);
        }
        if let Some(stack_list) = self.state.monitor_stack.get_mut(mon_key) {
            stack_list.push(client_key);
        }
        self.reorder_client_in_monitor_groups(client_key);
    }

    fn detachstack(&mut self, client_key: ClientKey) {
        if let Some(client) = self.state.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(stack_list) = self.state.monitor_stack.get_mut(mon_key) {
                    if let Some(pos) = stack_list.iter().position(|&k| k == client_key) {
                        stack_list.remove(pos);
                    }
                }
                let next_visible_client = self.find_next_visible_client_by_mon(mon_key);
                if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
                    if monitor.sel == Some(client_key) {
                        monitor.sel = next_visible_client;
                    }
                }
            }
        }
    }

    fn find_next_visible_client_by_mon(&self, mon_key: MonitorKey) -> Option<ClientKey> {
        if let Some(stack_list) = self.state.monitor_stack.get(mon_key) {
            for &client_key in stack_list {
                if let Some(_) = self.state.clients.get(client_key) {
                    if self.is_client_visible_on_monitor(client_key, mon_key) {
                        return Some(client_key);
                    }
                }
            }
        }
        None
    }

    fn is_client_visible_on_monitor(&self, client_key: ClientKey, mon_key: MonitorKey) -> bool {
        if let (Some(client), Some(monitor)) = (
            self.state.clients.get(client_key),
            self.state.monitors.get(mon_key),
        ) {
            client.state.is_sticky || (client.state.tags & monitor.tag_set[monitor.sel_tags]) > 0
        } else {
            false
        }
    }

    fn is_client_visible_by_key(&self, client_key: ClientKey) -> bool {
        if let Some(client) = self.state.clients.get(client_key) {
            if let Some(mon_key) = client.mon {
                if let Some(monitor) = self.state.monitors.get(mon_key) {
                    return client.state.is_sticky || (client.state.tags & monitor.tag_set[monitor.sel_tags]) > 0;
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
            if let Some(client) = self.state.clients.get(client_key) {
                if !client.state.is_floating
                    && self.is_client_visible_on_monitor(client_key, mon_key)
                {
                    return Some(client_key);
                }
            }
        }
        None
    }

    fn pop(&mut self, backend: &mut dyn Backend, client_key: ClientKey) {
        let mon_key = if let Some(client) = self.state.clients.get(client_key) {
            client.mon
        } else {
            return;
        };

        self.detach(client_key);
        self.attach_front(client_key);

        let _ = self.focus(backend, Some(client_key));
        if let Some(mon_key) = mon_key {
            self.arrange(backend, Some(mon_key));
        }
    }

    fn wintoclient(&self, win: WindowId) -> Option<ClientKey> {
        if let Some(bar_win) = self.status_bar_window {
            if bar_win == win {
                return self.status_bar_client;
            }
        }
        self.state.win_to_client.get(&win).copied()
    }

    fn log_x11_environment() {
        info!("[X11 Environment Debug]");
        info!("DISPLAY: {:?}", env::var("DISPLAY"));
        info!("XAUTHORITY: {:?}", env::var("XAUTHORITY"));
        info!("XDG_SESSION_TYPE: {:?}", env::var("XDG_SESSION_TYPE"));
        info!("USER: {:?}", env::var("USER"));
        info!("HOME: {:?}", env::var("HOME"));

        if let Ok(display) = env::var("DISPLAY") {
            let socket_path = format!("/tmp/.X11-unix/X{}", display.trim_start_matches(":"));
            info!("X11 socket path: {}", socket_path);
            info!(
                "X11 socket exists: {}",
                std::path::Path::new(&socket_path).exists()
            );
        }

        let x_running = std::process::Command::new("pgrep")
            .arg("-f")
            .arg("X|Xorg")
            .output()
            .map(|output| !output.stdout.is_empty())
            .unwrap_or(false);
        info!("X server running: {}", x_running);
    }

    pub fn restart(
        &mut self,
        _backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[restart] Preparing seamless restart");
        self.running.store(false, Ordering::SeqCst);
        self.is_restarting.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn is_bar_visible_on_mon(&self, mon_key: MonitorKey) -> bool {
        if let Some(m) = self.state.monitors.get(mon_key) {
            if let Some(p) = m.pertag.as_ref() {
                if let Some(&show) = p.show_bars.get(p.cur_tag) {
                    return show;
                }
            }
        }
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
                for (key, m) in self.state.monitors.iter() {
                    if self.is_bar_visible_on_mon(key) {
                        self.pending_bar_updates.insert(m.num);
                    }
                }
            }
        }
    }

    fn get_wm_class(
        &self,
        backend: &mut dyn Backend,
        window: WindowId,
    ) -> Option<(String, String)> {
        let (inst, cls) = backend.property_ops().get_class(window);
        if inst.is_empty() && cls.is_empty() {
            None
        } else {
            Some((inst, cls))
        }
    }

    fn applysizehints(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        x: &mut i32,
        y: &mut i32,
        w: &mut i32,
        h: &mut i32,
        interact: bool,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        *w = (*w).max(1);
        *h = (*h).max(1);
        let original_geometry = if let Some(client) = self.state.clients.get(client_key) {
            (
                client.geometry.x,
                client.geometry.y,
                client.geometry.w,
                client.geometry.h,
            )
        } else {
            return Err("Client not found".into());
        };
        self.apply_boundary_constraints(client_key, x, y, w, h, interact)?;
        let geometry_changed = self.apply_size_hints_constraints(backend, client_key, w, h)?;
        Ok(geometry_changed
            || *x != original_geometry.0
            || *y != original_geometry.1
            || *w != original_geometry.2
            || *h != original_geometry.3)
    }

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
            if let Some(client) = self.state.clients.get(client_key) {
                (
                    *w + 2 * client.geometry.border_w,
                    *h + 2 * client.geometry.border_w,
                    client.mon,
                )
            } else {
                return Err("Client not found".into());
            };

        if interact {
            self.constrain_to_screen(x, y, client_total_width, client_total_height);
        } else {
            if let Some(mon_key) = mon_key {
                if let Some(monitor) = self.state.monitors.get(mon_key) {
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

    fn constrain_to_screen(&self, x: &mut i32, y: &mut i32, total_width: i32, total_height: i32) {
        let min_x = -(total_width - 1);
        let max_x = self.s_w - 1;
        if min_x <= max_x {
            *x = (*x).clamp(min_x, max_x);
        } else {
            warn!(
                "Skip screen X clamp because max_x({}) < min_x({}); total_width={}, s_w={}",
                max_x, min_x, total_width, self.s_w
            );
            *x = min_x;
        }

        let min_y = -(total_height - 1);
        let max_y = self.s_h - 1;
        if min_y <= max_y {
            *y = (*y).clamp(min_y, max_y);
        } else {
            warn!(
                "Skip screen Y clamp because max_y({}) < min_y({}); total_height={}, s_h={}",
                max_y, min_y, total_height, self.s_h
            );
            *y = min_y;
        }
    }

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

        let min_x = wx - total_width + 1;
        let max_x = wx + ww - 1;
        if min_x <= max_x {
            *x = (*x).clamp(min_x, max_x);
        } else {
            warn!(
                "Skip monitor X clamp because max_x({}) < min_x({}); total_width={}, monitor_ww={}",
                max_x, min_x, total_width, ww
            );
            *x = min_x;
        }

        let min_y = wy - total_height + 1;
        let max_y = wy + wh - 1;
        if min_y <= max_y {
            *y = (*y).clamp(min_y, max_y);
        } else {
            warn!(
                "Skip monitor Y clamp because max_y({}) < min_y({}); total_height={}, monitor_wh={}",
                max_y, min_y, total_height, wh
            );
            *y = min_y;
        }
    }

    fn apply_size_hints_constraints(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        w: &mut i32,
        h: &mut i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let is_floating = self
            .state
            .clients
            .get(client_key)
            .map(|client| client.state.is_floating)
            .unwrap_or(false);

        if !CONFIG.load().behavior().resize_hints && !is_floating {
            return Ok(false);
        }

        self.ensure_size_hints_valid(backend, client_key)?;

        let hints = if let Some(client) = self.state.clients.get(client_key) {
            client.size_hints.clone()
        } else {
            return Err("Client not found".into());
        };

        let (new_w, new_h) = self.calculate_constrained_size(*w, *h, &hints);
        let changed = *w != new_w || *h != new_h;
        *w = new_w;
        *h = new_h;

        Ok(changed)
    }

    fn ensure_size_hints_valid(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hints_valid = self
            .state
            .clients
            .get(client_key)
            .map(|client| client.size_hints.hints_valid)
            .unwrap_or(false);
        if !hints_valid {
            self.updatesizehints(backend, client_key)?;
        }

        Ok(())
    }

    fn calculate_constrained_size(&self, mut w: i32, mut h: i32, hints: &SizeHints) -> (i32, i32) {
        w = self.apply_increments(w - hints.base_w, hints.inc_w) + hints.base_w;
        h = self.apply_increments(h - hints.base_h, hints.inc_h) + hints.base_h;

        (w, h) = self.apply_aspect_ratio_constraints(w, h, hints);

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

    fn apply_increments(&self, size: i32, increment: i32) -> i32 {
        if increment > 0 {
            (size / increment) * increment
        } else {
            size
        }
    }

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

    fn updatesizehints(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = self
            .state
            .clients
            .get(client_key)
            .map(|c| c.win)
            .ok_or("Client not found")?;

        match backend.property_ops().fetch_normal_hints(win)? {
            Some(h) => {
                let c = self
                    .state
                    .clients
                    .get_mut(client_key)
                    .ok_or("Client not found")?;
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
                if let Some(c) = self.state.clients.get_mut(client_key) {
                    c.size_hints.hints_valid = false;
                }
            }
        }
        Ok(())
    }

    pub fn cleanup(&mut self, backend: &mut dyn Backend) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup] Starting essential cleanup (letting Rust handle memory)");
        // Shut down IPC server (also handled by Drop, but explicit is clearer)
        if let Some(ref mut ipc) = self.ipc_server {
            ipc.shutdown();
        }
        self.ipc_server = None;
        self.cleanup_x11_resources(backend)?;
        self.cleanup_system_resources()?;
        backend.color_allocator().free_all_theme_pixels()?;
        backend.window_ops().flush()?;
        info!("[cleanup] Essential cleanup completed (Rust will handle the rest)");
        Ok(())
    }

    fn cleanup_x11_resources(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup_x11_resources] Cleaning X11 resources");

        self.cleanup_all_clients_x11_state(backend)?;

        self.cleanup_key_grabs(backend)?;

        self.reset_input_focus(backend)?;

        backend.cleanup()?;

        if let Err(e) = backend.cursor_provider().cleanup() {
            log::warn!("cursor cleanup failed: {:?}", e);
        }

        info!("[cleanup_x11_resources] X11 resources cleaned");
        Ok(())
    }

    fn cleanup_system_resources(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup_system_resources] Cleaning system resources");

        self.cleanup_statusbar_processes()?;

        self.cleanup_shared_memory_resources()?;

        info!("[cleanup_system_resources] System resources cleaned");
        Ok(())
    }

    fn cleanup_all_clients_x11_state(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cleanup_all_clients_x11_state]");
        let restarting = self.is_restarting.load(Ordering::SeqCst);

        let mut clients_to_process = Vec::new();
        for &mon_key in &self.state.monitor_order {
            if let Some(stack) = self.state.monitor_stack.get(mon_key) {
                for &ck in stack {
                    if let Some(c) = self.state.clients.get(ck) {
                        clients_to_process.push((c.win, c.geometry.old_border_w, ck));
                    }
                }
            }
        }
        for (win, old_border_w, ck) in clients_to_process {
            if let Some(_) = self.state.clients.get(ck) {
                if restarting {
                    backend.window_ops().ungrab_all_buttons(win)?;
                } else {
                    let _ = self.restore_client_x11_state(backend, win, old_border_w);
                }
            }
        }

        Ok(())
    }

    fn restore_client_x11_state(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        old_border_w: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Err(e) = backend
            .window_ops()
            .change_event_mask(win, EventMaskBits::NONE.bits())
        {
            log::warn!("Failed to clear events for {:?}: {:?}", win, e);
        }
        let changes = WindowChanges {
            border_width: Some(old_border_w as u32),
            ..Default::default()
        };
        if let Err(e) = backend.window_ops().apply_window_changes(win, changes) {
            log::warn!("Failed to restore border for {:?}: {:?}", win, e);
        }
        if let Err(e) = backend.window_ops().ungrab_all_buttons(win) {
            log::warn!("Failed to ungrab buttons for {:?}: {:?}", win, e);
        }
        if let Err(e) = self.setclientstate(backend, win, crate::jwm::WITHDRAWN_STATE as i64) {
            log::warn!("Failed to set withdrawn state for {:?}: {:?}", win, e);
        }
        Ok(())
    }

    fn cleanup_statusbar_processes(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut child = if let Some(child) = self.status_bar_child.take() {
            child
        } else {
            return Ok(());
        };
        let pid = child.id();
        let nix_pid = Pid::from_raw(pid as i32);
        match signal::kill(nix_pid, None) {
            Err(_) => {
                info!("Process already terminated",);
                return Ok(());
            }
            Ok(_) => {}
        }
        if let Ok(_) = signal::kill(nix_pid, Signal::SIGTERM) {
            let timeout = Duration::from_secs(3);
            let start = Instant::now();
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
            warn!("Graceful termination timeout, forcing kill");
        }
        signal::kill(nix_pid, Signal::SIGKILL)?;

        Ok(())
    }

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

    fn cleanup_key_grabs(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Err(e) = backend
            .key_ops()
            .clear_key_grabs(backend.root_window().expect("no root window"))
        {
            warn!("[cleanup_key_grabs] Failed to ungrab keys: {:?}", e);
        }
        Ok(())
    }

    fn reset_input_focus(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        backend.window_ops().set_input_focus_root()?;
        Ok(())
    }

    fn configurenotify(
        &mut self,
        backend: &mut dyn Backend,
        window: WindowId,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // If the backend reports the status bar moved/resized, track it and re-arrange.
        // This is required for Wayland layer-shell bars, where JWM does not control
        // the final geometry.
        if Some(window) == self.status_bar_window {
            if let Some(bar_key) = self.status_bar_client {
                if let Some(bar) = self.state.clients.get_mut(bar_key) {
                    bar.geometry.x = x;
                    bar.geometry.y = y;
                    bar.geometry.w = w as i32;
                    bar.geometry.h = h as i32;
                }

                if let Some(mon_key) = self.state.clients.get(bar_key).and_then(|c| c.mon) {
                    self.arrange(backend, Some(mon_key));
                } else {
                    self.arrange(backend, None);
                }
            }
            return Ok(());
        }

        if window == backend.root_window().expect("no root window") {
            let dirty = self.s_w != w as i32 || self.s_h != h as i32;
            self.s_w = w as i32;
            self.s_h = h as i32;
            if self.updategeom(backend) || dirty {
                self.handle_screen_geometry_change(backend)?;
            }
        }

        // For Wayland layer-shell (and other backend-driven docks), the compositor controls
        // the final geometry. Reflect it in our model and re-arrange so workareas update.
        if let Some(client_key) = self.wintoclient(window) {
            let layer_info = backend.property_ops().get_layer_surface_info(window);
            let is_likely_dock = self
                .state
                .clients
                .get(client_key)
                .map(|c| c.state.is_dock)
                .unwrap_or(false)
                || layer_info.is_some();

            if is_likely_dock {
                if let Some(c) = self.state.clients.get_mut(client_key) {
                    c.geometry.x = x;
                    c.geometry.y = y;
                    c.geometry.w = w as i32;
                    c.geometry.h = h as i32;
                }

                // Refresh type/layer metadata so exclusive_zone changes are honored.
                self.updatewindowtype(backend, client_key);

                if let Some(mon_key) = self.state.clients.get(client_key).and_then(|c| c.mon) {
                    self.arrange(backend, Some(mon_key));
                } else {
                    self.arrange(backend, None);
                }
            }
        }

        Ok(())
    }

    fn handle_screen_geometry_change(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[handle_screen_geometry_change]");
        let monitors: Vec<_> = self.state.monitor_order.to_vec();
        for mon_key in monitors {
            self.update_fullscreen_clients_on_monitor(backend, mon_key)?;
        }
        self.focus(backend, None)?;
        self.arrange(backend, None);
        Ok(())
    }

    fn update_fullscreen_clients_on_monitor(
        &mut self,
        backend: &mut dyn Backend,
        mon_key: MonitorKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let monitor_geometry = if let Some(monitor) = self.state.monitors.get(mon_key) {
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

        let fullscreen_clients: Vec<ClientKey> =
            if let Some(client_keys) = self.state.monitor_clients.get(mon_key) {
                client_keys
                    .iter()
                    .filter(|&&client_key| {
                        self.state
                            .clients
                            .get(client_key)
                            .map(|client| client.state.is_fullscreen)
                            .unwrap_or(false)
                    })
                    .copied()
                    .collect()
            } else {
                Vec::new()
            };

        for client_key in fullscreen_clients {
            let _ = self.resizeclient(
                backend,
                client_key,
                monitor_geometry.0,
                monitor_geometry.1,
                monitor_geometry.2,
                monitor_geometry.3,
            );
        }
        Ok(())
    }

    fn update_client_decoration(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        is_focused: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (win, border_w) = if let Some(client) = self.state.clients.get(client_key) {
            (client.win, client.geometry.border_w)
        } else {
            return Err("Client not found".into());
        };

        let scheme = if is_focused {
            SchemeType::Sel
        } else {
            SchemeType::Norm
        };
        if let Ok(pixel) = backend.color_allocator().get_border_pixel_of(scheme) {
            backend
                .window_ops()
                .set_decoration_style(win, border_w as u32, pixel)?;
        }
        Ok(())
    }

    fn grabkeys(&mut self, backend: &mut dyn Backend) -> Result<(), Box<dyn std::error::Error>> {
        let root_window = backend.root_window().expect("no root window");
        backend.key_ops().clear_key_grabs(root_window)?;
        let bindings: Vec<(Mods, KeySym)> = self
            .key_bindings
            .iter()
            .map(|k| (k.mask, k.key_sym))
            .collect();
        backend.key_ops().grab_keys(root_window, &bindings)?;
        Ok(())
    }

    fn setfullscreen(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        fullscreen: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.state.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        let is_fullscreen = self
            .state
            .clients
            .get(client_key)
            .map(|c| c.state.is_fullscreen)
            .unwrap_or(false);

        if fullscreen && !is_fullscreen {
            backend.property_ops().set_fullscreen_state(win, true)?;

            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.state.is_fullscreen = true;
                client.state.old_state = client.state.is_floating;
                client.geometry.old_border_w = client.geometry.border_w;
                client.geometry.border_w = 0;
                client.state.is_floating = true;
            }
            self.reorder_client_in_monitor_groups(client_key);
            if let Some(mon_key) = self.state.clients.get(client_key).and_then(|c| c.mon) {
                if let Some(monitor) = self.state.monitors.get(mon_key) {
                    let (mx, my, mw, mh) = (
                        monitor.geometry.m_x,
                        monitor.geometry.m_y,
                        monitor.geometry.m_w,
                        monitor.geometry.m_h,
                    );
                    self.resizeclient(backend, client_key, mx, my, mw, mh)?;
                }
            }
            let changes = WindowChanges {
                stack_mode: Some(StackMode::Above),
                ..Default::default()
            };
            backend.window_ops().apply_window_changes(win, changes)?;
        } else if !fullscreen && is_fullscreen {
            backend.property_ops().set_fullscreen_state(win, false)?;

            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.state.is_fullscreen = false;
                client.state.is_floating = client.state.old_state;
                client.geometry.border_w = client.geometry.old_border_w;
                client.geometry.x = client.geometry.old_x;
                client.geometry.y = client.geometry.old_y;
                client.geometry.w = client.geometry.old_w;
                client.geometry.h = client.geometry.old_h;
            }
            self.reorder_client_in_monitor_groups(client_key);
            let (x, y, w, h) = if let Some(client) = self.state.clients.get(client_key) {
                (
                    client.geometry.x,
                    client.geometry.y,
                    client.geometry.w,
                    client.geometry.h,
                )
            } else {
                return Ok(());
            };
            self.resizeclient(backend, client_key, x, y, w, h)?;
            if let Some(mon_key) = self.state.clients.get(client_key).and_then(|c| c.mon) {
                self.arrange(backend, Some(mon_key));
            }
        }
        Ok(())
    }

    fn seturgent(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.state.is_urgent = urgent;
        } else {
            return Err("Client not found".into());
        }

        let win = self
            .state
            .clients
            .get(client_key)
            .map(|c| c.win)
            .ok_or("Client not found")?;
        Ok(backend.property_ops().set_urgent_hint(win, urgent)?)
    }

    fn showhide_monitor(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        if let Some(stack_clients) = self.state.monitor_stack.get(mon_key).cloned() {
            for client_key in stack_clients {
                self.showhide_client(backend, client_key, mon_key);
            }
        }
    }

    fn showhide_client(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        mon_key: MonitorKey,
    ) {
        let is_visible = self.is_client_visible_on_monitor(client_key, mon_key);

        if is_visible {
            self.show_client(backend, client_key);
        } else {
            self.hide_client(backend, client_key);
        }
    }

    fn show_client(&mut self, backend: &mut dyn Backend, client_key: ClientKey) {
        // Cancel any in-flight Hide animation so it doesn't keep moving
        // the window off-screen.
        self.animations.remove(client_key);

        // Restore on-screen x from old_x if client.geometry.x is still at
        // the hidden position (negative, off-screen).
        if let Some(client) = self.state.clients.get_mut(client_key) {
            if client.geometry.x < -(client.geometry.w) {
                client.geometry.x = client.geometry.old_x;
            }
        }

        let (win, x, y, is_floating, is_fullscreen) =
            if let Some(client) = self.state.clients.get(client_key) {
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

        if let Err(e) = self.move_window(backend, win, x, y) {
            warn!("[show_client] Failed to move window {:?}: {:?}", win, e);
        }

        if is_floating && !is_fullscreen {
            let (w, h) = if let Some(client) = self.state.clients.get(client_key) {
                (client.geometry.w, client.geometry.h)
            } else {
                return;
            };
            self.resize_client(backend, client_key, x, y, w, h, false);
        }
    }

    fn hide_client(&mut self, backend: &mut dyn Backend, client_key: ClientKey) {
        let (win, x, y, w, h, width) = if let Some(client) = self.state.clients.get(client_key) {
            (
                client.win,
                client.geometry.x,
                client.geometry.y,
                client.geometry.w,
                client.geometry.h,
                client.total_width(),
            )
        } else {
            warn!("[hide_client] Client {:?} not found", client_key);
            return;
        };

        let hidden_x = width * -2;

        // Save visible geometry so show_client can restore it, then update
        // client.geometry to the hidden position. This prevents
        // tick_animations from snapping the window back on-screen when the
        // Hide animation completes.
        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.geometry.old_x = client.geometry.x;
            client.geometry.old_y = client.geometry.y;
            client.geometry.x = hidden_x;
            // y, w, h stay unchanged
        }

        let cfg = CONFIG.load();
        if cfg.animation_enabled() {
            let now = Instant::now();
            let visual = self
                .animations
                .current_visual_rect(client_key, now)
                .unwrap_or(Rect::new(x, y, w, h));
            let target = Rect::new(hidden_x, y, w, h);
            self.animations.start(
                client_key,
                visual,
                target,
                cfg.animation_duration(),
                cfg.animation_easing(),
                AnimationKind::Hide,
            );
        } else {
            if let Err(e) = self.move_window(backend, win, hidden_x, y) {
                warn!("[hide_client] Failed to hide window {:?}: {:?}", win, e);
            }
        }
    }

    fn resize_client(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        mut x: i32,
        mut y: i32,
        mut w: i32,
        mut h: i32,
        interact: bool,
    ) {
        if self
            .applysizehints(
                backend, client_key, &mut x, &mut y, &mut w, &mut h, interact,
            )
            .is_ok()
        {
            let _ = self.resizeclient(backend, client_key, x, y, w, h);
        }
    }

    fn resizeclient(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.geometry.old_x = client.geometry.x;
            client.geometry.old_y = client.geometry.y;
            client.geometry.old_w = client.geometry.w;
            client.geometry.old_h = client.geometry.h;

            client.geometry.x = x;
            client.geometry.y = y;
            client.geometry.w = w;
            client.geometry.h = h;

            let cfg = CONFIG.load();
            if cfg.animation_enabled() {
                let old_rect = Rect::new(
                    client.geometry.old_x,
                    client.geometry.old_y,
                    client.geometry.old_w,
                    client.geometry.old_h,
                );
                let target = Rect::new(x, y, w, h);
                let duration = cfg.animation_duration();
                let easing = cfg.animation_easing();
                drop(cfg);
                let now = Instant::now();
                let visual = self
                    .animations
                    .current_visual_rect(client_key, now)
                    .unwrap_or(old_rect);
                self.animations.start(
                    client_key,
                    visual,
                    target,
                    duration,
                    easing,
                    AnimationKind::Layout,
                );
            } else {
                backend.window_ops().configure(
                    client.win,
                    x,
                    y,
                    w as u32,
                    h as u32,
                    client.geometry.border_w as u32,
                )?;
            }
        }
        Ok(())
    }

    fn tick_animations(&mut self, backend: &mut dyn Backend) {
        let composited = backend.has_compositor();

        if !self.animations.has_active() {
            if composited {
                // No animations but compositor still needs to render
                // (window contents may have updated via damage)
                let scene = self.build_compositor_scene(&HashMap::new());
                if scene.is_empty() {
                    // Log once per second at most
                    static LAST_EMPTY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let prev = LAST_EMPTY.load(std::sync::atomic::Ordering::Relaxed);
                    if now > prev {
                        LAST_EMPTY.store(now, std::sync::atomic::Ordering::Relaxed);
                        log::warn!("[tick_animations] compositor scene is EMPTY (no windows to render)");
                    }
                }
                let _ = backend.compositor_render_frame(&scene);
            }
            return;
        }

        let now = Instant::now();
        let mut completed = Vec::new();
        let mut visual_overrides: HashMap<ClientKey, Rect> = HashMap::new();

        let keys: Vec<ClientKey> = self.animations.active.keys().copied().collect();
        for key in keys {
            let anim = match self.animations.active.get(&key) {
                Some(a) => a,
                None => continue,
            };
            let (rect, done) = anim.sample(now);

            if self.state.clients.get(key).is_none() {
                completed.push(key);
                continue;
            }

            if composited {
                // Store visual override — compositor draws at interpolated position.
                // Real window is already at the target position (set by resizeclient).
                visual_overrides.insert(key, rect);
            } else {
                // Non-composited fallback: physically move the window each frame
                if let Some(client) = self.state.clients.get(key) {
                    let _ = backend.window_ops().configure(
                        client.win,
                        rect.x,
                        rect.y,
                        rect.w as u32,
                        rect.h as u32,
                        client.geometry.border_w as u32,
                    );
                }
            }

            if done {
                completed.push(key);
            }
        }

        if composited {
            let scene = self.build_compositor_scene(&visual_overrides);
            let _ = backend.compositor_render_frame(&scene);
        }

        for key in completed {
            self.animations.active.remove(&key);
        }
    }

    /// Build an ordered scene for the compositor: Vec<(window_id_raw, x, y, w, h)>
    /// from bottom to top, using the last_stacking order. For windows with
    /// active animation overrides, use the interpolated rect instead of actual geometry.
    fn build_compositor_scene(
        &self,
        visual_overrides: &HashMap<ClientKey, Rect>,
    ) -> Vec<(u64, i32, i32, u32, u32)> {
        let mut scene = Vec::new();

        let debug_compositor = std::env::var("JWM_DEBUG_COMPOSITOR")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        // Iterate all monitors, using last_stacking order (bottom to top)
        for &mon_key in &self.state.monitor_order {
            if debug_compositor {
                let has_stacking = self.last_stacking.get(mon_key).is_some();
                let stack_len = self
                    .last_stacking
                    .get(mon_key)
                    .map(|s| s.len())
                    .unwrap_or(0);
                let client_count = self
                    .state
                    .monitor_clients
                    .get(mon_key)
                    .map(|c| c.len())
                    .unwrap_or(0);
                info!(
                    "[compositor_scene] mon={:?} has_stacking={} stack_len={} clients={}",
                    mon_key, has_stacking, stack_len, client_count
                );
            }
            // Use last_stacking if available, otherwise fall back to
            // monitor_stack so the compositor still has something to render
            // when restack() hasn't run yet for this monitor.
            let stacking_source: Vec<WindowId> =
                if let Some(stacking) = self.last_stacking.get(mon_key) {
                    stacking.clone()
                } else if let Some(stack) = self.state.monitor_stack.get(mon_key) {
                    // Fallback: build bottom-to-top from monitor_stack (which is top-to-bottom)
                    stack
                        .iter()
                        .rev()
                        .filter_map(|&ck| {
                            let c = self.state.clients.get(ck)?;
                            if self.is_client_visible_on_monitor(ck, mon_key) {
                                Some(c.win)
                            } else {
                                None
                            }
                        })
                        .collect()
                } else {
                    Vec::new()
                };

            for &win_id in &stacking_source {
                // Find the client key for this window
                if let Some(&ck) = self.state.win_to_client.get(&win_id) {
                    if let Some(client) = self.state.clients.get(ck) {
                        let (x, y, w, h) = if let Some(rect) = visual_overrides.get(&ck) {
                            (rect.x, rect.y, rect.w as u32, rect.h as u32)
                        } else {
                            (
                                client.geometry.x,
                                client.geometry.y,
                                client.geometry.w as u32,
                                client.geometry.h as u32,
                            )
                        };
                        if w > 0 && h > 0 {
                            scene.push((win_id.raw(), x, y, w, h));
                        }
                    }
                }
            }
        }

        // Also include the status bar if present
        if let Some(bar_key) = self.status_bar_client {
            if let Some(bar) = self.state.clients.get(bar_key) {
                let w = bar.geometry.w as u32;
                let h = bar.geometry.h as u32;
                if w > 0 && h > 0 {
                    scene.push((
                        bar.win.raw(),
                        bar.geometry.x,
                        bar.geometry.y,
                        w,
                        h,
                    ));
                }
            }
        }

        scene
    }

    fn sync_focused_floating_geometry(&mut self, backend: &mut dyn Backend) {
        let sel_key = match self.get_selected_client_key() {
            Some(k) => k,
            None => return,
        };
        let win = match self.state.clients.get(sel_key) {
            Some(c) if c.state.is_floating => c.win,
            _ => return,
        };
        let geom = match backend.window_ops().get_geometry(win) {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Some(client) = self.state.clients.get_mut(sel_key) {
            client.geometry.x = geom.x as i32;
            client.geometry.y = geom.y as i32;
            client.geometry.w = geom.w as i32;
            client.geometry.h = geom.h as i32;
            client.geometry.floating_x = geom.x as i32;
            client.geometry.floating_y = geom.y as i32;
            client.geometry.floating_w = geom.w as i32;
            client.geometry.floating_h = geom.h as i32;
        }
    }

    fn configure_client(
        &self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.state.clients.get(client_key) {
            backend.window_ops().configure(
                client.win,
                client.geometry.x,
                client.geometry.y,
                client.geometry.w as u32,
                client.geometry.h as u32,
                client.geometry.border_w as u32,
            )?;

            // 分离装饰设置
            let border_color = backend
                .color_allocator()
                .get_border_pixel_of(SchemeType::Norm)?;
            backend.window_ops().set_decoration_style(
                client.win,
                client.geometry.border_w as u32,
                border_color,
            )?;
        }
        Ok(())
    }

    fn move_window(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        x: i32,
        y: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        backend.window_ops().set_position(win, x, y)?;
        Ok(())
    }

    fn createmon(&mut self, show_bar: bool) -> WMMonitor {
        // info!("[createmon]");
        let mut m: WMMonitor = WMMonitor::new();
        m.tag_set[0] = 1;
        m.tag_set[1] = 1;
        m.layout.m_fact = CONFIG.load().m_fact();
        m.layout.n_master = CONFIG.load().n_master();
        m.lt[0] = Rc::new(LayoutEnum::FIBONACCI);
        m.lt[1] = Rc::new(LayoutEnum::TILE);
        m.lt_symbol = m.lt[0].symbol().to_string();
        m.pertag = Some(Pertag::new(show_bar, CONFIG.load().tags_length()));
        // SAFETY: pertag was just set to Some on the line above
        let ref_pertag = m.pertag.as_mut().expect("pertag just initialized");
        ref_pertag.cur_tag = 1;
        ref_pertag.prev_tag = 1;
        let default_layout_0 = m.lt[0].clone();
        let default_layout_1 = m.lt[1].clone();
        for i in 0..=CONFIG.load().tags_length() {
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
        backend: &mut dyn Backend,
        event_window: WindowId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.handle_statusbar_enter_generic(backend, event_window)? {
            return Ok(());
        }
        self.handle_regular_enter_generic(backend, event_window)?;
        Ok(())
    }

    fn handle_statusbar_enter_generic(
        &mut self,
        backend: &mut dyn Backend,
        event_window: WindowId,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if Some(event_window) == self.status_bar_window {
            if let Some(cur_bar_mon_id) = self.current_bar_monitor_id {
                if let Some(target_monitor_key) = self.get_monitor_by_id(cur_bar_mon_id) {
                    if Some(target_monitor_key) != self.state.sel_mon {
                        let current_sel = self.get_selected_client_key();
                        self.unfocus_client_opt(backend, current_sel, true)?;
                        self.state.sel_mon = Some(target_monitor_key);
                        self.focus(backend, None)?;
                    }
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn handle_regular_enter_generic(
        &mut self,
        backend: &mut dyn Backend,
        event_window: WindowId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.mouse_focus_blocked() {
            return Ok(());
        }
        let client_key_opt = self.wintoclient(event_window);
        let monitor_key_opt = if let Some(client_key) = client_key_opt {
            self.state
                .clients
                .get(client_key)
                .and_then(|client| client.mon)
        } else {
            self.wintomon(backend, Some(event_window))
        };
        let current_event_monitor_key = match monitor_key_opt {
            Some(monitor_key) => monitor_key,
            None => return Ok(()),
        };
        let is_on_selected_monitor = Some(current_event_monitor_key) == self.state.sel_mon;
        if !is_on_selected_monitor {
            self.switch_to_monitor(backend, current_event_monitor_key)?;
        }
        if self.should_focus_client(client_key_opt, is_on_selected_monitor) {
            self.focus(backend, client_key_opt)?;
        }
        Ok(())
    }

    fn destroynotify(
        &mut self,
        backend: &mut dyn Backend,
        window: WindowId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Clean up external struts for override-redirect windows (e.g. polybar)
        // that are never managed but may have set strut properties.
        if self.external_struts.remove(&window).is_some() {
            info!("[strut] Removed strut on destroy for {:?}", window);
            self.apply_strut_reservations();
            self.arrange(backend, None);
        }
        let c = self.wintoclient(window);
        if c.is_some() {
            self.unmanage(backend, c, true)?;
        }
        Ok(())
    }

    fn arrangemon(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        info!("[arrangemon]");

        let (layout_type, layout_symbol) = if let Some(monitor) = self.state.monitors.get(mon_key) {
            let sel_lt = monitor.sel_lt;
            let layout = &monitor.lt[sel_lt];
            (layout.clone(), layout.symbol().to_string())
        } else {
            warn!("Monitor {:?} not found", mon_key);
            return;
        };

        if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
            monitor.lt_symbol = layout_symbol;
            info!(
                "sel_lt: {}, ltsymbol: {:?}",
                monitor.sel_lt, monitor.lt_symbol
            );
        }

        match *layout_type {
            LayoutEnum::TILE => self.tile(backend, mon_key),
            LayoutEnum::MONOCLE => self.monocle(backend, mon_key),
            LayoutEnum::FIBONACCI => self.fibonacci(backend, mon_key),
            LayoutEnum::CENTERED_MASTER => self.centered_master(backend, mon_key),
            LayoutEnum::BSTACK => self.bstack(backend, mon_key),
            LayoutEnum::GRID => self.grid(backend, mon_key),
            LayoutEnum::DECK => self.deck(backend, mon_key),
            LayoutEnum::THREE_COL => self.three_col(backend, mon_key),
            LayoutEnum::TATAMI => self.tatami(backend, mon_key),
            LayoutEnum::FULLSCREEN => self.fullscreen_layout(backend, mon_key),
            LayoutEnum::FLOAT | _ => {}
        }
    }

    fn fibonacci(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        info!("[fibonacci] via pure layout engine");

        // 1. 获取显示器信息和配置
        let (wx, wy, ww, wh, mfact, nmaster, _monitor_num, _client_y_offset) =
            self.get_monitor_info(mon_key);

        // 计算可用区域 (优先使用 statusbar 的真实几何)
        let screen_area = self
            .monitor_work_area(mon_key)
            .unwrap_or(Rect::new(wx, wy, ww, wh));

        // 2. 收集需要参与布局的客户端 (使用现有的辅助函数)
        let raw_clients = self.collect_tileable_clients(mon_key);
        if raw_clients.is_empty() {
            return;
        }

        let (_effective_border, effective_gap) = self.apply_smart_borders(&raw_clients);
        let default_border = CONFIG.load().border_px() as i32;

        // 转换为 LayoutClient 结构
        let layout_clients: Vec<LayoutClient<ClientKey>> = raw_clients
            .iter()
            .map(|&(key, factor, _)| LayoutClient {
                key,
                factor,
                border_w: self.state.clients.get(key).map(|c| c.geometry.border_w).unwrap_or(default_border),
            })
            .collect();

        // 3. 构造参数
        let params = LayoutParams {
            screen_area,
            n_master: nmaster,
            m_fact: mfact,
            gap: effective_gap,
        };

        // 4. 计算布局
        let results = layout::calculate_fibonacci(&params, &layout_clients);

        // 5. 应用结果 (调整窗口大小和位置)
        for res in results {
            self.resize_client(
                backend, res.key, res.rect.x, res.rect.y, res.rect.w, res.rect.h, false,
            );
        }
    }

    fn tiling_layout_wrapper(
        &mut self,
        backend: &mut dyn Backend,
        mon_key: MonitorKey,
        name: &str,
        calc_fn: fn(&LayoutParams, &[LayoutClient<ClientKey>]) -> Vec<LayoutResult<ClientKey>>,
    ) {
        info!("[{}] via pure layout engine", name);
        let (wx, wy, ww, wh, mfact, nmaster, _monitor_num, _client_y_offset) =
            self.get_monitor_info(mon_key);

        let screen_area = self
            .monitor_work_area(mon_key)
            .unwrap_or(Rect::new(wx, wy, ww, wh));

        let raw_clients = self.collect_tileable_clients(mon_key);
        if raw_clients.is_empty() {
            return;
        }

        let (_effective_border, effective_gap) = self.apply_smart_borders(&raw_clients);
        let default_border = CONFIG.load().border_px() as i32;

        let layout_clients: Vec<LayoutClient<ClientKey>> = raw_clients
            .iter()
            .map(|&(key, factor, _)| LayoutClient {
                key,
                factor,
                border_w: self.state.clients.get(key).map(|c| c.geometry.border_w).unwrap_or(default_border),
            })
            .collect();

        let params = LayoutParams {
            screen_area,
            n_master: nmaster,
            m_fact: mfact,
            gap: effective_gap,
        };

        let results = calc_fn(&params, &layout_clients);

        for res in results {
            self.resize_client(
                backend, res.key, res.rect.x, res.rect.y, res.rect.w, res.rect.h, false,
            );
        }
    }

    fn centered_master(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        self.tiling_layout_wrapper(backend, mon_key, "centered_master", layout::calculate_centered_master);
    }

    fn bstack(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        self.tiling_layout_wrapper(backend, mon_key, "bstack", layout::calculate_bstack);
    }

    fn grid(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        self.tiling_layout_wrapper(backend, mon_key, "grid", layout::calculate_grid);
    }

    fn deck(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        self.tiling_layout_wrapper(backend, mon_key, "deck", layout::calculate_deck);
    }

    fn three_col(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        self.tiling_layout_wrapper(backend, mon_key, "three_col", layout::calculate_three_col);
    }

    fn tatami(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        self.tiling_layout_wrapper(backend, mon_key, "tatami", layout::calculate_tatami);
    }

    fn fullscreen_layout(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        info!("[fullscreen_layout] via pure layout engine");

        // 使用完整显示器区域 (m_x, m_y, m_w, m_h)，不是 work area
        let (mx, my, mw, mh) = if let Some(monitor) = self.state.monitors.get(mon_key) {
            (
                monitor.geometry.m_x,
                monitor.geometry.m_y,
                monitor.geometry.m_w,
                monitor.geometry.m_h,
            )
        } else {
            return;
        };

        let raw_clients = self.collect_tileable_clients(mon_key);
        if raw_clients.is_empty() {
            return;
        }

        // 全屏模式下 border_w = 0
        let layout_clients: Vec<LayoutClient<ClientKey>> = raw_clients
            .iter()
            .map(|&(key, factor, _border_w)| LayoutClient {
                key,
                factor,
                border_w: 0,
            })
            .collect();

        let params = LayoutParams {
            screen_area: Rect::new(mx, my, mw, mh),
            n_master: 0,
            m_fact: 0.0,
            gap: 0,
        };

        let results = layout::calculate_fullscreen(&params, &layout_clients);

        // 临时将 border_w 设为 0，应用布局后恢复
        for &(key, _, _original_border_w) in &raw_clients {
            if let Some(client) = self.state.clients.get_mut(key) {
                client.geometry.border_w = 0;
            }
        }

        for res in results {
            self.resize_client(
                backend, res.key, res.rect.x, res.rect.y, res.rect.w, res.rect.h, false,
            );
        }
    }

    fn dirtomon(&mut self, dir: &i32) -> Option<MonitorKey> {
        let selected_monitor_key = self.state.sel_mon?;
        if self.state.monitor_order.is_empty() {
            return None;
        }
        let current_index = self
            .state
            .monitor_order
            .iter()
            .position(|&key| key == selected_monitor_key)?;
        if *dir > 0 {
            let next_index = (current_index + 1) % self.state.monitor_order.len();
            Some(self.state.monitor_order[next_index])
        } else {
            let prev_index = if current_index == 0 {
                self.state.monitor_order.len() - 1
            } else {
                current_index - 1
            };
            Some(self.state.monitor_order[prev_index])
        }
    }

    fn ensure_bar_is_running(&mut self, shared_path: &str) {
        let now = std::time::Instant::now();

        if let Some(until) = self.status_bar_backoff_until {
            if now < until {
                return;
            }
        }

        // 1. 检查现有进程状态
        if let Some(child) = self.status_bar_child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    info!("Status bar process exited with: {status}");

                    // Mark as stopped so we can respawn later.
                    self.status_bar_child = None;

                    if status.success() {
                        self.status_bar_restart_failures = 0;
                    } else {
                        self.status_bar_restart_failures = self.status_bar_restart_failures.saturating_add(1);
                    }

                    // Backoff to avoid a tight respawn loop starving input/rendering.
                    let pow = self.status_bar_restart_failures.min(6);
                    let base_ms = 200u64;
                    let backoff_ms = base_ms.saturating_mul(1u64 << pow).min(10_000);
                    self.status_bar_backoff_until = Some(now + std::time::Duration::from_millis(backoff_ms));

                    // Don't respawn in the same tick.
                    return;
                }
                Ok(None) => {
                    return;
                }
                Err(e) => {
                    info!(
                        "Error attempting to wait on status bar child: {e}, will try to respawn."
                    );

                    self.status_bar_child = None;
                    self.status_bar_restart_failures = self.status_bar_restart_failures.saturating_add(1);

                    let pow = self.status_bar_restart_failures.min(6);
                    let base_ms = 200u64;
                    let backoff_ms = base_ms.saturating_mul(1u64 << pow).min(10_000);
                    self.status_bar_backoff_until = Some(now + std::time::Duration::from_millis(backoff_ms));
                    return;
                }
            }
        }

        if let Some(until) = self.status_bar_backoff_until {
            if now < until {
                return;
            }
        }

        // 3. 准备启动命令
        let mut command = if cfg!(feature = "nixgl") {
            let mut cmd = Command::new("nixGL");
            cmd.arg(CONFIG.load().status_bar_name()).arg(shared_path);
            cmd
        } else {
            let mut cmd = Command::new(CONFIG.load().status_bar_name());
            cmd.arg(shared_path);
            cmd
        };

        // Make sure the bar inherits the Wayland env we set up.
        if let Ok(v) = std::env::var("WAYLAND_DISPLAY") {
            command.env("WAYLAND_DISPLAY", v);
        }
        if let Ok(v) = std::env::var("XDG_RUNTIME_DIR") {
            command.env("XDG_RUNTIME_DIR", v);
        }

        // GTK4 bars may render via EGL buffers on some setups. When the compositor's renderer
        // can't import those buffers (common with certain driver stacks), the bar can become
        // invisible while still receiving input. Default to the cairo renderer unless the user
        // explicitly chose another one.
        if CONFIG.load().status_bar_name() == "gtk_bar" {
            if std::env::var_os("GSK_RENDERER").is_none() {
                command.env("GSK_RENDERER", "cairo");
            }
        }

        // 4. 执行启动并更新时间戳
        match command
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(child) => {
                info!("Spawning status bar (PID: {})", child.id());
                self.status_bar_child = Some(child);
                self.status_bar_last_spawn = Some(now);
                self.status_bar_backoff_until = None;
            }
            Err(e) => {
                error!("Failed to spawn status bar: {}", e);

                self.status_bar_restart_failures = self.status_bar_restart_failures.saturating_add(1);
                let pow = self.status_bar_restart_failures.min(6);
                let base_ms = 200u64;
                let backoff_ms = base_ms.saturating_mul(1u64 << pow).min(10_000);
                self.status_bar_backoff_until = Some(now + std::time::Duration::from_millis(backoff_ms));
            }
        }
    }

    fn restack(
        &mut self,
        backend: &mut dyn Backend,
        mon_key_opt: Option<MonitorKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[restack]");

        let mon_key = mon_key_opt.ok_or("Monitor is required for restack operation")?;
        let monitor = self
            .state
            .monitors
            .get(mon_key)
            .ok_or("Monitor not found")?;
        let monitor_num = monitor.num;

        let stack = self.get_monitor_stack(mon_key);

        let mut tiled_bottom_to_top: Vec<WindowId> = Vec::new();
        let mut floating_bottom_to_top: Vec<WindowId> = Vec::new();
        let mut pip_bottom_to_top: Vec<WindowId> = Vec::new();

        for &ck in stack.iter().rev() {
            if let Some(c) = self.state.clients.get(ck) {
                if !self.is_client_visible_on_monitor(ck, mon_key) {
                    continue;
                }
                if c.state.is_pip {
                    pip_bottom_to_top.push(c.win);
                } else if c.state.is_floating {
                    floating_bottom_to_top.push(c.win);
                } else {
                    tiled_bottom_to_top.push(c.win);
                }
            }
        }

        // Promote selected window to top of its layer, and if it's tiled,
        // raise it above floating windows so it's not obscured.
        let sel_win = monitor.sel.and_then(|ck| self.state.clients.get(ck)).map(|c| (c.win, c.state.is_floating, c.state.is_pip));

        let mut final_bottom_to_top: Vec<WindowId> =
            Vec::with_capacity(tiled_bottom_to_top.len() + floating_bottom_to_top.len() + pip_bottom_to_top.len());

        if let Some((win, is_floating, is_pip)) = sel_win {
            if is_pip {
                // PiP: promote within pip layer
                if let Some(idx) = pip_bottom_to_top.iter().position(|&w| w == win) {
                    let w = pip_bottom_to_top.remove(idx);
                    pip_bottom_to_top.push(w);
                }
                final_bottom_to_top.extend(tiled_bottom_to_top);
                final_bottom_to_top.extend(floating_bottom_to_top);
                final_bottom_to_top.extend(pip_bottom_to_top);
            } else if is_floating {
                // Floating: promote to top of floating layer (above other floats, below pip)
                if let Some(idx) = floating_bottom_to_top.iter().position(|&w| w == win) {
                    let w = floating_bottom_to_top.remove(idx);
                    floating_bottom_to_top.push(w);
                }
                final_bottom_to_top.extend(tiled_bottom_to_top);
                final_bottom_to_top.extend(floating_bottom_to_top);
                final_bottom_to_top.extend(pip_bottom_to_top);
            } else {
                // Tiled: raise focused tiled window above all floats so it's not obscured
                tiled_bottom_to_top.retain(|&w| w != win);
                final_bottom_to_top.extend(tiled_bottom_to_top);
                final_bottom_to_top.extend(floating_bottom_to_top);
                final_bottom_to_top.push(win); // focused tiled above floats
                final_bottom_to_top.extend(pip_bottom_to_top);
            }
        } else {
            final_bottom_to_top.extend(tiled_bottom_to_top);
            final_bottom_to_top.extend(floating_bottom_to_top);
            final_bottom_to_top.extend(pip_bottom_to_top);
        }

        let need_restack_windows = match self.last_stacking.get(mon_key) {
            Some(prev) => prev.as_slice() != final_bottom_to_top.as_slice(),
            None => true,
        };

        if need_restack_windows {
            backend.window_ops().restack_windows(&final_bottom_to_top)?;
            self.last_stacking
                .insert(mon_key, final_bottom_to_top.clone());
        }

        if self.current_bar_monitor_id == Some(monitor_num) {
            if let Some(bar_key) = self.status_bar_client {
                if let Some(bar_client) = self.state.clients.get(bar_key) {
                    let show_bar = monitor
                        .pertag
                        .as_ref()
                        .and_then(|p| p.show_bars.get(p.cur_tag))
                        .copied()
                        .unwrap_or(true);
                    if show_bar {
                        let changes = WindowChanges {
                            stack_mode: Some(StackMode::Above),
                            ..Default::default()
                        };
                        backend
                            .window_ops()
                            .apply_window_changes(bar_client.win, changes)?;
                    }
                }
            }
        }

        self.mark_bar_update_needed_if_visible(Some(monitor_num));

        info!("[restack] finish");
        Ok(())
    }

    fn flush_pending_bar_updates(&mut self) {
        if self.pending_bar_updates.is_empty() {
            return;
        }
        let target_mon_id = self
            .current_bar_monitor_id
            .or_else(|| {
                self.state
                    .sel_mon
                    .and_then(|k| self.state.monitors.get(k))
                    .map(|m| m.num)
            })
            .or_else(|| self.pending_bar_updates.iter().copied().next());
        if let Some(mon_id) = target_mon_id {
            if let Some(mon_key) = self.get_monitor_by_id(mon_id) {
                if !self.is_bar_visible_on_mon(mon_key) {
                    self.pending_bar_updates.clear();
                    return;
                }
                self.update_bar_message_for_monitor(Some(mon_key));
                if let Some(rb) = self.status_bar_shmem.as_mut() {
                    let _ = rb.try_write_message(&self.message);
                }
            }
        }
        self.pending_bar_updates.clear();
    }

    pub fn run(&mut self, backend: &mut dyn Backend) -> Result<(), Box<dyn std::error::Error>> {
        info!("[run] Handing over control to backend");
        Ok(backend.run(self)?)
    }

    fn process_commands_from_status_bar(&mut self, backend: &mut dyn Backend) {
        let mut commands_to_process: Vec<SharedCommand> = Vec::new();
        if let Some(buffer) = self.status_bar_shmem.as_mut() {
            while let Some(cmd) = buffer.receive_command() {
                commands_to_process.push(cmd);
            }
        }
        for cmd in commands_to_process {
            match cmd.cmd_type.into() {
                CommandType::ViewTag => {
                    info!(
                        "[process_commands] ViewTag command received: {}",
                        cmd.parameter
                    );
                    let arg = WMArgEnum::UInt(cmd.parameter);
                    let _ = self.view(backend, &arg);
                }
                CommandType::ToggleTag => {
                    info!(
                        "[process_commands] ToggleTag command received: {}",
                        cmd.parameter
                    );
                    let arg = WMArgEnum::UInt(cmd.parameter);
                    let _ = self.toggletag(backend, &arg);
                }
                CommandType::SetLayout => {
                    info!(
                        "[process_commands] SetLayout command received: {}",
                        cmd.parameter
                    );
                    let arg = WMArgEnum::Layout(Rc::new(LayoutEnum::from(cmd.parameter)));
                    let _ = self.setlayout(backend, &arg);
                }
                CommandType::None => {}
            }
        }
    }

    fn get_transient_for(&self, backend: &mut dyn Backend, window: WindowId) -> Option<WindowId> {
        backend.property_ops().transient_for(window)
    }

    fn arrange(&mut self, backend: &mut dyn Backend, m_target: Option<MonitorKey>) {
        info!("[arrange]");

        let monitors_to_process: Vec<MonitorKey> = match m_target {
            Some(monitor_key) => vec![monitor_key],
            None => self.state.monitor_order.clone(),
        };

        for &mon_key in &monitors_to_process {
            self.showhide_monitor(backend, mon_key);
        }

        for &mon_key in &monitors_to_process {
            self.arrangemon(backend, mon_key);
            let _ = self.restack(backend, Some(mon_key));
        }
        let _ = backend.window_ops().flush();
    }

    fn getrootptr(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(i32, i32), Box<dyn std::error::Error>> {
        let (x, y) = backend.input_ops().get_pointer_position()?;
        Ok((x as i32, y as i32))
    }

    fn recttomon(&mut self, backend: &mut dyn Backend, x: i32, y: i32) -> Option<MonitorKey> {
        if let Some(output_id) = backend.output_ops().output_at(x, y) {
            for (mon_key, &oid) in &self.state.output_map {
                if oid == output_id {
                    return Some(mon_key);
                }
            }
        }
        self.state.sel_mon
    }

    fn wintomon(&mut self, backend: &mut dyn Backend, w: Option<WindowId>) -> Option<MonitorKey> {
        if w.is_none() || w == backend.root_window() {
            if let Ok((x, y)) = self.getrootptr(backend) {
                return self.recttomon(backend, x, y);
            }
            return self.state.sel_mon;
        }
        let win_id = match w {
            Some(id) => id,
            None => return self.state.sel_mon,
        };
        if let Some(client_key) = self.wintoclient(win_id) {
            if let Some(client) = self.state.clients.get(client_key) {
                return client.mon.or(self.state.sel_mon);
            }
        }
        self.state.sel_mon
    }

    /// Returns `true` if `backend` is one of the Smithay-based compositors (udev, wayland-x11, wayland-winit).
    fn is_smithay_backend(backend: &dyn Backend) -> bool {
        backend
            .as_any()
            .is::<crate::backend::wayland_udev::backend::UdevBackend>()
            || backend
                .as_any()
                .is::<crate::backend::wayland_x11::backend::WaylandX11Backend>()
            || backend
                .as_any()
                .is::<crate::backend::wayland_winit::backend::WaylandWinitBackend>()
    }

    /// Returns `true` if `backend` is the udev/KMS backend (no Xwayland, no X11 DISPLAY).
    fn is_udev_backend(backend: &dyn Backend) -> bool {
        backend
            .as_any()
            .is::<crate::backend::wayland_udev::backend::UdevBackend>()
    }

    /// Set Wayland-related environment variables on a child `Command` so that
    /// toolkits can connect to this compositor.  When running the udev backend
    /// we propagate the XWayland DISPLAY so X11 apps can connect.
    fn setup_smithay_child_env(command: &mut Command, backend: &dyn Backend) {
        if Self::is_smithay_backend(backend) {
            if let Ok(v) = std::env::var("WAYLAND_DISPLAY") {
                command.env("WAYLAND_DISPLAY", &v);
            }
            if let Ok(v) = std::env::var("XDG_RUNTIME_DIR") {
                command.env("XDG_RUNTIME_DIR", &v);
            }
            if std::env::var_os("XDG_SESSION_TYPE").is_none() {
                command.env("XDG_SESSION_TYPE", "wayland");
            }
            if std::env::var_os("WINIT_UNIX_BACKEND").is_none() {
                command.env("WINIT_UNIX_BACKEND", "wayland");
            }
        }
        if Self::is_udev_backend(backend) {
            // With XWayland running, DISPLAY is set to e.g. ":0" and is valid.
            // Propagate it so X11 apps can connect via XWayland.
            if let Ok(display) = std::env::var("DISPLAY") {
                command.env("DISPLAY", &display);
            }
        }
    }

    /// Apply common child-process isolation: `setsid()` + restore `SIGCHLD` default.
    fn apply_child_pre_exec(command: &mut Command) {
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
    }

    pub fn spawn(
        &mut self,
        _backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[spawn]");

        let mut mut_arg: WMArgEnum = arg.clone();
        if let WMArgEnum::StringVec(ref mut v) = mut_arg {
            info!("[spawn] spawning command: {:?}", v);

            let mut command = Command::new(&v[0]);
            command.args(&v[1..]);

            Self::setup_smithay_child_env(&mut command, _backend);

            command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());

            Self::apply_child_pre_exec(&mut command);

            match command.spawn() {
                Ok(child) => {
                    debug!(
                        "[spawn] successfully spawned process with PID: {}",
                        child.id()
                    );
                }
                Err(e) => {
                    error!("[spawn] failed to spawn command {:?}: {}", v, e);
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    pub fn show_keybindings(
        &mut self,
        _backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[show_keybindings]");

        let mut lines: Vec<String> = Vec::new();
        for kc in CONFIG.load().key_configs() {
            let mods = kc.modifier.join("+");
            let shortcut = if mods.is_empty() {
                kc.key.clone()
            } else {
                format!("{}+{}", mods, kc.key)
            };

            let desc = match kc.function.as_str() {
                "spawn" => match &kc.argument {
                    crate::config::ArgumentConfig::StringVec(v) => {
                        format!("spawn {}", v.first().map(|s| s.as_str()).unwrap_or(""))
                    }
                    _ => "spawn".to_string(),
                },
                "setlayout" => match &kc.argument {
                    crate::config::ArgumentConfig::String(s) => format!("layout: {}", s),
                    crate::config::ArgumentConfig::UInt(_) => "toggle layout".to_string(),
                    _ => "setlayout".to_string(),
                },
                "focusstack" => match &kc.argument {
                    crate::config::ArgumentConfig::Int(i) => {
                        if *i > 0 { "focus next".to_string() } else { "focus prev".to_string() }
                    }
                    _ => "focusstack".to_string(),
                },
                "incnmaster" => match &kc.argument {
                    crate::config::ArgumentConfig::Int(i) => {
                        if *i > 0 { "master +1".to_string() } else { "master -1".to_string() }
                    }
                    _ => "incnmaster".to_string(),
                },
                "setmfact" => match &kc.argument {
                    crate::config::ArgumentConfig::Float(f) => {
                        if *f > 0.0 { "mfact +".to_string() } else { "mfact -".to_string() }
                    }
                    _ => "setmfact".to_string(),
                },
                "view" | "tag" | "toggleview" | "toggletag" => {
                    match &kc.argument {
                        crate::config::ArgumentConfig::UInt(u) => format!("{} tag {}", kc.function, u),
                        _ => kc.function.clone(),
                    }
                },
                other => other.to_string(),
            };

            lines.push(format!("{:<28} {}", shortcut, desc));
        }

        // 添加 tag 快捷键说明
        let tags_len = CONFIG.load().tags_length();
        lines.push(format!("{:<28} {}", "Mod1+[1-9]", format!("view tag 1-{}", tags_len)));
        lines.push(format!("{:<28} {}", "Mod1+Shift+[1-9]", format!("move to tag 1-{}", tags_len)));
        lines.push(format!("{:<28} {}", "Mod1+Ctrl+[1-9]", format!("toggle view tag 1-{}", tags_len)));
        lines.push(format!("{:<28} {}", "Mod1+Ctrl+Shift+[1-9]", format!("toggle tag 1-{}", tags_len)));
        lines.push(format!("{:<28} {}", "Mod1+0", "view all tags"));

        let text = lines.join("\n");

        let cfg = CONFIG.load();
        let dmenu_font = cfg.dmenu_font();
        let mut command = Command::new("dmenu");
        command.args(["-l", &lines.len().to_string(), "-fn", dmenu_font, "-p", "Keybindings:"]);
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::inherit());

        Self::apply_child_pre_exec(&mut command);
        Self::setup_smithay_child_env(&mut command, _backend);

        match command.spawn() {
            Ok(mut child) => {
                if let Some(stdin) = child.stdin.take() {
                    use std::io::Write;
                    let mut stdin = stdin;
                    let _ = stdin.write_all(text.as_bytes());
                }
            }
            Err(e) => {
                error!("[show_keybindings] failed to spawn dmenu: {:?}", e);
                return Err(e.into());
            }
        }

        Ok(())
    }

    pub fn reap_zombies(&mut self) {
        // 使用 WNOHANG 循环回收所有已退出的子进程
        loop {
            match waitpid(None, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::Exited(pid, status)) => {
                    info!("Child process {} exited with status {}", pid, status);

                    // 检查是否是状态栏退出了，如果是，清理句柄以便重启
                    if let Some(child) = &self.status_bar_child {
                        if child.id() as i32 == pid.as_raw() {
                            warn!("Status bar process died.");
                            self.status_bar_child = None;
                        }
                    }
                }
                Ok(WaitStatus::Signaled(pid, sig, _)) => {
                    info!("Child process {} killed by signal {:?}", pid, sig);
                    if let Some(child) = &self.status_bar_child {
                        if child.id() as i32 == pid.as_raw() {
                            self.status_bar_child = None;
                        }
                    }
                }
                // StillAlive 表示还有子进程在运行，Break 退出循环
                Ok(WaitStatus::StillAlive) => break,
                // Err 通常表示没有子进程了 (ECHILD)，也退出循环
                Err(_) => break,
                _ => break,
            }
        }
    }

    fn tile(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        info!("[tile] via pure layout engine");

        // 1. 准备数据
        let (wx, wy, ww, wh, mfact, nmaster, _monitor_num, _client_y_offset) =
            self.get_monitor_info(mon_key);

        // 计算可用区域 (优先使用 statusbar 的真实几何)
        let screen_area = self
            .monitor_work_area(mon_key)
            .unwrap_or(Rect::new(wx, wy, ww, wh));

        // 获取需要布局的客户端
        let raw_clients = self.collect_tileable_clients(mon_key);
        if raw_clients.is_empty() {
            return;
        }

        let (_effective_border, effective_gap) = self.apply_smart_borders(&raw_clients);
        let default_border = CONFIG.load().border_px() as i32;

        // 转换为纯数据结构 LayoutClient
        let layout_clients: Vec<LayoutClient<ClientKey>> = raw_clients
            .iter()
            .map(|&(key, factor, _)| LayoutClient {
                key,
                factor,
                border_w: self.state.clients.get(key).map(|c| c.geometry.border_w).unwrap_or(default_border),
            })
            .collect();

        // 2. 调用纯计算逻辑 (无副作用)
        let params = LayoutParams {
            screen_area,
            n_master: nmaster,
            m_fact: mfact,
            gap: effective_gap,
        };
        let results = layout::calculate_tile(&params, &layout_clients);

        // 3. 应用结果 (执行副作用：移动窗口)
        for res in results {
            self.resize_client(
                backend, res.key, res.rect.x, res.rect.y, res.rect.w, res.rect.h, false,
            );
        }
    }

    fn get_monitor_info(&self, mon_key: MonitorKey) -> (i32, i32, i32, i32, f32, u32, i32, i32) {
        if let Some(monitor) = self.state.monitors.get(mon_key) {
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
            (0, 0, 0, 0, 0.55, 1, 0, 0)
        }
    }

    /// Apply smart borders: single tiled window gets no border/gap;
    /// multiple tiled windows get the configured border and gap.
    fn apply_smart_borders(&mut self, clients: &[(ClientKey, f32, i32)]) -> (i32, i32) {
        let is_single = clients.len() == 1;
        let default_border = CONFIG.load().border_px() as i32;
        let effective_border = if is_single { 0 } else { default_border };
        let effective_gap = if is_single { 0 } else { CONFIG.load().gap_px() as i32 };
        for &(key, _, _) in clients {
            if let Some(client) = self.state.clients.get_mut(key) {
                client.geometry.border_w = effective_border;
            }
        }
        (effective_border, effective_gap)
    }

    fn collect_tileable_clients(&self, mon_key: MonitorKey) -> Vec<(ClientKey, f32, i32)> {
        let client_list = self.get_monitor_clients(mon_key);
        let mut clients = Vec::new();
        for &client_key in client_list {
            if let Some(client) = self.state.clients.get(client_key) {
                if !client.state.is_floating
                    && self.is_client_visible_on_monitor(client_key, mon_key)
                {
                    clients.push((client_key, client.state.client_fact, client.geometry.border_w));
                }
            }
        }
        clients
    }

    fn get_client_y_offset(&self, monitor: &WMMonitor) -> i32 {
        let show_bar = monitor
            .pertag
            .as_ref()
            .and_then(|p| p.show_bars.get(p.cur_tag))
            .copied()
            .unwrap_or(true);

        if show_bar {
            // Prefer the actual status bar geometry if we have it.
            // This is important for Wayland, where the bar may be a layer-shell surface
            // and its real size/position comes from the compositor arrangement.
            let fallback = CONFIG.load().status_bar_height() + CONFIG.load().status_bar_padding() * 2;
            let pad = CONFIG.load().status_bar_padding().max(0);

            if self.current_bar_monitor_id == Some(monitor.num) {
                if let Some(bar_key) = self.status_bar_client {
                    if let Some(bar) = self.state.clients.get(bar_key) {
                        let gap_from_top = (bar.geometry.y - monitor.geometry.w_y).max(0);
                        let dynamic = gap_from_top + bar.geometry.h + pad;
                        return dynamic.max(fallback);
                    }
                }
            }

            fallback
        } else {
            0
        }
    }

    fn monitor_work_area(&self, mon_key: MonitorKey) -> Option<Rect> {
        let monitor = self.state.monitors.get(mon_key)?;

        let debug_workarea = std::env::var("JWM_DEBUG_WORKAREA")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let wx = monitor.geometry.w_x;
        let wy = monitor.geometry.w_y;
        let ww = monitor.geometry.w_w;
        let wh = monitor.geometry.w_h;

        let show_bar = monitor
            .pertag
            .as_ref()
            .and_then(|p| p.show_bars.get(p.cur_tag))
            .copied()
            .unwrap_or(true);
        if !show_bar {
            return Some(Rect::new(wx, wy, ww, wh));
        }

        // Subtract all visible dock-like clients (includes Wayland layer-shell panels).
        let mut top = 0i32;
        let mut bottom = 0i32;
        let mut left = 0i32;
        let mut right = 0i32;

        let pad = CONFIG.load().status_bar_padding().max(0);
        let threshold = pad.max(8);

        if let Some(client_keys) = self.state.monitor_clients.get(mon_key) {
            for &client_key in client_keys {
                if Some(client_key) == self.status_bar_client {
                    // status bar is included via is_dock anyway; keep behavior consistent.
                }

                let client = match self.state.clients.get(client_key) {
                    Some(c) => c,
                    None => continue,
                };

                if !client.state.is_dock {
                    continue;
                }
                if !self.is_client_visible_on_monitor(client_key, mon_key) {
                    continue;
                }

                // Hidden bars use negative coordinates.
                if client.geometry.x <= -900 || client.geometry.y <= -900 {
                    continue;
                }

                // Compute dock rect in monitor coordinates.
                let dx = client.geometry.x;
                let dy = client.geometry.y;
                let dw = client.geometry.w.max(0);
                let dh = client.geometry.h.max(0);

                // Skip degenerate geometry.
                if dw == 0 || dh == 0 {
                    continue;
                }

                // Ignore wallpaper / background-like surfaces that cover (almost) the entire
                // monitor. Some layer-shell backgrounds may appear as "dock" due to
                // exclusive_zone semantics, but they must not shrink the tiling area.
                if dw >= (ww * 9 / 10) && dh >= (wh * 9 / 10) {
                    if debug_workarea {
                        info!(
                            "[workarea] skip fullscreen dock win={:?} geom=({},{} {}x{}) ww={} wh={}",
                            client.win,
                            dx,
                            dy,
                            dw,
                            dh,
                            ww,
                            wh
                        );
                    }
                    continue;
                }

                // Distances to edges (clamped).
                let dist_top = (dy - wy).abs();
                let dist_bottom = ((wy + wh) - (dy + dh)).abs();
                let dist_left = (dx - wx).abs();
                let dist_right = ((wx + ww) - (dx + dw)).abs();

                // Heuristic classification: prefer horizontal vs vertical panels.
                let is_horizontal = dw >= (ww * 2 / 3) && dh <= (wh / 2).max(1);
                let is_vertical = dh >= (wh * 2 / 3) && dw <= (ww / 2).max(1);

                let edge = if is_horizontal {
                    if dist_top <= dist_bottom { "top" } else { "bottom" }
                } else if is_vertical {
                    if dist_left <= dist_right { "left" } else { "right" }
                } else {
                    // Pick the closest edge.
                    let min = dist_top.min(dist_bottom).min(dist_left).min(dist_right);
                    if min == dist_top {
                        "top"
                    } else if min == dist_bottom {
                        "bottom"
                    } else if min == dist_left {
                        "left"
                    } else {
                        "right"
                    }
                };

                let exclusive_zone = client
                    .state
                    .dock_layer_info
                    .as_ref()
                    .map(|i| i.exclusive_zone)
                    .unwrap_or(0);

                let anchor_ok = client.state.dock_layer_info.as_ref().map(|i| {
                    let any = i.anchor_top || i.anchor_bottom || i.anchor_left || i.anchor_right;
                    if !any {
                        return true;
                    }
                    match edge {
                        "top" => i.anchor_top,
                        "bottom" => i.anchor_bottom,
                        "left" => i.anchor_left,
                        "right" => i.anchor_right,
                        _ => true,
                    }
                }).unwrap_or(true);

                let zone_px = if exclusive_zone == -1 {
                    match edge {
                        "top" | "bottom" => dh,
                        "left" | "right" => dw,
                        _ => 0,
                    }
                } else if exclusive_zone > 0 {
                    exclusive_zone
                } else {
                    0
                };

                if debug_workarea {
                    info!(
                        "[workarea] dock win={:?} edge={} geom=({},{} {}x{}) exclusive_zone={} zone_px={} dist(top/bot/left/right)=({}/{}/{}/{})",
                        client.win,
                        edge,
                        dx,
                        dy,
                        dw,
                        dh,
                        exclusive_zone,
                        zone_px,
                        dist_top,
                        dist_bottom,
                        dist_left,
                        dist_right
                    );
                }

                match edge {
                    "top" => {
                        if dist_top <= threshold {
                            if zone_px > 0 && anchor_ok {
                                top = top.max(zone_px + pad);
                            } else {
                                top = top.max((dy + dh - wy) + pad);
                            }
                        }
                    }
                    "bottom" => {
                        if dist_bottom <= threshold {
                            if zone_px > 0 && anchor_ok {
                                bottom = bottom.max(zone_px + pad);
                            } else {
                                bottom = bottom.max(((wy + wh) - dy) + pad);
                            }
                        }
                    }
                    "left" => {
                        if dist_left <= threshold {
                            if zone_px > 0 && anchor_ok {
                                left = left.max(zone_px + pad);
                            } else {
                                left = left.max((dx + dw - wx) + pad);
                            }
                        }
                    }
                    "right" => {
                        if dist_right <= threshold {
                            if zone_px > 0 && anchor_ok {
                                right = right.max(zone_px + pad);
                            } else {
                                right = right.max(((wx + ww) - dx) + pad);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // If we didn't observe any dock window yet, keep the historical top offset.
        if top == 0 && bottom == 0 && left == 0 && right == 0 {
            top = self.get_client_y_offset(monitor);
        }

        let x = wx + left;
        let y = wy + top;
        let w = (ww - left - right).max(0);
        let h = (wh - top - bottom).max(0);

        if debug_workarea {
            info!(
                "[workarea] result mon={} wx/wy/ww/wh=({},{},{},{}) offsets(top/bot/left/right)=({},{},{},{}) -> ({},{},{},{})",
                monitor.num,
                wx,
                wy,
                ww,
                wh,
                top,
                bottom,
                left,
                right,
                x,
                y,
                w,
                h
            );
        }
        Some(Rect::new(x, y, w, h))
    }

    pub fn togglefloating(
        &mut self,
        backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[togglefloating]");
        let Some(sel_mon_key) = self.state.sel_mon else {
            return Ok(());
        };
        let Some(sel_client_key) = self.state.monitors.get(sel_mon_key).and_then(|m| m.sel) else {
            return Ok(());
        };
        let geom = if let Some(client) = self.state.clients.get_mut(sel_client_key) {
            client.state.is_floating = !client.state.is_floating;
            if client.state.is_floating {
                if client.geometry.floating_w <= 0 || client.geometry.floating_h <= 0 {
                    client.geometry.floating_x = client.geometry.x;
                    client.geometry.floating_y = client.geometry.y;
                    client.geometry.floating_w = client.geometry.w;
                    client.geometry.floating_h = client.geometry.h;
                }
                Some((
                    client.geometry.floating_x,
                    client.geometry.floating_y,
                    client.geometry.floating_w,
                    client.geometry.floating_h,
                ))
            } else {
                client.geometry.floating_x = client.geometry.x;
                client.geometry.floating_y = client.geometry.y;
                client.geometry.floating_w = client.geometry.w;
                client.geometry.floating_h = client.geometry.h;
                None
            }
        } else {
            return Ok(());
        };

        if let Some((x, y, w, h)) = geom {
            self.resize_client(backend, sel_client_key, x, y, w, h, false);
        }

        self.reorder_client_in_monitor_groups(sel_client_key);

        self.arrange(backend, Some(sel_mon_key));
        Ok(())
    }

    pub fn togglesticky(
        &mut self,
        backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(sel_mon_key) = self.state.sel_mon else {
            return Ok(());
        };
        let Some(sel_client_key) = self.state.monitors.get(sel_mon_key).and_then(|m| m.sel) else {
            return Ok(());
        };
        if let Some(client) = self.state.clients.get_mut(sel_client_key) {
            client.state.is_sticky = !client.state.is_sticky;
            if client.state.is_sticky {
                // Ensure sticky client has current monitor tags
                if let Some(monitor) = self.state.monitors.get(sel_mon_key) {
                    let current_tags = monitor.tag_set[monitor.sel_tags];
                    if let Some(client) = self.state.clients.get_mut(sel_client_key) {
                        client.state.tags = current_tags;
                    }
                }
            }
        }
        self.arrange(backend, Some(sel_mon_key));
        Ok(())
    }

    fn update_sticky_tags(&mut self, mon_key: MonitorKey) {
        let new_tags = if let Some(monitor) = self.state.monitors.get(mon_key) {
            monitor.tag_set[monitor.sel_tags]
        } else {
            return;
        };
        let client_keys: Vec<ClientKey> = self
            .state
            .monitor_clients
            .get(mon_key)
            .map(|keys| keys.clone())
            .unwrap_or_default();
        for ck in client_keys {
            if let Some(client) = self.state.clients.get_mut(ck) {
                if client.state.is_sticky {
                    client.state.tags = new_tags;
                }
            }
        }
    }

    /// Toggle a named scratchpad.
    ///
    /// Argument encoding (via `StringVec`):
    ///   `["name", "cmd", "arg1", ...]`  — name + spawn command
    ///   `["name"]`                      — name only (uses default terminal)
    ///
    /// Legacy `Int(0)` falls back to the default name `"term"`.
    pub fn togglescratchpad(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Parse name and optional command from argument
        let (name, spawn_cmd) = match arg {
            WMArgEnum::StringVec(v) if !v.is_empty() => {
                let name = v[0].clone();
                let cmd = if v.len() > 1 {
                    v[1..].to_vec()
                } else {
                    crate::config::Config::get_termcmd()
                };
                (name, cmd)
            }
            _ => ("term".to_string(), crate::config::Config::get_termcmd()),
        };

        // Check if the scratchpad's client still exists
        if let Some(&sp_key) = self.scratchpads.get(&name) {
            if self.state.clients.get(sp_key).is_none() {
                self.scratchpads.remove(&name);
            }
        }

        if let Some(&sp_key) = self.scratchpads.get(&name) {
            // Scratchpad exists — toggle visibility
            let is_visible = self.is_client_visible_by_key(sp_key);
            if is_visible {
                // Hide: set tags to 0
                if let Some(client) = self.state.clients.get_mut(sp_key) {
                    client.state.tags = 0;
                }
                let mon_key = self.state.clients.get(sp_key).and_then(|c| c.mon);
                self.focus(backend, None)?;
                if let Some(mk) = mon_key {
                    self.arrange(backend, Some(mk));
                }
            } else {
                // Show: move to current monitor and tags
                let sel_mon_key = self.state.sel_mon;
                if let Some(mon_key) = sel_mon_key {
                    let current_tags = self
                        .state
                        .monitors
                        .get(mon_key)
                        .map(|m| m.tag_set[m.sel_tags])
                        .unwrap_or(1);

                    if let Some(client) = self.state.clients.get_mut(sp_key) {
                        client.state.tags = current_tags;
                        client.mon = Some(mon_key);
                        client.state.is_floating = true;
                    }

                    self.reorder_client_in_monitor_groups(sp_key);

                    // Center at 80% of monitor work area
                    if let Some(area) = self.monitor_work_area(mon_key) {
                        let w = (area.w as f32 * 0.8) as i32;
                        let h = (area.h as f32 * 0.8) as i32;
                        let x = area.x + (area.w - w) / 2;
                        let y = area.y + (area.h - h) / 2;
                        self.resize_client(backend, sp_key, x, y, w, h, false);
                    }

                    self.focus(backend, Some(sp_key))?;
                    self.arrange(backend, Some(mon_key));
                }
            }
        } else {
            // No scratchpad with this name — spawn command, mark pending
            if let Some(prog) = spawn_cmd.first() {
                let mut command = Command::new(prog);
                command.args(&spawn_cmd[1..]);

                Self::setup_smithay_child_env(&mut command, backend);
                command
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
                Self::apply_child_pre_exec(&mut command);

                match command.spawn() {
                    Ok(child) => {
                        info!(
                            "[togglescratchpad] spawned '{}' PID: {}",
                            name,
                            child.id()
                        );
                        self.scratchpad_pending_name = Some(name);
                    }
                    Err(e) => {
                        error!("[togglescratchpad] failed to spawn '{}': {}", name, e);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn togglepip(
        &mut self,
        backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(sel_mon_key) = self.state.sel_mon else {
            return Ok(());
        };
        let Some(sel_client_key) = self.state.monitors.get(sel_mon_key).and_then(|m| m.sel) else {
            return Ok(());
        };

        let is_pip = self
            .state
            .clients
            .get(sel_client_key)
            .map(|c| c.state.is_pip)
            .unwrap_or(false);

        if is_pip {
            // Exit PiP: restore state
            if let Some(client) = self.state.clients.get_mut(sel_client_key) {
                client.state.is_pip = false;
                client.state.is_floating = client.state.old_state;
                client.state.is_sticky = false;
            }
            self.reorder_client_in_monitor_groups(sel_client_key);
            let (fx, fy, fw, fh) = if let Some(client) = self.state.clients.get(sel_client_key) {
                (
                    client.geometry.floating_x,
                    client.geometry.floating_y,
                    client.geometry.floating_w,
                    client.geometry.floating_h,
                )
            } else {
                return Ok(());
            };
            if fw > 0 && fh > 0 {
                self.resize_client(backend, sel_client_key, fx, fy, fw, fh, false);
            }
            self.arrange(backend, Some(sel_mon_key));
        } else {
            // Enter PiP: save state, shrink to bottom-right
            if let Some(client) = self.state.clients.get_mut(sel_client_key) {
                client.state.old_state = client.state.is_floating;
                client.geometry.floating_x = client.geometry.x;
                client.geometry.floating_y = client.geometry.y;
                client.geometry.floating_w = client.geometry.w;
                client.geometry.floating_h = client.geometry.h;
                client.state.is_pip = true;
                client.state.is_floating = true;
                client.state.is_sticky = true;
            }

            self.reorder_client_in_monitor_groups(sel_client_key);

            // Position at bottom-right, 25% of monitor, 10px padding
            if let Some(area) = self.monitor_work_area(sel_mon_key) {
                let w = (area.w as f32 * 0.25) as i32;
                let h = (area.h as f32 * 0.25) as i32;
                let x = area.x + area.w - w - 10;
                let y = area.y + area.h - h - 10;
                self.resize_client(backend, sel_client_key, x, y, w, h, false);
            }

            self.arrange(backend, Some(sel_mon_key));
            self.restack(backend, Some(sel_mon_key))?;
        }

        Ok(())
    }

    fn focusin(
        &mut self,
        backend: &mut dyn Backend,
        event_window: WindowId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[focusin] Window {:?} got focus", event_window);
        let sel_client_key = self.get_selected_client_key();
        if let Some(client_key) = sel_client_key {
            if let Some(client) = self.state.clients.get(client_key) {
                if event_window != client.win {
                    if self.wintoclient(event_window).is_some() {
                        self.setfocus(backend, client_key)?;
                    } else {
                        // 是未知窗口（可能是输入法、系统弹窗等），允许它持有焦点
                        // 不要调用 setfocus
                        // debug!("Focus stolen by unmanaged window, ignoring allow...");
                    }
                }
            }
        }
        Ok(())
    }

    pub fn focusmon(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.state.monitor_order.len() <= 1 {
            return Ok(());
        }

        if let WMArgEnum::Int(i) = arg {
            if let Some(target_mon_key) = self.dirtomon(i) {
                if Some(target_mon_key) == self.state.sel_mon {
                    return Ok(());
                }
                self.switch_to_monitor(backend, target_mon_key)?;
                self.focus(backend, None)?;

                let mon_num = self.state.monitors.get(target_mon_key).map(|m| m.num);
                if let Some(num) = mon_num {
                    self.broadcast_ipc_event("monitor/focus", serde_json::json!({
                        "monitor": num,
                    }));
                }
            }
        }
        Ok(())
    }

    pub fn take_screenshot(
        &mut self,
        _backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let pictures_dir = std::env::var("XDG_PICTURES_DIR")
            .or_else(|_| std::env::var("HOME").map(|h| format!("{}/Pictures", h)))
            .unwrap_or_else(|_| "/tmp".to_string());
        let screenshot_path = format!("{}/screenshot-{}.png", pictures_dir, timestamp);

        if Self::is_udev_backend(_backend) {
            // Use grim + slurp for interactive region selection (requires wlr-screencopy
            // and wlr-layer-shell which this compositor now supports).
            // The shell command: grim -g "$(slurp)" <path>
            info!("[take_screenshot] launching grim + slurp → {}", screenshot_path);

            let mut command = Command::new("sh");
            command.args(["-c", &format!(
                "grim -g \"$(slurp)\" '{}'",
                screenshot_path,
            )]);

            Self::setup_smithay_child_env(&mut command, _backend);

            command
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            Self::apply_child_pre_exec(&mut command);

            match command.spawn() {
                Ok(child) => {
                    debug!("[take_screenshot] spawned grim+slurp PID: {}", child.id());
                }
                Err(e) => {
                    error!("[take_screenshot] failed to launch grim+slurp: {}", e);
                    // Fall back to compositor-level full-screen screenshot.
                    let path = std::path::PathBuf::from(&screenshot_path);
                    if let Ok(true) = _backend.take_screenshot_to_file(&path) {
                        info!("[take_screenshot] fallback compositor screenshot → {}", path.display());
                    }
                }
            }
            return Ok(());
        }

        // Non-udev backends: try compositor screenshot, then fallback to flameshot.
        let path = std::path::PathBuf::from(&screenshot_path);
        match _backend.take_screenshot_to_file(&path) {
            Ok(true) => {
                info!("[take_screenshot] compositor screenshot → {}", path.display());
                return Ok(());
            }
            Ok(false) => {
                info!("[take_screenshot] backend doesn't support compositor screenshots, falling back to flameshot");
            }
            Err(e) => {
                error!("[take_screenshot] compositor screenshot failed: {e}, falling back to flameshot");
            }
        }

        // Fallback: launch flameshot via XWayland.
        let program = "flameshot";
        let args = vec!["gui".to_string()];

        info!("[take_screenshot] launching: {} {:?}", program, args);

        let mut command = Command::new(program);
        command.args(&args);

        Self::setup_smithay_child_env(&mut command, _backend);

        command.env_remove("WAYLAND_DISPLAY");
        command.env("QT_QPA_PLATFORM", "xcb");

        command
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        Self::apply_child_pre_exec(&mut command);

        match command.spawn() {
            Ok(child) => {
                debug!("[take_screenshot] spawned PID: {}", child.id());
            }
            Err(e) => {
                error!("[take_screenshot] failed to launch {}: {}", program, e);
            }
        }
        Ok(())
    }

    pub fn tag(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[tag]");
        if let WMArgEnum::UInt(ui) = *arg {
            let sel_client_key = self.get_selected_client_key();
            let target_tag = ui & CONFIG.load().tagmask();

            if let Some(client_key) = sel_client_key {
                if target_tag > 0 {
                    if let Some(client) = self.state.clients.get_mut(client_key) {
                        client.state.tags = target_tag;
                    }
                    let _ = self.setclienttagprop(backend, client_key);

                    self.focus(backend, None)?;
                    self.arrange(backend, self.state.sel_mon);
                }
            }
        }
        Ok(())
    }

    pub fn tagmon(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[tagmon]");

        let sel_client_key = self.get_selected_client_key();
        if sel_client_key.is_none() {
            return Ok(());
        }
        if self.state.monitor_order.len() <= 1 {
            return Ok(());
        }
        if let WMArgEnum::Int(i) = *arg {
            let target_mon = self.dirtomon(&i);
            if let (Some(client_key), Some(target_mon_key)) = (sel_client_key, target_mon) {
                self.sendmon(backend, Some(client_key), Some(target_mon_key));
            }
        }
        Ok(())
    }

    fn sendmon(
        &mut self,
        backend: &mut dyn Backend,
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

        if let Some(client) = self.state.clients.get(client_key) {
            if client.mon == Some(target_mon_key) {
                return;
            }
        } else {
            return;
        }

        let _ = self.unfocus_client(backend, client_key, true);

        self.detach(client_key);
        self.detachstack(client_key);

        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.mon = Some(target_mon_key);
        }

        if let Some(target_monitor) = self.state.monitors.get(target_mon_key) {
            let target_tags = target_monitor.tag_set[target_monitor.sel_tags];

            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.state.tags = target_tags;
            }
        }

        self.attach_back(client_key);
        self.attachstack(client_key);

        let _ = self.setclienttagprop(backend, client_key);

        let _ = self.focus(backend, None);
        self.arrange(backend, None);
    }

    pub fn focusstack(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let direction = match *arg {
            WMArgEnum::Int(i) => i,
            _ => return Ok(()),
        };

        if direction == 0 {
            return Ok(());
        }

        if !self.can_focus_switch()? {
            return Ok(());
        }

        let target_client = if direction > 0 {
            self.find_next_visible_client()?
        } else {
            self.find_previous_visible_client()?
        };

        if let Some(client_key) = target_client {
            self.focus(backend, Some(client_key))?;
            self.restack(backend, self.state.sel_mon)?;
            self.suppress_mouse_focus_until =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(200));
        }
        Ok(())
    }

    fn can_focus_switch(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let sel_client_key = self.get_selected_client_key().ok_or("No selected client")?;

        if let Some(client) = self.state.clients.get(sel_client_key) {
            let is_locked_fullscreen =
                client.state.is_fullscreen && CONFIG.load().behavior().lock_fullscreen;
            Ok(!is_locked_fullscreen)
        } else {
            Err("Selected client not found".into())
        }
    }

    fn find_next_visible_client(&self) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.state.sel_mon.ok_or("No selected monitor")?;
        let current_sel = self.get_selected_client_key().ok_or("No selected client")?;
        let (tile_clients, floating_clients) = self.grouped_visible_clients(sel_mon_key);
        let current_is_floating = self
            .state
            .clients
            .get(current_sel)
            .map(|client| client.state.is_floating)
            .unwrap_or(false);

        let (current_group, other_group) = if current_is_floating {
            (&floating_clients, &tile_clients)
        } else {
            (&tile_clients, &floating_clients)
        };

        if let Some(next) = Self::next_in_group(current_group, current_sel) {
            return Ok(Some(next));
        }

        if let Some(next) = other_group.first().copied() {
            return Ok(Some(next));
        }

        // Wrap around to the first of current group
        if let Some(next) = current_group.first().copied() {
            if next != current_sel {
                return Ok(Some(next));
            }
        }

        Ok(None)
    }

    fn find_previous_visible_client(
        &self,
    ) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.state.sel_mon.ok_or("No selected monitor")?;
        let current_sel = self.get_selected_client_key().ok_or("No selected client")?;
        let (tile_clients, floating_clients) = self.grouped_visible_clients(sel_mon_key);
        let current_is_floating = self
            .state
            .clients
            .get(current_sel)
            .map(|client| client.state.is_floating)
            .unwrap_or(false);

        let (current_group, other_group) = if current_is_floating {
            (&floating_clients, &tile_clients)
        } else {
            (&tile_clients, &floating_clients)
        };

        if let Some(prev) = Self::prev_in_group(current_group, current_sel) {
            return Ok(Some(prev));
        }

        if let Some(prev) = other_group.last().copied() {
            return Ok(Some(prev));
        }

        // Wrap around to the last of current group
        if let Some(prev) = current_group.last().copied() {
            if prev != current_sel {
                return Ok(Some(prev));
            }
        }

        Ok(None)
    }

    fn grouped_visible_clients(&self, mon_key: MonitorKey) -> (Vec<ClientKey>, Vec<ClientKey>) {
        let total = self.state.monitor_clients.get(mon_key).map(|v| v.len()).unwrap_or(0);
        let mut tile_clients = Vec::with_capacity(total);
        let mut floating_clients = Vec::with_capacity(total / 4 + 1);

        if let Some(client_list) = self.state.monitor_clients.get(mon_key) {
            for &client_key in client_list {
                if !self.is_client_visible_on_monitor(client_key, mon_key) {
                    continue;
                }

                if let Some(client) = self.state.clients.get(client_key) {
                    if client.state.is_floating {
                        floating_clients.push(client_key);
                    } else {
                        tile_clients.push(client_key);
                    }
                }
            }
        }

        (tile_clients, floating_clients)
    }

    fn next_in_group(group: &[ClientKey], current_sel: ClientKey) -> Option<ClientKey> {
        group
            .iter()
            .position(|&k| k == current_sel)
            .and_then(|idx| group.get(idx + 1).copied())
    }

    fn prev_in_group(group: &[ClientKey], current_sel: ClientKey) -> Option<ClientKey> {
        group
            .iter()
            .position(|&k| k == current_sel)
            .and_then(|idx| idx.checked_sub(1).and_then(|prev_idx| group.get(prev_idx).copied()))
    }

    pub fn togglebar(
        &mut self,
        backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[togglebar]");

        let sel_mon_key = match self.state.sel_mon {
            Some(key) => key,
            None => return Ok(()),
        };

        let mut monitor_num_opt: Option<i32> = None;
        {
            if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
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
        }

        if let Some(mon_num) = monitor_num_opt {
            if self.current_bar_monitor_id == Some(mon_num) {
                self.position_statusbar_on_monitor(backend, mon_num)?;
                self.arrange(backend, Some(sel_mon_key));
                let _ = self.restack(backend, Some(sel_mon_key));
            }
            self.mark_bar_update_needed_if_visible(Some(mon_num));
        }

        Ok(())
    }

    fn refresh_bar_visibility_on_selected_monitor(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (sel_mon_key, mon_num) = match self.state.sel_mon {
            Some(k) => {
                if let Some(m) = self.state.monitors.get(k) {
                    (k, m.num)
                } else {
                    return Ok(());
                }
            }
            None => return Ok(()),
        };

        if self.current_bar_monitor_id == Some(mon_num) {
            self.position_statusbar_on_monitor(backend, mon_num)?;
            self.arrange(backend, Some(sel_mon_key));
            let _ = self.restack(backend, Some(sel_mon_key));
            self.mark_bar_update_needed_if_visible(Some(mon_num));
        }
        Ok(())
    }

    pub fn incnmaster(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let WMArgEnum::Int(i) = *arg {
            let sel_mon_key = self.state.sel_mon.ok_or("No monitor selected")?;

            if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
                let new_n = (monitor.layout.n_master as i32 + i).max(0) as u32;
                monitor.layout.n_master = new_n;
                // 关键：调用新方法同步状态
                monitor.update_current_tag_layout_params();
                info!("[incnmaster] Updated n_master to {}", new_n);
            }
            self.arrange(backend, Some(sel_mon_key));
        }
        Ok(())
    }

    pub fn setcfact(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[setcfact]");

        let client_key = match self.get_selected_client_key() {
            Some(k) => k,
            None => return Ok(()),
        };

        if let WMArgEnum::Float(f0) = *arg {
            let current_fact = if let Some(client) = self.state.clients.get(client_key) {
                client.state.client_fact
            } else {
                return Ok(());
            };

            let new_fact = if f0.abs() < 0.0001 {
                1.0
            } else {
                f0 + current_fact
            };

            if new_fact < 0.25 || new_fact > 4.0 {
                return Ok(());
            }

            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.state.client_fact = new_fact;
                info!(
                    "[setcfact] Updated client_fact to {} for client '{}'",
                    new_fact, client.name
                );
            }
            self.arrange(backend, self.state.sel_mon);
        }

        Ok(())
    }

    pub fn movestack(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[movestack]");
        let direction = match arg {
            WMArgEnum::Int(i) => *i,
            _ => return Ok(()),
        };

        let selected_client_key = self.get_selected_client_key().ok_or("No client selected")?;

        let target_client_key = if direction > 0 {
            self.find_next_tiled_client(selected_client_key)?
        } else {
            self.find_previous_tiled_client(selected_client_key)?
        };

        if let Some(target_key) = target_client_key {
            if selected_client_key != target_key {
                self.swap_clients_in_monitor(selected_client_key, target_key)?;

                self.arrange(backend, self.state.sel_mon);

                self.suppress_mouse_focus_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(200));
            }
        }

        Ok(())
    }

    fn is_tiled_and_visible(&self, client_key: ClientKey) -> bool {
        if let Some(client) = self.state.clients.get(client_key) {
            self.is_client_visible_by_key(client_key) && !client.state.is_floating
        } else {
            false
        }
    }

    fn find_next_tiled_client(
        &self,
        current_key: ClientKey,
    ) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.state.sel_mon.ok_or("No selected monitor")?;
        let client_list = self
            .state
            .monitor_clients
            .get(sel_mon_key)
            .ok_or("Monitor client list not found")?;

        let current_index = client_list
            .iter()
            .position(|&k| k == current_key)
            .ok_or("Current client not found in monitor list")?;

        for &client_key in &client_list[current_index + 1..] {
            if self.is_tiled_and_visible(client_key) {
                return Ok(Some(client_key));
            }
        }

        for &client_key in &client_list[..current_index] {
            if self.is_tiled_and_visible(client_key) {
                return Ok(Some(client_key));
            }
        }

        Ok(None)
    }

    fn find_previous_tiled_client(
        &self,
        current_key: ClientKey,
    ) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.state.sel_mon.ok_or("No selected monitor")?;
        let client_list = self
            .state
            .monitor_clients
            .get(sel_mon_key)
            .ok_or("Monitor client list not found")?;

        let current_index = client_list
            .iter()
            .position(|&k| k == current_key)
            .ok_or("Current client not found in monitor list")?;

        for &client_key in client_list[..current_index].iter().rev() {
            if self.is_tiled_and_visible(client_key) {
                return Ok(Some(client_key));
            }
        }

        for &client_key in client_list[current_index + 1..].iter().rev() {
            if self.is_tiled_and_visible(client_key) {
                return Ok(Some(client_key));
            }
        }

        Ok(None)
    }

    fn swap_clients_in_monitor(
        &mut self,
        client1_key: ClientKey,
        client2_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sel_mon_key = self.state.sel_mon.ok_or("No selected monitor")?;

        if let Some(client_list) = self.state.monitor_clients.get_mut(sel_mon_key) {
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

        if let Some(stack_list) = self.state.monitor_stack.get_mut(sel_mon_key) {
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

    pub fn setmfact(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let WMArgEnum::Float(f) = arg {
            let sel_mon_key = self.state.sel_mon.ok_or("No monitor selected")?;
            if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
                let new_mfact = if f < &1.0 {
                    f + monitor.layout.m_fact
                } else {
                    f - 1.0
                };
                if new_mfact >= 0.05 && new_mfact <= 0.95 {
                    monitor.layout.m_fact = new_mfact;
                    // 关键：调用新方法同步状态
                    monitor.update_current_tag_layout_params();
                }
            }
            self.arrange(backend, Some(sel_mon_key));
        }
        Ok(())
    }

    /// 退出当前 monitor 上所有全屏窗口的全屏状态
    fn exit_fullscreen_on_monitor(
        &mut self,
        backend: &mut dyn Backend,
        mon_key: MonitorKey,
    ) {
        let fs_clients: Vec<ClientKey> = self
            .state
            .monitor_clients
            .get(mon_key)
            .map(|keys| {
                keys.iter()
                    .copied()
                    .filter(|&ck| {
                        self.state
                            .clients
                            .get(ck)
                            .map(|c| c.state.is_fullscreen)
                            .unwrap_or(false)
                    })
                    .collect()
            })
            .unwrap_or_default();

        for ck in fs_clients {
            let _ = self.setfullscreen(backend, ck, false);
        }
    }

    pub fn setlayout(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[setlayout]");
        let sel_mon_key = self.state.sel_mon.ok_or("No selected monitor")?;

        let old_layout = self
            .state
            .monitors
            .get(sel_mon_key)
            .map(|m| m.lt[m.sel_lt].clone())
            .ok_or("No monitor")?;

        self.exit_fullscreen_on_monitor(backend, sel_mon_key);
        self.update_layout_selection(sel_mon_key, arg)?;

        let new_layout = self
            .state
            .monitors
            .get(sel_mon_key)
            .map(|m| m.lt[m.sel_lt].clone())
            .ok_or("No monitor")?;

        self.handle_fullscreen_layout_transition(backend, sel_mon_key, &old_layout, &new_layout)?;

        let (should_arrange, mon_num) = self.finalize_layout_update(sel_mon_key);

        if should_arrange {
            self.arrange(backend, Some(sel_mon_key));
        } else {
            self.mark_bar_update_needed_if_visible(mon_num);
        }

        self.broadcast_ipc_event("layout/set", serde_json::json!({
            "layout": format!("{:?}", *new_layout),
        }));

        Ok(())
    }

    pub fn cyclelayout(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[cyclelayout]");
        let sel_mon_key = self.state.sel_mon.ok_or("No selected monitor")?;

        let old_layout = self
            .state
            .monitors
            .get(sel_mon_key)
            .map(|m| m.lt[m.sel_lt].clone())
            .ok_or("No monitor")?;

        self.exit_fullscreen_on_monitor(backend, sel_mon_key);

        let dir = match arg {
            WMArgEnum::Int(i) => *i,
            _ => 1,
        };

        let cur_tag = self
            .state
            .monitors
            .get(sel_mon_key)
            .and_then(|m| m.pertag.as_ref())
            .map(|p| p.cur_tag)
            .ok_or("No pertag")?;

        let current = self
            .state
            .monitors
            .get(sel_mon_key)
            .map(|m| m.lt[m.sel_lt].clone())
            .ok_or("No monitor")?;

        let next = if dir >= 0 {
            current.cycle_next()
        } else {
            current.cycle_prev()
        };

        let next_rc = Rc::new(next.clone());
        self.set_new_layout(sel_mon_key, &next_rc, cur_tag);

        self.handle_fullscreen_layout_transition(backend, sel_mon_key, &old_layout, &next_rc)?;

        let (should_arrange, mon_num) = self.finalize_layout_update(sel_mon_key);
        if should_arrange {
            self.arrange(backend, Some(sel_mon_key));
        } else {
            self.mark_bar_update_needed_if_visible(mon_num);
        }

        self.broadcast_ipc_event("layout/set", serde_json::json!({
            "layout": format!("{:?}", next),
        }));

        Ok(())
    }

    /// Handle bar visibility and border_w changes when transitioning to/from fullscreen layout
    fn handle_fullscreen_layout_transition(
        &mut self,
        backend: &mut dyn Backend,
        mon_key: MonitorKey,
        old_layout: &LayoutEnum,
        new_layout: &LayoutEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let was_fullscreen = old_layout.is_fullscreen_layout();
        let is_fullscreen = new_layout.is_fullscreen_layout();

        if was_fullscreen == is_fullscreen {
            return Ok(());
        }

        let mon_num = self
            .state
            .monitors
            .get(mon_key)
            .map(|m| m.num);

        if is_fullscreen {
            // Entering fullscreen layout: hide bar
            if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
                if let Some(ref mut pertag) = monitor.pertag {
                    let cur_tag = pertag.cur_tag;
                    if let Some(show_bar) = pertag.show_bars.get_mut(cur_tag) {
                        *show_bar = false;
                    }
                }
            }
            if let Some(num) = mon_num {
                if self.current_bar_monitor_id == Some(num) {
                    self.position_statusbar_on_monitor(backend, num)?;
                }
            }
        } else {
            // Leaving fullscreen layout: show bar, restore border_w
            if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
                if let Some(ref mut pertag) = monitor.pertag {
                    let cur_tag = pertag.cur_tag;
                    if let Some(show_bar) = pertag.show_bars.get_mut(cur_tag) {
                        *show_bar = true;
                    }
                }
            }
            if let Some(num) = mon_num {
                if self.current_bar_monitor_id == Some(num) {
                    self.position_statusbar_on_monitor(backend, num)?;
                }
            }

            // Restore border_w for all clients on this monitor
            let border_w = CONFIG.load().border_px() as i32;
            let client_keys: Vec<ClientKey> = self
                .state
                .monitor_clients
                .get(mon_key)
                .map(|keys| keys.iter().copied().collect())
                .unwrap_or_default();

            for ck in client_keys {
                if let Some(client) = self.state.clients.get_mut(ck) {
                    if !client.state.is_floating {
                        client.geometry.border_w = border_w;
                    }
                }
            }
        }

        Ok(())
    }

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

    fn handle_specific_layout(
        &mut self,
        sel_mon_key: MonitorKey,
        layout: &Rc<LayoutEnum>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let monitor = self
            .state
            .monitors
            .get(sel_mon_key)
            .ok_or("Monitor not found")?;

        let current_layout = monitor.lt[monitor.sel_lt].clone();
        let cur_tag = monitor
            .pertag
            .as_ref()
            .ok_or("No pertag information")?
            .cur_tag;

        if **layout == *current_layout {
            self.toggle_layout_selection_impl(sel_mon_key, cur_tag);
        } else {
            self.set_new_layout(sel_mon_key, layout, cur_tag);
        }

        Ok(())
    }

    fn toggle_layout_selection(
        &mut self,
        sel_mon_key: MonitorKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cur_tag = self
            .state
            .monitors
            .get(sel_mon_key)
            .and_then(|m| m.pertag.as_ref())
            .map(|p| p.cur_tag)
            .ok_or("No pertag information available")?;

        self.toggle_layout_selection_impl(sel_mon_key, cur_tag);
        Ok(())
    }

    fn toggle_layout_selection_impl(&mut self, sel_mon_key: MonitorKey, cur_tag: usize) {
        if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
            if let Some(ref mut pertag) = monitor.pertag {
                pertag.sel_lts[cur_tag] ^= 1;
                monitor.sel_lt = pertag.sel_lts[cur_tag];
            }
        }
    }

    fn set_new_layout(&mut self, sel_mon_key: MonitorKey, layout: &Rc<LayoutEnum>, cur_tag: usize) {
        if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
            let sel_lt = monitor.sel_lt;
            if let Some(ref mut pertag) = monitor.pertag {
                pertag.lt_idxs[cur_tag][sel_lt] = Some(layout.clone());
                monitor.lt[sel_lt] = layout.clone();
            }
        }
    }

    fn finalize_layout_update(&mut self, sel_mon_key: MonitorKey) -> (bool, Option<i32>) {
        if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
            monitor.lt_symbol = monitor.lt[monitor.sel_lt].symbol().to_string();

            let has_selection = monitor.sel.is_some();
            let mon_num = monitor.num;

            (has_selection, Some(mon_num))
        } else {
            (false, None)
        }
    }

    pub fn zoom(
        &mut self,
        backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[zoom]");

        let sel_mon_key = match self.state.sel_mon {
            Some(key) => key,
            None => return Ok(()),
        };

        let selected_client_key = if let Some(monitor) = self.state.monitors.get(sel_mon_key) {
            monitor.sel
        } else {
            return Ok(());
        };

        let selected_client_key = match selected_client_key {
            Some(key) => key,
            None => return Ok(()), // 没有选中的客户端
        };

        if let Some(client) = self.state.clients.get(selected_client_key) {
            if client.state.is_floating {
                return Ok(()); // 浮动窗口不参与zoom
            }
        } else {
            return Ok(());
        }

        let first_tiled = self.nexttiled(sel_mon_key, None);

        let target_client_key = if Some(selected_client_key) == first_tiled {
            self.nexttiled(sel_mon_key, Some(selected_client_key))
        } else {
            Some(selected_client_key)
        };

        if let Some(client_key) = target_client_key {
            self.pop(backend, client_key);
        }

        Ok(())
    }

    pub fn loopview(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[loopview]");

        let direction = match arg {
            WMArgEnum::Int(val) => *val,
            _ => return Ok(()),
        };

        if direction == 0 {
            return Ok(());
        }

        let next_tag = self.calculate_next_tag(direction);

        if self.is_same_tag(next_tag) {
            return Ok(());
        }

        info!(
            "[loopview] next_tag: {}, direction: {}",
            next_tag, direction
        );

        let cur_tag = self.switch_to_tag(next_tag, next_tag)?;
        if let Some(sel_mon_key) = self.state.sel_mon {
            self.update_sticky_tags(sel_mon_key);
        }

        let sel_opt = self.apply_pertag_settings(cur_tag)?;

        self.focus(backend, sel_opt)?;
        self.arrange(backend, self.state.sel_mon.clone());

        self.refresh_bar_visibility_on_selected_monitor(backend)?;

        Ok(())
    }

    fn calculate_next_tag(&self, direction: i32) -> u32 {
        let current_tag = if let Some(sel_mon_key) = self.state.sel_mon {
            if let Some(monitor) = self.state.monitors.get(sel_mon_key) {
                monitor.tag_set[monitor.sel_tags]
            } else {
                warn!("[calculate_next_tag] Selected monitor not found");
                return 1; // 返回默认的第一个标签
            }
        } else {
            warn!("[calculate_next_tag] No monitor selected");
            return 1; // 返回默认的第一个标签
        };

        let current_tag_index = if current_tag == 0 {
            0 // 如果当前没有选中的tag，从第一个开始
        } else {
            current_tag.trailing_zeros() as usize
        };

        const MAX_TAGS: usize = 9;
        let next_tag_index = if direction > 0 {
            (current_tag_index + 1) % MAX_TAGS
        } else {
            if current_tag_index == 0 {
                MAX_TAGS - 1
            } else {
                current_tag_index - 1
            }
        };
        let next_tag = 1 << next_tag_index;

        info!(
            "[calculate_next_tag] current_tag: {}, next_tag: {}, direction: {}",
            current_tag, next_tag, direction
        );

        next_tag
    }

    pub fn view(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ui = match arg {
            WMArgEnum::UInt(val) => *val,
            _ => return Ok(()),
        };
        let target_mask = ui & CONFIG.load().tagmask();

        let sel_mon_key = match self.state.sel_mon {
            Some(k) => k,
            None => return Ok(()),
        };

        // 1. 检查是否无需切换
        if let Some(mon) = self.state.monitors.get(sel_mon_key) {
            if crate::core::workspace::WorkspaceManager::is_same_tag(mon, target_mask) {
                return Ok(());
            }
        }

        // 2. 状态变更 (纯逻辑)
        let mut client_to_focus = None;
        if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
            monitor.view_tag(target_mask, false); // false = not toggle, direct set
            // 获取该 Tag 上次选中的 Client
            client_to_focus = monitor.get_selected_client_for_current_tag();
        }
        self.update_sticky_tags(sel_mon_key);

        // 3. 副作用 (Backend / Arrange)
        self.focus(backend, client_to_focus)?;
        self.arrange(backend, Some(sel_mon_key));
        self.refresh_bar_visibility_on_selected_monitor(backend)?;
        self.update_ewmh_desktop(backend)?;

        self.broadcast_ipc_event("tag/view", serde_json::json!({
            "tag": target_mask,
        }));

        Ok(())
    }

    fn is_same_tag(&self, target_tag: u32) -> bool {
        if let Some(sel_mon_key) = self.state.sel_mon {
            if let Some(monitor) = self.state.monitors.get(sel_mon_key) {
                return target_tag == monitor.tag_set[monitor.sel_tags];
            }
        }
        false
    }

    fn switch_to_tag(
        &mut self,
        target_tag: u32,
        ui: u32,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let sel_mon_key = match self.state.sel_mon {
            Some(k) => k,
            None => return Ok(0),
        };
        let sel_mon_mut = if let Some(sel_mon) = self.state.monitors.get_mut(sel_mon_key) {
            sel_mon
        } else {
            return Ok(0);
        };

        info!("[switch_to_tag] tag_set: {:?}", sel_mon_mut.tag_set);
        info!("[switch_to_tag] old sel_tags: {}", sel_mon_mut.sel_tags);

        sel_mon_mut.sel_tags ^= 1;
        let new_sel_tags = sel_mon_mut.sel_tags;
        info!("[switch_to_tag] new sel_tags: {}", new_sel_tags);

        let cur_tag = if target_tag > 0 {
            sel_mon_mut.tag_set[new_sel_tags] = target_tag;

            let new_cur_tag = if ui == !0 {
                0 // 显示所有标签
            } else {
                ui.trailing_zeros() as usize + 1
            };

            if let Some(pertag) = sel_mon_mut.pertag.as_mut() {
                pertag.prev_tag = pertag.cur_tag;
                pertag.cur_tag = new_cur_tag;
            }

            new_cur_tag
        } else {
            if let Some(pertag) = sel_mon_mut.pertag.as_mut() {
                std::mem::swap(&mut pertag.prev_tag, &mut pertag.cur_tag);
                pertag.cur_tag
            } else {
                return Err("No pertag information available".into());
            }
        };

        info!(
            "[switch_to_tag] prev_tag: {}, cur_tag: {}",
            sel_mon_mut.pertag.as_ref().map(|p| p.prev_tag).unwrap_or(0),
            cur_tag
        );

        Ok(cur_tag)
    }

    fn apply_pertag_settings(
        &mut self,
        cur_tag: usize,
    ) -> Result<Option<ClientKey>, Box<dyn std::error::Error>> {
        let sel_mon_key = self.state.sel_mon.ok_or("No monitor selected")?;

        let (n_master, m_fact, sel_lt, layout_0, layout_1, sel_client_key) = {
            let monitor = self
                .state
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

        if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
            monitor.layout.n_master = n_master;
            monitor.layout.m_fact = m_fact;
            monitor.sel_lt = sel_lt;
            monitor.lt[sel_lt] = layout_0;
            monitor.lt[sel_lt ^ 1] = layout_1;
        } else {
            return Err("Monitor disappeared during operation".into());
        }

        if let Some(client_key) = sel_client_key {
            if let Some(client) = self.state.clients.get(client_key) {
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

    pub fn toggleview(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ui = match arg {
            WMArgEnum::UInt(val) => *val,
            _ => return Ok(()),
        };
        let mask = ui & CONFIG.load().tagmask();
        let sel_mon_key = self.state.sel_mon.ok_or("No monitor selected")?;

        // 1. 状态变更
        if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
            monitor.view_tag(mask, true); // true = toggle
        }
        self.update_sticky_tags(sel_mon_key);

        // 2. 副作用
        self.focus(backend, None)?;
        self.arrange(backend, Some(sel_mon_key));
        self.refresh_bar_visibility_on_selected_monitor(backend)?;
        self.update_ewmh_desktop(backend)?;

        Ok(())
    }

    pub fn toggletag(
        &mut self,
        backend: &mut dyn Backend,
        arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[toggletag]");

        let sel_client_key = if let Some(sel_mon_key) = self.state.sel_mon {
            if let Some(monitor) = self.state.monitors.get(sel_mon_key) {
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
            let current_tags = if let Some(client) = self.state.clients.get(sel_client_key) {
                client.state.tags
            } else {
                warn!("[toggletag] Selected client {:?} not found", sel_client_key);
                return Ok(());
            };

            let newtags = current_tags ^ (ui & CONFIG.load().tagmask());

            if newtags > 0 {
                if let Some(client) = self.state.clients.get_mut(sel_client_key) {
                    client.state.tags = newtags;
                } else {
                    return Ok(());
                }

                self.setclienttagprop(backend, sel_client_key)?;

                self.focus(backend, None)?;
                self.arrange(backend, self.state.sel_mon);
            }
        }

        Ok(())
    }

    pub fn quit(
        &mut self,
        _backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[quit]");
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn setup(&mut self, backend: &mut dyn Backend) -> Result<(), Box<dyn std::error::Error>> {
        info!("[setup]");
        let _ = self.updategeom(backend);
        backend.register_wm("jwm")?;

        let mask = (EventMaskBits::SUBSTRUCTURE_REDIRECT
            | EventMaskBits::STRUCTURE_NOTIFY
            | EventMaskBits::BUTTON_PRESS
            | EventMaskBits::POINTER_MOTION
            | EventMaskBits::ENTER_WINDOW
            | EventMaskBits::LEAVE_WINDOW
            | EventMaskBits::PROPERTY_CHANGE)
            .bits();

        let root = backend.root_window().expect("no root window");
        backend
            .cursor_provider()
            .apply(root, StdCursorKind::LeftPtr)?;
        backend
            .window_ops()
            .change_event_mask(backend.root_window().expect("no root window"), mask)?;
        self.grabkeys(backend)?;
        self.focus(backend, None)?;

        self.setup_initial_windows(backend)?;

        self.arrange(backend, None);
        let _ = self.restack(backend, self.state.sel_mon);
        let _ = self.focus(backend, None);
        let _ = self.update_ewmh_desktop(backend);

        backend.window_ops().flush()?;
        Ok(())
    }

    pub fn killclient(
        &mut self,
        backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[killclient]");
        let sel_client_key = match self.get_selected_client_key() {
            Some(k) => k,
            None => return Ok(()),
        };

        let client_win = if let Some(c) = self.state.clients.get(sel_client_key) {
            c.win
        } else {
            return Ok(());
        };

        info!("[killclient] Closing window {:?}", client_win);
        let res = backend.window_ops().close_window(client_win)?;
        if res == crate::backend::api::CloseResult::Forced {
            info!("[killclient] Force killed client");
        } else {
            info!("[killclient] Sent graceful close request");
        }

        Ok(())
    }

    fn handle_transient_for_change(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[handle_transient_for_change]");
        let (is_floating, win, client_name) =
            if let Some(client) = self.state.clients.get(client_key) {
                (client.state.is_floating, client.win, client.name.clone())
            } else {
                return Ok(());
            };

        if !is_floating {
            let transient_for = self.get_transient_for(backend, win);
            if let Some(parent_window) = transient_for {
                if self.wintoclient(parent_window).is_some() {
                    if let Some(client) = self.state.clients.get_mut(client_key) {
                        client.state.is_floating = true;
                    }

                    self.reorder_client_in_monitor_groups(client_key);

                    debug!(
                        "Window '{}' became floating due to transient_for: {:?}",
                        client_name, parent_window
                    );

                    let mon_key = self.state.clients.get(client_key).and_then(|c| c.mon);
                    self.arrange(backend, mon_key);
                }
            }
        }
        Ok(())
    }

    fn handle_normal_hints_change(
        &mut self,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.size_hints.hints_valid = false;
        }
        Ok(())
    }

    fn handle_wm_hints_change(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.updatewmhints(backend, client_key);
        self.mark_bar_update_needed_if_visible(None);

        if let Some(client) = self.state.clients.get(client_key) {
            debug!("WM hints updated for window {:?}", client.win);
        }
        Ok(())
    }

    fn updatetitle_by_key(&mut self, backend: &mut dyn Backend, client_key: ClientKey) {
        let win = if let Some(client) = self.state.clients.get(client_key) {
            client.win
        } else {
            return;
        };
        let new_title = self.fetch_window_title(backend, win);
        let title_for_event;
        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.name = new_title;
            title_for_event = client.name.clone();
            debug!("Updated title for window {:?}: '{}'", win, client.name);
        } else {
            return;
        }
        self.broadcast_ipc_event("window/title", serde_json::json!({
            "id": win.raw(), "name": title_for_event,
        }));
    }

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

    fn fetch_window_title(&mut self, backend: &mut dyn Backend, window: WindowId) -> String {
        let title = backend.property_ops().get_title(window);
        Self::truncate_chars(title, STEXT_MAX_LEN)
    }

    fn handle_title_change(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.updatetitle_by_key(backend, client_key);

        let should_update_bar = self.is_client_selected(client_key);

        if should_update_bar {
            let monitor_id = self
                .state
                .clients
                .get(client_key)
                .and_then(|client| client.mon)
                .and_then(|mon_key| self.state.monitors.get(mon_key))
                .map(|monitor| monitor.num);

            if let Some(id) = monitor_id {
                self.mark_bar_update_needed_if_visible(Some(id));

                if let Some(client) = self.state.clients.get(client_key) {
                    debug!(
                        "Title updated for selected window {:?}, updating status bar",
                        client.win
                    );
                }
            }
        }
        Ok(())
    }

    fn handle_window_type_change(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.updatewindowtype(backend, client_key);

        if let Some(client) = self.state.clients.get(client_key) {
            debug!("Window type updated for window {:?}", client.win);
        }
        Ok(())
    }

    fn handle_class_change(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = self
            .state
            .clients
            .get(client_key)
            .map(|c| c.win)
            .ok_or("Client not found")?;

        let (inst, cls) = backend.property_ops().get_class(win);
        if let Some(client) = self.state.clients.get_mut(client_key) {
            if !inst.is_empty() {
                client.instance = inst;
            }
            if !cls.is_empty() {
                client.class = cls;
            }
        }
        Ok(())
    }

    pub fn movemouse(
        &mut self,
        backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_key = self.get_selected_client_key().ok_or("No client selected")?;

        // 获取只读引用进行检查
        let (is_fullscreen, is_floating, win_id) =
            if let Some(c) = self.state.clients.get(client_key) {
                (c.state.is_fullscreen, c.state.is_floating, c.win)
            } else {
                return Ok(());
            };

        if is_fullscreen {
            return Ok(());
        }

        // 浮动检查：如果是平铺窗口，自动切换为浮动（保持当前几何，不恢复历史 floating_*）
        if !is_floating {
            self.enable_floating_keep_geometry(backend, client_key)?;
        }
        debug!(
            "Initiating move for window {:?} (floating: {}, fullscreen: {})",
            win_id, !is_floating, is_fullscreen
        );

        // [修改] 提升窗口堆叠顺序
        self.restack(backend, self.state.sel_mon)?;

        // [修改] 将控制权移交 Backend
        backend.begin_move(win_id)?;

        // Jwm 不再维护 InteractionState
        Ok(())
    }

    // [重构] resizemouse
    pub fn resizemouse(
        &mut self,
        backend: &mut dyn Backend,
        _arg: &WMArgEnum,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_key = self.get_selected_client_key().ok_or("No client selected")?;

        let (is_fullscreen, is_floating, win_id) =
            if let Some(c) = self.state.clients.get(client_key) {
                (c.state.is_fullscreen, c.state.is_floating, c.win)
            } else {
                return Ok(());
            };

        if is_fullscreen {
            return Ok(());
        }

        if !is_floating {
            self.enable_floating_keep_geometry(backend, client_key)?;
        }

        self.restack(backend, self.state.sel_mon)?;

        // [修改] 将控制权移交 Backend。
        // Wayland/udev 通常不能 warp 指针，所以根据鼠标落点选择更直观的 resize 边/角：
        // - 靠近边：Top/Bottom/Left/Right
        // - 靠近角：TopLeft/TopRight/BottomLeft/BottomRight
        // - 中间区域：退化为象限选择（避免出现“怎么拖都不动”的感觉）
        let geom = backend.window_ops().get_geometry(win_id)?;
        let (px, py) = backend.input_ops().get_pointer_position()?;

        let w = (geom.w as f64).max(1.0);
        let h = (geom.h as f64).max(1.0);

        let rel_x = px - geom.x as f64;
        let rel_y = py - geom.y as f64;

        // Dynamic grip size: small windows still get a usable edge area.
        let threshold = 24.0_f64.min(w / 3.0).min(h / 3.0).max(8.0);

        let near_left = rel_x <= threshold;
        let near_right = rel_x >= (w - threshold);
        let near_top = rel_y <= threshold;
        let near_bottom = rel_y >= (h - threshold);

        let edge = if near_top && near_left {
            crate::backend::api::ResizeEdge::TopLeft
        } else if near_top && near_right {
            crate::backend::api::ResizeEdge::TopRight
        } else if near_bottom && near_left {
            crate::backend::api::ResizeEdge::BottomLeft
        } else if near_bottom && near_right {
            crate::backend::api::ResizeEdge::BottomRight
        } else if near_top {
            crate::backend::api::ResizeEdge::Top
        } else if near_bottom {
            crate::backend::api::ResizeEdge::Bottom
        } else if near_left {
            crate::backend::api::ResizeEdge::Left
        } else if near_right {
            crate::backend::api::ResizeEdge::Right
        } else {
            // Not near any border: pick a quadrant as a reasonable default.
            let left = rel_x < (w / 2.0);
            let top = rel_y < (h / 2.0);
            match (top, left) {
                (true, true) => crate::backend::api::ResizeEdge::TopLeft,
                (true, false) => crate::backend::api::ResizeEdge::TopRight,
                (false, true) => crate::backend::api::ResizeEdge::BottomLeft,
                (false, false) => crate::backend::api::ResizeEdge::BottomRight,
            }
        };

        backend.begin_resize(win_id, edge)?;

        Ok(())
    }

    fn check_monitor_consistency(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 类似于之前的 check_monitor_change_after_resize
        let client_key = match self.get_selected_client_key() {
            Some(k) => k,
            None => return Ok(()),
        };

        let (x, y) = match self.state.clients.get(client_key) {
            Some(client) => (client.geometry.x, client.geometry.y),
            None => return Ok(()),
        };

        let target_monitor = self.recttomon(backend, x, y);
        if let Some(target_mon_key) = target_monitor {
            if Some(target_mon_key) != self.state.sel_mon {
                self.sendmon(backend, Some(client_key), Some(target_mon_key));
                self.state.sel_mon = Some(target_mon_key);
                self.focus(backend, None)?;
            }
        }
        Ok(())
    }

    fn setclienttagprop(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.state.clients.get(client_key) {
            let monitor_num = client
                .mon
                .and_then(|mk| self.state.monitors.get(mk))
                .map(|m| m.num as u32)
                .unwrap_or(0);

            backend.property_ops().set_client_info_props(
                client.win,
                client.state.tags,
                monitor_num,
            )?;
        }
        Ok(())
    }

    fn mouse_focus_blocked(&mut self) -> bool {
        if let Some(deadline) = self.suppress_mouse_focus_until {
            if std::time::Instant::now() < deadline {
                return true;
            }
            self.suppress_mouse_focus_until = None;
        }
        false
    }

    fn switch_to_monitor(
        &mut self,
        backend: &mut dyn Backend,
        target_monitor_key: MonitorKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let prev_sel = self.get_selected_client_key();

        self.state.sel_mon = Some(target_monitor_key);

        self.focus(backend, None)?;
        if let Some(old_key) = prev_sel {
            self.unfocus_client(backend, old_key, false)?;
        }

        let old_id = self.current_bar_monitor_id;
        let new_id = self.state.monitors.get(target_monitor_key).map(|m| m.num);
        if old_id != new_id {
            if let Some(id) = new_id {
                self.current_bar_monitor_id = Some(id);
                self.position_statusbar_on_monitor(backend, id)?;
            }
            if let Some(old) = old_id.and_then(|oid| self.get_monitor_by_id(oid)) {
                self.arrange(backend, Some(old));
            }
            self.arrange(backend, Some(target_monitor_key));
            self.restack(backend, Some(target_monitor_key))?;
        }

        Ok(())
    }

    fn should_focus_client(
        &self,
        client_key_opt: Option<ClientKey>,
        is_on_selected_monitor: bool,
    ) -> bool {
        if !is_on_selected_monitor {
            return true;
        }

        if client_key_opt.is_none() {
            return true;
        }

        let current_selected = self.get_selected_client_key();
        current_selected != client_key_opt
    }

    fn expose(
        &mut self,
        backend: &mut dyn Backend,
        window: WindowId,
        count: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[expose]");
        if count != 0 {
            return Ok(());
        }

        if let Some(monitor_key) = self.wintomon(backend, Some(window)) {
            if let Some(monitor) = self.state.monitors.get(monitor_key) {
                self.mark_bar_update_needed_if_visible(Some(monitor.num));
            }
        }

        Ok(())
    }

    fn unfocus_client_opt(
        &mut self,
        backend: &mut dyn Backend,
        client_key_opt: Option<ClientKey>,
        setfocus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client_key) = client_key_opt {
            self.unfocus_client(backend, client_key, setfocus)?;
        }
        Ok(())
    }

    fn focus(
        &mut self,
        backend: &mut dyn Backend,
        mut client_key_opt: Option<ClientKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[focus]");

        if let Some(client_key) = client_key_opt {
            if let Some(client) = self.state.clients.get(client_key) {
                info!("[focus] {}", client);
                if Some(client.win) == self.status_bar_window {
                    client_key_opt = None; // 忽略状态栏
                }
            }
        }

        let is_visible = match client_key_opt {
            Some(client_key) => self.is_client_visible_by_key(client_key),
            None => false,
        };

        if !is_visible {
            client_key_opt = self.find_visible_client();
        }

        self.handle_focus_change_by_key(backend, &client_key_opt)?;

        if let Some(client_key) = client_key_opt {
            self.set_client_focus_by_key(backend, client_key)?;
        } else {
            self.set_root_focus(backend)?;
        }

        self.update_monitor_selection_by_key(client_key_opt);

        self.mark_bar_update_needed_if_visible(None);

        // Broadcast focus event
        if let Some(ck) = client_key_opt {
            let event_data = self.state.clients.get(ck).map(|c| (c.win.raw(), c.name.clone()));
            if let Some((id, name)) = event_data {
                self.broadcast_ipc_event("window/focus", serde_json::json!({
                    "id": id, "name": name,
                }));
            }
        }

        Ok(())
    }

    fn find_visible_client(&self) -> Option<ClientKey> {
        let sel_mon_key = self.state.sel_mon?;

        if let Some(stack_clients) = self.state.monitor_stack.get(sel_mon_key) {
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
        backend: &mut dyn Backend,
        new_focus: &Option<ClientKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let current_sel = self.get_selected_client_key();

        if current_sel.is_some() && current_sel != *new_focus {
            if let Some(current_key) = current_sel {
                self.unfocus_client(backend, current_key, false)?;
            }
        }

        Ok(())
    }

    fn set_client_focus_by_key(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client_monitor_key = if let Some(client) = self.state.clients.get(client_key) {
            client.mon
        } else {
            return Err("Client not found".into());
        };

        if let Some(client_mon_key) = client_monitor_key {
            if Some(client_mon_key) != self.state.sel_mon {
                self.state.sel_mon = Some(client_mon_key);
            }
        }

        if let Some(client) = self.state.clients.get_mut(client_key) {
            if client.state.is_urgent {
                client.state.is_urgent = false;
                let _ = self.seturgent(backend, client_key, false);
            }
        }
        self.detachstack(client_key);
        self.attachstack(client_key);
        self.update_client_decoration(backend, client_key, true)?;
        self.grabbuttons(backend, client_key, true);
        self.setfocus(backend, client_key)?;
        Ok(())
    }

    fn update_monitor_selection_by_key(&mut self, client_key_opt: Option<ClientKey>) {
        if let Some(sel_mon_key) = self.state.sel_mon {
            if let Some(monitor) = self.state.monitors.get_mut(sel_mon_key) {
                // 使用新方法
                monitor.set_selected_client_for_current_tag(client_key_opt);
            }
        }
    }

    fn unfocus_client(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        setfocus: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(_client) = self.state.clients.get(client_key) {
            self.update_client_decoration(backend, client_key, false)?;
            self.grabbuttons(backend, client_key, false);
            if setfocus {
                backend.on_focused_client_changed(None)?;
            }
        }
        Ok(())
    }

    fn setfocus(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.state.clients.get(client_key) {
            backend.on_focused_client_changed(Some(client.win))?;
        }
        Ok(())
    }

    fn set_root_focus(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        backend.window_ops().set_input_focus_root()?;
        Ok(backend.on_focused_client_changed(None)?)
    }

    fn update_ewmh_desktop(
        &self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let total = CONFIG.load().tags_length() as u32;
        let current = if let Some(sel_mon_key) = self.state.sel_mon {
            if let Some(monitor) = self.state.monitors.get(sel_mon_key) {
                let tagset = monitor.tag_set[monitor.sel_tags];
                if tagset > 0 { tagset.trailing_zeros() } else { 0 }
            } else {
                0
            }
        } else {
            0
        };
        let names: Vec<String> = (1..=total).map(|i| i.to_string()).collect();
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        backend.on_desktop_changed(current, total, &name_refs)?;
        Ok(())
    }

    fn update_net_client_list(
        &mut self,
        backend: &mut dyn Backend,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut ordered: Vec<WindowId> = Vec::with_capacity(self.state.client_order.len());
        for &key in &self.state.client_order {
            if let Some(client) = self.state.clients.get(key) {
                ordered.push(client.win);
            }
        }

        let mut stacking: Vec<WindowId> = Vec::new();
        for &mon_key in &self.state.monitor_order {
            if let Some(stack) = self.state.monitor_stack.get(mon_key) {
                for &ck in stack.iter().rev() {
                    if let Some(c) = self.state.clients.get(ck) {
                        stacking.push(c.win);
                    }
                }
            }
        }

        backend.on_client_list_changed(&ordered, &stacking)?;
        Ok(())
    }

    fn setclientstate(
        &self,
        backend: &mut dyn Backend,
        win: WindowId,
        state: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(backend.property_ops().set_wm_state(win, state)?)
    }

    fn manage(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        geom: &Geometry,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[manage] Managing window {:?}", win);
        if self.wintoclient(win).is_some() {
            warn!("Window {:?} already managed", win);
            return Ok(());
        }
        let mut client = WMClient::new(win);
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
        client.name = self.fetch_window_title(backend, client.win);
        self.update_class_info(backend, &mut client);

        info!("{}", client);
        if client.is_status_bar(CONFIG.load().status_bar_name()) {
            info!("Detected status bar, managing as statusbar");
            let client_key = self.insert_client(client);
            let current_mon_id = self.get_sel_mon().map(|m| m.num).unwrap_or(0);
            self.status_bar_client = Some(client_key);
            self.status_bar_window = Some(win);
            self.current_bar_monitor_id = Some(current_mon_id);

            return self.manage_statusbar(backend, client_key, win, current_mon_id);
        }

        // Check for external strut (polybar, trayer, etc.)
        self.check_strut_on_manage(backend, win);

        let client_key = self.insert_client(client);
        self.manage_regular_client(backend, client_key)?;

        // Broadcast window/new event
        let new_event_data = self.state.clients.get(client_key).map(|c| {
            (c.win.raw(), c.name.clone(), c.class.clone())
        });
        if let Some((id, name, class)) = new_event_data {
            self.broadcast_ipc_event("window/new", serde_json::json!({
                "id": id, "name": name, "class": class,
            }));
        }

        // Appear animation for new windows
        {
            let cfg = CONFIG.load();
            if cfg.animation_enabled() {
                if let Some(client) = self.state.clients.get(client_key) {
                    let target = Rect::new(
                        client.geometry.x,
                        client.geometry.y,
                        client.geometry.w,
                        client.geometry.h,
                    );
                    // Start from 85% scale centered on target
                    let sw = (target.w as f32 * 0.85) as i32;
                    let sh = (target.h as f32 * 0.85) as i32;
                    let sx = target.x + (target.w - sw) / 2;
                    let sy = target.y + (target.h - sh) / 2;
                    let from = Rect::new(sx, sy, sw, sh);
                    self.animations.start(
                        client_key,
                        from,
                        target,
                        cfg.animation_duration(),
                        cfg.animation_easing(),
                        AnimationKind::Appear,
                    );
                }
            }
        }

        // Detect named scratchpad window
        if let Some(sp_name) = self.scratchpad_pending_name.take() {
            self.scratchpads.insert(sp_name.clone(), client_key);
            info!("[manage] detected scratchpad '{}' client {:?}", sp_name, client_key);
            let mon_key = self.state.clients.get(client_key).and_then(|c| c.mon);
            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.state.is_floating = true;
            }
            if let Some(mk) = mon_key {
                if let Some(area) = self.monitor_work_area(mk) {
                    let w = (area.w as f32 * 0.8) as i32;
                    let h = (area.h as f32 * 0.8) as i32;
                    let x = area.x + (area.w - w) / 2;
                    let y = area.y + (area.h - h) / 2;
                    self.resize_client(backend, client_key, x, y, w, h, false);
                }
                let _ = self.focus(backend, Some(client_key));
                self.arrange(backend, Some(mk));
            }
        }

        Ok(())
    }

    fn setup_client_window(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_popup_like(backend, client_key) {
            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.geometry.border_w = 0;
            }
            self.update_client_decoration(backend, client_key, false)?;

            self.configure_client(backend, client_key)?;
            if let Some(client) = self.state.clients.get(client_key) {
                self.setclientstate(backend, client.win, NORMAL_STATE as i64)?;
            }
            return Ok(());
        }

        let win = if let Some(client) = self.state.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        info!("Setting up window {:?}", win);

        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.geometry.border_w = CONFIG.load().border_px() as i32;
        }

        self.update_client_decoration(backend, client_key, true)?;

        self.configure_client(backend, client_key)?;

        let (x, y, w, h) = if let Some(client) = self.state.clients.get(client_key) {
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
        let changes = WindowChanges {
            x: Some(x),
            y: Some(y),
            width: Some(w as u32),
            height: Some(h as u32),
            ..Default::default()
        };
        backend.window_ops().apply_window_changes(win, changes)?;

        if let Some(client) = self.state.clients.get(client_key) {
            self.setclientstate(backend, client.win, NORMAL_STATE as i64)?;
        }

        Ok(())
    }

    fn parent_client_of(
        &self,
        backend: &mut dyn Backend,
        child_key: ClientKey,
    ) -> Option<ClientKey> {
        let child_win = self.state.clients.get(child_key).map(|c| c.win)?;
        let parent_win = self.get_transient_for(backend, child_win)?;
        self.wintoclient(parent_win)
    }

    fn handle_new_client_focus(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (client_win, client_mon_key, is_never_focus) =
            if let Some(c) = self.state.clients.get(client_key) {
                (c.win, c.mon, c.state.never_focus)
            } else {
                return Err("Client not found".into());
            };
        let current_sel = self.get_selected_client_key();
        let current_sel_mon = self.state.sel_mon;
        if self.is_popup_like(backend, client_key) {
            let parent_key_opt = self.parent_client_of(backend, client_key);
            let sibling = parent_key_opt
                .and_then(|pk| self.state.clients.get(pk))
                .map(|pc| pc.win);
            let changes = WindowChanges {
                sibling: sibling,
                stack_mode: Some(StackMode::Above),
                ..Default::default()
            };
            backend
                .window_ops()
                .apply_window_changes(client_win, changes)?;

            let should_focus_this = if let Some(c) = self.state.clients.get(client_key) {
                if c.state.never_focus {
                    false
                } else {
                    let types = backend.property_ops().get_window_types(c.win);
                    let is_transient = backend.property_ops().transient_for(c.win).is_some();

                    // Transient 窗口（用户交互触发的子窗口）应获得焦点
                    if is_transient {
                        true
                    } else {
                        let is_no_auto_focus = types.contains(&WindowType::Tooltip)
                            || types.contains(&WindowType::Notification)
                            || types.contains(&WindowType::Dnd)
                            || types.contains(&WindowType::Combo);
                        !is_no_auto_focus
                    }
                }
            } else {
                false
            };

            if should_focus_this {
                self.focus(backend, Some(client_key))?;
            } else {
                if let Some(pk) = parent_key_opt {
                    let _ = self.set_client_focus_by_key(backend, pk);
                } else if let Some(prev_sel) = current_sel {
                    let _ = self.set_client_focus_by_key(backend, prev_sel);
                } else {
                    let _ = self.set_root_focus(backend);
                }
            }

            return Ok(());
        }
        let is_on_selected_monitor = client_mon_key.is_some() && client_mon_key == current_sel_mon;
        if is_on_selected_monitor {
            if let Some(mon_key) = client_mon_key {
                if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
                    monitor.sel = Some(client_key);
                }
                self.arrange(backend, Some(mon_key));
            }

            if !is_never_focus {
                if let Some(prev_sel) = current_sel {
                    if prev_sel != client_key {
                        self.unfocus_client(backend, prev_sel, false)?;
                    }
                }
                self.focus(backend, Some(client_key))?;
            } else {
                if let Some(prev_sel) = current_sel {
                    let _ = self.set_client_focus_by_key(backend, prev_sel);
                } else {
                    let _ = self.set_root_focus(backend);
                }
            }
            return Ok(());
        }

        if let Some(target_mon_key) = client_mon_key {
            if let Some(monitor) = self.state.monitors.get_mut(target_mon_key) {
                monitor.sel = Some(client_key);
            }
            self.arrange(backend, Some(target_mon_key));
        }

        if CONFIG.load().behavior().focus_follows_new_window && !is_never_focus {
            if let Some(target_mon_key) = client_mon_key {
                self.switch_to_monitor(backend, target_mon_key)?;
                self.focus(backend, Some(client_key))?;
            }
        } else {
            if let Some(prev_sel) = current_sel {
                let _ = self.set_client_focus_by_key(backend, prev_sel);
            } else {
                let _ = self.set_root_focus(backend);
            }
        }

        Ok(())
    }

    fn grabbuttons(&mut self, backend: &mut dyn Backend, client_key: ClientKey, focused: bool) {
        let win = if let Some(c) = self.state.clients.get(client_key) {
            c.win
        } else {
            return;
        };
        let _ = backend.window_ops().ungrab_all_buttons(win);

        if focused {
            let buttons = crate::config::CONFIG.load().get_buttons();
            let modifiers_combinations = [
                Mods::NONE,
                Mods::CAPS,
                Mods::NUMLOCK,
                Mods::CAPS | Mods::NUMLOCK,
            ];
            for btn_conf in buttons {
                if btn_conf.click_type == WMClickType::ClickClientWin {
                    let clean_conf_mask = btn_conf.mask
                        & (Mods::SHIFT
                            | Mods::CONTROL
                            | Mods::ALT
                            | Mods::SUPER
                            | Mods::MOD2
                            | Mods::MOD3
                            | Mods::MOD5);
                    for &lock_state in &modifiers_combinations {
                        let final_mask = clean_conf_mask | lock_state;
                        let _ = backend.window_ops().grab_button(
                            win,
                            btn_conf.button.to_u8(),
                            (EventMaskBits::BUTTON_PRESS | EventMaskBits::BUTTON_RELEASE).bits(),
                            final_mask,
                        );
                    }
                }
            }
        } else {
            let _ = backend.window_ops().grab_button_any_anymod(
                win,
                (EventMaskBits::BUTTON_PRESS | EventMaskBits::BUTTON_RELEASE).bits(),
            );
        }
    }

    fn manage_regular_client(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.handle_transient_for(backend, client_key)?;

        self.adjust_client_position(backend, client_key);

        self.setup_client_window(backend, client_key)?;

        self.updatewindowtype(backend, client_key);
        self.updatesizehints(backend, client_key)?;
        self.updatewmhints(backend, client_key);

        self.attach_back(client_key);
        self.attachstack(client_key);

        self.register_client_events(backend, client_key)?;
        self.grabbuttons(backend, client_key, false);

        let already_mapped = match self.state.clients.get(client_key) {
            Some(client) => backend
                .window_ops()
                .get_window_attributes(client.win)
                .map(|a| a.map_state_viewable)
                .unwrap_or(false),
            None => false,
        };
        if !already_mapped {
            self.map_client_window(backend, client_key)?;
        }

        self.update_net_client_list(backend)?;

        self.handle_new_client_focus(backend, client_key)?;

        self.suppress_mouse_focus_until =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(300));

        Ok(())
    }

    fn handle_transient_for(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.state.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        match self.get_transient_for(backend, win) {
            Some(transient_for_win) => {
                if let Some(parent_client_key) = self.wintoclient(transient_for_win) {
                    let (parent_mon, parent_tags) =
                        if let Some(parent) = self.state.clients.get(parent_client_key) {
                            (parent.mon, parent.state.tags)
                        } else {
                            return Err("Parent client not found".into());
                        };

                    if let Some(client) = self.state.clients.get_mut(client_key) {
                        client.mon = parent_mon;
                        client.state.tags = parent_tags;
                        client.state.is_floating = true;
                        warn!(
                            "[handle_transient_for] Client {} is transient for parent",
                            client
                        );
                    }
                } else {
                    info!("[handle_transient_for] parent client is None");
                    if let Some(client) = self.state.clients.get_mut(client_key) {
                        client.mon = self.state.sel_mon;
                    }
                    self.applyrules_by_key(backend, client_key);
                }
            }
            None => {
                info!("no WM_TRANSIENT_FOR property");
                if let Some(client) = self.state.clients.get_mut(client_key) {
                    client.mon = self.state.sel_mon;
                }
                self.applyrules_by_key(backend, client_key);
            }
        }
        Ok(())
    }

    fn update_class_info(&mut self, backend: &mut dyn Backend, client: &mut WMClient) {
        if let Some((inst, cls)) = self.get_wm_class(backend, client.win) {
            client.instance = inst;
            client.class = cls;
        }
    }

    fn rule_matches(&self, rule: &WMRule, name: &str, class: &str, instance: &str) -> bool {
        if rule.name.is_empty() && rule.class.is_empty() && rule.instance.is_empty() {
            return false;
        }
        let name_matches = rule.name.is_empty() || name.contains(&rule.name);
        let class_matches = rule.class.is_empty() || class.contains(&rule.class);
        let instance_matches = rule.instance.is_empty() || instance.contains(&rule.instance);
        name_matches && class_matches && instance_matches
    }

    fn apply_single_rule(&mut self, client_key: ClientKey, rule: &WMRule) {
        if let Some(client) = self.state.clients.get_mut(client_key) {
            info!("[apply_single_rule] Applying rule: {:?}", rule);
            client.state.is_floating = rule.is_floating;
            if rule.tags > 0 {
                client.state.tags |= rule.tags as u32;
            }
            if rule.monitor >= 0 {
                let target_monitor = self
                    .state
                    .monitor_order
                    .iter()
                    .find(|&&mon_key| {
                        if let Some(monitor) = self.state.monitors.get(mon_key) {
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

    fn set_default_tags(&mut self, client_key: ClientKey) {
        if let Some(client) = self.state.clients.get_mut(client_key) {
            let current_tags = client.state.tags & CONFIG.load().tagmask();
            if current_tags > 0 {
                client.state.tags = current_tags;
            } else {
                if let Some(mon_key) = client.mon {
                    if let Some(monitor) = self.state.monitors.get(mon_key) {
                        client.state.tags = monitor.tag_set[monitor.sel_tags];
                    }
                } else {
                    client.state.tags = 1;
                }
            }
            info!(
                "[set_default_tags] Set tags to {} for client {:?}",
                client.state.tags, client.win
            );
        }
    }

    fn applyrules_by_key(&mut self, backend: &mut dyn Backend, client_key: ClientKey) {
        let (win, name, mut class, mut instance) =
            if let Some(client) = self.state.clients.get(client_key) {
                (
                    client.win,
                    client.name.clone(),
                    client.class.clone(),
                    client.instance.clone(),
                )
            } else {
                return;
            };
        if class.is_empty() && instance.is_empty() {
            if let Some((inst, cls)) = self.get_wm_class(backend, win) {
                instance = inst;
                class = cls;

                if let Some(client) = self.state.clients.get_mut(client_key) {
                    client.instance = instance.clone();
                    client.class = class.clone();
                }
            }
        }
        info!(
            "[applyrules_by_key] win: {:?}, name: '{}', instance: '{}', class: '{}'",
            win, name, instance, class
        );
        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.state.is_floating = false;
        }
        if name.is_empty() && class.is_empty() && instance.is_empty() {
            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.state.is_floating = true;
            }
            info!("No window info available, setting as floating");
        }
        let mut rule_applied = false;
        for rule in &CONFIG.load().get_rules() {
            if self.rule_matches(rule, &name, &class, &instance) {
                self.apply_single_rule(client_key, rule);
                rule_applied = true;
                break;
            }
        }
        if !rule_applied {
            info!("No matching rule found, using defaults");
        }
        self.set_default_tags(client_key);
        if let Some(client) = self.state.clients.get(client_key) {
            info!(
                "Final state - class: '{}', instance: '{}', name: '{}', tags: {}, floating: {}",
                client.class,
                client.instance,
                client.name,
                client.state.tags,
                client.state.is_floating
            );
        }
    }

    fn register_client_events(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.state.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        let mask = (EventMaskBits::ENTER_WINDOW
            | EventMaskBits::FOCUS_CHANGE
            | EventMaskBits::PROPERTY_CHANGE
            | EventMaskBits::STRUCTURE_NOTIFY
            | EventMaskBits::POINTER_MOTION)
            .bits();
        backend.window_ops().change_event_mask(win, mask)?;
        info!(
            "[register_client_events] Events registered for window {:?}",
            win
        );
        Ok(())
    }

    fn map_client_window(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.state.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };

        backend.window_ops().map_window(win)?;
        info!("[map_client_window] Successfully mapped window {:?}", win);
        Ok(())
    }

    fn manage_statusbar(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        win: WindowId,
        current_mon_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mon_key = self.get_monitor_by_id(current_mon_id);
        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.mon = mon_key;
            client.state.never_focus = true;
            client.state.is_floating = true;
            client.state.is_dock = true;
            client.state.tags = CONFIG.load().tagmask();
            client.geometry.border_w = 0;
        }

        self.position_statusbar_on_monitor(backend, current_mon_id)?;

        self.setup_statusbar_window_by_key(backend, client_key)?;

        backend.window_ops().map_window(win)?;
        Ok(())
    }

    fn set_bar_strut(
        &self,
        backend: &mut dyn Backend,
        bar_win: WindowId,
        mon: &WMMonitor,
        bar_height: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let top_amount = bar_height.max(0) as u32;
        let top_start_x = mon.geometry.m_x.max(0) as u32;
        let top_end_x = (mon.geometry.m_x + mon.geometry.m_w - 1).max(0) as u32;
        Ok(backend.property_ops().set_window_strut_top(
            bar_win,
            top_amount,
            top_start_x,
            top_end_x,
        )?)
    }

    fn remove_bar_strut(
        &self,
        backend: &mut dyn Backend,
        bar_win: WindowId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(backend.property_ops().clear_window_strut(bar_win)?)
    }

    fn position_statusbar_on_monitor(
        &mut self,
        backend: &mut dyn Backend,
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
        let monitor = match self.state.monitors.get(mon_key) {
            Some(m) => m,
            None => return Ok(()),
        };

        let show_bar = monitor
            .pertag
            .as_ref()
            .and_then(|p| p.show_bars.get(p.cur_tag))
            .copied()
            .unwrap_or(true);

        let (client_win, client_height) =
            if let Some(client) = self.state.clients.get_mut(client_key) {
                if show_bar {
                    let pad = CONFIG.load().status_bar_padding();
                    let border_width = client.geometry.border_w;
                    client.geometry.x = monitor.geometry.m_x + pad;
                    client.geometry.y = monitor.geometry.m_y + pad;
                    client.geometry.w = monitor.geometry.m_w - 2 * pad - 2 * border_width;
                    client.geometry.h = CONFIG.load().status_bar_height();

                    let changes = WindowChanges {
                        x: Some(client.geometry.x),
                        y: Some(client.geometry.y),
                        width: Some(client.geometry.w as u32),
                        height: Some(client.geometry.h as u32),
                        ..Default::default()
                    };
                    backend
                        .window_ops()
                        .apply_window_changes(client.win, changes)?;
                    (client.win, Some(client.geometry.h))
                } else {
                    let changes = WindowChanges {
                        x: Some(-1000),
                        y: Some(-1000),
                        ..Default::default()
                    };
                    backend
                        .window_ops()
                        .apply_window_changes(client.win, changes)?;
                    (client.win, None)
                }
            } else {
                return Ok(());
            };

        if let Some(height) = client_height {
            self.set_bar_strut(backend, client_win, monitor, height)?;
        } else {
            self.remove_bar_strut(backend, client_win)?;
        }
        Ok(())
    }

    fn setup_statusbar_window_by_key(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let win = if let Some(client) = self.state.clients.get(client_key) {
            client.win
        } else {
            return Err("Client not found".into());
        };
        info!(
            "[setup_statusbar_window_by_key] Setting up statusbar window {:?}",
            win
        );

        let mask_bits = (EventMaskBits::STRUCTURE_NOTIFY
            | EventMaskBits::PROPERTY_CHANGE
            | EventMaskBits::ENTER_WINDOW)
            .bits();
        backend.window_ops().change_event_mask(win, mask_bits)?;
        self.configure_client(backend, client_key)?;
        info!(
            "[setup_statusbar_window_by_key] Statusbar window setup completed for {:?}",
            win
        );
        Ok(())
    }

    fn get_monitor_by_id(&self, monitor_id: i32) -> Option<MonitorKey> {
        self.state
            .monitors
            .iter()
            .find(|(_, monitor)| monitor.num == monitor_id)
            .map(|(key, _)| key)
    }

    fn maprequest(
        &mut self,
        backend: &mut dyn Backend,
        window: WindowId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let window_attr = backend.window_ops().get_window_attributes(window)?;
        if window_attr.override_redirect {
            debug!(
                "Ignoring map request for override_redirect window: {:?}",
                window
            );
            return Ok(());
        }
        if self.wintoclient(window).is_none() {
            let geom = backend.window_ops().get_geometry(window)?;
            self.manage(backend, window, &geom)?;
        } else {
            debug!(
                "Window {:?} is already managed, ignoring map request",
                window
            );
        }
        Ok(())
    }

    fn monocle(&mut self, backend: &mut dyn Backend, mon_key: MonitorKey) {
        info!("[monocle] via pure layout engine");
        let (wx, wy, ww, wh, _, _, monitor_num, _client_y_offset) = self.get_monitor_info(mon_key);
        let mut visible_count = 0u32;
        let mut tiled_keys = Vec::new();
        if let Some(client_keys) = self.state.monitor_clients.get(mon_key) {
            for &client_key in client_keys {
                if let Some(client) = self.state.clients.get(client_key) {
                    let is_visible = self.is_client_visible_on_monitor(client_key, mon_key);

                    if is_visible {
                        visible_count += 1;
                        if !client.state.is_floating {
                            tiled_keys.push(client_key);
                        }
                    }
                }
            }
        }

        let default_border = CONFIG.load().border_px() as i32;
        let effective_border = if tiled_keys.len() == 1 { 0 } else { default_border };
        for &ck in &tiled_keys {
            if let Some(client) = self.state.clients.get_mut(ck) {
                client.geometry.border_w = effective_border;
            }
        }

        let layout_clients: Vec<LayoutClient<ClientKey>> = tiled_keys
            .iter()
            .map(|&key| LayoutClient {
                key,
                factor: 1.0,
                border_w: effective_border,
            })
            .collect();
        if visible_count > 0 {
            let formatted_string = format!("[{}]", visible_count);
            if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
                monitor.lt_symbol = formatted_string.clone();
            }
            info!(
                "[monocle] formatted_string: {}, monitor_num: {}",
                formatted_string, monitor_num
            );
        }
        if layout_clients.is_empty() {
            return;
        }
        let screen_area = self
            .monitor_work_area(mon_key)
            .unwrap_or(Rect::new(wx, wy, ww, wh));
        // 纯计算
        let params = LayoutParams {
            screen_area,
            n_master: 0, // 不相关
            m_fact: 0.0, // 不相关
            gap: 0,      // monocle 不使用 gap
        };
        let results = layout::calculate_monocle(&params, &layout_clients);
        // 应用
        for res in results {
            self.resize_client(
                backend, res.key, res.rect.x, res.rect.y, res.rect.w, res.rect.h, false,
            );
        }
    }

    fn handle_monitor_switch_by_key(
        &mut self,
        backend: &mut dyn Backend,
        new_monitor_key: Option<MonitorKey>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let current_sel = self.get_selected_client_key();
        if let Some(sel_key) = current_sel {
            self.unfocus_client(backend, sel_key, true)?;
        }

        self.state.sel_mon = new_monitor_key;

        self.focus(backend, None)?;

        if let Some(monitor_key) = new_monitor_key {
            if let Some(monitor) = self.state.monitors.get(monitor_key) {
                debug!("Switched to monitor {} via mouse motion", monitor.num);
            }
        }

        Ok(())
    }

    fn unmanage(
        &mut self,
        backend: &mut dyn Backend,
        client_key: Option<ClientKey>,
        destroyed: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("unmanage");
        let client_key = match client_key {
            Some(key) => key,
            None => return Ok(()),
        };

        let win = if let Some(client) = self.state.clients.get(client_key) {
            client.win
        } else {
            warn!("[unmanage] Client {:?} not found", client_key);
            return Ok(());
        };

        // Remove any external strut reservation for this window
        self.remove_strut_on_unmanage(backend, win);

        if Some(win) == self.status_bar_window {
            return self.unmanage_statusbar();
        }

        // Broadcast window/close event before removing the client
        let close_event_data = self.state.clients.get(client_key).map(|c| {
            (c.win.raw(), c.name.clone())
        });
        if let Some((id, name)) = close_event_data {
            self.broadcast_ipc_event("window/close", serde_json::json!({
                "id": id, "name": name,
            }));
        }

        self.unmanage_regular_client(backend, client_key, destroyed)?;
        Ok(())
    }

    fn unmanage_statusbar(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        debug!("unmanage_statusbar");
        self.cleanup_statusbar_processes()?;
        self.status_bar_shmem = None;
        self.status_bar_child = None;
        if let Some(bar_win) = self.status_bar_window {
            self.state.win_to_client.remove(&bar_win);
        }
        if let Some(bar_key) = self.status_bar_client {
            self.state.clients.remove(bar_key);
            self.state.client_order.retain(|&k| k != bar_key);
        }
        self.status_bar_client = None;
        self.status_bar_window = None;
        info!("Successfully removed statusbar",);
        Ok(())
    }

    fn is_popup_like(&self, backend: &mut dyn Backend, client_key: ClientKey) -> bool {
        let client = if let Some(client) = self.state.clients.get(client_key) {
            client
        } else {
            return false;
        };
        let types = backend.property_ops().get_window_types(client.win);
        for t in types {
            match t {
                WindowType::Dialog
                | WindowType::PopupMenu
                | WindowType::DropdownMenu
                | WindowType::Tooltip
                | WindowType::Notification
                | WindowType::Combo
                | WindowType::Dnd
                | WindowType::Utility
                | WindowType::Splash => {
                    info!("popup type {:?}", t);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn adjust_client_position(&mut self, backend: &mut dyn Backend, client_key: ClientKey) {
        info!("[adjust_client_position]");
        let (client_total_width, client_mon_key_opt, win) = if let Some(client) =
            self.state.clients.get(client_key)
        {
            (client.total_width(), client.mon, client.win)
        } else {
            error!("Client {:?} not found", client_key);
            return;
        };

        // Most popup-like windows (menus/tooltips/etc.) should not be clamped by the WM.
        // Notifications are a special case: if they spawn at monitor y=0 they can end up
        // hidden under the status bar. Dialogs are another special case: apps sometimes
        // position transient dialogs at y=0, and we still want them to respect the monitor
        // workarea (i.e. avoid any top strut / status bar).
        if self.is_popup_like(backend, client_key) {
            let types = backend.property_ops().get_window_types(win);
            let should_clamp = types.contains(&WindowType::Notification)
                || types.contains(&WindowType::Dialog);

            if !should_clamp {
                info!("is_popup_like (skip position adjustment)");
                return;
            }

            if types.contains(&WindowType::Dialog) {
                info!("popup-like Dialog (clamp to workarea)");
            }
        }
        let client_mon_key = if let Some(mon_key) = client_mon_key_opt {
            mon_key
        } else {
            error!("Client has no monitor assigned!");
            return;
        };
        let (mon_wx, mon_wy, mon_ww, mon_wh) =
            if let Some(monitor) = self.state.monitors.get(client_mon_key) {
                (
                    monitor.geometry.w_x,
                    monitor.geometry.w_y,
                    monitor.geometry.w_w,
                    monitor.geometry.w_h,
                )
            } else {
                error!("Monitor {:?} not found", client_mon_key);
                return;
            };
        info!("{:?}", win);
        let (mut client_x, mut client_y, _client_w, _client_h) =
            if let Some(client) = self.state.clients.get(client_key) {
                (
                    client.geometry.x,
                    client.geometry.y,
                    client.geometry.w,
                    client.geometry.h,
                )
            } else {
                return;
            };
        if client_x + client_total_width > mon_wx + mon_ww {
            client_x = mon_wx + mon_ww - client_total_width;
            info!("Adjusted X to prevent overflow: {}", client_x);
        }
        let client_total_height = if let Some(client) = self.state.clients.get(client_key) {
            client.total_height()
        } else {
            return;
        };
        if client_y + client_total_height > mon_wy + mon_wh {
            client_y = mon_wy + mon_wh - client_total_height;
            info!("Adjusted Y to prevent overflow: {}", client_y);
        }
        if client_x < mon_wx {
            client_x = mon_wx;
            info!("Adjusted X to workarea left: {}", client_x);
        }
        if client_y < mon_wy {
            client_y = mon_wy;
            info!("Adjusted Y to workarea top: {}", client_y);
        }

        // Clamp to workarea by default (so dialogs avoid the status bar strut), and additionally
        // clamp transient dialogs to their parent window bounds so they don't jump across tiled
        // columns (e.g. right tile spawning a dialog at x=0).
        let mut clamp = self
            .monitor_work_area(client_mon_key)
            .unwrap_or(Rect::new(mon_wx, mon_wy, mon_ww, mon_wh));

        let types = backend.property_ops().get_window_types(win);
        let is_dialog = types.contains(&WindowType::Dialog);
        if is_dialog {
            if let Some(parent_key) = self.parent_client_of(backend, client_key) {
                if let Some(parent) = self.state.clients.get(parent_key) {
                    let parent_rect = Rect::new(
                        parent.geometry.x,
                        parent.geometry.y,
                        parent.total_width(),
                        parent.total_height(),
                    );

                    // Intersect clamp rect with parent rect.
                    let left = clamp.x.max(parent_rect.x);
                    let top = clamp.y.max(parent_rect.y);
                    let right = (clamp.x + clamp.w).min(parent_rect.x + parent_rect.w);
                    let bottom = (clamp.y + clamp.h).min(parent_rect.y + parent_rect.h);
                    let w = (right - left).max(0);
                    let h = (bottom - top).max(0);

                    if w > 0 && h > 0 {
                        clamp = Rect::new(left, top, w, h);
                        info!(
                            "Dialog transient clamp: parent=({},{} {}x{}) clamp=({},{} {}x{})",
                            parent_rect.x,
                            parent_rect.y,
                            parent_rect.w,
                            parent_rect.h,
                            clamp.x,
                            clamp.y,
                            clamp.w,
                            clamp.h
                        );
                    } else {
                        warn!(
                            "Skip transient parent clamp because intersection is empty; parent=({},{} {}x{}) clamp=({},{} {}x{})",
                            parent_rect.x,
                            parent_rect.y,
                            parent_rect.w,
                            parent_rect.h,
                            clamp.x,
                            clamp.y,
                            clamp.w,
                            clamp.h
                        );
                    }
                }
            }
        }

        // Clamp to the computed clamp rect (workarea or workarea∩parent).
        let min_x = clamp.x;
        let max_x = clamp.x + clamp.w - client_total_width;
        if min_x <= max_x {
            client_x = client_x.clamp(min_x, max_x);
        } else {
            client_x = min_x;
            warn!(
                "Skip X clamp because max_x({}) < min_x({}); client_total_width={}, clamp_w={}",
                max_x, min_x, client_total_width, clamp.w
            );
        }

        let min_y = clamp.y;
        let max_y = clamp.y + clamp.h - client_total_height;
        if min_y <= max_y {
            client_y = client_y.clamp(min_y, max_y);
        } else {
            client_y = min_y;
            warn!(
                "Skip Y clamp because max_y({}) < min_y({}); client_total_height={}, clamp_h={}",
                max_y, min_y, client_total_height, clamp.h
            );
        }

        // Keep within the monitor bounds as a final guard.
        let min_x = mon_wx;
        let max_x = mon_wx + mon_ww - client_total_width;
        if min_x <= max_x {
            client_x = client_x.clamp(min_x, max_x);
        } else {
            client_x = min_x;
            warn!(
                "Skip X clamp because max_x({}) < min_x({}); client_total_width={}, mon_ww={}",
                max_x, min_x, client_total_width, mon_ww
            );
        }

        let min_y = mon_wy;
        let max_y = mon_wy + mon_wh - client_total_height;
        if min_y <= max_y {
            client_y = client_y.clamp(min_y, max_y);
        } else {
            client_y = min_y;
            warn!(
                "Skip Y clamp because max_y({}) < min_y({}); client_total_height={}, mon_wh={}",
                max_y, min_y, client_total_height, mon_wh
            );
        }
        if let Some(client) = self.state.clients.get_mut(client_key) {
            client.geometry.x = client_x;
            client.geometry.y = client_y;
            info!(
                "Final position: ({}, {}) {}x{}",
                client.geometry.x, client.geometry.y, client.geometry.w, client.geometry.h
            );
        }
    }

    fn unmanage_regular_client(
        &mut self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
        destroyed: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.animations.remove(client_key);
        let win = self.state.clients.get(client_key).map(|c| c.win);
        if let Some(client) = self.state.clients.get(client_key) {
            info!("[unmanage_regular_client] Removing client {}", client);
        }
        self.scratchpads.retain(|_, &mut v| v != client_key);
        let mon_key = self
            .state
            .clients
            .get(client_key)
            .and_then(|client| client.mon);
        if let Some(mon_key) = mon_key {
            self.clear_pertag_references(client_key, mon_key);
        }
        self.detach(client_key);
        self.detachstack(client_key);
        if !destroyed {
            self.cleanup_window_state(backend, client_key)?;
        }
        if let Some(win) = win {
            self.state.win_to_client.remove(&win);
        }
        self.state.clients.remove(client_key);
        self.state.client_order.retain(|&k| k != client_key);
        self.state.client_stack_order.retain(|&k| k != client_key);
        self.focus(backend, None)?;
        self.update_net_client_list(backend)?;
        if let Some(mon_key) = mon_key {
            self.arrange(backend, Some(mon_key));
        }

        Ok(())
    }

    fn clear_pertag_references(&mut self, client_key: ClientKey, mon_key: MonitorKey) {
        if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
            if let Some(ref mut pertag) = monitor.pertag {
                for i in 0..=CONFIG.load().tags_length() {
                    if pertag.sel[i] == Some(client_key) {
                        pertag.sel[i] = None;
                    }
                }
            }
        }
    }

    fn cleanup_window_state(
        &self,
        backend: &mut dyn Backend,
        client_key: ClientKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client = if let Some(client) = self.state.clients.get(client_key) {
            client
        } else {
            return Err("Client not found".into());
        };
        let win = client.win;
        let old_border_w = client.geometry.old_border_w;
        if let Err(e) = backend
            .window_ops()
            .change_event_mask(win, EventMaskBits::NONE.bits())
        {
            warn!("[cleanup_window_state] Failed to clear event mask: {:?}", e);
        }
        let changes = WindowChanges {
            border_width: Some(old_border_w as u32),
            ..Default::default()
        };
        if let Err(e) = backend.window_ops().apply_window_changes(win, changes) {
            log::warn!(
                "[cleanup_window_state] Failed to restore border width: {:?}",
                e
            );
        }
        if let Err(e) = backend.window_ops().ungrab_all_buttons(win) {
            warn!("[cleanup_window_state] Failed to ungrab buttons: {:?}", e);
        }
        if let Err(e) = self.setclientstate(backend, win, WITHDRAWN_STATE as i64) {
            warn!("[cleanup_window_state] Failed to set client state: {:?}", e);
        }

        info!(
            "[cleanup_window_state] Window cleanup completed for {:?}",
            win
        );
        Ok(())
    }

    fn unmapnotify(
        &mut self,
        backend: &mut dyn Backend,
        window: WindowId,
        from_configure: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // info!("[unmapnotify]");
        if let Some(client_key) = self.wintoclient(window) {
            if from_configure {
                debug!("Unmap from configure for window {:?}", window);
                let client = if let Some(client) = self.state.clients.get(client_key) {
                    client
                } else {
                    return Ok(());
                };
                self.setclientstate(backend, client.win, WITHDRAWN_STATE as i64)?;
            } else {
                debug!("Real unmap for window {:?}, unmanaging", window);
                self.unmanage(backend, Some(client_key), false)?;
            }
        } else {
            debug!("Unmap event for unmanaged window: 0{:?}", window);
        }
        Ok(())
    }

    fn updategeom(&mut self, backend: &mut dyn Backend) -> bool {
        info!("[updategeom]");
        let outputs = backend.output_ops().enumerate_outputs();

        let dirty = if outputs.len() <= 1 {
            self.setup_single_monitor()
        } else {
            let mons: Vec<(i32, i32, i32, i32)> = outputs
                .iter()
                .map(|o| (o.x, o.y, o.width, o.height))
                .collect();
            self.setup_multiple_monitors(mons)
        };

        if dirty {
            let root_window = backend.root_window();
            self.state.sel_mon = self.wintomon(backend, root_window);
            if self.state.sel_mon.is_none() && !self.state.monitor_order.is_empty() {
                self.state.sel_mon = self.state.monitor_order.first().copied();
            }
        }
        dirty
    }

    fn setup_single_monitor(&mut self) -> bool {
        let mut dirty = false;

        if self.state.monitor_order.is_empty() {
            let new_monitor = self.createmon(CONFIG.load().show_bar());
            let mon_key = self.insert_monitor(new_monitor);
            self.state.sel_mon = Some(mon_key);
            dirty = true;
        }

        if let Some(&mon_key) = self.state.monitor_order.first() {
            if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
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
        let current_num_monitors = self.state.monitor_order.len();

        if num_detected_monitors > current_num_monitors {
            dirty = true;
            for _ in current_num_monitors..num_detected_monitors {
                let new_monitor = self.createmon(CONFIG.load().show_bar());
                let mon_key = self.insert_monitor(new_monitor);
                info!(
                    "[setup_multiple_monitors] Created new monitor {:?}",
                    mon_key
                );
            }
        }

        for (i, &(x, y, w, h)) in monitors.iter().enumerate() {
            if let Some(&mon_key) = self.state.monitor_order.get(i) {
                if let Some(monitor) = self.state.monitors.get_mut(mon_key) {
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

        if num_detected_monitors < current_num_monitors {
            dirty = true;
            self.remove_excess_monitors(num_detected_monitors);
        }

        dirty
    }

    fn remove_excess_monitors(&mut self, target_count: usize) {
        while self.state.monitor_order.len() > target_count {
            if let Some(mon_key_to_remove) = self.state.monitor_order.pop() {
                self.move_clients_to_first_monitor(mon_key_to_remove);

                if self.state.sel_mon == Some(mon_key_to_remove) {
                    self.state.sel_mon = self.state.monitor_order.first().copied();
                }

                self.state.monitors.remove(mon_key_to_remove);
                self.state.monitor_clients.remove(mon_key_to_remove);
                self.state.monitor_stack.remove(mon_key_to_remove);

                info!(
                    "[remove_excess_monitors] Removed monitor {:?}",
                    mon_key_to_remove
                );
            }
        }
    }

    fn move_clients_to_first_monitor(&mut self, from_monitor_key: MonitorKey) {
        let target_monitor_key = if let Some(&first_mon_key) = self.state.monitor_order.first() {
            first_mon_key
        } else {
            warn!("[move_clients_to_first_monitor] No target monitor available");
            return;
        };

        let clients_to_move: Vec<ClientKey> = self
            .state
            .monitor_clients
            .get(from_monitor_key)
            .cloned()
            .unwrap_or_default();

        let target_tags = if let Some(target_monitor) = self.state.monitors.get(target_monitor_key)
        {
            target_monitor.tag_set[target_monitor.sel_tags]
        } else {
            1
        };

        for client_key in clients_to_move {
            if let Some(client) = self.state.clients.get_mut(client_key) {
                client.mon = Some(target_monitor_key);
                client.state.tags = target_tags;
            }

            self.detach_from_monitor(client_key, from_monitor_key);

            self.attach_to_monitor(client_key, target_monitor_key);

            info!(
                "[move_clients_to_first_monitor] Moved client {:?} from monitor {:?} to {:?}",
                client_key, from_monitor_key, target_monitor_key
            );
        }
    }

    fn updatewindowtype(&mut self, backend: &mut dyn Backend, client_key: ClientKey) {
        // 先获取必要信息，避免借用冲突
        let (win, is_popup_like) = if let Some(client) = self.state.clients.get(client_key) {
            (client.win, self.is_popup_like(backend, client_key))
        } else {
            return;
        };

        let was_floating = self
            .state
            .clients
            .get(client_key)
            .map(|client| client.state.is_floating)
            .unwrap_or(false);

        // 处理全屏
        if backend.property_ops().is_fullscreen(win) {
            let _ = self.setfullscreen(backend, client_key, true);
        }

        // 获取窗口类型
        let types = backend.property_ops().get_window_types(win);
        let is_desktop = types.contains(&WindowType::Desktop);
        let is_dock = types.contains(&WindowType::Dock);
        let is_transient = backend.property_ops().transient_for(win).is_some();

        let layer_info = backend.property_ops().get_layer_surface_info(win);

        // 获取可变引用进行修改
        if let Some(c) = self.state.clients.get_mut(client_key) {
            c.state.is_dock = is_dock;
            c.state.dock_layer_info = if is_dock { layer_info } else { None };

            // 1. 如果是 Popup / Dock / Notification / Desktop
            if is_popup_like || is_desktop {
                c.state.is_floating = true;

                // 如果是 通知、Dock、桌面，则设置为所有标签可见
                // 但如果窗口是 transient（有父窗口），说明是用户交互触发的子窗口，
                // 不应设置 never_focus，否则会导致弹窗失焦后被应用自动关闭
                if types.contains(&WindowType::Notification)
                    || types.contains(&WindowType::Tooltip)
                    || types.contains(&WindowType::Dock)
                    || types.contains(&WindowType::Desktop)
                {
                    if !is_transient {
                        c.state.tags = crate::config::CONFIG.load().tagmask();
                        c.state.never_focus = true;
                    }
                }
            }
        }

        let is_floating_now = self
            .state
            .clients
            .get(client_key)
            .map(|client| client.state.is_floating)
            .unwrap_or(was_floating);
        if is_floating_now != was_floating {
            self.reorder_client_in_monitor_groups(client_key);
        }
    }

    fn updatewmhints(&mut self, backend: &mut dyn Backend, client_key: ClientKey) {
        let win = match self.state.clients.get(client_key) {
            Some(c) => c.win,
            None => return,
        };
        if let Some(hints) = backend.property_ops().get_wm_hints(win) {
            if hints.urgent {
                let is_focused = self.is_client_selected(client_key);
                if is_focused {
                    let _ = backend.property_ops().set_urgent_hint(win, false);
                } else {
                    if let Some(c) = self.state.clients.get_mut(client_key) {
                        c.state.is_urgent = true;
                    }
                }
            } else {
                if let Some(c) = self.state.clients.get_mut(client_key) {
                    c.state.is_urgent = false;
                }
            }
            if let Some(input_ok) = hints.input {
                if let Some(c) = self.state.clients.get_mut(client_key) {
                    c.state.never_focus = !input_ok;
                }
            } else {
                if let Some(c) = self.state.clients.get_mut(client_key) {
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
                error!("Monitor key is None, cannot update bar message.");
                return;
            }
        };

        let monitor = if let Some(monitor) = self.state.monitors.get(mon_key) {
            monitor
        } else {
            error!("Monitor {:?} not found", mon_key);
            return;
        };

        self.message = SharedMessage::default();
        let mut monitor_info_for_message = MonitorInfo::default();

        monitor_info_for_message.monitor_x = monitor.geometry.w_x;
        monitor_info_for_message.monitor_y = monitor.geometry.w_y;
        monitor_info_for_message.monitor_width = monitor.geometry.w_w;
        monitor_info_for_message.monitor_height = monitor.geometry.w_h;
        monitor_info_for_message.monitor_num = monitor.num;
        monitor_info_for_message.set_ltsymbol(&monitor.lt_symbol);

        let (occupied_tags_mask, urgent_tags_mask) = self.calculate_tag_masks(mon_key);

        for i in 0..CONFIG.load().tags_length() {
            let tag_bit = 1 << i;

            let is_filled_tag = self.is_filled_tag(mon_key, tag_bit);

            let monitor = match self.state.monitors.get(mon_key) {
                Some(m) => m,
                None => break,
            };
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
        let selected_client_name = self.get_selected_client_name(mon_key);
        monitor_info_for_message.set_client_name(&selected_client_name);
        self.message.monitor_info = monitor_info_for_message;
    }

    fn calculate_tag_masks(&self, mon_key: MonitorKey) -> (u32, u32) {
        let mut occupied_tags_mask = 0u32;
        let mut urgent_tags_mask = 0u32;

        let config_mask = crate::config::CONFIG.load().tagmask();

        if let Some(client_keys) = self.state.monitor_clients.get(mon_key) {
            for &client_key in client_keys {
                if let Some(client) = self.state.clients.get(client_key) {
                    if Some(client_key) == self.status_bar_client {
                        continue;
                    }

                    let effective_tags = client.state.tags & config_mask;

                    if effective_tags == config_mask {
                        continue;
                    }

                    occupied_tags_mask |= effective_tags;

                    if client.state.is_urgent {
                        urgent_tags_mask |= effective_tags;
                    }
                }
            }
        }

        let final_occupied = occupied_tags_mask & config_mask;
        let final_urgent = urgent_tags_mask & config_mask;

        log::info!(
            "[MaskDebug] Occupied: {:b}, Urgent: {:b}",
            final_occupied,
            final_urgent
        );

        (final_occupied, final_urgent)
    }

    fn is_filled_tag(&self, mon_key: MonitorKey, tag_bit: u32) -> bool {
        // 如果不是当前选中的显示器，不用高亮 Focus 状态
        if self.state.sel_mon != Some(mon_key) {
            return false;
        }

        if let Some(monitor) = self.state.monitors.get(mon_key) {
            if let Some(sel_client_key) = monitor.sel {
                if let Some(client) = self.state.clients.get(sel_client_key) {
                    let mask = crate::config::CONFIG.load().tagmask();

                    if (client.state.tags & mask) == mask {
                        // 策略 A: 直接返回 false。
                        // 视觉效果: 状态栏显示当前 Tag 为 "Selected" (通常是亮色)，
                        // 其他 Tag 恢复为 "Occupied" 或 "Empty"。
                        // 这是最符合直觉的，因为 Sticky 窗口是浮在所有 Tag 之上的。
                        return false;

                        // 策略 B (备选): 只高亮当前 Monitor 正在查看的 Tag
                        // return (monitor.tag_set[monitor.sel_tags] & tag_bit) != 0;
                    }

                    return (client.state.tags & tag_bit) != 0;
                }
            }
        }
        false
    }

    fn get_selected_client_name(&self, mon_key: MonitorKey) -> String {
        if let Some(monitor) = self.state.monitors.get(mon_key) {
            if let Some(sel_client_key) = monitor.sel {
                if let Some(client) = self.state.clients.get(sel_client_key) {
                    return client.name.clone();
                }
            }
        }
        String::new()
    }

    // =========================================================================
    // IPC processing
    // =========================================================================

    fn process_ipc(&mut self, backend: &mut dyn Backend) {
        let ipc = match self.ipc_server.as_mut() {
            Some(s) => s,
            None => return,
        };

        ipc.accept_connections();
        let messages = ipc.poll_clients();

        for msg in messages {
            match msg {
                IncomingIpc::Command { client_id, name, args } => {
                    let resp = self.handle_ipc_command(backend, &name, &args);
                    if let Some(ipc) = self.ipc_server.as_mut() {
                        ipc.respond(client_id, &resp);
                    }
                }
                IncomingIpc::Query { client_id, name, args } => {
                    let resp = self.handle_ipc_query(&name, &args);
                    if let Some(ipc) = self.ipc_server.as_mut() {
                        ipc.respond(client_id, &resp);
                    }
                }
                IncomingIpc::Subscribe { client_id, topics } => {
                    if let Some(ipc) = self.ipc_server.as_mut() {
                        ipc.subscribe(client_id, topics);
                        ipc.respond(client_id, &IpcResponse::ok(None));
                    }
                }
            }
        }
    }

    fn handle_ipc_command(
        &mut self,
        backend: &mut dyn Backend,
        name: &str,
        args: &serde_json::Value,
    ) -> IpcResponse {
        // Special command: reload_config
        if name == "reload_config" {
            return self.do_config_reload(backend);
        }

        match ipc::dispatch_command(name, args) {
            Ok((func, arg)) => {
                match func(self, backend, &arg) {
                    Ok(()) => IpcResponse::ok(None),
                    Err(e) => IpcResponse::err(format!("{e}")),
                }
            }
            Err(e) => IpcResponse::err(e),
        }
    }

    fn handle_ipc_query(&self, name: &str, _args: &serde_json::Value) -> IpcResponse {
        match name {
            "get_windows" => {
                let windows = self.query_windows();
                IpcResponse::ok(Some(serde_json::to_value(windows).unwrap_or_default()))
            }
            "get_workspaces" => {
                let workspaces = self.query_workspaces();
                IpcResponse::ok(Some(serde_json::to_value(workspaces).unwrap_or_default()))
            }
            "get_monitors" => {
                let monitors = self.query_monitors();
                IpcResponse::ok(Some(serde_json::to_value(monitors).unwrap_or_default()))
            }
            "get_tree" => {
                let tree = self.query_tree();
                IpcResponse::ok(Some(serde_json::to_value(tree).unwrap_or_default()))
            }
            "get_config" => {
                let cfg = CONFIG.load();
                IpcResponse::ok(Some(serde_json::json!({
                    "border_px": cfg.border_px(),
                    "gap_px": cfg.gap_px(),
                    "snap": cfg.snap(),
                    "m_fact": cfg.m_fact(),
                    "n_master": cfg.n_master(),
                    "tags_length": cfg.tags_length(),
                    "show_bar": cfg.show_bar(),
                })))
            }
            "get_version" => {
                IpcResponse::ok(Some(serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "name": "jwm",
                })))
            }
            _ => IpcResponse::err(format!("unknown query: {name}")),
        }
    }

    // -------------------------------------------------------------------------
    // Query helpers
    // -------------------------------------------------------------------------

    fn query_windows(&self) -> Vec<WindowInfo> {
        let sel_client = self.get_selected_client_key();
        self.state.client_order.iter().filter_map(|&ck| {
            let c = self.state.clients.get(ck)?;
            if Some(c.win) == self.status_bar_window {
                return None;
            }
            Some(WindowInfo {
                id: c.win.raw(),
                name: c.name.clone(),
                class: c.class.clone(),
                instance: c.instance.clone(),
                tags: c.state.tags,
                monitor: c.monitor_num as i32,
                x: c.geometry.x,
                y: c.geometry.y,
                w: c.geometry.w,
                h: c.geometry.h,
                is_floating: c.state.is_floating,
                is_fullscreen: c.state.is_fullscreen,
                is_urgent: c.state.is_urgent,
                is_sticky: c.state.is_sticky,
                is_focused: sel_client == Some(ck),
            })
        }).collect()
    }

    fn query_workspaces(&self) -> Vec<WorkspaceInfo> {
        let cfg = CONFIG.load();
        let mut result = Vec::new();
        for &mk in &self.state.monitor_order {
            let mon = match self.state.monitors.get(mk) {
                Some(m) => m,
                None => continue,
            };
            let active_tags = mon.tag_set[mon.sel_tags];
            let client_count = self.state.monitor_clients.get(mk)
                .map(|v| v.len())
                .unwrap_or(0);
            for i in 0..cfg.tags_length() {
                let tag_bit = 1u32 << i;
                let is_active = (active_tags & tag_bit) != 0;
                result.push(WorkspaceInfo {
                    tag_mask: tag_bit,
                    tag_index: i,
                    monitor: mon.num,
                    layout: format!("{:?}", *mon.lt[mon.sel_lt]),
                    m_fact: mon.layout.m_fact,
                    n_master: mon.layout.n_master,
                    num_clients: if is_active { client_count } else { 0 },
                    focused: is_active && self.state.sel_mon == Some(mk),
                });
            }
        }
        result
    }

    fn query_monitors(&self) -> Vec<MonitorInfoIpc> {
        self.state.monitor_order.iter().filter_map(|&mk| {
            let m = self.state.monitors.get(mk)?;
            Some(MonitorInfoIpc {
                num: m.num,
                x: m.geometry.m_x,
                y: m.geometry.m_y,
                w: m.geometry.m_w,
                h: m.geometry.m_h,
                active_tags: m.tag_set[m.sel_tags],
                layout: format!("{:?}", *m.lt[m.sel_lt]),
                focused: self.state.sel_mon == Some(mk),
            })
        }).collect()
    }

    fn query_tree(&self) -> Vec<TreeNode> {
        self.state.monitor_order.iter().filter_map(|&mk| {
            let m = self.state.monitors.get(mk)?;
            let sel_client = m.sel;
            let windows: Vec<WindowInfo> = self.state.monitor_clients.get(mk)
                .map(|clients| {
                    clients.iter().filter_map(|&ck| {
                        let c = self.state.clients.get(ck)?;
                        if Some(c.win) == self.status_bar_window {
                            return None;
                        }
                        Some(WindowInfo {
                            id: c.win.raw(),
                            name: c.name.clone(),
                            class: c.class.clone(),
                            instance: c.instance.clone(),
                            tags: c.state.tags,
                            monitor: c.monitor_num as i32,
                            x: c.geometry.x,
                            y: c.geometry.y,
                            w: c.geometry.w,
                            h: c.geometry.h,
                            is_floating: c.state.is_floating,
                            is_fullscreen: c.state.is_fullscreen,
                            is_urgent: c.state.is_urgent,
                            is_sticky: c.state.is_sticky,
                            is_focused: sel_client == Some(ck),
                        })
                    }).collect()
                })
                .unwrap_or_default();
            Some(TreeNode {
                monitor: MonitorInfoIpc {
                    num: m.num,
                    x: m.geometry.m_x,
                    y: m.geometry.m_y,
                    w: m.geometry.m_w,
                    h: m.geometry.m_h,
                    active_tags: m.tag_set[m.sel_tags],
                    layout: format!("{:?}", *m.lt[m.sel_lt]),
                    focused: self.state.sel_mon == Some(mk),
                },
                windows,
            })
        }).collect()
    }

    // =========================================================================
    // IPC event broadcast helper
    // =========================================================================

    pub fn broadcast_ipc_event(&mut self, event_type: &str, payload: serde_json::Value) {
        if let Some(ipc) = self.ipc_server.as_mut() {
            ipc.broadcast(&IpcEvent {
                event: event_type.to_string(),
                payload,
            });
        }
    }

    // =========================================================================
    // Config hot-reload
    // =========================================================================

    fn check_config_reload(&mut self, backend: &mut dyn Backend) {
        let now = Instant::now();

        // Check if file modification time changed
        if let Ok(mtime) = crate::config::Config::get_config_modified_time() {
            if self.config_last_modified != Some(mtime) {
                // File changed — start or restart debounce timer
                self.config_last_modified = Some(mtime);
                self.config_reload_debounce = Some(now);
            }
        }

        // Process debounced reload
        if let Some(debounce_start) = self.config_reload_debounce {
            if now.duration_since(debounce_start) >= Duration::from_millis(300) {
                self.config_reload_debounce = None;
                info!("[config] detected config file change, reloading");
                let resp = self.do_config_reload(backend);
                if resp.success {
                    info!("[config] reload successful");
                } else {
                    warn!("[config] reload failed: {:?}", resp.error);
                }
            }
        }
    }

    fn do_config_reload(&mut self, backend: &mut dyn Backend) -> IpcResponse {
        match crate::config::reload_global() {
            Ok(()) => {
                self.apply_config_changes(backend);
                self.broadcast_ipc_event("config/reload", serde_json::json!({}));
                IpcResponse::ok(None)
            }
            Err(e) => IpcResponse::err(format!("config reload failed: {e}")),
        }
    }

    fn apply_config_changes(&mut self, backend: &mut dyn Backend) {
        let cfg = CONFIG.load();

        // 1. Rebind keys
        self.key_bindings = cfg.get_keys();
        if let Err(e) = self.grabkeys(backend) {
            warn!("[config] failed to re-grab keys: {e}");
        }

        // 2. Re-apply color schemes
        let colors = cfg.colors();
        let alloc = backend.color_allocator();
        let _ = alloc.free_all_theme_pixels();
        if let (Ok(norm_fg), Ok(norm_bg), Ok(norm_border)) = (
            ArgbColor::from_hex(&colors.dark_sea_green1, colors.opaque),
            ArgbColor::from_hex(&colors.light_sky_blue1, colors.opaque),
            ArgbColor::from_hex(&colors.light_sky_blue1, colors.opaque),
        ) {
            alloc.set_scheme(SchemeType::Norm, ColorScheme::new(norm_fg, norm_bg, norm_border));
        }
        if let (Ok(sel_fg), Ok(sel_bg), Ok(sel_border)) = (
            ArgbColor::from_hex(&colors.dark_sea_green2, colors.opaque),
            ArgbColor::from_hex(&colors.pale_turquoise1, colors.opaque),
            ArgbColor::from_hex(&colors.cyan, colors.opaque),
        ) {
            alloc.set_scheme(SchemeType::Sel, ColorScheme::new(sel_fg, sel_bg, sel_border));
        }
        let _ = alloc.allocate_schemes_pixels();

        // 3. Re-arrange all monitors (border/gap changes take effect)
        let mon_keys: Vec<MonitorKey> = self.state.monitor_order.clone();
        for mk in &mon_keys {
            self.arrange(backend, Some(*mk));
        }

        // 4. Update decoration on all visible clients
        let sel_ck = self.get_selected_client_key();
        let client_keys: Vec<ClientKey> = self.state.client_order.clone();
        for ck in client_keys {
            if let Some(client) = self.state.clients.get(ck) {
                if Some(client.win) != self.status_bar_window {
                    let is_sel = sel_ck == Some(ck);
                    let _ = self.update_client_decoration(backend, ck, is_sel);
                }
            }
        }
    }
}
