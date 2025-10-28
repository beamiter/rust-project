// src/backend/wayland/backend.rs
use std::sync::{Arc, Mutex};

use super::{
    color::WaylandColorAllocator, cursor::WaylandCursorProvider, event_source::WaylandEventSource,
    input_ops::WaylandInputOps, key_ops::WaylandKeyOps, output_ops::WaylandOutputOps,
    window_ops::WaylandWindowOps,
};
use crate::backend::api::{
    Backend, Capabilities, ColorAllocator, CursorProvider, EventSource, EwmhFacade, InputOps,
    KeyOps, OutputOps, PropertyOps, WindowId, WindowOps,
};

pub struct WaylandBackend {
    caps: Capabilities,

    window_ops: Box<dyn WindowOps>,
    input_ops: Box<dyn InputOps>,
    // 以 trait 对象形式存储，便于 input_ops_handle 返回
    input_ops_arc: Arc<Mutex<dyn InputOps + Send>>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,

    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,
    event_source: Box<dyn EventSource>,
}

impl WaylandBackend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let event_source = WaylandEventSource::new()?;

        // 显式传递 Registry、Space 与 Seat 给 WindowOps
        let window_ops: Box<dyn WindowOps> = Box::new(WaylandWindowOps::new(
            event_source.registry(),
            event_source.space(),
            event_source.seat(),
        )?);

        // 输入子系统：分别构造 Box 与 Arc<Mutex<dyn InputOps + Send>>
        let wl_input_for_arc = WaylandInputOps::new(event_source.pointer_controller());
        let input_ops_arc: Arc<Mutex<dyn InputOps + Send>> = Arc::new(Mutex::new(wl_input_for_arc));

        let input_ops: Box<dyn InputOps> =
            Box::new(WaylandInputOps::new(event_source.pointer_controller()));

        let output_ops: Box<dyn OutputOps> =
            Box::new(WaylandOutputOps::new(event_source.registry())?);

        let property_ops: Box<dyn PropertyOps> = Box::new(
            super::property_ops::WaylandPropertyOps::new(event_source.registry()),
        );

        let key_ops: Box<dyn KeyOps> =
            Box::new(WaylandKeyOps::new(event_source.keyboard_controller()));

        let cursor_provider: Box<dyn CursorProvider> =
            Box::new(WaylandCursorProvider::new(event_source.cursor_controller()));

        let color_allocator: Box<dyn ColorAllocator> = Box::new(WaylandColorAllocator::new());

        let caps = Capabilities {
            can_warp_pointer: false,       // Wayland不支持warp
            has_active_window_prop: false, // 无 _NET_ACTIVE_WINDOW
            supports_client_list: false,   // 无 _NET_CLIENT_LIST
            ..Default::default()
        };

        Ok(Self {
            caps,
            window_ops,
            input_ops_arc,
            input_ops,
            property_ops,
            output_ops,
            key_ops,
            cursor_provider,
            color_allocator,
            event_source: Box::new(event_source),
        })
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

    fn input_ops_handle(&self) -> Arc<Mutex<dyn InputOps + Send>> {
        self.input_ops_arc.clone()
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
        None
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
        // Wayland 下没有 root window 概念，返回内部“虚拟 root”
        WindowId(0)
    }
}
