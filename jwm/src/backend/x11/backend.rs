use crate::backend::api::BackendEvent;
// src/backend/x11/backend.rs
use self::ids::X11IdRegistry;
use crate::backend::api::EventHandler;
use crate::backend::api::EwmhFeature;
use crate::backend::api::Geometry;
use crate::backend::api::HitTarget;
use crate::backend::api::ResizeEdge;
use crate::backend::common_define::EventMaskBits;
use crate::backend::common_define::StdCursorKind;
use crate::backend::common_define::WindowId;
use crate::backend::error::BackendError;
use crate::jwm::InteractionAction;
use calloop::signals::{Signal, Signals};
use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::env;
use x11rb::connection::Connection;
use x11rb::connection::RequestConnection;
use x11rb::protocol::randr::ConnectionExt as RandrExt;
use x11rb::protocol::randr::NotifyMask;
use x11rb::protocol::xproto::Screen;
use x11rb::rust_connection::RustConnection;

use calloop::{
    EventLoop,
    timer::{TimeoutAction, Timer},
};

use crate::backend::api::{
    Backend, Capabilities, ColorAllocator, CursorProvider, EwmhFacade, InputOps, KeyOps, OutputOps,
    PropertyOps, WindowOps,
};

use super::Atoms;
use self::{
    color::X11ColorAllocator,
    cursor::X11CursorProvider,
    event_source::X11EventSource,
    ewmh_facade::X11EwmhFacade,
    input_ops::X11InputOps,
    key_ops::X11KeyOps,
    output_ops::X11OutputOps,
    property_ops::X11PropertyOps,
    window_ops::X11WindowOps,
};

pub struct X11LoopData<'a> {
    pub backend: &'a mut X11Backend,
    pub handler: &'a mut dyn EventHandler,
    pub should_exit: bool,
}

#[allow(dead_code)]
pub struct X11Backend {
    conn: Arc<RustConnection>,
    screen: Screen,
    root: WindowId,
    root_x11: u32,
    ids: X11IdRegistry,
    atoms: Atoms,

    caps: Capabilities,

    window_ops: Box<dyn WindowOps>,
    input_ops: Box<dyn InputOps>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,
    ewmh_facade: Option<Box<dyn EwmhFacade>>,

    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,

    _init_event_source: Option<X11EventSource>,

    interaction: Option<X11Interaction>,

    compositor: Option<super::compositor::Compositor>,
}

struct X11Interaction {
    win: WindowId,
    start_geom: Geometry,
    start_root_x: f64,
    start_root_y: f64,
    action: InteractionAction,
}

impl X11Backend {
    fn debug_drag_enabled() -> bool {
        env::var("JWM_DEBUG_DRAG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true)
    }

    fn enrich_event_with_output(&self, mut ev: BackendEvent) -> BackendEvent {
        let fill_output = |x: f64, y: f64| self.output_ops.output_at(x as i32, y as i32);

        match &mut ev {
            BackendEvent::ButtonPress {
                target,
                root_x,
                root_y,
                ..
            } => {
                if matches!(target, HitTarget::Background { .. }) {
                    *target = HitTarget::Background {
                        output: fill_output(*root_x, *root_y),
                    };
                }
            }
            BackendEvent::MotionNotify {
                target,
                root_x,
                root_y,
                ..
            } => {
                if matches!(target, HitTarget::Background { .. }) {
                    *target = HitTarget::Background {
                        output: fill_output(*root_x, *root_y),
                    };
                }
            }
            BackendEvent::ButtonRelease { target, .. } => {
                if matches!(target, HitTarget::Background { .. }) {}
            }
            // Invalidate output cache on screen layout changes
            BackendEvent::ScreenLayoutChanged => {
                self.output_ops.invalidate_output_cache();
            }
            _ => {}
        }

        ev
    }
    pub fn new() -> Result<Self, BackendError> {
        let (raw_conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)?;
        let conn = Arc::new(raw_conn);
        use x11rb::connection::Connection;
        let screen = conn.setup().roots[screen_num].clone();
        let ids = X11IdRegistry::new(1);
        let root_x11 = screen.root;
        let root = ids.intern(root_x11);

        if conn
            .extension_information(x11rb::protocol::randr::X11_EXTENSION_NAME)?
            .is_some()
        {
            let mask =
                NotifyMask::SCREEN_CHANGE | NotifyMask::OUTPUT_CHANGE | NotifyMask::CRTC_CHANGE;
            conn.randr_select_input(screen.root, mask)?;
        }

        let numlock_mask = Arc::new(Mutex::new(0u16));
        let atoms = Atoms::new(conn.as_ref())?.reply()?;

        let window_ops: Box<dyn WindowOps> = Box::new(X11WindowOps::new(
            conn.clone(),
            atoms.clone(),
            numlock_mask.clone(),
            screen.root,
            ids.clone(),
        ));

        let x11_input_ops = X11InputOps::new(conn.clone(), screen.root, ids.clone());
        let input_ops: Box<dyn InputOps> = Box::new(x11_input_ops.clone());
        let property_ops: Box<dyn PropertyOps> = Box::new(X11PropertyOps::new(
            conn.clone(),
            atoms.clone(),
            ids.clone(),
        ));
        let output_ops: Box<dyn OutputOps> = Box::new(X11OutputOps::new(
            conn.clone(),
            screen.root,
            screen.width_in_pixels as i32,
            screen.height_in_pixels as i32,
        ));
        let key_ops: Box<dyn KeyOps> = Box::new(X11KeyOps::new(
            conn.clone(),
            numlock_mask.clone(),
            ids.clone(),
        ));
        let ewmh_facade: Option<Box<dyn EwmhFacade>> = Some(Box::new(X11EwmhFacade::new(
            conn.clone(),
            root,
            atoms.clone(),
            ids.clone(),
        )));
        let cursor_provider: Box<dyn CursorProvider> =
            Box::new(X11CursorProvider::new(conn.clone(), ids.clone())?);
        let color_allocator: Box<dyn ColorAllocator> = Box::new(X11ColorAllocator::new(
            conn.clone(),
            screen.default_colormap,
        ));

        let event_source =
            X11EventSource::new(conn.clone(), atoms.clone(), screen.root, ids.clone());

        let caps = Capabilities {
            can_warp_pointer: true,
            supports_client_list: true,
            ..Default::default()
        };

        // Try to initialize compositor (GPU compositing)
        // Compositor uses MANUAL redirect + GLX texture_from_pixmap, which requires
        // direct GLX rendering. This does NOT work in nested X servers (Xephyr/Xnest)
        // because GLX renders to the host GPU framebuffer, not the nested server's.
        // Default: OFF. Set JWM_COMPOSITOR=1 to enable.
        let compositor = if env::var("JWM_COMPOSITOR").map(|v| v == "1").unwrap_or(false) {
            match super::compositor::Compositor::new(
                conn.clone(),
                root_x11,
                screen.width_in_pixels as u32,
                screen.height_in_pixels as u32,
            ) {
                Ok(c) => {
                    log::info!("GPU compositor initialized successfully");
                    Some(c)
                }
                Err(e) => {
                    log::warn!("Compositor init failed, falling back to non-composited mode: {e}");
                    None
                }
            }
        } else {
            log::info!("Compositor disabled (set JWM_COMPOSITOR=1 to enable)");
            None
        };

        Ok(Self {
            conn,
            screen,
            root,
            root_x11,
            ids,
            atoms,
            caps,
            window_ops,
            input_ops,
            property_ops,
            output_ops,
            key_ops,
            ewmh_facade,
            cursor_provider,
            color_allocator,
            _init_event_source: Some(event_source),
            interaction: None,
            compositor,
        })
    }

    fn compositor_handle_event(&mut self, event: &BackendEvent) {
        let compositor = match self.compositor.as_mut() {
            Some(c) => c,
            None => return,
        };
        match event {
            BackendEvent::WindowMapped(win) => {
                if let Ok(x11w) = self.ids.x11(*win) {
                    if x11w != self.root_x11 {
                        if let Ok(geom) = self.window_ops.get_geometry(*win) {
                            compositor.add_window(x11w, geom.x, geom.y, geom.w, geom.h);
                        }
                    }
                }
            }
            BackendEvent::WindowUnmapped(win) => {
                if let Ok(x11w) = self.ids.x11(*win) {
                    compositor.remove_window(x11w);
                }
            }
            BackendEvent::WindowDestroyed(win) => {
                if let Ok(x11w) = self.ids.x11(*win) {
                    compositor.remove_window(x11w);
                }
            }
            BackendEvent::WindowConfigured { window, x, y, width, height } => {
                if let Ok(x11w) = self.ids.x11(*window) {
                    compositor.update_geometry(x11w, *x, *y, *width, *height);
                }
            }
            BackendEvent::DamageNotify { drawable } => {
                if let Ok(x11w) = self.ids.x11(*drawable) {
                    compositor.mark_damaged(x11w);
                }
            }
            _ => {}
        }
    }

    pub fn atoms(&self) -> &Atoms {
        &self.atoms
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }
}

impl Backend for X11Backend {
    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn root_window(&self) -> Option<WindowId> {
        Some(self.root)
    }

    fn check_existing_wm(&self) -> Result<(), BackendError> {
        let mask_bits = EventMaskBits::SUBSTRUCTURE_REDIRECT.bits();
        self.window_ops
            .change_event_mask(self.root, mask_bits)
            .map_err(|e| {
                BackendError::Message(format!(
                    "Another window manager is already running: {:?}",
                    e
                ))
            })
    }

    fn request_render(&mut self) {
        let _ = self.conn.flush();
    }

    fn has_compositor(&self) -> bool {
        self.compositor.is_some()
    }

    fn compositor_render_frame(
        &mut self,
        scene: &[(u64, i32, i32, u32, u32)],
    ) -> Result<bool, BackendError> {
        let compositor = match self.compositor.as_mut() {
            Some(c) => c,
            None => return Ok(false),
        };
        // Convert WindowId raw u64 to x11 window u32 via ids registry
        let x11_scene: Vec<(u32, i32, i32, u32, u32)> = scene
            .iter()
            .filter_map(|&(wid_raw, x, y, w, h)| {
                let wid = WindowId::from_raw(wid_raw);
                self.ids.x11(wid).ok().map(|x11w| (x11w, x, y, w, h))
            })
            .collect();
        if !scene.is_empty() && x11_scene.is_empty() {
            log::warn!(
                "[compositor] scene has {} entries but x11_scene is empty (ID lookup failed)",
                scene.len()
            );
        }
        // Lazily register any windows in the scene that the compositor doesn't
        // yet track.  This happens for windows that were already mapped before
        // the compositor was initialised (e.g. during setup_initial_windows).
        for &(x11w, x, y, w, h) in &x11_scene {
            if !compositor.has_window(x11w) && x11w != self.root_x11 {
                log::info!(
                    "[compositor] lazily adding untracked window 0x{:x} {}x{} at ({},{})",
                    x11w, w, h, x, y
                );
                compositor.add_window(x11w, x, y, w, h);
            }
        }

        let _ = self.conn.flush();
        Ok(compositor.render_frame(&x11_scene))
    }
    fn as_any(&self) -> &dyn Any {
        self
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
    fn register_wm(&self, wm_name: &str) -> Result<(), BackendError> {
        if let Some(facade) = self.ewmh_facade.as_ref() {
            let _support_win = facade.setup_supporting_wm_check(wm_name)?;
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
                EwmhFeature::CurrentDesktop,
                EwmhFeature::NumberOfDesktops,
                EwmhFeature::DesktopNames,
                EwmhFeature::DesktopViewport,
                EwmhFeature::WmMoveResize,
            ];
            facade.declare_supported(&supported)?;
        }
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), BackendError> {
        // Drop compositor before other X11 resources
        self.compositor.take();

        // Free X11 resources
        let _ = self.color_allocator.free_all_theme_pixels();
        let _ = self.cursor_provider.cleanup();

        if let Some(facade) = self.ewmh_facade.as_ref() {
            let _ = facade.reset_root_properties();
        }
        Ok(())
    }

    fn on_focused_client_changed(&mut self, win: Option<WindowId>) -> Result<(), BackendError> {
        if let Some(w) = win {
            // 1. 设置 X11 输入焦点
            self.window_ops.set_input_focus(w)?;

            // 2. 发送 WM_TAKE_FOCUS (ICCCM)
            let _ = self.window_ops.send_take_focus(w);

            // 3. 更新 EWMH 属性
            if let Some(facade) = self.ewmh_facade.as_ref() {
                facade.set_active_window(w)?;
            }
        } else {
            // 清除焦点到 Root
            self.window_ops.set_input_focus_root()?;
            if let Some(facade) = self.ewmh_facade.as_ref() {
                facade.clear_active_window()?;
            }
        }
        Ok(())
    }

    fn on_client_list_changed(
        &mut self,
        clients: &[WindowId],
        stack: &[WindowId],
    ) -> Result<(), BackendError> {
        if let Some(facade) = self.ewmh_facade.as_ref() {
            facade.set_client_list(clients)?;
            facade.set_client_list_stacking(stack)?;
        }
        Ok(())
    }

    fn on_desktop_changed(
        &mut self,
        current: u32,
        total: u32,
        names: &[&str],
    ) -> Result<(), BackendError> {
        if let Some(facade) = self.ewmh_facade.as_ref() {
            facade.set_desktop_info(current, total, names)?;
        }
        Ok(())
    }
    // [实现] 开始移动
    fn begin_move(&mut self, win: WindowId) -> Result<(), BackendError> {
        let geom = self.window_ops.get_geometry(win)?;
        let (rx, ry) = self.input_ops.get_pointer_position()?;
        if Self::debug_drag_enabled() {
            log::info!(
                "[drag] begin_move win={:?} geom={:?} pointer=({:.1},{:.1})",
                win,
                geom,
                rx,
                ry
            );
        }

        // 1. 设置光标
        self.cursor_provider.get(StdCursorKind::Hand)?; // 预加载
        self.input_ops.set_cursor(StdCursorKind::Hand)?;

        // 2. 抓取指针
        let mask = (EventMaskBits::BUTTON_RELEASE | EventMaskBits::POINTER_MOTION).bits();
        // X11 Cursor handle 是一层封装，这里需要解开
        let cursor_handle = self.cursor_provider.get(StdCursorKind::Hand)?.0;

        if self.input_ops.grab_pointer(mask, Some(cursor_handle))? {
            self.interaction = Some(X11Interaction {
                win,
                start_geom: geom,
                start_root_x: rx,
                start_root_y: ry,
                action: InteractionAction::Move,
            });
        } else if Self::debug_drag_enabled() {
            log::info!("[drag] begin_move grab_pointer failed win={:?}", win);
        }
        Ok(())
    }

    // [实现] 开始调整大小
    fn begin_resize(&mut self, win: WindowId, edge: ResizeEdge) -> Result<(), BackendError> {
        let geom = self.window_ops.get_geometry(win)?;
        let (rx, ry) = self.input_ops.get_pointer_position()?;

        if Self::debug_drag_enabled() {
            log::info!("[drag] begin_resize win={:?} edge={:?} geom={:?}", win, edge, geom);
        }

        // Do not warp pointer: resizemouse already picked an edge based on current cursor.
        let cursor_kind = match edge {
            ResizeEdge::Top | ResizeEdge::Bottom => StdCursorKind::VDoubleArrow,
            ResizeEdge::Left | ResizeEdge::Right => StdCursorKind::HDoubleArrow,
            ResizeEdge::TopLeft => StdCursorKind::TopLeftCorner,
            ResizeEdge::TopRight => StdCursorKind::TopRightCorner,
            ResizeEdge::BottomLeft => StdCursorKind::BottomLeftCorner,
            ResizeEdge::BottomRight => StdCursorKind::BottomRightCorner,
        };

        self.cursor_provider.get(cursor_kind)?; // 预加载
        self.input_ops.set_cursor(cursor_kind)?;
        let cursor_handle = self.cursor_provider.get(cursor_kind)?.0;
        let mask = (EventMaskBits::BUTTON_RELEASE | EventMaskBits::POINTER_MOTION).bits();

        if self.input_ops.grab_pointer(mask, Some(cursor_handle))? {
            self.interaction = Some(X11Interaction {
                win,
                start_geom: geom,
                start_root_x: rx,
                start_root_y: ry,
                action: InteractionAction::Resize(edge),
            });
        } else if Self::debug_drag_enabled() {
            log::info!("[drag] begin_resize grab_pointer failed win={:?}", win);
        }
        Ok(())
    }

    // [实现] 处理 Motion
    fn handle_motion(&mut self, x: f64, y: f64, _time: u32) -> Result<bool, BackendError> {
        if let Some(ref state) = self.interaction {
            let dx = (x - state.start_root_x) as i32;
            let dy = (y - state.start_root_y) as i32;

            match state.action {
                InteractionAction::Move => {
                    let new_x = state.start_geom.x + dx;
                    let new_y = state.start_geom.y + dy;
                    if Self::debug_drag_enabled() {
                        log::debug!(
                            "[drag] motion(move) win={:?} start=({},{}) dxdy=({},{}) -> pos=({},{}) keep_size=({}x{})",
                            state.win,
                            state.start_geom.x,
                            state.start_geom.y,
                            dx,
                            dy,
                            new_x,
                            new_y,
                            state.start_geom.w,
                            state.start_geom.h
                        );
                    }
                    self.window_ops.set_position(state.win, new_x, new_y)?;
                }
                InteractionAction::Resize(_) => {
                    let new_w = (state.start_geom.w as i32 + dx).max(1) as u32;
                    let new_h = (state.start_geom.h as i32 + dy).max(1) as u32;

                    if Self::debug_drag_enabled() {
                        log::debug!(
                            "[drag] motion(resize) win={:?} start_size=({}x{}) dxdy=({},{}) -> size=({}x{}) pos=({},{}) border={}",
                            state.win,
                            state.start_geom.w,
                            state.start_geom.h,
                            dx,
                            dy,
                            new_w,
                            new_h,
                            state.start_geom.x,
                            state.start_geom.y,
                            state.start_geom.border
                        );
                    }

                    self.window_ops.configure(
                        state.win,
                        state.start_geom.x,
                        state.start_geom.y,
                        new_w,
                        new_h,
                        state.start_geom.border,
                    )?;
                }
            }
            // 告诉 Jwm 这个事件被处理了
            return Ok(true);
        }
        Ok(false)
    }

    // [实现] 处理 ButtonRelease
    fn handle_button_release(&mut self, _time: u32) -> Result<bool, BackendError> {
        if self.interaction.is_some() {
            if Self::debug_drag_enabled() {
                if let Some(state) = self.interaction.as_ref() {
                    log::info!("[drag] end_interaction win={:?} action={:?}", state.win, state.action);
                } else {
                    log::info!("[drag] end_interaction");
                }
            }
            self.input_ops.ungrab_pointer()?;
            self.input_ops.set_cursor(StdCursorKind::LeftPtr)?;
            self.interaction = None;
            return Ok(true);
        }
        Ok(false)
    }
    fn cursor_provider(&mut self) -> &mut dyn CursorProvider {
        &mut *self.cursor_provider
    }
    fn color_allocator(&mut self) -> &mut dyn ColorAllocator {
        &mut *self.color_allocator
    }

    fn run(&mut self, handler: &mut dyn EventHandler) -> Result<(), BackendError> {
        let mut event_loop: EventLoop<X11LoopData> = EventLoop::try_new()?;
        let handle = event_loop.handle();

        // 1. 注册 X11 事件源
        let mut x11_source = if let Some(src) = self._init_event_source.take() {
            src
        } else {
            X11EventSource::new(
                self.conn.clone(),
                self.atoms.clone(),
                self.screen.root,
                self.ids.clone(),
            )
        };

        // Pass damage event base to event source if compositor is active
        if let Some(ref compositor) = self.compositor {
            x11_source.set_damage_event_base(compositor.damage_event_base());
        }

        handle
            .insert_source(x11_source, |event, _, data| {
                // Compositor hooks: track window lifecycle and damage
                data.backend.compositor_handle_event(&event);
                let event = data.backend.enrich_event_with_output(event);
                if let Err(e) = data.handler.handle_event(data.backend, event) {
                    log::error!("Error handling X11 event: {:?}", e);
                }
            })
            .map_err(|e| BackendError::Message(format!("Failed to insert X11 source: {}", e)))?;

        // 2. 注册 Signals
        let signals = Signals::new(&[Signal::SIGCHLD])?;
        handle
            .insert_source(signals, |event, _, data| {
                if event.signal() == Signal::SIGCHLD {
                    if let Err(e) = data.handler.handle_event(
                        data.backend,
                        crate::backend::api::BackendEvent::ChildProcessExited,
                    ) {
                        log::error!("Error handling SIGCHLD: {:?}", e);
                    }
                }
            })
            .map_err(|e| BackendError::Message(format!("Failed to insert Signal source: {}", e)))?;

        // 3. 注册 Timer
        // Timer 绝对不是 Send/Sync 的，必须转 String
        let update_interval = Duration::from_millis(20);
        let timer = Timer::from_duration(update_interval);
        handle
            .insert_source(timer, move |_, _, data| {
                if let Err(e) = data.handler.update(data.backend) {
                    log::error!("Error in update loop: {:?}", e);
                }
                if data.handler.should_exit() {
                    data.should_exit = true;
                }
                TimeoutAction::ToDuration(update_interval)
            })
            .map_err(|e| BackendError::Message(format!("Failed to insert Timer source: {}", e)))?;

        // 4. 运行事件循环
        let mut loop_data = X11LoopData {
            backend: self,
            handler,
            should_exit: false,
        };
        loop {
            event_loop
                .dispatch(None, &mut loop_data)
                .map_err(|e| BackendError::Other(Box::new(e)))?; // calloop::Error 是 Send+Sync 的，可以用 Box
            if loop_data.should_exit {
                break;
            }
        }

        Ok(())
    }
}

mod ids {
    use crate::backend::common_define::WindowId;
    use crate::backend::error::BackendError;
    use std::collections::HashMap;
    use std::sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    };

    #[derive(Clone, Default)]
    pub(super) struct X11IdRegistry {
        next: Arc<AtomicU64>,
        x11_to_wid: Arc<RwLock<HashMap<u32, WindowId>>>,
        wid_to_x11: Arc<RwLock<HashMap<WindowId, u32>>>,
    }

    impl X11IdRegistry {
        pub(super) fn new(start: u64) -> Self {
            Self {
                next: Arc::new(AtomicU64::new(start)),
                x11_to_wid: Arc::new(RwLock::new(HashMap::new())),
                wid_to_x11: Arc::new(RwLock::new(HashMap::new())),
            }
        }

        /// X11 window(u32) intern  WindowId
        pub(super) fn intern(&self, x11: u32) -> WindowId {
            if let Some(id) = self.x11_to_wid.read().unwrap().get(&x11).copied() {
                return id;
            }
            // 
            let mut w = self.x11_to_wid.write().unwrap();
            if let Some(id) = w.get(&x11).copied() {
                return id;
            }

            let id = WindowId::from_raw(self.next.fetch_add(1, Ordering::Relaxed));
            w.insert(x11, id);
            self.wid_to_x11.write().unwrap().insert(id, x11);
            id
        }

        pub(super) fn x11(&self, id: WindowId) -> Result<u32, BackendError> {
            self.wid_to_x11
                .read()
                .unwrap()
                .get(&id)
                .copied()
                .ok_or(BackendError::NotFound("WindowId not mapped to X11 window"))
        }

        pub(super) fn remove_x11(&self, x11: u32) {
            if let Some(id) = self.x11_to_wid.write().unwrap().remove(&x11) {
                self.wid_to_x11.write().unwrap().remove(&id);
            }
        }
    }
}

mod adapter {
    use crate::backend::common_define::MouseButton;
    use crate::backend::common_define::{EventMaskBits, Mods};
    use x11rb::protocol::xproto::{ButtonIndex, EventMask, KeyButMask};

    pub(super) fn mods_from_x11(mask: KeyButMask, numlock_mask: KeyButMask) -> Mods {
        let mut m = Mods::empty();
        let raw = mask.bits();

        if raw & KeyButMask::SHIFT.bits() != 0 {
            m |= Mods::SHIFT;
        }
        if raw & KeyButMask::CONTROL.bits() != 0 {
            m |= Mods::CONTROL;
        }
        if raw & KeyButMask::MOD1.bits() != 0 {
            m |= Mods::ALT;
        }
        if raw & KeyButMask::MOD2.bits() != 0 {
            // If NumLock is mapped to Mod2, don't treat it as a regular modifier.
            if !numlock_mask.contains(KeyButMask::MOD2) {
                m |= Mods::MOD2;
            }
        }
        if raw & KeyButMask::MOD3.bits() != 0 {
            if !numlock_mask.contains(KeyButMask::MOD3) {
                m |= Mods::MOD3;
            }
        }
        if raw & KeyButMask::MOD4.bits() != 0 {
            m |= Mods::SUPER;
        }
        if raw & KeyButMask::MOD5.bits() != 0 {
            if !numlock_mask.contains(KeyButMask::MOD5) {
                m |= Mods::MOD5;
            }
        }
        if raw & KeyButMask::LOCK.bits() != 0 {
            m |= Mods::CAPS;
        }
        if raw & numlock_mask.bits() != 0 {
            m |= Mods::NUMLOCK;
        }
        m
    }

    pub(super) fn mods_to_x11(mods: Mods, numlock_mask: KeyButMask) -> KeyButMask {
        let mut m = KeyButMask::default();
        if mods.contains(Mods::SHIFT) {
            m |= KeyButMask::SHIFT;
        }
        if mods.contains(Mods::CONTROL) {
            m |= KeyButMask::CONTROL;
        }
        if mods.contains(Mods::ALT) {
            m |= KeyButMask::MOD1;
        }
        if mods.contains(Mods::MOD2) {
            m |= KeyButMask::MOD2;
        }
        if mods.contains(Mods::MOD3) {
            m |= KeyButMask::MOD3;
        }
        if mods.contains(Mods::SUPER) {
            m |= KeyButMask::MOD4;
        }
        if mods.contains(Mods::MOD5) {
            m |= KeyButMask::MOD5;
        }
        if mods.contains(Mods::CAPS) {
            m |= KeyButMask::LOCK;
        }
        if mods.contains(Mods::NUMLOCK) {
            m |= numlock_mask;
        }
        m
    }

    #[allow(dead_code)]
    pub(super) fn button_from_x11(detail: u8) -> MouseButton {
        MouseButton::from_u8(detail)
    }

    #[allow(dead_code)]
    pub(super) fn button_to_x11(btn: MouseButton) -> ButtonIndex {
        ButtonIndex::from(btn.to_u8())
    }

    pub(super) fn event_mask_from_generic(bits: u32) -> EventMask {
        let mut m = EventMask::default();
        if (bits & EventMaskBits::BUTTON_PRESS.bits()) != 0 {
            m |= EventMask::BUTTON_PRESS;
        }
        if (bits & EventMaskBits::BUTTON_RELEASE.bits()) != 0 {
            m |= EventMask::BUTTON_RELEASE;
        }
        if (bits & EventMaskBits::POINTER_MOTION.bits()) != 0 {
            m |= EventMask::POINTER_MOTION;
        }
        if (bits & EventMaskBits::ENTER_WINDOW.bits()) != 0 {
            m |= EventMask::ENTER_WINDOW;
        }
        if (bits & EventMaskBits::LEAVE_WINDOW.bits()) != 0 {
            m |= EventMask::LEAVE_WINDOW;
        }
        if (bits & EventMaskBits::PROPERTY_CHANGE.bits()) != 0 {
            m |= EventMask::PROPERTY_CHANGE;
        }
        if (bits & EventMaskBits::STRUCTURE_NOTIFY.bits()) != 0 {
            m |= EventMask::STRUCTURE_NOTIFY;
        }
        if (bits & EventMaskBits::SUBSTRUCTURE_REDIRECT.bits()) != 0 {
            m |= EventMask::SUBSTRUCTURE_REDIRECT;
        }
        if (bits & EventMaskBits::FOCUS_CHANGE.bits()) != 0 {
            m |= EventMask::FOCUS_CHANGE;
        }
        m
    }
}

mod color {
    use crate::backend::api::ColorAllocator;
    use crate::backend::common_define::{ArgbColor, ColorScheme, Pixel, SchemeType};
    use crate::backend::error::BackendError;
    use std::collections::HashMap;
    use std::sync::Arc;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::Colormap;

    pub(super) struct X11ColorAllocator<C: Connection> {
        conn: Arc<C>,
        colormap: Colormap,

        pixel_cache: HashMap<u32, Pixel>,
        schemes: HashMap<SchemeType, ColorScheme>,
    }

    impl<C: Connection> X11ColorAllocator<C> {
        pub(super) fn new(conn: Arc<C>, colormap: Colormap) -> Self {
            Self {
                conn,
                colormap,
                pixel_cache: HashMap::new(),
                schemes: HashMap::new(),
            }
        }

        fn ensure_pixel(&mut self, color: ArgbColor) -> Result<Pixel, BackendError> {
            if let Some(p) = self.pixel_cache.get(&color.value).copied() {
                return Ok(p);
            }
            let (_, r, g, b) = color.components();
            let pix = self.alloc_rgb(r, g, b)?;
            self.pixel_cache.insert(color.value, pix);
            Ok(pix)
        }

        fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, BackendError> {
            use x11rb::protocol::xproto::ConnectionExt;
            let reply = (*self.conn)
                .alloc_color(
                    self.colormap,
                    (r as u16) << 8,
                    (g as u16) << 8,
                    (b as u16) << 8,
                )?
                .reply()?;
            Ok(Pixel(reply.pixel))
        }

        fn free_pixels(&mut self, pixels: &[Pixel]) -> Result<(), BackendError> {
            if pixels.is_empty() {
                return Ok(());
            }
            use x11rb::protocol::xproto::ConnectionExt;
            let raw: Vec<u32> = pixels.iter().map(|p| p.0).collect();
            (*self.conn).free_colors(self.colormap, 0, &raw)?;
            Ok(())
        }
    }

    impl<C: Connection + Send + Sync + 'static> ColorAllocator for X11ColorAllocator<C> {
        fn set_scheme(&mut self, t: SchemeType, s: ColorScheme) {
            self.schemes.insert(t, s);
        }

        fn get_border_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, BackendError> {
            let s = self
                .schemes
                .get(&t)
                .ok_or(BackendError::NotFound("scheme not found"))?
                .clone();
            self.ensure_pixel(s.border)
        }

        fn allocate_schemes_pixels(&mut self) -> Result<(), BackendError> {
            let mut colors: Vec<ArgbColor> = Vec::new();
            for s in self.schemes.values() {
                colors.push(s.fg);
                colors.push(s.bg);
                colors.push(s.border);
            }
            colors.sort_by_key(|c| c.value);
            colors.dedup();
            for c in colors {
                let _ = self.ensure_pixel(c)?;
            }
            Ok(())
        }

        fn free_all_theme_pixels(&mut self) -> Result<(), BackendError> {
            if self.pixel_cache.is_empty() {
                return Ok(());
            }
            let pixels: Vec<Pixel> = self.pixel_cache.values().copied().collect();
            self.free_pixels(&pixels)?;
            self.pixel_cache.clear();
            Ok(())
        }
    }
}

mod cursor {
    use crate::backend::api::CursorProvider;
    use crate::backend::common_define::{CursorHandle, StdCursorKind, WindowId};
    use crate::backend::error::BackendError;
    use super::ids::X11IdRegistry;
    use std::collections::HashMap;
    use std::sync::Arc;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    #[allow(dead_code)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub(super) enum X11StdCursor {
        XCursor = 0,
        Arrow = 2,
        BasedArrowDown = 4,
        BasedArrowUp = 6,
        Boat = 8,
        Bogosity = 10,
        BottomLeftCorner = 12,
        BottomRightCorner = 14,
        BottomSide = 16,
        BottomTee = 18,
        BoxSpiral = 20,
        CenterPtr = 22,
        Circle = 24,
        Clock = 26,
        CoffeeMug = 28,
        Cross = 30,
        CrossReverse = 32,
        Crosshair = 34,
        DiamondCross = 36,
        Dot = 38,
        Dotbox = 40,
        DoubleArrow = 42,
        DraftLarge = 44,
        DraftSmall = 46,
        DrapedBox = 48,
        Exchange = 50,
        Fleur = 52,
        Gobbler = 54,
        Gumby = 56,
        Hand1 = 58,
        Hand2 = 60,
        Heart = 62,
        Icon = 64,
        IronCross = 66,
        LeftPtr = 68,
        LeftSide = 70,
        LeftTee = 72,
        Leftbutton = 74,
        LlAngle = 76,
        LrAngle = 78,
        Man = 80,
        Middlebutton = 82,
        Mouse = 84,
        Pencil = 86,
        Pirate = 88,
        Plus = 90,
        QuestionArrow = 92,
        RightPtr = 94,
        RightSide = 96,
        RightTee = 98,
        Rightbutton = 100,
        RtlLogo = 102,
        Sailboat = 104,
        SbDownArrow = 106,
        SbHDoubleArrow = 108,
        SbLeftArrow = 110,
        SbRightArrow = 112,
        SbUpArrow = 114,
        SbVDoubleArrow = 116,
        Shuttle = 118,
        Sizing = 120,
        Spider = 122,
        Spraycan = 124,
        Star = 126,
        Target = 128,
        Tcross = 130,
        TopLeftArrow = 132,
        TopLeftCorner = 134,
        TopRightCorner = 136,
        TopSide = 138,
        TopTee = 140,
        Trek = 142,
        UlAngle = 144,
        Umbrella = 146,
        UrAngle = 148,
        Watch = 150,
        Xterm = 152,
    }

    impl X11StdCursor {
        pub(super) fn create(&self, conn: &impl Connection, font: Font) -> Result<Cursor, BackendError> {
            let cursor_id = conn.generate_id()?;
            let glyph = *self as u16;
            conn.create_glyph_cursor(
                cursor_id,
                font,
                font,
                glyph,
                glyph + 1,
                0,
                0,
                0, // 
                65535,
                65535,
                65535, // 
            )?;
            Ok(cursor_id)
        }

        #[allow(dead_code)]
        pub(super) fn create_colored(
            &self,
            conn: &impl Connection,
            font: Font,
            fg_r: u16,
            fg_g: u16,
            fg_b: u16,
            bg_r: u16,
            bg_g: u16,
            bg_b: u16,
        ) -> Result<Cursor, BackendError> {
            let cursor_id = conn.generate_id()?;
            let glyph = *self as u16;
            conn.create_glyph_cursor(
                cursor_id,
                font,
                font,
                glyph,
                glyph + 1,
                fg_r,
                fg_g,
                fg_b,
                bg_r,
                bg_g,
                bg_b,
            )?;
            Ok(cursor_id)
        }

        #[allow(dead_code)]
        pub(super) fn description(&self) -> &'static str {
            match self {
                Self::XCursor => "Default X cursor",
                Self::Arrow => "Standard arrow",
                Self::BasedArrowDown => "Down arrow",
                Self::BasedArrowUp => "Up arrow",
                Self::Boat => "Boat shape",
                Self::Bogosity => "Error/invalid indicator",
                Self::BottomLeftCorner => "Bottom-left corner resize",
                Self::BottomRightCorner => "Bottom-right corner resize",
                Self::BottomSide => "Bottom side resize",
                Self::BottomTee => "Bottom T shape",
                Self::BoxSpiral => "Box spiral",
                Self::CenterPtr => "Center pointer",
                Self::Circle => "Circle",
                Self::Clock => "Clock/waiting",
                Self::CoffeeMug => "Coffee mug",
                Self::Cross => "Cross",
                Self::CrossReverse => "Reverse cross",
                Self::Crosshair => "Crosshair",
                Self::DiamondCross => "Diamond cross",
                Self::Dot => "Dot",
                Self::Dotbox => "Dotted box",
                Self::DoubleArrow => "Double arrow",
                Self::DraftLarge => "Large draft",
                Self::DraftSmall => "Small draft",
                Self::DrapedBox => "Draped box",
                Self::Exchange => "Exchange",
                Self::Fleur => "Four-way move",
                Self::Gobbler => "Pac-man",
                Self::Gumby => "Gumby character",
                Self::Hand1 => "Hand pointer 1",
                Self::Hand2 => "Hand pointer 2",
                Self::Heart => "Heart shape",
                Self::Icon => "Icon",
                Self::IronCross => "Iron cross",
                Self::LeftPtr => "Left pointer (standard)",
                Self::LeftSide => "Left side resize",
                Self::LeftTee => "Left T shape",
                Self::Leftbutton => "Left button",
                Self::LlAngle => "Lower-left angle",
                Self::LrAngle => "Lower-right angle",
                Self::Man => "Man figure",
                Self::Middlebutton => "Middle button",
                Self::Mouse => "Mouse",
                Self::Pencil => "Pencil",
                Self::Pirate => "Pirate",
                Self::Plus => "Plus sign",
                Self::QuestionArrow => "Question arrow",
                Self::RightPtr => "Right pointer",
                Self::RightSide => "Right side resize",
                Self::RightTee => "Right T shape",
                Self::Rightbutton => "Right button",
                Self::RtlLogo => "RTL logo",
                Self::Sailboat => "Sailboat",
                Self::SbDownArrow => "Scrollbar down arrow",
                Self::SbHDoubleArrow => "Horizontal double arrow",
                Self::SbLeftArrow => "Scrollbar left arrow",
                Self::SbRightArrow => "Scrollbar right arrow",
                Self::SbUpArrow => "Scrollbar up arrow",
                Self::SbVDoubleArrow => "Vertical double arrow",
                Self::Shuttle => "Shuttle",
                Self::Sizing => "Sizing",
                Self::Spider => "Spider",
                Self::Spraycan => "Spray can",
                Self::Star => "Star",
                Self::Target => "Target",
                Self::Tcross => "T cross",
                Self::TopLeftArrow => "Top-left arrow",
                Self::TopLeftCorner => "Top-left corner resize",
                Self::TopRightCorner => "Top-right corner resize",
                Self::TopSide => "Top side resize",
                Self::TopTee => "Top T shape",
                Self::Trek => "Star Trek",
                Self::UlAngle => "Upper-left angle",
                Self::Umbrella => "Umbrella",
                Self::UrAngle => "Upper-right angle",
                Self::Watch => "Watch/waiting",
                Self::Xterm => "Text cursor",
            }
        }

        #[allow(dead_code)]
        pub(super) fn common_cursors() -> &'static [X11StdCursor] {
            &[
                Self::LeftPtr,           // 
                Self::Hand1,             // 
                Self::Xterm,             // 
                Self::Watch,             // 
                Self::Crosshair,         // 
                Self::Fleur,             // 
                Self::SbHDoubleArrow,    // 
                Self::SbVDoubleArrow,    // 
                Self::TopLeftCorner,     // 
                Self::TopRightCorner,    // 
                Self::BottomLeftCorner,  // 
                Self::BottomRightCorner, // 
                Self::Sizing,            // 
            ]
        }

        #[allow(dead_code)]
        pub(super) fn all_cursors() -> &'static [X11StdCursor] {
            &[
                Self::XCursor,
                Self::Arrow,
                Self::BasedArrowDown,
                Self::BasedArrowUp,
                Self::Boat,
                Self::Bogosity,
                Self::BottomLeftCorner,
                Self::BottomRightCorner,
                Self::BottomSide,
                Self::BottomTee,
                Self::BoxSpiral,
                Self::CenterPtr,
                Self::Circle,
                Self::Clock,
                Self::CoffeeMug,
                Self::Cross,
                Self::CrossReverse,
                Self::Crosshair,
                Self::DiamondCross,
                Self::Dot,
                Self::Dotbox,
                Self::DoubleArrow,
                Self::DraftLarge,
                Self::DraftSmall,
                Self::DrapedBox,
                Self::Exchange,
                Self::Fleur,
                Self::Gobbler,
                Self::Gumby,
                Self::Hand1,
                Self::Hand2,
                Self::Heart,
                Self::Icon,
                Self::IronCross,
                Self::LeftPtr,
                Self::LeftSide,
                Self::LeftTee,
                Self::Leftbutton,
                Self::LlAngle,
                Self::LrAngle,
                Self::Man,
                Self::Middlebutton,
                Self::Mouse,
                Self::Pencil,
                Self::Pirate,
                Self::Plus,
                Self::QuestionArrow,
                Self::RightPtr,
                Self::RightSide,
                Self::RightTee,
                Self::Rightbutton,
                Self::RtlLogo,
                Self::Sailboat,
                Self::SbDownArrow,
                Self::SbHDoubleArrow,
                Self::SbLeftArrow,
                Self::SbRightArrow,
                Self::SbUpArrow,
                Self::SbVDoubleArrow,
                Self::Shuttle,
                Self::Sizing,
                Self::Spider,
                Self::Spraycan,
                Self::Star,
                Self::Target,
                Self::Tcross,
                Self::TopLeftArrow,
                Self::TopLeftCorner,
                Self::TopRightCorner,
                Self::TopSide,
                Self::TopTee,
                Self::Trek,
                Self::UlAngle,
                Self::Umbrella,
                Self::UrAngle,
                Self::Watch,
                Self::Xterm,
            ]
        }
    }

    pub(super) struct X11CursorProvider<C: Connection> {
        conn: Arc<C>,
        cursor_font: Font,
        cache: HashMap<StdCursorKind, Cursor>,
        ids: X11IdRegistry,
    }

    impl<C: Connection> X11CursorProvider<C> {
        pub(super) fn new(conn: Arc<C>, ids: X11IdRegistry) -> Result<Self, BackendError> {
            use x11rb::protocol::xproto::ConnectionExt;
            let font = conn.generate_id()?;
            conn.open_font(font, b"cursor")?;
            Ok(Self {
                conn,
                cursor_font: font,
                cache: HashMap::new(),
                ids,
            })
        }

        fn map_kind(kind: StdCursorKind) -> X11StdCursor {
            match kind {
                StdCursorKind::LeftPtr => X11StdCursor::LeftPtr,
                StdCursorKind::Hand => X11StdCursor::Hand1,
                StdCursorKind::XTerm => X11StdCursor::Xterm,
                StdCursorKind::Watch => X11StdCursor::Watch,
                StdCursorKind::Crosshair => X11StdCursor::Crosshair,
                StdCursorKind::Fleur => X11StdCursor::Fleur,
                StdCursorKind::HDoubleArrow => X11StdCursor::SbHDoubleArrow,
                StdCursorKind::VDoubleArrow => X11StdCursor::SbVDoubleArrow,
                StdCursorKind::TopLeftCorner => X11StdCursor::TopLeftCorner,
                StdCursorKind::TopRightCorner => X11StdCursor::TopRightCorner,
                StdCursorKind::BottomLeftCorner => X11StdCursor::BottomLeftCorner,
                StdCursorKind::BottomRightCorner => X11StdCursor::BottomRightCorner,
                StdCursorKind::Sizing => X11StdCursor::Sizing,
            }
        }
    }

    impl<C: Connection + Send + Sync + 'static> CursorProvider for X11CursorProvider<C> {
        fn preload_common(&mut self) -> Result<(), BackendError> {
            for kind in [
                StdCursorKind::LeftPtr,
                StdCursorKind::Hand,
                StdCursorKind::XTerm,
                StdCursorKind::Watch,
                StdCursorKind::Crosshair,
                StdCursorKind::Fleur,
                StdCursorKind::HDoubleArrow,
                StdCursorKind::VDoubleArrow,
                StdCursorKind::TopLeftCorner,
                StdCursorKind::TopRightCorner,
                StdCursorKind::BottomLeftCorner,
                StdCursorKind::BottomRightCorner,
                StdCursorKind::Sizing,
            ] {
                let _ = self.get(kind)?;
            }
            Ok(())
        }

        fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, BackendError> {
            if let Some(&c) = self.cache.get(&kind) {
                return Ok(CursorHandle(c as u64));
            }
            let x11_cursor = Self::map_kind(kind);
            let cursor = x11_cursor.create(&*self.conn, self.cursor_font)?;
            self.cache.insert(kind, cursor);
            Ok(CursorHandle(cursor as u64))
        }

        fn apply(&mut self, window: WindowId, kind: StdCursorKind) -> Result<(), BackendError> {
            use x11rb::protocol::xproto::ConnectionExt;
            let c = match self.get(kind) {
                Ok(h) => h.0 as u32,
                Err(e) => return Err(e),
            };
            (*self.conn).change_window_attributes(
                self.ids.x11(window)?,
                &ChangeWindowAttributesAux::new().cursor(c),
            )?;
            Ok(())
        }

        fn cleanup(&mut self) -> Result<(), BackendError> {
            use x11rb::protocol::xproto::ConnectionExt;
            for &cursor in self.cache.values() {
                let _ = (*self.conn).free_cursor(cursor);
            }
            let _ = (*self.conn).close_font(self.cursor_font);
            Ok(())
        }
    }
}

mod event_source {
    use std::os::unix::io::{AsRawFd, BorrowedFd};
    use std::sync::Arc;
    use x11rb::connection::Connection;
    use x11rb::protocol::{Event as XEvent, xproto};
    use x11rb::rust_connection::RustConnection;

    use crate::backend::api::{
        BackendEvent, NetWmAction, NetWmState, PropertyKind, StackMode, WindowChanges,
    };
    use crate::backend::api::{HitTarget, NotifyMode};
    use crate::backend::error::BackendError;
    use crate::backend::x11::Atoms;
    use super::ids::X11IdRegistry;

    use calloop::{EventSource, Interest, Mode, Poll, PostAction, Readiness, Token, TokenFactory};

    pub(super) struct X11EventSource {
        conn: Arc<RustConnection>,
        atoms: Atoms,
        root_x11: u32,
        ids: X11IdRegistry,
        damage_event_base: Option<u8>,
    }

    impl X11EventSource {
        pub(super) fn new(conn: Arc<RustConnection>, atoms: Atoms, root_x11: u32, ids: X11IdRegistry) -> Self {
            Self {
                conn,
                atoms,
                root_x11,
                ids,
                damage_event_base: None,
            }
        }

        pub(super) fn set_damage_event_base(&mut self, base: u8) {
            self.damage_event_base = Some(base);
        }

        fn hit_target_from_event_window(&self, event_window: u32) -> HitTarget {
            if event_window == self.root_x11 {
                HitTarget::Background { output: None }
            } else {
                HitTarget::Surface(self.ids.intern(event_window))
            }
        }

        fn map_property_kind(&self, atom: u32) -> PropertyKind {
            if atom == self.atoms.WM_TRANSIENT_FOR {
                PropertyKind::TransientFor
            } else if atom == u32::from(xproto::AtomEnum::WM_NORMAL_HINTS) {
                PropertyKind::SizeHints
            } else if atom == u32::from(xproto::AtomEnum::WM_HINTS) {
                PropertyKind::Urgency
            } else if atom == u32::from(xproto::AtomEnum::WM_NAME) || atom == self.atoms._NET_WM_NAME {
                PropertyKind::Title
            } else if atom == u32::from(xproto::AtomEnum::WM_CLASS) {
                PropertyKind::Class
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE {
                PropertyKind::WindowType
            } else if atom == self.atoms.WM_PROTOCOLS {
                PropertyKind::Protocols
            } else if atom == self.atoms._NET_WM_STRUT
                || atom == self.atoms._NET_WM_STRUT_PARTIAL
            {
                PropertyKind::Strut
            } else {
                PropertyKind::Other
            }
        }

        fn map_net_wm_action(action: u32) -> Option<NetWmAction> {
            match action {
                0 => Some(NetWmAction::Remove),
                1 => Some(NetWmAction::Add),
                2 => Some(NetWmAction::Toggle),
                _ => None,
            }
        }

        fn map_event(&self, ev: XEvent) -> Option<BackendEvent> {
            match ev {
                XEvent::ButtonPress(e) => Some(BackendEvent::ButtonPress {
                    target: self.hit_target_from_event_window(e.event),
                    state: e.state.bits(),
                    detail: e.detail,
                    time: e.time,
                    root_x: e.root_x as f64,
                    root_y: e.root_y as f64,
                }),
                XEvent::MotionNotify(e) => Some(BackendEvent::MotionNotify {
                    target: self.hit_target_from_event_window(e.event),
                    root_x: e.root_x as f64,
                    root_y: e.root_y as f64,
                    time: e.time,
                }),
                XEvent::ButtonRelease(e) => Some(BackendEvent::ButtonRelease {
                    target: self.hit_target_from_event_window(e.event),
                    time: e.time,
                }),
                XEvent::RandrScreenChangeNotify(_) => Some(BackendEvent::ScreenLayoutChanged),
                XEvent::RandrNotify(_) => Some(BackendEvent::ScreenLayoutChanged),
                XEvent::KeyPress(e) => Some(BackendEvent::KeyPress {
                    keycode: e.detail,
                    state: e.state.bits(),
                    time: e.time,
                }),
                XEvent::MapRequest(e) => Some(BackendEvent::WindowCreated(self.ids.intern(e.window))),
                XEvent::MapNotify(e) => Some(BackendEvent::WindowMapped(self.ids.intern(e.window))),
                XEvent::UnmapNotify(e) => Some(BackendEvent::WindowUnmapped(self.ids.intern(e.window))),
                XEvent::DestroyNotify(e) => {
                    let id = self.ids.intern(e.window);
                    self.ids.remove_x11(e.window);
                    Some(BackendEvent::WindowDestroyed(id))
                }
                XEvent::ConfigureNotify(e) => Some(BackendEvent::WindowConfigured {
                    window: self.ids.intern(e.window),
                    x: e.x as i32,
                    y: e.y as i32,
                    width: e.width as u32,
                    height: e.height as u32,
                }),
                XEvent::EnterNotify(e) => {
                    let mode = match e.mode {
                        xproto::NotifyMode::NORMAL => NotifyMode::Normal,
                        xproto::NotifyMode::GRAB => NotifyMode::Grab,
                        xproto::NotifyMode::UNGRAB => NotifyMode::Ungrab,
                        _ => NotifyMode::Grab,
                    };
                    Some(BackendEvent::EnterNotify {
                        window: self.ids.intern(e.event),
                        subwindow: if e.child != 0 {
                            Some(self.ids.intern(e.child))
                        } else {
                            None
                        },
                        mode,
                        root_x: e.root_x as f64,
                        root_y: e.root_y as f64,
                    })
                }
                XEvent::LeaveNotify(e) => {
                    let mode = match e.mode {
                        xproto::NotifyMode::NORMAL => NotifyMode::Normal,
                        xproto::NotifyMode::GRAB => NotifyMode::Grab,
                        xproto::NotifyMode::UNGRAB => NotifyMode::Ungrab,
                        _ => NotifyMode::Grab,
                    };
                    Some(BackendEvent::LeaveNotify {
                        window: self.ids.intern(e.event),
                        mode,
                    })
                }
                XEvent::FocusIn(e) => Some(BackendEvent::FocusIn {
                    window: self.ids.intern(e.event),
                }),
                XEvent::FocusOut(e) => Some(BackendEvent::FocusOut {
                    window: self.ids.intern(e.event),
                }),
                XEvent::ConfigureRequest(e) => {
                    let changes = WindowChanges {
                        x: if e.value_mask.contains(xproto::ConfigWindow::X) {
                            Some(e.x as i32)
                        } else {
                            None
                        },
                        y: if e.value_mask.contains(xproto::ConfigWindow::Y) {
                            Some(e.y as i32)
                        } else {
                            None
                        },
                        width: if e.value_mask.contains(xproto::ConfigWindow::WIDTH) {
                            Some(e.width as u32)
                        } else {
                            None
                        },
                        height: if e.value_mask.contains(xproto::ConfigWindow::HEIGHT) {
                            Some(e.height as u32)
                        } else {
                            None
                        },
                        border_width: if e.value_mask.contains(xproto::ConfigWindow::BORDER_WIDTH) {
                            Some(e.border_width as u32)
                        } else {
                            None
                        },
                        sibling: if e.value_mask.contains(xproto::ConfigWindow::SIBLING) {
                            Some(self.ids.intern(e.sibling))
                        } else {
                            None
                        },
                        stack_mode: if e.value_mask.contains(xproto::ConfigWindow::STACK_MODE) {
                            match e.stack_mode {
                                xproto::StackMode::ABOVE => Some(StackMode::Above),
                                xproto::StackMode::BELOW => Some(StackMode::Below),
                                xproto::StackMode::TOP_IF => Some(StackMode::TopIf),
                                xproto::StackMode::BOTTOM_IF => Some(StackMode::BottomIf),
                                xproto::StackMode::OPPOSITE => Some(StackMode::Opposite),
                                _ => None,
                            }
                        } else {
                            None
                        },
                    };
                    Some(BackendEvent::ConfigureRequest {
                        window: self.ids.intern(e.window),
                        mask_bits: e.value_mask.bits(),
                        changes,
                    })
                }
                XEvent::PropertyNotify(e) => {
                    if e.state == xproto::Property::DELETE.into() {
                        return None;
                    }
                    let kind = self.map_property_kind(e.atom);
                    Some(BackendEvent::PropertyChanged {
                        window: self.ids.intern(e.window),
                        kind,
                    })
                }
                XEvent::ClientMessage(e) => {
                    let data32 = e.data.as_data32();
                    if e.type_ == self.atoms._NET_WM_STATE && e.format == 32 && data32.len() >= 2 {
                        let window = self.ids.intern(e.window);
                        if let Some(action) = Self::map_net_wm_action(data32[0]) {
                            for &atom in &[data32[1], data32[2]] {
                                if atom == self.atoms._NET_WM_STATE_FULLSCREEN {
                                    return Some(BackendEvent::WindowStateRequest {
                                        window,
                                        action,
                                        state: NetWmState::Fullscreen,
                                    });
                                }
                            }
                        }
                    }
                    if e.type_ == self.atoms._NET_ACTIVE_WINDOW {
                        return Some(BackendEvent::ActiveWindowMessage {
                            window: self.ids.intern(e.window),
                        });
                    }
                    if e.type_ == self.atoms._NET_WM_MOVERESIZE && e.format == 32 {
                        let direction = data32.get(2).copied().unwrap_or(0);
                        let button = data32.get(3).copied().unwrap_or(0);
                        return Some(BackendEvent::MoveResizeRequest {
                            window: self.ids.intern(e.window),
                            direction,
                            button,
                        });
                    }
                    Some(BackendEvent::ClientMessage {
                        window: self.ids.intern(e.window),
                        type_: e.type_,
                        data: [
                            data32.get(0).copied().unwrap_or(0),
                            data32.get(1).copied().unwrap_or(0),
                            data32.get(2).copied().unwrap_or(0),
                            data32.get(3).copied().unwrap_or(0),
                            data32.get(4).copied().unwrap_or(0),
                        ],
                        format: e.format,
                    })
                }
                XEvent::MappingNotify(_) => Some(BackendEvent::MappingNotify),
                XEvent::Expose(e) => Some(BackendEvent::Expose {
                    window: self.ids.intern(e.window),
                }),
                XEvent::Unknown(ref raw) => {
                    if let Some(base) = self.damage_event_base {
                        if raw.len() >= 8 && (raw[0] & 0x7f) == base {
                            // DamageNotify: bytes 4..8 contain the drawable (u32 LE)
                            let drawable = u32::from_ne_bytes([
                                raw[4], raw[5], raw[6], raw[7],
                            ]);
                            return Some(BackendEvent::DamageNotify {
                                drawable: self.ids.intern(drawable),
                            });
                        }
                    }
                    None
                }
                _ => None,
            }
        }

        pub(super) fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>> {
            let ev = self.conn.poll_for_event()?;
            Ok(ev.and_then(|e| self.map_event(e)))
        }
    }

    impl EventSource for X11EventSource {
        type Event = BackendEvent;
        type Metadata = ();
        type Ret = ();
        type Error = BackendError;

        fn process_events<F>(
            &mut self,
            _readiness: Readiness,
            _token: Token,
            mut callback: F,
        ) -> Result<PostAction, Self::Error>
        where
            F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
        {
            // Batch events and coalesce motion events for better performance
            let mut pending_motion: Option<BackendEvent> = None;

            loop {
                match self.poll_event() {
                    Ok(Some(event)) => {
                        match &event {
                            BackendEvent::MotionNotify { .. } => {
                                // Coalesce motion events - only keep the latest one
                                // This dramatically reduces processing overhead during drags
                                pending_motion = Some(event);
                            }
                            _ => {
                                // For non-motion events, first flush any pending motion
                                if let Some(m) = pending_motion.take() {
                                    callback(m, &mut ());
                                }
                                callback(event, &mut ());
                            }
                        }
                    }
                    Ok(None) => {
                        // No more events - flush any pending motion event
                        if let Some(m) = pending_motion.take() {
                            callback(m, &mut ());
                        }
                        break;
                    }
                    Err(e) => {
                        log::error!("X11 poll error: {:?}", e);
                        let err_msg = format!("X11 poll error: {}", e);
                        return Err(BackendError::from(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            err_msg,
                        )));
                    }
                }
            }
            Ok(PostAction::Continue)
        }

        fn register(
            &mut self,
            poll: &mut Poll,
            token_factory: &mut TokenFactory,
        ) -> calloop::Result<()> {
            let raw_fd = self.conn.stream().as_raw_fd();
            unsafe {
                let fd = BorrowedFd::borrow_raw(raw_fd);
                poll.register(fd, Interest::READ, Mode::Level, token_factory.token())
            }
        }

        fn reregister(
            &mut self,
            poll: &mut Poll,
            token_factory: &mut TokenFactory,
        ) -> calloop::Result<()> {
            let raw_fd = self.conn.stream().as_raw_fd();
            let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
            poll.reregister(fd, Interest::READ, Mode::Level, token_factory.token())
        }

        fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
            let raw_fd = self.conn.stream().as_raw_fd();
            let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
            poll.unregister(fd)
        }
    }
}

mod ewmh_facade {
    use crate::backend::api::{EwmhFacade, EwmhFeature};
    use crate::backend::common_define::WindowId;
    use crate::backend::error::BackendError;
    use crate::backend::x11::Atoms;
    use super::ids::X11IdRegistry;
    use std::sync::Arc;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::protocol::xproto::CreateWindowAux;
    use x11rb::protocol::xproto::*;
    use x11rb::protocol::xproto::{AtomEnum, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    pub(super) struct X11EwmhFacade<C: Connection> {
        conn: Arc<C>,
        root: WindowId,
        atoms: Atoms,
        ids: X11IdRegistry,
    }

    impl<C: Connection + Send + Sync + 'static> X11EwmhFacade<C> {
        pub(super) fn new(conn: Arc<C>, root: WindowId, atoms: Atoms, ids: X11IdRegistry) -> Self {
            Self {
                conn,
                root,
                atoms,
                ids,
            }
        }
        fn feature_to_atom(&self, f: EwmhFeature) -> u32 {
            match f {
                EwmhFeature::ActiveWindow => self.atoms._NET_ACTIVE_WINDOW,
                EwmhFeature::Supported => self.atoms._NET_SUPPORTED,
                EwmhFeature::WmName => self.atoms._NET_WM_NAME,
                EwmhFeature::WmState => self.atoms._NET_WM_STATE,
                EwmhFeature::SupportingWmCheck => self.atoms._NET_SUPPORTING_WM_CHECK,
                EwmhFeature::WmStateFullscreen => self.atoms._NET_WM_STATE_FULLSCREEN,
                EwmhFeature::ClientList => self.atoms._NET_CLIENT_LIST,
                EwmhFeature::ClientInfo => self.atoms._NET_CLIENT_INFO,
                EwmhFeature::WmWindowType => self.atoms._NET_WM_WINDOW_TYPE,
                EwmhFeature::WmWindowTypeDialog => self.atoms._NET_WM_WINDOW_TYPE_DIALOG,
                EwmhFeature::CurrentDesktop => self.atoms._NET_CURRENT_DESKTOP,
                EwmhFeature::NumberOfDesktops => self.atoms._NET_NUMBER_OF_DESKTOPS,
                EwmhFeature::DesktopNames => self.atoms._NET_DESKTOP_NAMES,
                EwmhFeature::DesktopViewport => self.atoms._NET_DESKTOP_VIEWPORT,
                EwmhFeature::WmMoveResize => self.atoms._NET_WM_MOVERESIZE,
            }
        }
    }

    impl<C: Connection + Send + Sync + 'static> EwmhFacade for X11EwmhFacade<C> {
        fn declare_supported(&self, features: &[EwmhFeature]) -> Result<(), BackendError> {
            let atoms: Vec<u32> = features.iter().map(|f| self.feature_to_atom(*f)).collect();
            let r = self.ids.x11(self.root)?;
            self.conn.change_property32(
                PropMode::REPLACE,
                r,
                self.atoms._NET_SUPPORTED,
                AtomEnum::ATOM,
                &atoms,
            )?;
            Ok(())
        }

        fn reset_root_properties(&self) -> Result<(), BackendError> {
            for &prop in [
                self.atoms._NET_ACTIVE_WINDOW,
                self.atoms._NET_CLIENT_LIST,
                self.atoms._NET_SUPPORTED,
                self.atoms._NET_CLIENT_LIST_STACKING,
                self.atoms._NET_SUPPORTING_WM_CHECK,
                self.atoms._NET_CURRENT_DESKTOP,
                self.atoms._NET_NUMBER_OF_DESKTOPS,
                self.atoms._NET_DESKTOP_NAMES,
                self.atoms._NET_DESKTOP_VIEWPORT,
            ]
            .iter()
            {
                let r = self.ids.x11(self.root)?;
                let _ = self.conn.delete_property(r, prop);
            }
            Ok(())
        }
        fn setup_supporting_wm_check(&self, wm_name: &str) -> Result<WindowId, BackendError> {
            let frame_win = self.conn.generate_id()?;
            let aux = CreateWindowAux::new()
                .event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS)
                .override_redirect(1);
            let r = self.ids.x11(self.root)?;
            self.conn.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                frame_win,
                r,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &aux,
            )?;
            self.conn.change_property32(
                PropMode::REPLACE,
                r,
                self.atoms._NET_SUPPORTING_WM_CHECK,
                AtomEnum::WINDOW,
                &[frame_win],
            )?;
            self.conn.change_property32(
                PropMode::REPLACE,
                frame_win,
                self.atoms._NET_SUPPORTING_WM_CHECK,
                AtomEnum::WINDOW,
                &[frame_win],
            )?;
            // WM_NAME (STRING)
            x11rb::wrapper::ConnectionExt::change_property8(
                &*self.conn,
                PropMode::REPLACE,
                frame_win,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                wm_name.as_bytes(),
            )?;
            Ok(self.ids.intern(frame_win))
        }

        fn set_active_window(&self, win: WindowId) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            let r = self.ids.x11(self.root)?;
            self.conn.change_property32(
                PropMode::REPLACE,
                r,
                self.atoms._NET_ACTIVE_WINDOW,
                AtomEnum::WINDOW,
                &[w],
            )?;
            Ok(())
        }

        fn clear_active_window(&self) -> Result<(), BackendError> {
            use x11rb::protocol::xproto::ConnectionExt as RawExt;
            let r = self.ids.x11(self.root)?;
            self.conn
                .delete_property(r, self.atoms._NET_ACTIVE_WINDOW)?;
            Ok(())
        }

        fn set_client_list(&self, list: &[WindowId]) -> Result<(), BackendError> {
            let r = self.ids.x11(self.root)?;
            let raw: Vec<u32> = list.iter().filter_map(|&w| self.ids.x11(w).ok()).collect();
            self.conn.change_property32(
                PropMode::REPLACE,
                r,
                self.atoms._NET_CLIENT_LIST,
                AtomEnum::WINDOW,
                &raw,
            )?;
            Ok(())
        }

        fn set_client_list_stacking(&self, list: &[WindowId]) -> Result<(), BackendError> {
            let r = self.ids.x11(self.root)?;
            let raw: Vec<u32> = list.iter().filter_map(|&w| self.ids.x11(w).ok()).collect();
            self.conn.change_property32(
                PropMode::REPLACE,
                r,
                self.atoms._NET_CLIENT_LIST_STACKING,
                AtomEnum::WINDOW,
                &raw,
            )?;
            Ok(())
        }

        fn set_desktop_info(&self, current: u32, total: u32, names: &[&str]) -> Result<(), BackendError> {
            let r = self.ids.x11(self.root)?;

            // _NET_NUMBER_OF_DESKTOPS
            self.conn.change_property32(
                PropMode::REPLACE,
                r,
                self.atoms._NET_NUMBER_OF_DESKTOPS,
                AtomEnum::CARDINAL,
                &[total],
            )?;

            // _NET_CURRENT_DESKTOP
            self.conn.change_property32(
                PropMode::REPLACE,
                r,
                self.atoms._NET_CURRENT_DESKTOP,
                AtomEnum::CARDINAL,
                &[current],
            )?;

            // _NET_DESKTOP_NAMES (null-separated UTF8 strings)
            let mut name_bytes: Vec<u8> = Vec::new();
            for name in names {
                name_bytes.extend_from_slice(name.as_bytes());
                name_bytes.push(0);
            }
            x11rb::wrapper::ConnectionExt::change_property8(
                &*self.conn,
                PropMode::REPLACE,
                r,
                self.atoms._NET_DESKTOP_NAMES,
                self.atoms.UTF8_STRING,
                &name_bytes,
            )?;

            // _NET_DESKTOP_VIEWPORT (all zeros for single-screen virtual desktops)
            let viewports: Vec<u32> = vec![0; total as usize * 2];
            self.conn.change_property32(
                PropMode::REPLACE,
                r,
                self.atoms._NET_DESKTOP_VIEWPORT,
                AtomEnum::CARDINAL,
                &viewports,
            )?;

            Ok(())
        }
    }
}

mod input_ops {
    use crate::backend::error::BackendError;
    use std::sync::Arc;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    use crate::backend::api::AllowMode;
    use crate::backend::api::InputOps as InputOpsTrait;
    use crate::backend::common_define::StdCursorKind;
    use crate::backend::common_define::WindowId;
    use super::ids::X11IdRegistry;

    pub(super) struct X11InputOps<C: Connection> {
        conn: Arc<C>,
        root_x11: u32,
        ids: X11IdRegistry,
    }

    impl<C: Connection> Clone for X11InputOps<C> {
        fn clone(&self) -> Self {
            Self {
                conn: self.conn.clone(),
                root_x11: self.root_x11,
                ids: self.ids.clone(),
            }
        }
    }

    impl<C: Connection + Send + Sync + 'static> X11InputOps<C> {
        pub(super) fn new(conn: Arc<C>, root_x11: u32, ids: X11IdRegistry) -> Self {
            Self {
                conn,
                root_x11,
                ids,
            }
        }

        fn map_allow_mode(mode: AllowMode) -> Allow {
            match mode {
                AllowMode::AsyncPointer => Allow::ASYNC_POINTER,
                AllowMode::ReplayPointer => Allow::REPLAY_POINTER,
                AllowMode::SyncPointer => Allow::SYNC_POINTER,
                AllowMode::AsyncKeyboard => Allow::ASYNC_KEYBOARD,
                AllowMode::SyncKeyboard => Allow::SYNC_KEYBOARD,
                AllowMode::ReplayKeyboard => Allow::REPLAY_KEYBOARD,
                AllowMode::AsyncBoth => Allow::ASYNC_BOTH,
                AllowMode::SyncBoth => Allow::SYNC_BOTH,
            }
        }

        pub(super) fn allow_events_raw(&self, mode: Allow, time: u32) -> Result<(), BackendError> {
            self.conn.allow_events(mode, time)?;
            Ok(())
        }

        pub(super) fn query_pointer(&self) -> Result<QueryPointerReply, BackendError> {
            Ok(self.conn.query_pointer(self.root_x11)?.reply()?)
        }

        #[allow(dead_code)]
        pub(super) fn flush(&self) -> Result<(), BackendError> {
            self.conn.flush()?;
            Ok(())
        }
    }

    impl<C: Connection + Send + Sync + 'static> InputOpsTrait for X11InputOps<C> {
        fn get_pointer_position(&self) -> Result<(f64, f64), BackendError> {
            let reply = self.query_pointer()?;
            // X11  f64
            Ok((reply.root_x as f64, reply.root_y as f64))
        }

        fn grab_pointer(&self, _mask: u32, cursor: Option<u64>) -> Result<bool, BackendError> {
            let cursor_id = cursor.map(|c| c as u32).unwrap_or(0);
            //  Grab Pointer  ButtonRelease  Motion
            let mask = EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;

            let reply = self
                .conn
                .grab_pointer(
                    false,
                    self.root_x11,
                    mask,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    0u32, // None confine_to
                    cursor_id,
                    0u32, // Current time
                )?
                .reply()?;

            Ok(reply.status == GrabStatus::SUCCESS)
        }

        fn set_cursor(&self, _kind: StdCursorKind) -> Result<(), BackendError> {
            Ok(())
        }

        fn ungrab_pointer(&self) -> Result<(), BackendError> {
            self.conn.ungrab_pointer(0u32)?;
            Ok(())
        }

        fn allow_events(&self, mode: AllowMode, time: u32) -> Result<(), BackendError> {
            let allow = Self::map_allow_mode(mode);
            self.allow_events_raw(allow, time)
        }

        fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), BackendError> {
            let reply = self.query_pointer()?;
            Ok((
                reply.root_x as i32,
                reply.root_y as i32,
                reply.mask.bits() as u16,
                0,
            ))
        }

        fn warp_pointer_to_window(&self, win: WindowId, x: i16, y: i16) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            self.conn.warp_pointer(0u32, w, 0, 0, 0, 0, x, y)?;
            Ok(())
        }
    }
}

mod key_ops {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    use log::warn;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    use crate::backend::api::KeyOps;
    use crate::backend::common_define::WindowId;
    use crate::backend::common_define::{KeySym, Mods};
    use crate::backend::error::BackendError;
    use super::adapter::mods_to_x11;
    use super::ids::X11IdRegistry;

    pub(super) struct X11KeyOps<C: Connection> {
        conn: Arc<C>,
        cache: HashMap<u8, u32>,
        numlock_mask: Arc<Mutex<u16>>,
        ids: X11IdRegistry,
        /// Cached full keyboard mapping (keysyms array + keysyms_per_keycode)
        /// Populated on first use, invalidated by clear_cache().
        full_keymap: Option<(Vec<u32>, u8, u8)>, // (keysyms, per_keycode, min_keycode)
    }

    impl<C: Connection> X11KeyOps<C> {
        pub(super) fn new(conn: Arc<C>, numlock_mask: Arc<Mutex<u16>>, ids: X11IdRegistry) -> Self {
            let mut ops = Self {
                conn: conn.clone(),
                cache: HashMap::new(),
                numlock_mask,
                ids,
                full_keymap: None,
            };
            let _ = ops.detect_and_store_numlock();
            ops
        }

        /// Ensure the full keyboard mapping is cached
        fn ensure_keymap_cached(&mut self) -> Result<(&[u32], u8, u8), BackendError> {
            if self.full_keymap.is_none() {
                let setup = self.conn.setup();
                let min = setup.min_keycode;
                let max = setup.max_keycode;
                let mapping = self
                    .conn
                    .get_keyboard_mapping(min, (max - min) + 1)?
                    .reply()?;
                let per = mapping.keysyms_per_keycode;
                self.full_keymap = Some((mapping.keysyms, per, min));

                // Pre-populate the keysym cache for all keycodes
                if let Some(ref km) = self.full_keymap {
                    let per_usize = km.1 as usize;
                    for offset in 0..((km.0.len()) / per_usize.max(1)) {
                        let kc = km.2 + offset as u8;
                        if let Some(&ks) = km.0.get(offset * per_usize) {
                            if ks != 0 {
                                self.cache.insert(kc, ks);
                            }
                        }
                    }
                }
            }
            let km = self.full_keymap.as_ref()
                .ok_or_else(|| BackendError::Message("keymap not initialized".into()))?;
            Ok((&km.0, km.1, km.2))
        }

        fn detect_and_store_numlock(&mut self) -> Result<(), BackendError> {
            // Populate the keymap cache during initialization so we
            // don't need to query it again later
            let (keysyms, per, min) = self.ensure_keymap_cached()?;
            let per_usize = per as usize;

            const XK_NUM_LOCK: u32 = 0xFF7F;
            let mut numkc: u8 = 0;
            let max = min as usize + keysyms.len() / per_usize.max(1);
            for kc_usize in (min as usize)..max {
                let idx = (kc_usize - min as usize) * per_usize;
                if idx < keysyms.len() {
                    for i in 0..per_usize {
                        if keysyms[idx + i] == XK_NUM_LOCK {
                            numkc = kc_usize as u8;
                            break;
                        }
                    }
                }
                if numkc != 0 {
                    break;
                }
            }

            let mask = if numkc == 0 {
                0
            } else {
                self.find_modifier_mask(numkc)? as u16
            };

            *self.numlock_mask.lock().unwrap() = mask;
            Ok(())
        }

        fn find_modifier_mask(&self, target_keycode: u8) -> Result<u8, BackendError> {
            let mm = self.conn.get_modifier_mapping()?.reply()?;
            let per = mm.keycodes_per_modifier() as usize;
            for mod_index in 0..8 {
                let start = mod_index * per;
                let end = start + per;
                if end <= mm.keycodes.len() {
                    for &kc in &mm.keycodes[start..end] {
                        if kc == target_keycode && kc != 0 {
                            return Ok(1 << mod_index);
                        }
                    }
                }
            }
            Ok(0)
        }
    }

    impl<C: Connection + Send + Sync + 'static> KeyOps for X11KeyOps<C> {
        fn clean_mods(&self, raw: u16) -> Mods {
            let numlock = *self.numlock_mask.lock().unwrap();
            let raw_mask = x11rb::protocol::xproto::KeyButMask::from(raw);
            let numlock_mask = x11rb::protocol::xproto::KeyButMask::from(numlock);
            super::adapter::mods_from_x11(raw_mask, numlock_mask)
        }

        fn clear_key_grabs(&self, root: WindowId) -> Result<(), BackendError> {
            let r = self.ids.x11(root)?;
            self.conn.ungrab_key(Grab::ANY, r, ModMask::ANY.into())?;
            Ok(())
        }

        fn grab_keys(&self, root: WindowId, bindings: &[(Mods, KeySym)]) -> Result<(), BackendError> {
            let numlock_local = *self.numlock_mask.lock().unwrap();
            let r = self.ids.x11(root)?;

            // Query keyboard mapping once for all bindings
            let setup = self.conn.setup();
            let min = setup.min_keycode;
            let max = setup.max_keycode;
            let mapping = self
                .conn
                .get_keyboard_mapping(min, (max - min) + 1)?
                .reply()?;
            let per = mapping.keysyms_per_keycode as usize;

            use x11rb::protocol::xproto::{KeyButMask as KBM, ModMask};
            let numlock_mask_obj = KBM::from(numlock_local);

            for (mods, keysym) in bindings {
                for (offset, keysyms_for_keycode) in mapping.keysyms.chunks(per).enumerate() {
                    let keycode = min + offset as u8;
                    let matched = keysyms_for_keycode.iter().any(|&ks| ks == *keysym);
                    if matched {
                        let base = mods_to_x11(*mods, numlock_mask_obj);
                        let combos = [
                            base,
                            base | KBM::LOCK,
                            base | numlock_mask_obj,
                            base | KBM::LOCK | numlock_mask_obj,
                        ];
                        for mm in combos {
                            let cookie = self.conn.grab_key(
                                false,
                                r,
                                ModMask::from(mm.bits()),
                                keycode,
                                GrabMode::ASYNC,
                                GrabMode::ASYNC,
                            )?;
                            if let Err(e) = cookie.check() {
                                // If another client grabbed the same key, X11 will typically
                                // report BadAccess asynchronously. Surface it for debugging.
                                warn!(
                                    "X11 grab_key failed (keysym=0x{:x}, keycode={}, mods=0x{:x}): {:?}",
                                    *keysym,
                                    keycode,
                                    mm.bits(),
                                    e
                                );
                            }
                        }
                    }
                }
            }

            self.conn.flush()?;
            Ok(())
        }

        fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, BackendError> {
            if let Some(&ks) = self.cache.get(&keycode) {
                return Ok(ks);
            }

            // Ensure full keymap is cached and use it
            let (keysyms, per, min) = self.ensure_keymap_cached()?;
            let per_usize = per as usize;
            if keycode >= min && per_usize > 0 {
                let offset = (keycode - min) as usize;
                if let Some(&ks) = keysyms.get(offset * per_usize) {
                    return Ok(ks);
                }
            }

            Ok(0)
        }

        fn clear_cache(&mut self) {
            self.cache.clear();
            self.full_keymap = None;
        }
    }
}

mod output_ops {
    use crate::backend::api::{OutputInfo, OutputOps, ScreenInfo};
    use crate::backend::common_define::OutputId;
    use std::sync::{Arc, Mutex};
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::{self, ConnectionExt as RandrExt};

    /// Calculate refresh rate in millihertz from a RandR ModeInfo.
    fn calc_refresh_mhz(mode: &randr::ModeInfo) -> u32 {
        if mode.htotal == 0 || mode.vtotal == 0 {
            return 60000;
        }
        let mut vtotal = mode.vtotal as u64;
        let flags = u32::from(mode.mode_flags);
        if flags & (1 << 4) != 0 {
            vtotal *= 2; // DoubleScan
        }
        if flags & (1 << 0) != 0 {
            vtotal /= 2; // Interlace (fields per second → frames)
        }
        let denom = mode.htotal as u64 * vtotal;
        if denom == 0 {
            return 60000;
        }
        ((mode.dot_clock as u64 * 1000) / denom) as u32
    }

    pub(super) struct X11OutputOps<C: Connection> {
        conn: Arc<C>,
        root: u32,
        sw: i32,
        sh: i32,
        /// Cached output layout - invalidated on RandR events
        /// None = cache miss, Some(vec) = cached outputs
        cached_outputs: Arc<Mutex<Option<Vec<OutputInfo>>>>,
    }

    impl<C: Connection> X11OutputOps<C> {
        pub(super) fn new(conn: Arc<C>, root: u32, sw: i32, sh: i32) -> Self {
            Self {
                conn,
                root,
                sw,
                sh,
                cached_outputs: Arc::new(Mutex::new(None)),
            }
        }

        /// Invalidate output cache - call on RandR events
        fn invalidate_cache(&self) {
            if let Ok(mut cache) = self.cached_outputs.lock() {
                *cache = None;
            }
        }

        /// Get cached outputs or query if cache miss
        fn get_cached_or_query(&self) -> Vec<OutputInfo> {
            // Fast path: check cache
            if let Ok(cache) = self.cached_outputs.lock() {
                if let Some(ref outputs) = *cache {
                    return outputs.clone();
                }
            }

            // Cache miss: query X11
            let outputs = self.query_outputs_internal();

            // Update cache
            if let Ok(mut cache) = self.cached_outputs.lock() {
                *cache = Some(outputs.clone());
            }

            outputs
        }

        /// Internal query method (does the actual X11 round-trips)
        fn query_outputs_internal(&self) -> Vec<OutputInfo> {
            // Try RandR 1.5 first (monitors API)
            if let Ok(ver) = self.conn.randr_query_version(1, 5) {
                if let Ok(v) = ver.reply() {
                    if (v.major_version > 1) || (v.major_version == 1 && v.minor_version >= 5) {
                        if let Ok(cookie) = self.conn.randr_get_monitors(self.root, true) {
                            if let Ok(reply) = cookie.reply() {
                                // Pre-fetch screen resources for mode info lookup
                                let modes: Vec<randr::ModeInfo> = self
                                    .conn
                                    .randr_get_screen_resources(self.root)
                                    .ok()
                                    .and_then(|c| c.reply().ok())
                                    .map(|r| r.modes)
                                    .unwrap_or_default();

                                let mut out = Vec::with_capacity(4);
                                for (i, m) in reply.monitors.into_iter().enumerate() {
                                    if m.width > 0 && m.height > 0 {
                                        // Resolve refresh rate: output → output_info → crtc → crtc_info → mode
                                        let refresh = m
                                            .outputs
                                            .first()
                                            .and_then(|&output| {
                                                self.conn
                                                    .randr_get_output_info(output, 0)
                                                    .ok()?
                                                    .reply()
                                                    .ok()
                                            })
                                            .and_then(|oi| {
                                                if oi.crtc == 0 {
                                                    return None;
                                                }
                                                self.conn
                                                    .randr_get_crtc_info(oi.crtc, 0)
                                                    .ok()?
                                                    .reply()
                                                    .ok()
                                            })
                                            .and_then(|ci| {
                                                modes
                                                    .iter()
                                                    .find(|mode| mode.id == ci.mode)
                                                    .map(calc_refresh_mhz)
                                            })
                                            .unwrap_or(60000);

                                        out.push(OutputInfo {
                                            id: OutputId(i as u64),
                                            name: format!("Monitor-{}", i),
                                            x: m.x as i32,
                                            y: m.y as i32,
                                            width: m.width as i32,
                                            height: m.height as i32,
                                            scale: 1.0,
                                            refresh_rate: refresh,
                                        });
                                    }
                                }
                                if !out.is_empty() {
                                    return out;
                                }
                            }
                        }
                    }
                }
            }

            // Fallback: RandR 1.2 CRTC enumeration
            if let Ok(cookie) = self.conn.randr_get_screen_resources(self.root) {
                if let Ok(resources) = cookie.reply() {
                    let modes = &resources.modes;
                    let mut out = Vec::with_capacity(4);
                    for (i, crtc) in resources.crtcs.iter().enumerate() {
                        if let Ok(cookie) = self.conn.randr_get_crtc_info(*crtc, 0) {
                            if let Ok(ci) = cookie.reply() {
                                if ci.width > 0 && ci.height > 0 {
                                    let refresh = modes
                                        .iter()
                                        .find(|m| m.id == ci.mode)
                                        .map(calc_refresh_mhz)
                                        .unwrap_or(60000);
                                    out.push(OutputInfo {
                                        id: OutputId(i as u64),
                                        name: format!("CRTC-{}", i),
                                        x: ci.x as i32,
                                        y: ci.y as i32,
                                        width: ci.width as i32,
                                        height: ci.height as i32,
                                        scale: 1.0,
                                        refresh_rate: refresh,
                                    });
                                }
                            }
                        }
                    }
                    if !out.is_empty() {
                        return out;
                    }
                }
            }

            // Ultimate fallback: single screen
            vec![OutputInfo {
                id: OutputId(0),
                name: "Default".to_string(),
                x: 0,
                y: 0,
                width: self.sw,
                height: self.sh,
                scale: 1.0,
                refresh_rate: 60000,
            }]
        }
    }

    impl<C: Connection + Send + Sync + 'static> OutputOps for X11OutputOps<C> {
        fn screen_info(&self) -> ScreenInfo {
            ScreenInfo {
                width: self.sw,
                height: self.sh,
            }
        }

        fn output_at(&self, x: i32, y: i32) -> Option<OutputId> {
            // Use cached outputs instead of querying every single time (5-10ms savings!)
            let outputs = self.get_cached_or_query();
            for output in outputs {
                if x >= output.x
                    && x < output.x + output.width
                    && y >= output.y
                    && y < output.y + output.height
                {
                    return Some(output.id);
                }
            }
            None
        }

        fn enumerate_outputs(&self) -> Vec<OutputInfo> {
            self.get_cached_or_query()
        }

        fn invalidate_output_cache(&self) {
            self.invalidate_cache();
        }
    }
}

mod property_ops {
    use crate::backend::api::NormalHints;
    use crate::backend::api::StrutPartial;
    use crate::backend::api::WmHints;
    use crate::backend::api::{PropertyOps as PropertyOpsTrait, WindowType};
    use crate::backend::common_define::WindowId;
    use crate::backend::error::BackendError;
    use crate::backend::x11::Atoms;
    use super::ids::X11IdRegistry;
    use std::sync::Arc;
    use x11rb::connection::Connection;
    use x11rb::properties::WmSizeHints;
    use x11rb::protocol::xproto::*;
    use x11rb::wrapper::ConnectionExt as _;

    pub(super) struct X11PropertyOps<C: Connection> {
        conn: Arc<C>,
        atoms: Atoms,
        ids: X11IdRegistry,
    }

    impl<C: Connection> X11PropertyOps<C> {
        pub(super) fn new(conn: Arc<C>, atoms: Atoms, ids: X11IdRegistry) -> Self {
            Self { conn, atoms, ids }
        }
    }

    impl<C: Connection + Send + Sync + 'static> X11PropertyOps<C> {
        fn get_text_property(&self, win: WindowId, atom: Atom) -> Option<String> {
            let w = self.ids.x11(win).ok()?;
            let reply = self
                .conn
                .get_property(false, w, atom, AtomEnum::ANY, 0, u32::MAX)
                .ok()?
                .reply()
                .ok()?;

            if reply.value.is_empty() || reply.format != 8 {
                return None;
            }

            let value = reply.value;
            if reply.type_ == self.atoms.UTF8_STRING {
                Self::parse_utf8(&value)
            } else if reply.type_ == u32::from(AtomEnum::STRING) {
                Some(Self::parse_latin1(&value))
            } else {
                Self::parse_utf8(&value).or_else(|| Some(Self::parse_latin1(&value)))
            }
        }

        fn parse_utf8(value: &[u8]) -> Option<String> {
            String::from_utf8(value.to_vec()).ok()
        }
        fn parse_latin1(value: &[u8]) -> String {
            value.iter().map(|&b| b as char).collect()
        }

        fn get_net_wm_state_atoms(&self, win: WindowId) -> Result<Vec<u32>, BackendError> {
            let w = self.ids.x11(win)?;
            let reply = self
                .conn
                .get_property(
                    false,
                    w,
                    self.atoms._NET_WM_STATE,
                    AtomEnum::ATOM,
                    0,
                    u32::MAX,
                )?
                .reply()?;
            if reply.format != 32 {
                return Ok(Vec::new());
            }
            Ok(reply.value32().into_iter().flatten().collect())
        }

        fn set_net_wm_state_atoms(&self, win: WindowId, atoms: &[u32]) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            self.conn.change_property32(
                PropMode::REPLACE,
                w,
                self.atoms._NET_WM_STATE,
                AtomEnum::ATOM,
                atoms,
            )?;
            Ok(())
        }

        fn add_net_wm_state_atom(&self, win: WindowId, atom: u32) -> Result<(), BackendError> {
            let mut states = self.get_net_wm_state_atoms(win)?;
            if !states.iter().any(|&a| a == atom) {
                states.push(atom);
                self.set_net_wm_state_atoms(win, &states)?;
            }
            Ok(())
        }

        fn remove_net_wm_state_atom(&self, win: WindowId, atom: u32) -> Result<(), BackendError> {
            let mut states = self.get_net_wm_state_atoms(win)?;
            let len_before = states.len();
            states.retain(|&a| a != atom);
            if states.len() != len_before {
                self.set_net_wm_state_atoms(win, &states)?;
            }
            Ok(())
        }

        fn atom_to_window_type(&self, atom: u32) -> WindowType {
            if atom == self.atoms._NET_WM_WINDOW_TYPE_DESKTOP {
                WindowType::Desktop
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_DOCK {
                WindowType::Dock
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_TOOLBAR {
                WindowType::Toolbar
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_MENU {
                WindowType::Menu
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_UTILITY {
                WindowType::Utility
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_SPLASH {
                WindowType::Splash
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_DIALOG {
                WindowType::Dialog
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_DROPDOWN_MENU {
                WindowType::DropdownMenu
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_POPUP_MENU {
                WindowType::PopupMenu
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_TOOLTIP {
                WindowType::Tooltip
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_NOTIFICATION {
                WindowType::Notification
            } else if atom == self.atoms._NET_WM_WINDOW_TYPE_COMBO {
                WindowType::Combo
            }
            // else if atom == self.atoms._NET_WM_WINDOW_TYPE_DND { WindowType::Dnd }
            else {
                WindowType::Unknown
            }
        }
    }

    impl<C: Connection + Send + Sync + 'static> PropertyOpsTrait for X11PropertyOps<C> {
        fn get_title(&self, win: WindowId) -> String {
            if let Some(title) = self.get_text_property(win, self.atoms._NET_WM_NAME) {
                return title;
            }
            if let Some(title) = self.get_text_property(win, AtomEnum::WM_NAME.into()) {
                return title;
            }
            "".to_string()
        }

        fn get_class(&self, win: WindowId) -> (String, String) {
            let w = match self.ids.x11(win) {
                Ok(w) => w,
                Err(_) => return (String::new(), String::new()),
            };
            let reply = match self
                .conn
                .get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            {
                Ok(cookie) => cookie.reply().ok(),
                Err(_) => None,
            };

            if let Some(reply) = reply {
                if reply.type_ == u32::from(AtomEnum::STRING) && reply.format == 8 {
                    let value = reply.value;
                    if !value.is_empty() {
                        let mut parts = value.split(|&b| b == 0u8).filter(|s| !s.is_empty());
                        let instance = parts
                            .next()
                            .and_then(|s| String::from_utf8(s.to_vec()).ok())
                            .unwrap_or_default();
                        let class = parts
                            .next()
                            .and_then(|s| String::from_utf8(s.to_vec()).ok())
                            .unwrap_or_default();
                        return (instance.to_lowercase(), class.to_lowercase());
                    }
                }
            }
            (String::new(), String::new())
        }

        fn get_window_types(&self, win: WindowId) -> Vec<WindowType> {
            let w = match self.ids.x11(win) {
                Ok(w) => w,
                Err(_) => return Vec::new(),
            };
            let mut result = Vec::new();
            if let Ok(reply) = self.conn.get_property(
                false,
                w,
                self.atoms._NET_WM_WINDOW_TYPE,
                AtomEnum::ATOM,
                0,
                u32::MAX,
            ) {
                if let Ok(rep) = reply.reply() {
                    if rep.format == 32 {
                        for atom in rep.value32().into_iter().flatten() {
                            let wt = self.atom_to_window_type(atom);
                            if wt != WindowType::Unknown {
                                result.push(wt);
                            }
                        }
                    }
                }
            }
            if result.is_empty() {
                if self.transient_for(win).is_some() {
                    result.push(WindowType::Dnd);
                } else {
                    result.push(WindowType::Normal);
                }
            }
            result
        }

        fn is_fullscreen(&self, win: WindowId) -> bool {
            let states = self.get_net_wm_state_atoms(win).unwrap_or_default();
            states
                .iter()
                .any(|&a| a == self.atoms._NET_WM_STATE_FULLSCREEN)
        }

        fn set_fullscreen_state(&self, win: WindowId, on: bool) -> Result<(), BackendError> {
            if on {
                self.add_net_wm_state_atom(win, self.atoms._NET_WM_STATE_FULLSCREEN)
            } else {
                self.remove_net_wm_state_atom(win, self.atoms._NET_WM_STATE_FULLSCREEN)
            }
        }

        fn get_wm_hints(&self, win: WindowId) -> Option<WmHints> {
            let w = self.ids.x11(win).ok()?;
            let prop = self
                .conn
                .get_property(false, w, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, 0, 20)
                .ok()?
                .reply()
                .ok()?;

            let mut it = prop.value32()?.into_iter();
            let flags = it.next()?;
            const X_URGENCY_HINT: u32 = 1 << 8;
            const INPUT_HINT: u32 = 1 << 0;

            let urgent = (flags & X_URGENCY_HINT) != 0;
            let input = if (flags & INPUT_HINT) != 0 {
                it.next().map(|v| v != 0)
            } else {
                None
            };
            Some(WmHints { urgent, input })
        }

        fn set_urgent_hint(&self, win: WindowId, urgent: bool) -> Result<(), BackendError> {
            const X_URGENCY_HINT: u32 = 1 << 8;
            let w = self.ids.x11(win)?;
            let cookie =
                self.conn
                    .get_property(false, w, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, 0, 20)?;

            let mut data = Vec::new();
            if let Ok(reply) = cookie.reply() {
                data = reply.value32().into_iter().flatten().collect();
            }
            if data.is_empty() {
                data.push(0);
            }

            if urgent {
                data[0] |= X_URGENCY_HINT;
            } else {
                data[0] &= !X_URGENCY_HINT;
            }

            self.conn.change_property32(
                PropMode::REPLACE,
                w,
                AtomEnum::WM_HINTS,
                AtomEnum::WM_HINTS,
                &data,
            )?;
            Ok(())
        }

        fn transient_for(&self, win: WindowId) -> Option<WindowId> {
            let w = self.ids.x11(win).ok()?;
            let r = self
                .conn
                .get_property(
                    false,
                    w,
                    self.atoms.WM_TRANSIENT_FOR,
                    AtomEnum::WINDOW,
                    0,
                    1,
                )
                .ok()?
                .reply()
                .ok()?;

            if r.format == 32 {
                if let Some(t) = r.value32()?.next() {
                    if t != 0 && t != w {
                        return Some(self.ids.intern(t));
                    }
                }
            }
            None
        }

        fn fetch_normal_hints(&self, win: WindowId) -> Result<Option<NormalHints>, BackendError> {
            let w = self.ids.x11(win)?;
            let reply_opt = WmSizeHints::get_normal_hints(&self.conn, w)?.reply()?;
            if let Some(r) = reply_opt {
                let (mut base_w, mut base_h) = (0, 0);
                let (mut inc_w, mut inc_h) = (0, 0);
                let (mut max_w, mut max_h) = (0, 0);
                let (mut min_w, mut min_h) = (0, 0);
                let (mut min_aspect, mut max_aspect) = (0.0, 0.0);

                if let Some((w, h)) = r.base_size {
                    base_w = w;
                    base_h = h;
                }
                if let Some((w, h)) = r.size_increment {
                    inc_w = w;
                    inc_h = h;
                }
                if let Some((w, h)) = r.max_size {
                    max_w = w;
                    max_h = h;
                }
                if let Some((w, h)) = r.min_size {
                    min_w = w;
                    min_h = h;
                }
                if let Some((min, max)) = r.aspect {
                    min_aspect = min.numerator as f32 / min.denominator as f32;
                    max_aspect = max.numerator as f32 / max.denominator as f32;
                }
                Ok(Some(NormalHints {
                    base_w,
                    base_h,
                    inc_w,
                    inc_h,
                    max_w,
                    max_h,
                    min_w,
                    min_h,
                    min_aspect,
                    max_aspect,
                }))
            } else {
                Ok(None)
            }
        }

        fn set_window_strut_top(
            &self,
            win: WindowId,
            top: u32,
            start_x: u32,
            end_x: u32,
        ) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            let strut = [0, 0, top, 0];
            self.conn.change_property32(
                PropMode::REPLACE,
                w,
                self.atoms._NET_WM_STRUT,
                AtomEnum::CARDINAL,
                &strut,
            )?;
            let partial = [0, 0, top, 0, 0, 0, 0, 0, start_x, end_x, 0, 0];
            self.conn.change_property32(
                PropMode::REPLACE,
                w,
                self.atoms._NET_WM_STRUT_PARTIAL,
                AtomEnum::CARDINAL,
                &partial,
            )?;
            Ok(())
        }

        fn clear_window_strut(&self, win: WindowId) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            let _ = self.conn.delete_property(w, self.atoms._NET_WM_STRUT);
            let _ = self
                .conn
                .delete_property(w, self.atoms._NET_WM_STRUT_PARTIAL);
            Ok(())
        }

        fn get_window_strut_partial(&self, win: WindowId) -> Option<StrutPartial> {
            let w = self.ids.x11(win).ok()?;
            // Try _NET_WM_STRUT_PARTIAL first (12 CARDINAL values)
            if let Ok(reply) = self
                .conn
                .get_property(
                    false,
                    w,
                    self.atoms._NET_WM_STRUT_PARTIAL,
                    AtomEnum::CARDINAL,
                    0,
                    12,
                )
                .ok()?
                .reply()
            {
                if reply.format == 32 {
                    let vals: Vec<u32> = reply.value32()?.collect();
                    if vals.len() >= 12 {
                        return Some(StrutPartial {
                            left: vals[0],
                            right: vals[1],
                            top: vals[2],
                            bottom: vals[3],
                            left_start_y: vals[4],
                            left_end_y: vals[5],
                            right_start_y: vals[6],
                            right_end_y: vals[7],
                            top_start_x: vals[8],
                            top_end_x: vals[9],
                            bottom_start_x: vals[10],
                            bottom_end_x: vals[11],
                        });
                    }
                }
            }
            // Fallback: _NET_WM_STRUT (4 CARDINAL values, no start/end ranges)
            if let Ok(reply) = self
                .conn
                .get_property(
                    false,
                    w,
                    self.atoms._NET_WM_STRUT,
                    AtomEnum::CARDINAL,
                    0,
                    4,
                )
                .ok()?
                .reply()
            {
                if reply.format == 32 {
                    let vals: Vec<u32> = reply.value32()?.collect();
                    if vals.len() >= 4 {
                        return Some(StrutPartial {
                            left: vals[0],
                            right: vals[1],
                            top: vals[2],
                            bottom: vals[3],
                            ..Default::default()
                        });
                    }
                }
            }
            None
        }

        fn set_client_info_props(
            &self,
            win: WindowId,
            tags: u32,
            monitor_num: u32,
        ) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            let data = [tags, monitor_num];
            self.conn.change_property32(
                PropMode::REPLACE,
                w,
                self.atoms._NET_CLIENT_INFO,
                AtomEnum::CARDINAL,
                &data,
            )?;
            Ok(())
        }

        fn get_wm_state(&self, win: WindowId) -> Result<i64, BackendError> {
            let w = self.ids.x11(win)?;
            let reply = self
                .conn
                .get_property(false, w, self.atoms.WM_STATE, self.atoms.WM_STATE, 0, 2)?
                .reply()?;
            if reply.format != 32 {
                return Ok(-1);
            }
            Ok(reply
                .value32()
                .into_iter()
                .flatten()
                .next()
                .map(|v| v as i64)
                .unwrap_or(-1))
        }

        fn set_wm_state(&self, win: WindowId, state: i64) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            let data: [u32; 2] = [state as u32, 0];
            self.conn.change_property32(
                PropMode::REPLACE,
                w,
                self.atoms.WM_STATE,
                self.atoms.WM_STATE,
                &data,
            )?;
            Ok(())
        }
    }
}

mod window_ops {
    use crate::backend::api::{CloseResult, Geometry, WindowAttributes, WindowOps};
    use crate::backend::api::{StackMode, WindowChanges};
    use crate::backend::common_define::{Mods, Pixel, WindowId};
    use crate::backend::error::BackendError;
    use crate::backend::x11::Atoms;
    use crate::backend::x11::batch::X11RequestBatcher;
    use super::adapter::{event_mask_from_generic, mods_to_x11};
    use super::ids::X11IdRegistry;
    use log::debug;
    use std::env;
    use std::sync::Arc;
    use std::sync::Mutex;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;
    use x11rb::x11_utils::Serialize;

    pub(super) struct X11WindowOps<C: Connection> {
        conn: Arc<C>,
        atoms: Atoms,
        numlock_mask: Arc<Mutex<u16>>,
        root_x11: u32,
        ids: X11IdRegistry,
        batcher: X11RequestBatcher,
    }

    impl<C: Connection> X11WindowOps<C> {
        fn debug_drag_enabled() -> bool {
            env::var("JWM_DEBUG_DRAG")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(true)
        }

        pub(super) fn new(
            conn: Arc<C>,
            atoms: Atoms,
            numlock_mask: Arc<Mutex<u16>>,
            root_x11: u32,
            ids: X11IdRegistry,
        ) -> Self {
            Self {
                conn,
                atoms,
                numlock_mask,
                root_x11,
                ids,
                batcher: X11RequestBatcher::new(),
            }
        }

        fn send_configure_notify_internal(
            &self,
            win: WindowId,
            x: i16,
            y: i16,
            width: u16,
            height: u16,
            border: u16,
        ) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            let event = ConfigureNotifyEvent {
                response_type: CONFIGURE_NOTIFY_EVENT,
                sequence: 0,
                event: w,
                window: w,
                x,
                y,
                width,
                height,
                border_width: border,
                above_sibling: 0,
                override_redirect: false,
            };
            self.conn
                .send_event(false, w, EventMask::STRUCTURE_NOTIFY, event)?;
            self.batcher.mark_op(&*self.conn)?;
            Ok(())
        }
    }

    impl<C: Connection + Send + Sync + 'static> WindowOps for X11WindowOps<C> {
        fn set_position(&self, win: WindowId, x: i32, y: i32) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            if Self::debug_drag_enabled() {
                debug!("[drag] x11 set_position win={:?} x={} y={}", win, x, y);
            }
            let aux = ConfigureWindowAux::new().x(x).y(y);
            self.conn.configure_window(w, &aux)?;
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
            let wid = self.ids.x11(win)?;

            if Self::debug_drag_enabled() {
                debug!(
                    "[drag] x11 configure win={:?} x={} y={} w={} h={} border={}",
                    win, x, y, w, h, border
                );
            }

            // 1. 
            let aux = ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(w)
                .height(h)
                .border_width(border);
            self.conn.configure_window(wid, &aux)?;

            // 2.  ConfigureNotify (ICCCM )
            self.send_configure_notify_internal(
                win,
                x as i16,
                y as i16,
                w as u16,
                h as u16,
                border as u16,
            )?;

            Ok(())
        }

        fn set_decoration_style(
            &self,
            win: WindowId,
            border_width: u32,
            border_color: Pixel,
        ) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            // 
            let aux_attr = ChangeWindowAttributesAux::new().border_pixel(border_color.0);
            self.conn.change_window_attributes(w, &aux_attr)?;
            // 
            let aux_conf = ConfigureWindowAux::new().border_width(border_width);
            self.conn.configure_window(w, &aux_conf)?;
            Ok(())
        }

        fn raise_window(&self, win: WindowId) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            let aux = ConfigureWindowAux::new().stack_mode(x11rb::protocol::xproto::StackMode::ABOVE);
            self.conn.configure_window(w, &aux)?;
            Ok(())
        }

        fn restack_windows(&self, windows: &[WindowId]) -> Result<(), BackendError> {
            if windows.is_empty() {
                return Ok(());
            }
            // Raise the first window to the top of the stack
            let first = self.ids.x11(windows[0])?;
            let aux = ConfigureWindowAux::new()
                .stack_mode(x11rb::protocol::xproto::StackMode::ABOVE);
            self.conn.configure_window(first, &aux)?;

            // Stack subsequent windows above their predecessor using sibling
            let mut prev = first;
            for &win in &windows[1..] {
                if let Ok(w) = self.ids.x11(win) {
                    let aux = ConfigureWindowAux::new()
                        .sibling(prev)
                        .stack_mode(x11rb::protocol::xproto::StackMode::ABOVE);
                    self.conn.configure_window(w, &aux)?;
                    prev = w;
                }
            }
            Ok(())
        }

        fn close_window(&self, win: WindowId) -> Result<CloseResult, BackendError> {
            let w = self.ids.x11(win)?;
            let supports_delete = {
                let reply = self
                    .conn
                    .get_property(false, w, self.atoms.WM_PROTOCOLS, AtomEnum::ATOM, 0, 1024)?
                    .reply()?;
                reply
                    .value32()
                    .into_iter()
                    .flatten()
                    .any(|a| a == self.atoms.WM_DELETE_WINDOW)
            };

            if supports_delete {
                let event = ClientMessageEvent::new(
                    32,
                    w,
                    self.atoms.WM_PROTOCOLS,
                    [self.atoms.WM_DELETE_WINDOW, 0, 0, 0, 0],
                );
                self.conn.send_event(
                    false,
                    w,
                    EventMask::NO_EVENT,
                    event.serialize(), // 
                )?;
                // 
                self.conn.flush()?;
                return Ok(CloseResult::Graceful);
            }

            self.conn.kill_client(w)?;
            Ok(CloseResult::Forced)
        }

        fn scan_windows(&self) -> Result<Vec<WindowId>, BackendError> {
            let tree = self
                .conn
                .query_tree(self.conn.setup().roots[0].root)?
                .reply()?;
            Ok(tree.children.iter().map(|&w| self.ids.intern(w)).collect())
        }

        fn change_event_mask(&self, win: WindowId, mask: u32) -> Result<(), BackendError> {
            debug!("[change_event_mask]");
            let w = self.ids.x11(win)?;
            let x_mask = event_mask_from_generic(mask);
            let aux = ChangeWindowAttributesAux::new().event_mask(x_mask);
            self.conn.change_window_attributes(w, &aux)?;
            Ok(())
        }

        fn grab_button_any_anymod(
            &self,
            win: WindowId,
            event_mask_bits: u32,
        ) -> Result<(), BackendError> {
            let x_mask = event_mask_from_generic(event_mask_bits);
            let w = self.ids.x11(win)?;
            self.conn.grab_button(
                false,
                w,
                x_mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                0u32,
                ButtonIndex::ANY,
                ModMask::ANY.into(),
            )?;
            Ok(())
        }

        fn grab_button(
            &self,
            win: WindowId,
            button: u8,
            event_mask_bits: u32,
            mods: Mods,
        ) -> Result<(), BackendError> {
            let x_mask = event_mask_from_generic(event_mask_bits);
            let bi = ButtonIndex::from(button);
            let numlock_val = *self.numlock_mask.lock().unwrap();
            let numlock_obj = KeyButMask::from(numlock_val);
            let x_mods = mods_to_x11(mods, numlock_obj);
            let mods_bits = ModMask::from(x_mods.bits());
            let w = self.ids.x11(win)?;
            self.conn.grab_button(
                false,
                w,
                x_mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                0u32,
                bi,
                mods_bits,
            )?;
            Ok(())
        }

        fn map_window(&self, win: WindowId) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            self.conn.map_window(w)?;
            Ok(())
        }

        fn apply_window_changes(
            &self,
            win: WindowId,
            changes: WindowChanges,
        ) -> Result<(), BackendError> {
            let mut aux = ConfigureWindowAux::new();
            if let Some(x) = changes.x {
                aux = aux.x(x);
            }
            if let Some(y) = changes.y {
                aux = aux.y(y);
            }
            if let Some(w) = changes.width {
                aux = aux.width(w);
            }
            if let Some(h) = changes.height {
                aux = aux.height(h);
            }
            if let Some(b) = changes.border_width {
                aux = aux.border_width(b);
            }
            if let Some(sibling) = changes.sibling {
                aux = aux.sibling(self.ids.x11(sibling)?);
            }
            if let Some(mode) = changes.stack_mode {
                let x_mode = match mode {
                    StackMode::Above => x11rb::protocol::xproto::StackMode::ABOVE,
                    StackMode::Below => x11rb::protocol::xproto::StackMode::BELOW,
                    StackMode::TopIf => x11rb::protocol::xproto::StackMode::TOP_IF,
                    StackMode::BottomIf => x11rb::protocol::xproto::StackMode::BOTTOM_IF,
                    StackMode::Opposite => x11rb::protocol::xproto::StackMode::OPPOSITE,
                };
                aux = aux.stack_mode(x_mode);
            }

            let w = self.ids.x11(win)?;
            self.conn.configure_window(w, &aux)?;
            Ok(())
        }

        fn set_input_focus_root(&self) -> Result<(), BackendError> {
            self.conn
                .set_input_focus(InputFocus::POINTER_ROOT, self.root_x11, x11rb::CURRENT_TIME)?;
            Ok(())
        }

        fn unmap_window(&self, win: WindowId) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            self.conn.unmap_window(w)?;
            Ok(())
        }

        fn set_input_focus(&self, win: WindowId) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            self.conn
                .set_input_focus(InputFocus::PARENT, w, x11rb::CURRENT_TIME)?;
            Ok(())
        }

        fn send_take_focus(&self, win: WindowId) -> Result<bool, BackendError> {
            let w = self.ids.x11(win)?;
            let reply = self
                .conn
                .get_property(false, w, self.atoms.WM_PROTOCOLS, AtomEnum::ATOM, 0, 1024)?
                .reply()?;
            let supports_take_focus = reply
                .value32()
                .into_iter()
                .flatten()
                .any(|a| a == self.atoms.WM_TAKE_FOCUS);
            if supports_take_focus {
                let event = ClientMessageEvent::new(
                    32,
                    w,
                    self.atoms.WM_PROTOCOLS,
                    [self.atoms.WM_TAKE_FOCUS, x11rb::CURRENT_TIME as u32, 0, 0, 0],
                );
                self.conn.send_event(false, w, EventMask::NO_EVENT, event.serialize())?;
                self.conn.flush()?;
                return Ok(true);
            }
            Ok(false)
        }

        fn get_geometry(&self, win: WindowId) -> Result<Geometry, BackendError> {
            let w = self.ids.x11(win)?;
            let reply = self.conn.get_geometry(w)?.reply()?;
            Ok(Geometry {
                x: reply.x as i32,
                y: reply.y as i32,
                w: reply.width as u32,
                h: reply.height as u32,
                border: reply.border_width as u32,
            })
        }

        fn flush(&self) -> Result<(), BackendError> {
            self.batcher.flush(&*self.conn)?;
            Ok(())
        }

        fn kill_client(&self, win: WindowId) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            self.conn.kill_client(w)?;
            Ok(())
        }

        fn get_window_attributes(&self, win: WindowId) -> Result<WindowAttributes, BackendError> {
            let w = self.ids.x11(win)?;
            let r = self.conn.get_window_attributes(w)?.reply()?;
            Ok(WindowAttributes {
                override_redirect: r.override_redirect,
                map_state_viewable: r.map_state == MapState::VIEWABLE,
            })
        }

        fn get_tree_child(&self, win: WindowId) -> Result<Vec<WindowId>, BackendError> {
            let w = self.ids.x11(win)?;
            let tree_reply = self.conn.query_tree(w)?.reply()?;
            Ok(tree_reply
                .children
                .iter()
                .map(|&c| self.ids.intern(c))
                .collect())
        }

        fn ungrab_all_buttons(&self, win: WindowId) -> Result<(), BackendError> {
            let w = self.ids.x11(win)?;
            self.conn
                .ungrab_button(ButtonIndex::ANY, w, ModMask::ANY.into())?;
            Ok(())
        }
    }
}

