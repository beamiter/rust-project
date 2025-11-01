// src/backend/wayland/backend.rs
use std::any::Any;
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

use super::event_source::CompositorCommand;
use smithay::reexports::calloop::channel::Sender as CommandSender;

pub struct WaylandBackend {
    caps: Capabilities,

    window_ops: Box<dyn WindowOps>,
    input_ops: Box<dyn InputOps>,
    input_ops_arc: Arc<Mutex<dyn InputOps + Send>>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,

    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,
    event_source: Box<dyn EventSource>,
    command_tx: CommandSender<CompositorCommand>,
}

impl WaylandBackend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let event_source = WaylandEventSource::new()?;
        let command_tx = event_source.command_sender();
        // Get the handles from the event source instance
        let registry = event_source.registry();
        let space = event_source.space();

        let window_ops: Box<dyn WindowOps> = Box::new(WaylandWindowOps::new(
            registry.clone(),
            space.clone(),
            command_tx.clone(),
        )?);

        let input_ops_arc: Arc<Mutex<dyn InputOps + Send>> =
            Arc::new(Mutex::new(WaylandInputOps::new(command_tx.clone())));
        let input_ops: Box<dyn InputOps> = Box::new(WaylandInputOps::new(command_tx.clone()));

        let output_ops: Box<dyn OutputOps> =
            Box::new(WaylandOutputOps::new(registry.clone(), space.clone()));

        let property_ops: Box<dyn PropertyOps> = Box::new(
            super::property_ops::WaylandPropertyOps::new(registry.clone()),
        );

        let key_ops: Box<dyn KeyOps> =
            Box::new(WaylandKeyOps::new(super::key_ops::KeyboardController::new()));

        let cursor_provider: Box<dyn CursorProvider> = Box::new(WaylandCursorProvider::new());

        let color_allocator: Box<dyn ColorAllocator> = Box::new(WaylandColorAllocator::new());

        let caps = Capabilities {
            can_warp_pointer: false,
            has_active_window_prop: false,
            supports_client_list: false,
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
            command_tx,
        })
    }

    pub fn command_sender(&self) -> CommandSender<CompositorCommand> {
        self.command_tx.clone()
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
        WindowId(0)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
