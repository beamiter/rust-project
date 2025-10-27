// src/backend/wayland/backend.rs
use std::sync::{Arc, Mutex};

use crate::backend::api::{
    Backend, Capabilities, ColorAllocator, CursorProvider, EwmhFacade, EventSource, InputOps, KeyOps,
    OutputOps, PropertyOps, WindowId, WindowOps,
};
use super::{
    color::WaylandColorAllocator, cursor::WaylandCursorProvider, event_source::WaylandEventSource,
    input_ops::WaylandInputOps, key_ops::WaylandKeyOps, output_ops::WaylandOutputOps,
    window_ops::WaylandWindowOps,
};

pub struct WaylandBackend {
    caps: Capabilities,

    window_ops: Box<dyn WindowOps>,
    input_ops: Box<dyn InputOps>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,

    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,
    event_source: Box<dyn EventSource>,
}

impl WaylandBackend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // 事件源：smithay + winit 驱动的最小 compositor，将事件桥接为 JWM 的 BackendEvent
        let event_source = WaylandEventSource::new()?;

        // 子服务：共享 event_source 的内部注册表（窗口id->记录、屏幕尺寸等）
        let window_ops: Box<dyn WindowOps> =
            Box::new(WaylandWindowOps::new(event_source.registry())?);
        let input_ops: Box<dyn InputOps> =
            Box::new(WaylandInputOps::new(event_source.pointer_controller()));
        let output_ops: Box<dyn OutputOps> =
            Box::new(WaylandOutputOps::new(event_source.registry())?);
        // Wayland 下 PropertyOps/EWMH 无意义，提供最小 no-op/近似语义
        let property_ops: Box<dyn PropertyOps> = super::window_ops::no_op_property_ops();
        let key_ops: Box<dyn KeyOps> = Box::new(WaylandKeyOps::new(event_source.keyboard_controller()));

        let cursor_provider: Box<dyn CursorProvider> =
            Box::new(WaylandCursorProvider::new(event_source.cursor_controller()));
        let color_allocator: Box<dyn ColorAllocator> = Box::new(WaylandColorAllocator::new());

        let caps = Capabilities {
            can_warp_pointer: false,            // Wayland不支持warp
            has_active_window_prop: false,      // 无 _NET_ACTIVE_WINDOW
            supports_client_list: false,        // 无 _NET_CLIENT_LIST
            ..Default::default()
        };

        Ok(Self {
            caps,
            window_ops,
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
        // Wayland 下 InputOps 不需要跨线程 drag_loop，返回同一个对象的 Arc<Mutex>
        Arc::new(Mutex::new(WaylandInputOps::new(
            super::input_ops::PointerController::new_dummy(),
        )))
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
        // Wayland 下没有 root window 概念，这里返回内部的“虚拟 root”，固定为 0
        WindowId(0)
    }
}
