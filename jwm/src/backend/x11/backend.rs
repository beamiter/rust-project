// src/backend/x11/backend.rs
use std::sync::Arc;
use x11rb::protocol::xproto::Screen;
use x11rb::rust_connection::RustConnection;

use crate::backend::api::{
    Backend, Capabilities, EventSource, EwmhFacade, InputOps, KeyOps, OutputOps, PropertyOps,
    WindowId, WindowOps,
};
use crate::backend::traits::{ColorAllocator, CursorProvider};
use crate::backend::x11::key_ops::X11KeyOps;

use super::{
    color::X11ColorAllocator, cursor::X11CursorProvider, event_source::X11EventSource,
    ewmh_facade::X11EwmhFacade, input_ops::X11InputOps, output_ops::X11OutputOps,
    property_ops::X11PropertyOps, window_ops::X11WindowOps, Atoms,
};

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
    event_source: Box<dyn EventSource>,
}

impl X11Backend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // 连接 X11
        let (raw_conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)?;
        let conn = Arc::new(raw_conn);
        use x11rb::connection::Connection;
        let screen = conn.setup().roots[screen_num].clone();
        let root = WindowId(screen.root as u64);

        // Atoms
        let atoms = Atoms::new(conn.as_ref())?.reply()?;

        // 子服务
        let window_ops: Box<dyn WindowOps> = Box::new(X11WindowOps::new(conn.clone()));
        let input_ops: Box<dyn InputOps> = Box::new(X11InputOps::new(conn.clone(), screen.root));
        let property_ops: Box<dyn PropertyOps> =
            Box::new(X11PropertyOps::new(conn.clone(), atoms.clone()));
        let output_ops: Box<dyn OutputOps> = Box::new(X11OutputOps::new(
            conn.clone(),
            screen.root,
            screen.width_in_pixels as i32,
            screen.height_in_pixels as i32,
        ));
        let key_ops: Box<dyn KeyOps> = Box::new(X11KeyOps::new(conn.clone()));
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
        let event_source: Box<dyn EventSource> = Box::new(X11EventSource::new(conn.clone()));

        let caps = Capabilities {
            can_warp_pointer: true,
            has_active_window_prop: true,
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
            event_source,
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

    fn event_source(&mut self) -> &mut dyn EventSource {
        &mut *self.event_source
    }

    fn root_window(&self) -> WindowId {
        self.root
    }
}
