// src/backend/x11/backend.rs
use crate::backend::api::EventHandler;
use crate::backend::api::EwmhFeature;
use crate::backend::api::Geometry;
use crate::backend::api::ResizeEdge;
use crate::backend::common_define::EventMaskBits;
use crate::backend::common_define::StdCursorKind;
use crate::backend::common_define::WindowId;
use crate::jwm::InteractionAction;
use calloop::signals::{Signal, Signals};
use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Duration;
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

use super::{
    Atoms, color::X11ColorAllocator, cursor::X11CursorProvider, event_source::X11EventSource,
    ewmh_facade::X11EwmhFacade, input_ops::X11InputOps, key_ops::X11KeyOps,
    output_ops::X11OutputOps, property_ops::X11PropertyOps, window_ops::X11WindowOps,
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
}

struct X11Interaction {
    win: WindowId,
    start_geom: Geometry,
    start_root_x: f64,
    start_root_y: f64,
    action: InteractionAction,
}

impl X11Backend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (raw_conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)?;
        let conn = Arc::new(raw_conn);
        use x11rb::connection::Connection;
        let screen = conn.setup().roots[screen_num].clone();
        let root = WindowId::X11(screen.root as u64);

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
        ));

        let x11_input_ops = X11InputOps::new(conn.clone(), screen.root);
        let input_ops: Box<dyn InputOps> = Box::new(x11_input_ops.clone());
        let property_ops: Box<dyn PropertyOps> =
            Box::new(X11PropertyOps::new(conn.clone(), atoms.clone()));
        let output_ops: Box<dyn OutputOps> = Box::new(X11OutputOps::new(
            conn.clone(),
            screen.root,
            screen.width_in_pixels as i32,
            screen.height_in_pixels as i32,
        ));
        let key_ops: Box<dyn KeyOps> = Box::new(X11KeyOps::new(conn.clone(), numlock_mask.clone()));
        let ewmh_facade: Option<Box<dyn EwmhFacade>> = Some(Box::new(X11EwmhFacade::new(
            conn.clone(),
            root,
            atoms.clone(),
        )));
        let cursor_provider: Box<dyn CursorProvider> =
            Box::new(X11CursorProvider::new(conn.clone())?);
        let color_allocator: Box<dyn ColorAllocator> = Box::new(X11ColorAllocator::new(
            conn.clone(),
            screen.default_colormap,
        ));

        let event_source = X11EventSource::new(conn.clone(), atoms.clone());

        let caps = Capabilities {
            can_warp_pointer: true,
            supports_client_list: true,
            ..Default::default()
        };

        Ok(Self {
            conn,
            screen,
            root,
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
        })
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

    fn check_existing_wm(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mask_bits = EventMaskBits::SUBSTRUCTURE_REDIRECT.bits();
        self.window_ops
            .change_event_mask(self.root, mask_bits)
            .map_err(|e| format!("Another window manager is already running: {:?}", e).into())
    }

    fn request_render(&mut self) {
        let _ = self.conn.flush();
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
    fn register_wm(&self, wm_name: &str) -> Result<(), Box<dyn std::error::Error>> {
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
            ];
            facade.declare_supported(&supported)?;
        }
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(facade) = self.ewmh_facade.as_ref() {
            let _ = facade.reset_root_properties();
        }
        Ok(())
    }

    fn on_focused_client_changed(
        &mut self,
        win: Option<WindowId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(w) = win {
            // 1. 设置 X11 输入焦点
            self.window_ops.set_input_focus(w)?;

            // 2. 更新 EWMH 属性
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
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(facade) = self.ewmh_facade.as_ref() {
            facade.set_client_list(clients)?;
            facade.set_client_list_stacking(stack)?;
        }
        Ok(())
    }
    // [实现] 开始移动
    fn begin_move(&mut self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let geom = self.window_ops.get_geometry(win)?;
        let (rx, ry) = self.input_ops.get_pointer_position()?;

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
        }
        Ok(())
    }

    // [实现] 开始调整大小
    fn begin_resize(
        &mut self,
        win: WindowId,
        edge: ResizeEdge,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let geom = self.window_ops.get_geometry(win)?;
        let (_rx, _ry) = self.input_ops.get_pointer_position()?;

        if self.caps.can_warp_pointer {
            let _ = self.input_ops.warp_pointer_to_window(
                win,
                (geom.w + geom.border) as i16,
                (geom.h + geom.border) as i16,
            );
        }

        // 重新获取 Warp 后的位置
        let (rx_new, ry_new) = self.input_ops.get_pointer_position()?;

        self.input_ops.set_cursor(StdCursorKind::Fleur)?; // 或者 Sizing
        let cursor_handle = self.cursor_provider.get(StdCursorKind::Fleur)?.0;
        let mask = (EventMaskBits::BUTTON_RELEASE | EventMaskBits::POINTER_MOTION).bits();

        if self.input_ops.grab_pointer(mask, Some(cursor_handle))? {
            self.interaction = Some(X11Interaction {
                win,
                start_geom: geom,
                start_root_x: rx_new,
                start_root_y: ry_new,
                action: InteractionAction::Resize(edge),
            });
        }
        Ok(())
    }

    // [实现] 处理 Motion
    fn handle_motion(
        &mut self,
        x: f64,
        y: f64,
        _time: u32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if let Some(ref state) = self.interaction {
            let dx = (x - state.start_root_x) as i32;
            let dy = (y - state.start_root_y) as i32;

            match state.action {
                InteractionAction::Move => {
                    let new_x = state.start_geom.x + dx;
                    let new_y = state.start_geom.y + dy;
                    self.window_ops.set_position(state.win, new_x, new_y)?;
                }
                InteractionAction::Resize(_) => {
                    let new_w = (state.start_geom.w as i32 + dx).max(1) as u32;
                    let new_h = (state.start_geom.h as i32 + dy).max(1) as u32;

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
    fn handle_button_release(&mut self, _time: u32) -> Result<bool, Box<dyn std::error::Error>> {
        if self.interaction.is_some() {
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

    fn run(&mut self, handler: &mut dyn EventHandler) -> Result<(), Box<dyn std::error::Error>> {
        let mut event_loop: EventLoop<X11LoopData> =
            EventLoop::try_new().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        let handle = event_loop.handle();
        // =========================================================
        // 1. 注册 X11 事件源 (这是主要的事件来源)
        // =========================================================
        let x11_source = if let Some(src) = self._init_event_source.take() {
            src
        } else {
            X11EventSource::new(self.conn.clone(), self.atoms.clone())
        };
        handle
            .insert_source(x11_source, |event, _, data| {
                if let Err(e) = data.handler.handle_event(data.backend, event) {
                    log::error!("Error handling X11 event: {:?}", e);
                }
            })
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        // =========================================================
        // 2. [新增] 注册 Signals 事件源 (专门处理僵尸进程)
        // =========================================================
        let signals = Signals::new(&[Signal::SIGCHLD])
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
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
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        // =========================================================
        // 3. 注册 Update 定时器
        // =========================================================
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
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        // =========================================================
        // 4. 运行事件循环
        // =========================================================
        let mut loop_data = X11LoopData {
            backend: self,
            handler,
            should_exit: false,
        };
        loop {
            event_loop
                .dispatch(None, &mut loop_data)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
            if loop_data.should_exit {
                break;
            }
        }

        Ok(())
    }
}
