// src/backend/wayland/backend.rs
use std::sync::{Arc, Mutex};

use crossbeam_channel::{unbounded, Sender};

use crate::backend::api::{
    AllowMode, Backend, Capabilities, ColorAllocator, CursorProvider, EventSource, EwmhFacade,
    InputOps, KeyOps, OutputOps, PropertyOps, WindowId, WindowOps,
};
use super::{
    color::WlColorAllocator,
    cursor::WlCursorProvider,
    event_source::WlEventSource,
    input_ops::WlInputOps,
    key_ops::WlKeyOps,
    output_ops::WlOutputOps,
    property_ops::WlPropertyOps,
    window_ops::WlWindowOps,
    compositor::CompositorState,
};

// 代理：把 Arc<Mutex<WlInputOps>> 暴露为 dyn InputOps
struct WlInputOpsProxy {
    inner: Arc<Mutex<WlInputOps>>,
}

impl InputOps for WlInputOpsProxy {
    fn grab_pointer(
        &self,
        mask: u32,
        cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.inner.lock().unwrap().grab_pointer(mask, cursor)
    }
    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.lock().unwrap().ungrab_pointer()
    }
    fn allow_events(&self, mode: AllowMode, time: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.lock().unwrap().allow_events(mode, time)
    }
    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>> {
        self.inner.lock().unwrap().query_pointer_root()
    }
    fn warp_pointer_to_window(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner.lock().unwrap().warp_pointer_to_window(win, x, y)
    }
    fn drag_loop(
        &self,
        cursor: Option<u64>,
        warp_to: Option<(i16, i16)>,
        target: WindowId,
        on_motion: &mut dyn FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.inner
            .lock()
            .unwrap()
            .drag_loop(cursor, warp_to, target, on_motion)
    }
}

pub struct WaylandBackend {
    caps: Capabilities,
    root: WindowId,

    window_ops: Box<dyn WindowOps>,
    // 对外只读借用
    input_ops_ref: Box<dyn InputOps>,
    // 供 drag_loop 使用的可变句柄（Arc<Mutex<dyn InputOps>>>）
    input_ops_arc: Arc<Mutex<dyn InputOps + Send>>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,
    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,
    event_source: Box<dyn EventSource>,

    #[allow(dead_code)]
    tx: Sender<crate::backend::api::BackendEvent>,
}

impl WaylandBackend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = unbounded();

        let state = CompositorState::new(tx.clone());
        let windows = state.windows.clone();
        let drag_rt = state.drag_rt.clone();
        let _handle = state.spawn()?; // 现阶段：占位的后台线程，会发两个 MapRequest

        let (w, h) = (1280, 800);

        let caps = Capabilities {
            can_warp_pointer: false,
            has_active_window_prop: false,
            supports_client_list: false,
        };

        let window_ops: Box<dyn WindowOps> = Box::new(WlWindowOps::new(windows.clone()));

        // 构造真正实现（底层共享）
        let base_input = Arc::new(Mutex::new(WlInputOps::new(drag_rt.clone())));
        // 供 &dyn InputOps 用的代理
        let input_ops_ref: Box<dyn InputOps> =
            Box::new(WlInputOpsProxy { inner: base_input.clone() });
        // 供 handle() 用的 Arc<Mutex<dyn InputOps + Send>>
        let input_ops_arc: Arc<Mutex<dyn InputOps + Send>> =
            Arc::new(Mutex::new(WlInputOpsProxy { inner: base_input.clone() }));

        let property_ops: Box<dyn PropertyOps> = Box::new(WlPropertyOps::new());
        let output_ops: Box<dyn OutputOps> = Box::new(WlOutputOps::new(w, h));
        let key_ops: Box<dyn KeyOps> = Box::new(WlKeyOps::new());
        let cursor_provider: Box<dyn CursorProvider> = Box::new(WlCursorProvider::new());
        let color_allocator: Box<dyn ColorAllocator> = Box::new(WlColorAllocator::new());
        let event_source: Box<dyn EventSource> = Box::new(WlEventSource::new(rx));

        Ok(Self {
            caps,
            root: WindowId(1),
            window_ops,
            input_ops_ref,
            input_ops_arc,
            property_ops,
            output_ops,
            key_ops,
            cursor_provider,
            color_allocator,
            event_source,
            tx,
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
        &*self.input_ops_ref
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
        self.root
    }

    fn init_visual(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
