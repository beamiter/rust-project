// src/backend/wayland/input_ops.rs
use crate::backend::api::{AllowMode, InputOps, WindowId};

#[derive(Clone)]
pub struct PointerController;
impl PointerController {
    pub fn new() -> Self { Self }
    pub fn new_dummy() -> Self { Self }
}

pub struct WaylandInputOps {
    _ctrl: PointerController,
}
impl WaylandInputOps {
    pub fn new(ctrl: PointerController) -> Self { Self { _ctrl: ctrl } }
}
impl InputOps for WaylandInputOps {
    fn grab_pointer(&self, _mask: u32, _cursor: Option<u64>) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(true)
    }
    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn allow_events(&self, _mode: AllowMode, _time: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>> {
        Ok((0, 0, 0, 0))
    }
    fn warp_pointer_to_window(&self, _win: WindowId, _x: i16, _y: i16) -> Result<(), Box<dyn std::error::Error>> {
        Err("Wayland does not support pointer warping".into())
    }
    fn drag_loop(
        &self,
        _cursor: Option<u64>,
        _warp_to: Option<(i16, i16)>,
        _target: WindowId,
        _on_motion: &mut dyn FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(()) // 交互由 Wayland handler side 实现，本接口 no-op
    }
}
