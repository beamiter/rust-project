use crate::backend::api::{AllowMode, InputOps, StdCursorKind, WindowId};

pub struct WaylandInputOps {}

impl WaylandInputOps {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl InputOps for WaylandInputOps {
    fn set_cursor(&self, kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn grab_pointer(
        &self,
        mask: u32,
        cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(true)
    }

    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn allow_events(&self, mode: AllowMode, time: u32) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>> {
        Ok((0, 0, 0, 0))
    }

    fn warp_pointer_to_window(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn drag_loop(
        &self,
        cursor: Option<u64>,
        warp_to: Option<(i16, i16)>,
        target: WindowId,
        on_motion: &mut dyn FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
