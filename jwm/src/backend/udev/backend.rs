use super::dummy_ops::*;
use crate::backend::api::{
    Backend, Capabilities, ColorAllocator, CursorProvider, EventHandler, InputOps, KeyOps,
    OutputOps, PropertyOps, WindowOps,
};
use crate::backend::common_define::WindowId;
use crate::backend::error::BackendError;

use std::any::Any;

// Smithay imports
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::{Display, DisplayHandle};

// --- 添加 SendWrapper ---
struct SendWrapper<T>(T);

// 强制实现 Send。这是 Unsafe 的！
// 前提：我们保证 UdevBackend 只在主线程使用，或者所有权转移不会导致并发访问。
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
// ------------------------

// 状态结构体
pub struct JwmUdevState {
    pub display_handle: DisplayHandle,
    pub start_time: std::time::Instant,
}

pub struct UdevBackend {
    display: Display<JwmUdevState>,
    // 使用 SendWrapper 包裹 EventLoop
    event_loop: SendWrapper<EventLoop<'static, JwmUdevState>>,
    state: JwmUdevState,

    // Ops storage
    window_ops: Box<dyn WindowOps>,
    input_ops: Box<dyn InputOps>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,
    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,
}

impl UdevBackend {
    pub fn new() -> Result<Self, BackendError> {
        let event_loop = EventLoop::try_new().map_err(|e| BackendError::Other(Box::new(e)))?;
        let display = Display::new().map_err(|e| BackendError::Other(Box::new(e)))?;
        let display_handle = display.handle();

        let state = JwmUdevState {
            display_handle,
            start_time: std::time::Instant::now(),
        };

        Ok(Self {
            display,
            // 包装 EventLoop
            event_loop: SendWrapper(event_loop),
            state,
            window_ops: Box::new(DummyWindowOps),
            input_ops: Box::new(DummyInputOps),
            property_ops: Box::new(DummyPropertyOps),
            output_ops: Box::new(DummyOutputOps),
            key_ops: Box::new(DummyKeyOps),
            cursor_provider: Box::new(DummyCursorProvider),
            color_allocator: Box::new(DummyColorAllocator),
        })
    }
}

// ... 下面的 impl Backend for UdevBackend 保持不变 ...
// ... run 方法中的 self.event_loop.dispatch 也能正常工作，因为实现了 DerefMut
impl Backend for UdevBackend {
    // ... 前面的方法保持不变 ...
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
        println!("Running JWM Udev Backend (Skeleton)...");

        loop {
            // 通过 DerefMut 访问 event_loop
            match self
                .event_loop
                .dispatch(Some(std::time::Duration::from_millis(16)), &mut self.state)
            {
                Ok(_) => {
                    handler.update(self)?;
                    self.display
                        .flush_clients()
                        .map_err(|e| BackendError::Other(Box::new(e)))?;
                    if handler.should_exit() {
                        break;
                    }
                }
                Err(e) => {
                    return Err(BackendError::Other(Box::new(e)));
                }
            }
        }
        Ok(())
    }
}
