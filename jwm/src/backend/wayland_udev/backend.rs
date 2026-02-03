use crate::backend::wayland_dummy_ops::*;
use crate::backend::wayland_key_ops::UdevKeyOps;
use crate::backend::wayland::state::JwmWaylandState;

#[path = "../udev_kms.rs"]
mod kms;
use self::kms::KmsState;
use crate::backend::api::{
    Backend, BackendEvent, Capabilities, ColorAllocator, CursorProvider, EventHandler, HitTarget,
    InputOps, KeyOps, OutputInfo, OutputOps, PropertyOps, ScreenInfo, WindowOps, WindowType,
};
use crate::backend::common_define::{Mods, OutputId, WindowId};
use crate::backend::error::BackendError;
use crate::config::CONFIG;

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use drm::control::{connector, Device as ControlDevice, ModeTypeFlags};

use smithay::backend::input::{
    AbsolutePositionEvent, Event as InputEventExt, InputEvent, KeyboardKeyEvent, PointerButtonEvent,
    PointerMotionEvent,
};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Event as SessionEvent;
use smithay::backend::session::Session;
use smithay::backend::udev::{UdevBackend as SmithayUdevBackend, UdevEvent};
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay::reexports::calloop::channel::{self, Sender};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{Timer, TimeoutAction};
use smithay::reexports::input::Libinput;
use smithay::reexports::wayland_server::{Display, DisplayHandle};
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, SERIAL_COUNTER as SCOUNTER};
use smithay::input::keyboard::{FilterResult, ModifiersState};
use smithay::input::pointer::{ButtonEvent, MotionEvent};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::desktop::layer_map_for_output;
use smithay::wayland::shell::wlr_layer::{KeyboardInteractivity, Layer as WlrLayer};

struct SendWrapper<T>(T);

unsafe impl<T> Send for SendWrapper<T> {}

impl<T> std::ops::Deref for SendWrapper<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for SendWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

struct SharedState {
    pointer_x: f64,
    pointer_y: f64,
    mods_state: u16,
    /// Cached key bindings (mods, keysym) for key event suppression.
    key_bindings: Vec<(crate::backend::common_define::Mods, crate::backend::common_define::KeySym)>,
    /// xkb keycode (0..=255) -> base (unmodified) keysym.
    keysym_table: Vec<crate::backend::common_define::KeySym>,
    /// xkb keycodes that were intercepted on press and should be intercepted on release.
    suppressed_keycodes: HashSet<u8>,
    outputs: Vec<OutputInfo>,
    output_key_to_id: HashMap<u64, OutputId>,
    next_output_raw: u64,
    device_paths: HashMap<u64, PathBuf>,

    kms_needs_reinit: bool,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            pointer_x: 0.0,
            pointer_y: 0.0,
            mods_state: 0,
            key_bindings: Vec::new(),
            keysym_table: vec![0; 256],
            suppressed_keycodes: HashSet::new(),
            outputs: Vec::new(),
            output_key_to_id: HashMap::new(),
            next_output_raw: 0,
            device_paths: HashMap::new(),

            kms_needs_reinit: false,
        }
    }
}

struct UdevOutputOps {
    shared: Arc<Mutex<SharedState>>,
}

impl OutputOps for UdevOutputOps {
    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        self.shared.lock().unwrap().outputs.clone()
    }

    fn screen_info(&self) -> ScreenInfo {
        let shared = self.shared.lock().unwrap();
        let mut w = 0i32;
        let mut h = 0i32;
        for o in &shared.outputs {
            w = w.max(o.x + o.width);
            h = h.max(o.y + o.height);
        }
        if w == 0 {
            w = 1920;
        }
        if h == 0 {
            h = 1080;
        }
        ScreenInfo { width: w, height: h }
    }

    fn output_at(&self, x: i32, y: i32) -> Option<OutputId> {
        let shared = self.shared.lock().unwrap();
        shared
            .outputs
            .iter()
            .find(|o| x >= o.x && y >= o.y && x < (o.x + o.width) && y < (o.y + o.height))
            .map(|o| o.id)
    }
}

struct UdevInputOps {
    shared: Arc<Mutex<SharedState>>,
}

impl InputOps for UdevInputOps {
    fn set_cursor(&self, _kind: crate::backend::common_define::StdCursorKind) -> Result<(), BackendError> {
        Ok(())
    }

    fn get_pointer_position(&self) -> Result<(f64, f64), BackendError> {
        let shared = self.shared.lock().unwrap();
        Ok((shared.pointer_x, shared.pointer_y))
    }

    fn grab_pointer(&self, _mask: u32, _cursor: Option<u64>) -> Result<bool, BackendError> {
        Ok(true)
    }

    fn ungrab_pointer(&self) -> Result<(), BackendError> {
        Ok(())
    }

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), BackendError> {
        let shared = self.shared.lock().unwrap();
        Ok((
            shared.pointer_x as i32,
            shared.pointer_y as i32,
            shared.mods_state,
            0,
        ))
    }
}

struct WaylandWindowOps {
    state: SendWrapper<*mut JwmWaylandState>,

    flush_tx: Sender<()>,
    flush_pending: Arc<AtomicBool>,
}

unsafe impl Send for WaylandWindowOps {}

impl WaylandWindowOps {
    unsafe fn with_state_mut<R>(&self, f: impl FnOnce(&mut JwmWaylandState) -> R) -> R {
        // Safety: We only ever call this from the compositor thread.
        unsafe { f(&mut *self.state.0) }
    }

    fn request_flush(&self) {
        if !self.flush_pending.swap(true, Ordering::SeqCst) {
            let _ = self.flush_tx.send(());
        }
    }
}

impl WindowOps for WaylandWindowOps {
    fn set_position(&self, _win: WindowId, _x: i32, _y: i32) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                if let Some(geo) = state.window_geometry.get_mut(&_win) {
                    geo.x = _x;
                    geo.y = _y;
                }
                state.needs_redraw = true;
            });
        }
        self.request_flush();
        Ok(())
    }

    fn configure(
        &self,
        win: WindowId,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        border: u32,
    ) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                state.pending_initial_configure.remove(&win);
                state.window_geometry.insert(
                    win,
                    crate::backend::api::Geometry {
                        x,
                        y,
                        w,
                        h,
                        border,
                    },
                );
                if let Some(toplevel) = state.try_lookup_toplevel(win) {
                    toplevel.with_pending_state(|s| {
                        s.size = Some((w as i32, h as i32).into());
                    });
                    toplevel.send_configure();
                }
                state.reconstrain_popups_for_toplevel(win);
                state.needs_redraw = true;
            });
        }
        self.request_flush();
        Ok(())
    }

    fn set_decoration_style(
        &self,
        _win: WindowId,
        _border_width: u32,
        _border_color: crate::backend::common_define::Pixel,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn raise_window(&self, _win: WindowId) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                if let Some(pos) = state.window_stack.iter().position(|w| *w == _win) {
                    state.window_stack.remove(pos);
                    state.window_stack.push(_win);
                }
                state.needs_redraw = true;
            });
        }
        self.request_flush();
        Ok(())
    }
    fn map_window(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn unmap_window(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn close_window(&self, _win: WindowId) -> Result<crate::backend::api::CloseResult, BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                if let Some(toplevel) = state.try_lookup_toplevel(_win) {
                    toplevel.send_close();
                }
            });
        }
        self.request_flush();
        Ok(crate::backend::api::CloseResult::Graceful)
    }
    fn set_input_focus(&self, _win: WindowId) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                let serial = SCOUNTER.next_serial();
                if let Some(surface) = state.surface_for_window(_win) {
                    if let Some(kbd) = state.seat.get_keyboard() {
                        kbd.set_focus(state, Some(surface), serial);
                    }
                }
            });
        }
        self.request_flush();
        Ok(())
    }
    fn set_input_focus_root(&self) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                let serial = SCOUNTER.next_serial();
                if let Some(kbd) = state.seat.get_keyboard() {
                    kbd.set_focus(state, None, serial);
                }
            });
        }
        self.request_flush();
        Ok(())
    }
    fn get_window_attributes(&self, _win: WindowId) -> Result<crate::backend::api::WindowAttributes, BackendError> {
        let viewable = unsafe { self.with_state_mut(|state| state.mapped_windows.contains(&_win)) };
        Ok(crate::backend::api::WindowAttributes {
            override_redirect: false,
            map_state_viewable: viewable,
        })
    }
    fn get_geometry(&self, _win: WindowId) -> Result<crate::backend::api::Geometry, BackendError> {
        let geo = unsafe { self.with_state_mut(|state| state.window_geometry.get(&_win).copied()) };
        Ok(geo.unwrap_or_default())
    }
    fn scan_windows(&self) -> Result<Vec<WindowId>, BackendError> {
        let wins = unsafe { self.with_state_mut(|state| state.window_stack.clone()) };
        Ok(wins)
    }
    fn flush(&self) -> Result<(), BackendError> {
        self.request_flush();
        Ok(())
    }
    fn kill_client(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn apply_window_changes(
        &self,
        _win: WindowId,
        _changes: crate::backend::api::WindowChanges,
    ) -> Result<(), BackendError> {
        let current = self.get_geometry(_win)?;
        let x = _changes.x.unwrap_or(current.x);
        let y = _changes.y.unwrap_or(current.y);
        let w = _changes.width.unwrap_or(current.w);
        let h = _changes.height.unwrap_or(current.h);
        let border = _changes.border_width.unwrap_or(current.border);
        self.configure(_win, x, y, w, h, border)
    }
}

struct WaylandPropertyOps {
    state: SendWrapper<*mut JwmWaylandState>,

    flush_tx: Sender<()>,
    flush_pending: Arc<AtomicBool>,
}

unsafe impl Send for WaylandPropertyOps {}

impl WaylandPropertyOps {
    unsafe fn with_state_mut<R>(&self, f: impl FnOnce(&mut JwmWaylandState) -> R) -> R {
        unsafe { f(&mut *self.state.0) }
    }

    fn request_flush(&self) {
        if !self.flush_pending.swap(true, Ordering::SeqCst) {
            let _ = self.flush_tx.send(());
        }
    }
}

impl PropertyOps for WaylandPropertyOps {
    fn get_title(&self, win: WindowId) -> String {
        unsafe { self.with_state_mut(|state| state.window_title.get(&win).cloned()) }
            .unwrap_or_else(|| "Wayland Window".to_string())
    }

    fn get_class(&self, win: WindowId) -> (String, String) {
        let app_id = unsafe { self.with_state_mut(|state| state.window_app_id.get(&win).cloned()) }
            .unwrap_or_else(|| "app".to_string());
        (app_id.clone(), app_id)
    }

    fn get_window_types(&self, win: WindowId) -> Vec<WindowType> {
        // Best-effort classification so JWM can treat status bars/docks correctly.
        // For layer-shell surfaces, exclusive_zone is the canonical hint.
        let (title, app_id, layer_info) = unsafe {
            self.with_state_mut(|state| {
                (
                    state.window_title.get(&win).cloned().unwrap_or_default(),
                    state.window_app_id.get(&win).cloned().unwrap_or_default(),
                    state.window_layer_info.get(&win).copied(),
                )
            })
        };

        if let Some(info) = layer_info {
            if info.exclusive_zone != 0 {
                return vec![WindowType::Dock];
            }
        }

        let bar_name = crate::config::CONFIG.status_bar_name();
        if !bar_name.is_empty() && (app_id == bar_name || title == bar_name) {
            return vec![WindowType::Dock];
        }

        vec![WindowType::Normal]
    }

    fn get_layer_surface_info(&self, win: WindowId) -> Option<crate::backend::api::LayerSurfaceInfo> {
        unsafe { self.with_state_mut(|state| state.window_layer_info.get(&win).copied()) }
    }

    fn is_fullscreen(&self, win: WindowId) -> bool {
        unsafe { self.with_state_mut(|state| state.window_is_fullscreen.get(&win).copied()) }
            .unwrap_or(false)
    }

    fn set_fullscreen_state(&self, win: WindowId, on: bool) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                state.window_is_fullscreen.insert(win, on);
                if let Some(toplevel) = state.try_lookup_toplevel(win) {
                    toplevel.with_pending_state(|s| {
                        if on {
                            s.states.set(xdg_toplevel::State::Fullscreen);
                        } else {
                            s.states.unset(xdg_toplevel::State::Fullscreen);
                            s.fullscreen_output = None;
                        }
                    });
                    toplevel.send_configure();
                }
            });
        }
        self.request_flush();
        Ok(())
    }

    fn transient_for(&self, _win: WindowId) -> Option<WindowId> {
        unsafe {
            self.with_state_mut(|state| {
                let toplevel = state.toplevels.get(&_win)?;
                let parent_surface = toplevel.parent()?;
                state.surface_to_window.get(&parent_surface.id()).copied()
            })
        }
    }

    fn get_wm_hints(&self, _win: WindowId) -> Option<crate::backend::api::WmHints> {
        None
    }

    fn set_urgent_hint(&self, _win: WindowId, _urgent: bool) -> Result<(), BackendError> {
        Ok(())
    }

    fn fetch_normal_hints(
        &self,
        _win: WindowId,
    ) -> Result<Option<crate::backend::api::NormalHints>, BackendError> {
        Ok(None)
    }

    fn set_window_strut_top(
        &self,
        _win: WindowId,
        _top: u32,
        _start_x: u32,
        _end_x: u32,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn clear_window_strut(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }

    fn get_wm_state(&self, _win: WindowId) -> Result<i64, BackendError> {
        Ok(1)
    }

    fn set_wm_state(&self, _win: WindowId, _state: i64) -> Result<(), BackendError> {
        Ok(())
    }

    fn set_client_info_props(
        &self,
        _win: WindowId,
        _tags: u32,
        _monitor_num: u32,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}

pub struct UdevBackend {
    display_handle: DisplayHandle,
    event_loop: SendWrapper<EventLoop<'static, JwmWaylandState>>,
    state: Box<JwmWaylandState>,
    #[allow(dead_code)]
    socket_name: Option<String>,
    pending_events: Arc<Mutex<VecDeque<BackendEvent>>>,

    flush_tx: Sender<()>,
    flush_pending: Arc<AtomicBool>,

    shared: Arc<Mutex<SharedState>>,
    session: LibSeatSession,

    kms: Option<Rc<RefCell<KmsState>>>,

    window_ops: Box<dyn WindowOps>,
    input_ops: Box<dyn InputOps>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,
    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,
}

// NOTE: smithay state + calloop handle types are not thread-safe.
// JWM runs the backend on the main thread only, so this is acceptable as long as
// `UdevBackend` is never moved across threads.
unsafe impl Send for UdevBackend {}

impl UdevBackend {
    fn request_flush(&self) {
        if !self.flush_pending.swap(true, Ordering::SeqCst) {
            let _ = self.flush_tx.send(());
        }
    }

    fn maybe_reinit_kms(&mut self) {
        let should = {
            let mut s = self.shared.lock().unwrap();
            if s.kms_needs_reinit {
                s.kms_needs_reinit = false;
                true
            } else {
                false
            }
        };
        if !should {
            return;
        }

        let selected = {
            let s = self.shared.lock().unwrap();
            s.device_paths
                .iter()
                .min_by_key(|(id, _)| *id)
                .map(|(id, p)| (*id, p.clone()))
        };

        let Some((dev_id, dev_path)) = selected else {
            // No DRM devices; drop KMS if any.
            if let Some(old) = self.kms.take() {
                if let Some(token) = old.borrow_mut().registration_token.take() {
                    let _ = self.event_loop.handle().remove(token);
                }
            }
            return;
        };

        let output_layout: std::collections::HashMap<u64, (i32, i32)> = {
            let s = self.shared.lock().unwrap();
            let mut id_to_key: HashMap<OutputId, u64> = HashMap::new();
            for (key, id) in &s.output_key_to_id {
                id_to_key.insert(*id, *key);
            }
            let mut layout = std::collections::HashMap::new();
            for o in &s.outputs {
                if let Some(key) = id_to_key.get(&o.id) {
                    layout.insert(*key, (o.x, o.y));
                }
            }
            layout
        };

        let display_handle = self.display_handle.clone();
        match KmsState::new(
            &mut self.session,
            &dev_path,
            dev_id,
            &output_layout,
            &display_handle,
            self.flush_tx.clone(),
            self.flush_pending.clone(),
            self.event_loop.handle(),
        ) {
            Ok(new_kms) => {
                // Remove old notifier after new one is registered.
                if let Some(old) = self.kms.take() {
                    if let Some(token) = old.borrow_mut().registration_token.take() {
                        let _ = self.event_loop.handle().remove(token);
                    }
                }

                self.kms = Some(new_kms);
                self.state.needs_redraw = true;
                self.state.outputs = self
                    .kms
                    .as_ref()
                    .map(|k| k.borrow().outputs())
                    .unwrap_or_default();
                self.request_flush();
            }
            Err(err) => {
                log::warn!("KMS re-init failed (keeping previous state): {err}");
            }
        }
    }

    pub fn new() -> Result<Self, BackendError> {
        let event_loop: EventLoop<'static, JwmWaylandState> =
            EventLoop::try_new().map_err(|e| BackendError::Other(Box::new(e)))?;
        let display = Rc::new(RefCell::new(
            Display::new().map_err(|e| BackendError::Other(Box::new(e)))?,
        ));
        let display_handle = display.borrow().handle();

        // Flush outgoing Wayland messages on demand (e.g. vblank-driven frame callbacks).
        // Coalesce requests to avoid piling up flush messages under heavy input/vblank.
        let (flush_tx, flush_rx) = channel::channel::<()>();
        let flush_pending = Arc::new(AtomicBool::new(false));
        {
            let display = display.clone();
            let flush_pending = flush_pending.clone();
            event_loop
                .handle()
                .insert_source(flush_rx, move |_, _, _state| {
                    if let Err(err) = display.borrow_mut().flush_clients() {
                        log::debug!("wayland flush_clients failed: {err:?}");
                    }
                    flush_pending.store(false, Ordering::SeqCst);
                })
                .map_err(|e| {
                    BackendError::Message(format!(
                        "calloop insert_source(wayland flush) failed: {e}"
                    ))
                })?;
        }

        // Wake the event loop on Wayland client requests, and dispatch them immediately.
        // We duplicate the display poll fd so the event source doesn't need to own `Display`.
        let wayland_poll_fd = {
            use std::os::fd::AsFd as _;
            smithay::reexports::rustix::io::dup(display.borrow().as_fd())
                .map_err(|e| BackendError::Other(Box::new(e)))?
        };
        {
            let display = display.clone();
            let flush_tx = flush_tx.clone();
            let flush_pending = flush_pending.clone();
            event_loop
                .handle()
                .insert_source(
                    Generic::new(wayland_poll_fd, Interest::READ, Mode::Level),
                    move |_, _, state| {
                        if let Err(err) = display.borrow_mut().dispatch_clients(state) {
                            log::warn!("wayland dispatch_clients failed: {err:?}");
                        }
                        if !flush_pending.swap(true, Ordering::SeqCst) {
                            let _ = flush_tx.send(());
                        }
                        Ok(PostAction::Continue)
                    },
                )
                .map_err(|e| BackendError::Message(format!("calloop insert_source(wayland display) failed: {e}")))?;
        }

        // Safety net: if the WM doesn't configure a new toplevel quickly enough, clients can stall
        // forever waiting for the initial xdg_toplevel configure. Keep a small timeout-based
        // fallback to ensure we eventually send one.
        {
            let initial_configure_timeout = Duration::from_millis(250);
            let tick = Duration::from_millis(50);
            let timer = Timer::from_duration(tick);
            event_loop
                .handle()
                .insert_source(timer, move |_, _, state| {
                    state.ensure_initial_configure_timeout(initial_configure_timeout);
                    TimeoutAction::ToDuration(tick)
                })
                .map_err(|e| {
                    BackendError::Message(format!(
                        "calloop insert_source(initial configure timer) failed: {e}"
                    ))
                })?;
        }

        let shared = Arc::new(Mutex::new(SharedState::default()));
        let pending_events = Arc::new(Mutex::new(VecDeque::<BackendEvent>::new()));

        // Prepare key binding suppression table (like X11 grabs) for the udev/Wayland path.
        // We match against the same (mods, keysym) pair that JWM uses for shortcuts.
        {
            let allowed_mods = crate::backend::common_define::Mods::SHIFT
                | crate::backend::common_define::Mods::CONTROL
                | crate::backend::common_define::Mods::ALT
                | crate::backend::common_define::Mods::SUPER
                | crate::backend::common_define::Mods::MOD2
                | crate::backend::common_define::Mods::MOD3
                | crate::backend::common_define::Mods::MOD5;

            let key_bindings = CONFIG
                .get_keys()
                .into_iter()
                .map(|k| (k.mask & allowed_mods, k.key_sym))
                .collect::<Vec<_>>();

            let mut s = shared.lock().unwrap();
            s.key_bindings = key_bindings;
        }

        let (mut session, notifier) =
            LibSeatSession::new().map_err(|e| BackendError::Other(Box::new(e)))?;
        let seat_name = session.seat();

        let (wayland_state, socket_name) = JwmWaylandState::init(
            &display_handle,
            event_loop.handle(),
            pending_events.clone(),
            seat_name.clone(),
            true,
        )
        .map_err(|e| BackendError::Message(format!("wayland init failed: {e}")))?;

        if let Some(name) = socket_name.as_deref() {
            // SAFETY: JWM's backend is single-threaded and we set this once at startup.
            unsafe {
                std::env::set_var("WAYLAND_DISPLAY", name);
            }
        }
        let mut state = Box::new(wayland_state);

        let udev_backend = SmithayUdevBackend::new(&seat_name)
            .map_err(|e| BackendError::Other(Box::new(io::Error::new(io::ErrorKind::Other, format!("udev init failed: {e:?}")))))?;

        let mut libinput_context = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
            session.clone().into(),
        );
        libinput_context
            .udev_assign_seat(&seat_name)
            .map_err(|e| BackendError::Other(Box::new(io::Error::new(io::ErrorKind::Other, format!("libinput udev_assign_seat failed: {e:?}")))))?;
        let libinput_backend = LibinputInputBackend::new(libinput_context.clone());

        {
            let mut shared_guard = shared.lock().unwrap();
            for (device_id, path) in udev_backend.device_list() {
                shared_guard
                    .device_paths
                    .insert(device_id, path.to_path_buf());
            }
        }
        rebuild_outputs(&shared, &pending_events)?;

        // Keep a copy of output geometries in the Wayland state for popup constraining.
        {
            let s = shared.lock().unwrap();
            state.output_rects = s
                .outputs
                .iter()
                .map(|o| smithay::utils::Rectangle::new((o.x, o.y).into(), (o.width, o.height).into()))
                .collect();
        }

        // Minimal visible output: initialize KMS and render a solid background.
        // If this fails (e.g. missing permissions / no DRM device), keep running headless.
        let kms = {
            let selected = {
                let s = shared.lock().unwrap();
                s.device_paths
                    .iter()
                    .min_by_key(|(id, _)| *id)
                    .map(|(id, p)| (*id, p.clone()))
            };

            match selected {
                Some((dev_id, p)) => {
                    let output_layout: std::collections::HashMap<u64, (i32, i32)> = {
                        let s = shared.lock().unwrap();
                        let mut id_to_key: HashMap<OutputId, u64> = HashMap::new();
                        for (key, id) in &s.output_key_to_id {
                            id_to_key.insert(*id, *key);
                        }
                        let mut layout = std::collections::HashMap::new();
                        for o in &s.outputs {
                            if let Some(key) = id_to_key.get(&o.id) {
                                layout.insert(*key, (o.x, o.y));
                            }
                        }
                        layout
                    };

                    match KmsState::new(
                        &mut session,
                        &p,
                        dev_id,
                        &output_layout,
                        &display_handle,
                        flush_tx.clone(),
                        flush_pending.clone(),
                        event_loop.handle(),
                    ) {
                        Ok(kms) => Some(kms),
                        Err(err) => {
                            log::warn!("KMS init failed (running headless): {err}");
                            None
                        }
                    }
                }
                None => None,
            }
        };

        if let Some(kms) = &kms {
            state.outputs = kms.borrow().outputs();
        }

        {
            let pending_events = pending_events.clone();
            let shared = shared.clone();
            let flush_tx = flush_tx.clone();
            let flush_pending = flush_pending.clone();
            event_loop
                .handle()
                .insert_source(libinput_backend, move |event, _, state| {
                    match event {
                        InputEvent::PointerMotion { event, .. } => {
                            let delta = event.delta();
                            let time = event.time_msec();
                            let (x, y, output) = {
                                let mut s = shared.lock().unwrap();
                                s.pointer_x += delta.x;
                                s.pointer_y += delta.y;
                                let x = s.pointer_x;
                                let y = s.pointer_y;
                                let output = s
                                    .outputs
                                    .iter()
                                    .find(|o| (x as i32) >= o.x && (y as i32) >= o.y && (x as i32) < (o.x + o.width) && (y as i32) < (o.y + o.height))
                                    .map(|o| o.id);
                                (x, y, output)
                            };

                            let location: Point<f64, Logical> = (x, y).into();
                            state.pointer_location = location;
                            state.needs_redraw = true;

                            // If a popup grab is active, leaving the grab area dismisses the popups.
                            if let Some(grab_win) = state.popup_grab_toplevel {
                                if let Some(area) = state.popup_grab_area(grab_win) {
                                    let px = location.x.round() as i32;
                                    let py = location.y.round() as i32;
                                    let inside = px >= area.loc.x
                                        && py >= area.loc.y
                                        && px < area.loc.x + area.size.w
                                        && py < area.loc.y + area.size.h;
                                    if !inside {
                                        state.dismiss_popups_for_toplevel(grab_win);
                                        state.needs_redraw = true;
                                    }
                                }
                            }

                            let under = state.surface_under(location);
                            let hit = under
                                .as_ref()
                                .and_then(|(win, _, _)| win.map(HitTarget::Surface));
                            let focus = under.map(|(_win, surface, origin)| (surface, origin));

                            if let Some(pointer) = state.seat.get_pointer() {
                                pointer.motion(
                                    state,
                                    focus,
                                    &MotionEvent {
                                        location,
                                        serial: SCOUNTER.next_serial(),
                                        time,
                                    },
                                );
                                pointer.frame(state);
                            }

                            pending_events.lock().unwrap().push_back(BackendEvent::MotionNotify {
                                target: hit.unwrap_or(HitTarget::Background { output }),
                                root_x: x,
                                root_y: y,
                                time,
                            });
                        }
                        InputEvent::PointerMotionAbsolute { event, .. } => {
                            let time = event.time_msec();
                            let (x, y, output) = {
                                let mut s = shared.lock().unwrap();
                                let (w, h, origin_x, origin_y, output) = if let Some(first) = s.outputs.first() {
                                    (first.width.max(1) as i32, first.height.max(1) as i32, first.x, first.y, Some(first.id))
                                } else {
                                    (1920, 1080, 0, 0, None)
                                };
                                let pos = event.position_transformed(smithay::utils::Size::from((w, h)));
                                s.pointer_x = origin_x as f64 + pos.x;
                                s.pointer_y = origin_y as f64 + pos.y;
                                (s.pointer_x, s.pointer_y, output)
                            };

                            let location: Point<f64, Logical> = (x, y).into();
                            state.pointer_location = location;
                            state.needs_redraw = true;

                            // If a popup grab is active, leaving the grab area dismisses the popups.
                            if let Some(grab_win) = state.popup_grab_toplevel {
                                if let Some(area) = state.popup_grab_area(grab_win) {
                                    let px = location.x.round() as i32;
                                    let py = location.y.round() as i32;
                                    let inside = px >= area.loc.x
                                        && py >= area.loc.y
                                        && px < area.loc.x + area.size.w
                                        && py < area.loc.y + area.size.h;
                                    if !inside {
                                        state.dismiss_popups_for_toplevel(grab_win);
                                        state.needs_redraw = true;
                                    }
                                }
                            }

                            let under = state.surface_under(location);
                            let hit = under
                                .as_ref()
                                .and_then(|(win, _, _)| win.map(HitTarget::Surface));
                            let focus = under.map(|(_win, surface, origin)| (surface, origin));

                            if let Some(pointer) = state.seat.get_pointer() {
                                pointer.motion(
                                    state,
                                    focus,
                                    &MotionEvent {
                                        location,
                                        serial: SCOUNTER.next_serial(),
                                        time,
                                    },
                                );
                                pointer.frame(state);
                            }

                            pending_events.lock().unwrap().push_back(BackendEvent::MotionNotify {
                                target: hit.unwrap_or(HitTarget::Background { output }),
                                root_x: x,
                                root_y: y,
                                time,
                            });
                        }
                        InputEvent::PointerButton { event, .. } => {
                            let time = event.time_msec();
                            let button_code = event.button_code();
                            let pressed = matches!(event.state(), smithay::backend::input::ButtonState::Pressed);

                            // libinput / evdev button codes are Linux input codes (e.g. BTN_LEFT=272),
                            // while JWM expects X11-like button numbers (1=left, 2=middle, 3=right).
                            // Map the common codes explicitly.
                            let detail_btn: u8 = match button_code {
                                272 => 1, // BTN_LEFT
                                273 => 3, // BTN_RIGHT
                                274 => 2, // BTN_MIDDLE
                                275 => 8, // BTN_SIDE
                                276 => 9, // BTN_EXTRA
                                _ => (button_code & 0xFF) as u8,
                            };
                            let (x, y, output) = {
                                let s = shared.lock().unwrap();
                                let x = s.pointer_x;
                                let y = s.pointer_y;
                                let _mods = s.mods_state;
                                let output = s
                                    .outputs
                                    .iter()
                                    .find(|o| (x as i32) >= o.x && (y as i32) >= o.y && (x as i32) < (o.x + o.width) && (y as i32) < (o.y + o.height))
                                    .map(|o| o.id);
                                (x, y, output)
                            };

                            let location: Point<f64, Logical> = (x, y).into();

                            // Minimal xdg_popup grab behavior: if a popup grab is active for a
                            // toplevel, a click outside all of its popups dismisses them.
                            if pressed {
                                if let Some(grab_win) = state.popup_grab_toplevel {
                                    let in_any_popup = state
                                        .popup_rects_for_toplevel(grab_win)
                                        .iter()
                                        .any(|(_surf, rect)| {
                                            let x0 = rect.loc.x as f64;
                                            let y0 = rect.loc.y as f64;
                                            let x1 = x0 + rect.size.w as f64;
                                            let y1 = y0 + rect.size.h as f64;
                                            location.x >= x0
                                                && location.y >= y0
                                                && location.x < x1
                                                && location.y < y1
                                        });

                                    if !in_any_popup {
                                        state.dismiss_popups_for_toplevel(grab_win);
                                        state.needs_redraw = true;
                                    }
                                }
                            }

                            let under = state.surface_under(location);
                            let hit = under
                                .as_ref()
                                .and_then(|(win, _, _)| win.map(HitTarget::Surface));
                            let focus = under.map(|(_win, surface, origin)| (surface, origin));

                            if let Some(pointer) = state.seat.get_pointer() {
                                // Ensure focus is up-to-date before sending the button.
                                pointer.motion(
                                    state,
                                    focus,
                                    &MotionEvent {
                                        location,
                                        serial: SCOUNTER.next_serial(),
                                        time,
                                    },
                                );

                                pointer.button(
                                    state,
                                    &ButtonEvent {
                                        serial: SCOUNTER.next_serial(),
                                        time,
                                        button: button_code,
                                        state: if pressed {
                                            smithay::backend::input::ButtonState::Pressed
                                        } else {
                                            smithay::backend::input::ButtonState::Released
                                        },
                                    },
                                );
                                pointer.frame(state);
                            }

                            // Focus follows click: if the user clicks a normal surface, it should
                            // receive keyboard focus. For layer-shell surfaces, only focus if it
                            // requested keyboard interactivity (OnDemand/Exclusive), otherwise keep
                            // the current focus (e.g. clicking a non-interactive panel shouldn't
                            // steal focus from the active app).
                            if pressed {
                                if let Some(kbd) = state.seat.get_keyboard() {
                                    if let Some((_win, surface, _origin)) = state.surface_under(location) {
                                        let layer_interactivity = state
                                            .layer_shell_state
                                            .layer_surfaces()
                                            .find(|l| l.wl_surface().id() == surface.id())
                                            .map(|l| l.with_cached_state(|d| d.keyboard_interactivity));

                                        let should_focus = match layer_interactivity {
                                            Some(KeyboardInteractivity::None) => false,
                                            Some(KeyboardInteractivity::OnDemand) => true,
                                            Some(KeyboardInteractivity::Exclusive) => true,
                                            None => true,
                                        };

                                        if should_focus {
                                            let win = state.surface_to_window.get(&surface.id()).copied();
                                            kbd.set_focus(
                                                state,
                                                Some(surface.clone()),
                                                SCOUNTER.next_serial(),
                                            );
                                            state.set_active_toplevel(win);
                                        }
                                    }
                                }
                            }

                            if pressed {
                                let mods_state = shared.lock().unwrap().mods_state;

                                let debug_buttons = std::env::var("JWM_DEBUG_BUTTONS")
                                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                                    .unwrap_or(false);
                                if debug_buttons {
                                    log::info!(
                                        "[udev:btn->wm] button_code={} detail={} mods_state=0x{:x} x={:.1} y={:.1} target={:?}",
                                        button_code,
                                        detail_btn,
                                        mods_state,
                                        x,
                                        y,
                                        hit.unwrap_or(HitTarget::Background { output })
                                    );
                                }
                                pending_events.lock().unwrap().push_back(BackendEvent::ButtonPress {
                                    target: hit.unwrap_or(HitTarget::Background { output }),
                                    state: mods_state,
                                    detail: detail_btn,
                                    time,
                                    root_x: x,
                                    root_y: y,
                                });
                            } else {
                                pending_events.lock().unwrap().push_back(BackendEvent::ButtonRelease {
                                    target: hit.unwrap_or(HitTarget::Background { output }),
                                    time,
                                });
                            }
                        }
                        InputEvent::Keyboard { event, .. } => {
                            let time = InputEventExt::time_msec(&event);
                            let keycode = event.key_code();
                            let state_key = event.state();
                            let serial = SCOUNTER.next_serial();
                            let pressed = matches!(state_key, smithay::backend::input::KeyState::Pressed);

                            let debug_keys = std::env::var("JWM_DEBUG_KEYS")
                                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                                .unwrap_or(false);

                            // Layer-shell surfaces can request exclusive keyboard interactivity
                            // (e.g. lock screens / OSD). If such a surface exists on Top/Overlay,
                            // route keyboard events directly to it and do not emit WM shortcuts.
                            let mut handled_by_exclusive_layer = false;

                            // If nothing is focused, focus the surface under the pointer (best-effort).
                            if let Some(kbd) = state.seat.get_keyboard() {
                                if debug_keys && pressed {
                                    // Smithay exposes keycodes in the XKB/Wayland domain (evdev + 8).
                                    // Keep them as-is; adding +8 again will shift keys (e.g. Enter -> j).
                                    let xkb_keycode_u8 = u8::try_from(u32::from(keycode)).unwrap_or(0);
                                    let evdev_keycode = u32::from(keycode).saturating_sub(8);
                                    log::info!(
                                        "[udev:key] xkb_keycode={} evdev_keycode={} focus_before={} time={}",
                                        xkb_keycode_u8,
                                        evdev_keycode,
                                        kbd.current_focus().is_some(),
                                        time
                                    );
                                }

                                // Check exclusive layer-shell first.
                                let bar_name = crate::config::CONFIG.status_bar_name();

                                let exclusive_surface = state
                                    .layer_shell_state
                                    .layer_surfaces()
                                    .rev()
                                    .find_map(|layer| {
                                        // Do not let the status bar block WM shortcuts.
                                        // Some bars mistakenly request `Exclusive` keyboard interactivity.
                                        if !bar_name.is_empty() {
                                            if let Some(win) = state
                                                .surface_to_window
                                                .get(&layer.wl_surface().id())
                                                .copied()
                                            {
                                                let title = state
                                                    .window_title
                                                    .get(&win)
                                                    .map(|s| s.as_str())
                                                    .unwrap_or("");
                                                let app_id = state
                                                    .window_app_id
                                                    .get(&win)
                                                    .map(|s| s.as_str())
                                                    .unwrap_or("");
                                                if title == bar_name || app_id == bar_name {
                                                    return None;
                                                }
                                            }
                                        }

                                        let exclusive = layer.with_cached_state(|data| {
                                            let exclusive_zone: i32 = data.exclusive_zone.into();
                                            data.keyboard_interactivity == KeyboardInteractivity::Exclusive
                                                && (data.layer == WlrLayer::Top
                                                    || data.layer == WlrLayer::Overlay)
                                                // Bars/docks often set a non-zero exclusive zone to reserve space.
                                                // Treat those as non-blocking for WM shortcuts.
                                                && exclusive_zone == 0
                                        });
                                        if !exclusive {
                                            return None;
                                        }

                                        // Only focus if the layer surface is actually mapped.
                                        let mapped = state.outputs.iter().any(|o| {
                                            let map = layer_map_for_output(o);
                                            map.layers().any(|l| l.layer_surface() == &layer)
                                        });
                                        if mapped {
                                            Some(layer.wl_surface().clone())
                                        } else {
                                            None
                                        }
                                    });

                                if let Some(surface) = exclusive_surface {
                                    handled_by_exclusive_layer = true;

                                    if debug_keys && pressed {
                                        log::info!("[udev:key] handled_by_exclusive_layer=true");
                                    }
                                    kbd.set_focus(state, Some(surface), serial);

                                    let _ = kbd.input::<(), _>(
                                        state,
                                        keycode,
                                        state_key,
                                        serial,
                                        time,
                                        |_, modifiers, _handle| {
                                            let mods_bits = mods_from_smithay(modifiers).bits();
                                            if let Some(mut s) = shared.lock().ok() {
                                                s.mods_state = mods_bits;
                                            }
                                            // Do not intercept any keys while an exclusive layer is active.
                                            FilterResult::Forward
                                        },
                                    );

                                    // Keep modifier state correct even if Smithay decides not to call
                                    // the filter closure (e.g. when no focus exists).
                                    let mods_bits = mods_from_smithay(&kbd.modifier_state()).bits();
                                    if let Some(mut s) = shared.lock().ok() {
                                        s.mods_state = mods_bits;
                                    }
                                }

                                if handled_by_exclusive_layer {
                                    // Skip best-effort focus selection and WM shortcut emission.
                                } else {
                                if kbd.current_focus().is_none() {
                                    let (px, py) = {
                                        let s = shared.lock().unwrap();
                                        (s.pointer_x, s.pointer_y)
                                    };
                                    let location: Point<f64, Logical> = (px, py).into();
                                    if let Some((_win, surface, _origin)) = state.surface_under(location) {
                                        kbd.set_focus(state, Some(surface), serial);
                                    }
                                }

                                let _ = kbd.input::<(), _>(
                                    state,
                                    keycode,
                                    state_key,
                                    serial,
                                    time,
                                    |_, modifiers, _handle| {
                                        let mods_bits = mods_from_smithay(modifiers).bits();

                                        // Smithay provides XKB/Wayland keycodes already (evdev + 8).
                                        // Use them directly for xkbcommon lookups.
                                        let xkb_keycode_u8 = u8::try_from(u32::from(keycode)).unwrap_or(0);

                                        let Some(mut s) = shared.lock().ok() else {
                                            return FilterResult::Forward;
                                        };

                                        s.mods_state = mods_bits;

                                        // Keep key press/release symmetric: if we intercepted a press,
                                        // also intercept its release so clients don't see a stray release.
                                        if !pressed {
                                            if s.suppressed_keycodes.remove(&xkb_keycode_u8) {
                                                return FilterResult::Intercept(());
                                            }
                                            return FilterResult::Forward;
                                        }

                                        let keysym = s
                                            .keysym_table
                                            .get(xkb_keycode_u8 as usize)
                                            .copied()
                                            .unwrap_or(0);

                                        let allowed_mods = crate::backend::common_define::Mods::SHIFT
                                            | crate::backend::common_define::Mods::CONTROL
                                            | crate::backend::common_define::Mods::ALT
                                            | crate::backend::common_define::Mods::SUPER
                                            | crate::backend::common_define::Mods::MOD2
                                            | crate::backend::common_define::Mods::MOD3
                                            | crate::backend::common_define::Mods::MOD5;
                                        let clean_mods = crate::backend::common_define::Mods::from_bits_truncate(mods_bits)
                                            & allowed_mods;

                                        let should_suppress = s
                                            .key_bindings
                                            .iter()
                                            .any(|(m, ks)| *ks == keysym && *m == clean_mods);

                                        if should_suppress {
                                            s.suppressed_keycodes.insert(xkb_keycode_u8);
                                            FilterResult::Intercept(())
                                        } else {
                                            FilterResult::Forward
                                        }
                                    },
                                );

                                // Keep modifier state correct even if Smithay decides not to call
                                // the filter closure (e.g. when no focus exists).
                                let mods_bits = mods_from_smithay(&kbd.modifier_state()).bits();
                                if let Some(mut s) = shared.lock().ok() {
                                    s.mods_state = mods_bits;
                                }
                                }
                            } else if debug_keys && pressed {
                                log::warn!("[udev:key] seat.get_keyboard() returned None (no keyboard configured?)");
                            }

                            // JWM only uses press for shortcuts for now.
                            if !handled_by_exclusive_layer
                                && matches!(state_key, smithay::backend::input::KeyState::Pressed)
                            {
                                // Smithay provides XKB/Wayland keycodes already (evdev + 8).
                                let keycode_u32 = u32::from(keycode);
                                let keycode_u8 = u8::try_from(keycode_u32).unwrap_or(0);
                                let mods_state = shared.lock().unwrap().mods_state;
                                pending_events.lock().unwrap().push_back(BackendEvent::KeyPress {
                                    keycode: keycode_u8,
                                    state: mods_state,
                                    time,
                                });

                                if debug_keys {
                                    log::info!(
                                        "[udev:key->wm] keycode={} mods_state=0x{:x} time={}",
                                        keycode_u8,
                                        mods_state,
                                        time
                                    );
                                }
                            }
                        }
                        _ => {}
                    }

                    // Input events can enqueue Wayland protocol messages; flush them promptly.
                    if !flush_pending.swap(true, Ordering::SeqCst) {
                        let _ = flush_tx.send(());
                    }
                })
                .map_err(|e| BackendError::Message(format!("calloop insert_source(libinput) failed: {e}")))?;
        }

        {
            let shared = shared.clone();
            let pending_events = pending_events.clone();
            event_loop
                .handle()
                .insert_source(notifier, move |event, &mut (), _state| {
                    match event {
                        SessionEvent::PauseSession => {
                            libinput_context.suspend();
                            pending_events.lock().unwrap().push_back(BackendEvent::ScreenLayoutChanged);
                        }
                        SessionEvent::ActivateSession => {
                            let _ = libinput_context.resume();
                            pending_events.lock().unwrap().push_back(BackendEvent::ScreenLayoutChanged);
                            let _ = rebuild_outputs(&shared, &pending_events);
                            {
                                let s = shared.lock().unwrap();
                                _state.output_rects = s
                                    .outputs
                                    .iter()
                                    .map(|o| smithay::utils::Rectangle::new(
                                        (o.x, o.y).into(),
                                        (o.width, o.height).into(),
                                    ))
                                    .collect();
                            }
                            if let Some(grab_win) = _state.popup_grab_toplevel {
                                _state.reconstrain_popups_for_toplevel(grab_win);
                            }
                            shared.lock().unwrap().kms_needs_reinit = true;
                        }
                    }
                })
                .map_err(|e| BackendError::Message(format!("calloop insert_source(libseat notifier) failed: {e}")))?;
        }

        {
            let shared = shared.clone();
            let pending_events = pending_events.clone();
            event_loop
                .handle()
                .insert_source(udev_backend, move |event, _, _state| {
                    match event {
                        UdevEvent::Added { device_id, path } => {
                            shared
                                .lock()
                                .unwrap()
                                .device_paths
                                .insert(device_id, path.to_path_buf());
                        }
                        UdevEvent::Changed { device_id } => {
                            let _ = device_id;
                        }
                        UdevEvent::Removed { device_id } => {
                            shared.lock().unwrap().device_paths.remove(&device_id);
                        }
                    }
                    let _ = rebuild_outputs(&shared, &pending_events);
                    {
                        let s = shared.lock().unwrap();
                        _state.output_rects = s
                            .outputs
                            .iter()
                            .map(|o| smithay::utils::Rectangle::new(
                                (o.x, o.y).into(),
                                (o.width, o.height).into(),
                            ))
                            .collect();
                    }
                    if let Some(grab_win) = _state.popup_grab_toplevel {
                        _state.reconstrain_popups_for_toplevel(grab_win);
                    }
                    shared.lock().unwrap().kms_needs_reinit = true;
                    pending_events.lock().unwrap().push_back(BackendEvent::ScreenLayoutChanged);
                })
                .map_err(|e| BackendError::Message(format!("calloop insert_source(udev) failed: {e}")))?;
        }

        let output_ops: Box<dyn OutputOps> = Box::new(UdevOutputOps {
            shared: shared.clone(),
        });
        let input_ops: Box<dyn InputOps> = Box::new(UdevInputOps {
            shared: shared.clone(),
        });

        let state_ptr: *mut JwmWaylandState = &mut *state;

        // Build the base keycode->keysym translation table used both by JWM (KeyOps) and by
        // the Smithay input filter (for shortcut suppression).
        let mut key_ops_impl = UdevKeyOps::new()?;
        {
            let mut s = shared.lock().unwrap();
            let mut non_zero = 0usize;
            for kc in 0u16..=255u16 {
                let u8_kc = kc as u8;
                let sym = key_ops_impl.keysym_from_keycode(u8_kc)?;
                s.keysym_table[u8_kc as usize] = sym;
                if sym != 0 {
                    non_zero += 1;
                }
            }

            // If almost everything is NoSymbol, shortcuts will never match.
            if non_zero < 32 {
                log::warn!(
                    "xkb keysym table looks mostly empty (non_zero={non_zero}/256); keyboard shortcuts likely won't work"
                );
            }
        }

        Ok(Self {
            display_handle,
            event_loop: SendWrapper(event_loop),
            state,
            socket_name,
            pending_events,

            flush_tx: flush_tx.clone(),
            flush_pending: flush_pending.clone(),

            shared,
            session,

            kms,
            window_ops: Box::new(WaylandWindowOps {
                state: SendWrapper(state_ptr),
                flush_tx: flush_tx.clone(),
                flush_pending: flush_pending.clone(),
            }),
            input_ops,
            property_ops: Box::new(WaylandPropertyOps {
                state: SendWrapper(state_ptr),
                flush_tx: flush_tx.clone(),
                flush_pending: flush_pending.clone(),
            }),
            output_ops,
            key_ops: Box::new(key_ops_impl),
            cursor_provider: Box::new(DummyCursorProvider),
            color_allocator: Box::new(DummyColorAllocator),
        })
    }
}

fn mods_from_smithay(mods: &ModifiersState) -> Mods {
    let mut out = Mods::empty();

    if mods.shift {
        out |= Mods::SHIFT;
    }
    if mods.ctrl {
        out |= Mods::CONTROL;
    }
    if mods.alt {
        out |= Mods::ALT;
    }
    if mods.logo {
        out |= Mods::SUPER;
    }
    if mods.caps_lock {
        out |= Mods::CAPS;
    }
    if mods.num_lock {
        out |= Mods::NUMLOCK;
    }

    out
}

impl Backend for UdevBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            can_warp_pointer: false,
            supports_client_list: false,
        }
    }

    fn root_window(&self) -> Option<WindowId> {
        Some(WindowId::from_raw(0))
    }

    fn check_existing_wm(&self) -> Result<(), BackendError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn on_focused_client_changed(&mut self, win: Option<WindowId>) -> Result<(), BackendError> {
        match win {
            Some(w) => self.window_ops.set_input_focus(w)?,
            None => self.window_ops.set_input_focus_root()?,
        }

        self.state.set_active_toplevel(win);
        self.state.needs_redraw = true;
        self.request_flush();
        Ok(())
    }

    fn window_ops(&self) -> &dyn WindowOps {
        &*self.window_ops
    }
    fn input_ops(&self) -> &dyn InputOps {
        &*self.input_ops
    }

    fn property_ops(&self) -> &dyn PropertyOps {
        &*self.property_ops
    }
    fn output_ops(&self) -> &dyn OutputOps {
        &*self.output_ops
    }
    fn key_ops(&self) -> &dyn KeyOps {
        &*self.key_ops
    }
    fn key_ops_mut(&mut self) -> &mut dyn KeyOps {
        &mut *self.key_ops
    }
    fn cursor_provider(&mut self) -> &mut dyn CursorProvider {
        &mut *self.cursor_provider
    }
    fn color_allocator(&mut self) -> &mut dyn ColorAllocator {
        &mut *self.color_allocator
    }

    fn run(&mut self, handler: &mut dyn EventHandler) -> Result<(), BackendError> {
        loop {
            let mut handled_any = false;
            loop {
                let next = { self.pending_events.lock().unwrap().pop_front() };
                match next {
                    Some(ev) => {
                        handled_any = true;
                        handler.handle_event(self, ev)?;
                    }
                    None => break,
                }
            }

            if handled_any {
                handler.update(self)?;
            }

            self.maybe_reinit_kms();

            let mut had_redraw = false;
            if let Some(kms) = &self.kms {
                if self.state.needs_redraw {
                    had_redraw = true;
                    kms.borrow_mut().request_render();
                    self.state.needs_redraw = false;
                }
                kms.borrow_mut().render_if_needed(&*self.state);
            }

            if handler.should_exit() {
                break;
            }

            // Block only when there's no pending work; otherwise, poll once to
            // allow queued calloop sources (notably Wayland flush) to run.
            let has_pending_events = !self.pending_events.lock().unwrap().is_empty();
            let timeout = if has_pending_events || handled_any || had_redraw {
                Some(std::time::Duration::ZERO)
            } else {
                None
            };
            self.event_loop
                .dispatch(timeout, &mut *self.state)
                .map_err(|e| BackendError::Other(Box::new(e)))?;
        }

        Ok(())
    }
}

fn rebuild_outputs(
    shared: &Arc<Mutex<SharedState>>,
    pending_events: &Arc<Mutex<VecDeque<BackendEvent>>>,
) -> Result<(), BackendError> {
    let (old_outputs, device_paths, mut key_to_id, mut next_raw) = {
        let s = shared.lock().unwrap();
        (
            s.outputs.clone(),
            s.device_paths.clone(),
            s.output_key_to_id.clone(),
            s.next_output_raw,
        )
    };

    let mut new_outputs: Vec<(u64, OutputInfo)> = Vec::new();
    let mut x_cursor = 0i32;
    let mut device_paths: Vec<(u64, PathBuf)> = device_paths.into_iter().collect();
    device_paths.sort_by_key(|(id, _)| *id);
    for (dev_id, path) in device_paths {
        let scanned = scan_drm_outputs(dev_id, &path)?;
        for (key, mut info) in scanned {
            info.x = x_cursor;
            info.y = 0;
            x_cursor += info.width.max(1);
            new_outputs.push((key, info));
        }
    }

    let mut final_outputs: Vec<OutputInfo> = Vec::new();
    for (key, mut info) in new_outputs {
        let id = key_to_id.entry(key).or_insert_with(|| {
            let raw = next_raw;
            next_raw = next_raw.wrapping_add(1);
            OutputId(raw)
        });
        info.id = *id;
        final_outputs.push(info);
    }

    let mut old_by_id: HashMap<OutputId, OutputInfo> = HashMap::new();
    for o in old_outputs {
        old_by_id.insert(o.id, o);
    }

    let mut new_by_id: HashMap<OutputId, OutputInfo> = HashMap::new();
    for o in &final_outputs {
        new_by_id.insert(o.id, o.clone());
    }

    {
        let mut q = pending_events.lock().unwrap();
        for (id, old) in &old_by_id {
            if !new_by_id.contains_key(id) {
                q.push_back(BackendEvent::OutputRemoved(*id));
            } else {
                let new = new_by_id.get(id).unwrap();
                if new.width != old.width
                    || new.height != old.height
                    || new.x != old.x
                    || new.y != old.y
                    || new.refresh_rate != old.refresh_rate
                    || new.name != old.name
                {
                    q.push_back(BackendEvent::OutputChanged(new.clone()));
                }
            }
        }
        for (id, new) in &new_by_id {
            if !old_by_id.contains_key(id) {
                q.push_back(BackendEvent::OutputAdded(new.clone()));
            }
        }
    }

    {
        let mut s = shared.lock().unwrap();
        s.outputs = final_outputs;
        s.output_key_to_id = key_to_id;
        s.next_output_raw = next_raw;
    }
    Ok(())
}

fn scan_drm_outputs(dev_id: u64, path: &Path) -> Result<Vec<(u64, OutputInfo)>, BackendError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| BackendError::Other(Box::new(e)))?;

    #[derive(Debug)]
    struct DrmCard(std::fs::File);
    impl std::os::unix::io::AsFd for DrmCard {
        fn as_fd(&self) -> std::os::unix::io::BorrowedFd<'_> {
            self.0.as_fd()
        }
    }
    impl drm::Device for DrmCard {}
    impl drm::control::Device for DrmCard {}

    let card = DrmCard(file);

    let res = card
        .resource_handles()
        .map_err(|e| BackendError::Other(Box::new(io::Error::new(io::ErrorKind::Other, format!("drm resources failed: {e:?}")))))?;

    let mut outputs = Vec::new();
    for conn_handle in res.connectors() {
        let conn = card
            .get_connector(*conn_handle, true)
            .map_err(|e| BackendError::Other(Box::new(io::Error::new(io::ErrorKind::Other, format!("drm get_connector failed: {e:?}")))))?;

        if conn.state() != connector::State::Connected {
            continue;
        }
        if conn.modes().is_empty() {
            continue;
        }

        let mode = conn
            .modes()
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .or_else(|| conn.modes().first())
            .unwrap();

        let width = mode.size().0 as i32;
        let height = mode.size().1 as i32;
        let refresh_rate = mode.vrefresh().saturating_mul(1000);

        let name = format!("{:?}-{}", conn.interface(), conn.interface_id());
        let key = ((dev_id as u64) << 32) | (u32::from(*conn_handle) as u64);

        outputs.push((
            key,
            OutputInfo {
                id: OutputId(0),
                name,
                x: 0,
                y: 0,
                width,
                height,
                scale: 1.0,
                refresh_rate,
            },
        ));
    }

    Ok(outputs)
}
