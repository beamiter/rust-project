// src/backend/x11/backend.rs
use crate::backend::api::EventHandler;
use crate::backend::common_define::WindowId;
use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Duration;
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
    fn root_window(&self) -> WindowId {
        self.root
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
    fn ewmh_facade(&self) -> Option<&dyn EwmhFacade> {
        self.ewmh_facade.as_deref()
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
        // 1. 注册 X11 事件源
        let x11_source = if let Some(src) = self._init_event_source.take() {
            src
        } else {
            X11EventSource::new(self.conn.clone(), self.atoms.clone())
        };
        handle
            .insert_source(x11_source, |event, _, data| {
                if let Err(e) = data.handler.handle_event(data.backend, event) {
                    log::error!("Error handling event: {:?}", e);
                }
            })
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        // 2. 注册 Update 定时器
        let update_interval = Duration::from_millis(10);
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

        // 3. 构造 LoopData
        let mut loop_data = X11LoopData {
            backend: self,
            handler,
            should_exit: false,
        };

        // 4. 运行事件循环
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
