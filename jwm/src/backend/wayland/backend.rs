use std::any::Any;
use std::sync::{Arc, Mutex};
use x11rb::protocol::xproto::Screen;
use x11rb::rust_connection::RustConnection;

use crate::backend::api::{
    Backend, Capabilities, ColorAllocator, CursorProvider, EventSource, EwmhFacade, InputOps,
    KeyOps, OutputOps, PropertyOps, WindowId, WindowOps,
};

use super::{
    color::WaylandColorAllocator, cursor::WaylandCursorProvider, event_source::WaylandEventSource,
    ewmh_facade::WaylandEwmhFacade, input_ops::WaylandInputOps, key_ops::WaylandKeyOps,
    output_ops::WaylandOutputOps, property_ops::WaylandPropertyOps, window_ops::WaylandWindowOps,
};

pub struct WaylandBackend {
    conn: Arc<RustConnection>,
    screen: Screen,
    root: WindowId,

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

impl WaylandBackend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {})
    }
}

impl Backend for WaylandBackend {
    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn window_ops(&self) -> &dyn WindowOps {
        &*self.window_ops
    }
    fn input_ops(&self) -> &dyn InputOps {
        &*self.input_ops
    }
    fn input_ops_handle(&self) -> std::sync::Arc<std::sync::Mutex<dyn InputOps + Send>> {
        Arc::new(Mutex::new(WaylandInputOps::new()))
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

    fn as_any(&self) -> &dyn Any {
        self
    }
}
