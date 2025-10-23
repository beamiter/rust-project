// src/backend/wayland/input_ops.rs
use crate::backend::api::{AllowMode, InputOps};
use crate::backend::common_define::WindowId;
use std::sync::{Arc, Mutex};

pub struct WlInputOps {
    // drag loop 期间用到的通道由 event_source 提供
    pub(crate) drag_runtime: Arc<Mutex<DragRuntime>>,
}

pub struct DragRuntime {
    pub in_drag: bool,
    // 由 event_source 的指针事件填充的坐标
    pub last_root_x: i32,
    pub last_root_y: i32,
    pub last_time: u32,
    pub mouse_down: bool,
}

impl WlInputOps {
    pub fn new(drag_runtime: Arc<Mutex<DragRuntime>>) -> Self {
        Self { drag_runtime }
    }
}

impl InputOps for WlInputOps {
    fn grab_pointer(&self, _mask: u32, _cursor: Option<u64>) -> Result<bool, Box<dyn std::error::Error>> {
        // Wayland 无全局 grab。拖拽时用 in_drag 标记 + 事件过滤
        Ok(true)
    }

    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut rt = self.drag_runtime.lock().unwrap();
        rt.in_drag = false;
        Ok(())
    }

    fn allow_events(&self, _mode: AllowMode, _time: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>> {
        let rt = self.drag_runtime.lock().unwrap();
        Ok((rt.last_root_x, rt.last_root_y, 0, 0))
    }

    fn warp_pointer_to_window(&self, _win: WindowId, _x: i16, _y: i16) -> Result<(), Box<dyn std::error::Error>> {
        // Wayland 不允许 warp，忽略
        Ok(())
    }

    fn drag_loop(
        &self,
        _cursor: Option<u64>,
        _warp_to: Option<(i16, i16)>,
        _target: WindowId,
        on_motion: &mut dyn FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut rt = self.drag_runtime.lock().unwrap();
            rt.in_drag = true;
            rt.mouse_down = true;
        }
        // 简易循环：轮询drag_runtime直到 mouse_down=false
        loop {
            {
                let rt = self.drag_runtime.lock().unwrap();
                if !rt.in_drag || !rt.mouse_down {
                    break;
                }
                let _ = on_motion(rt.last_root_x as i16, rt.last_root_y as i16, rt.last_time)?;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(())
    }
}
