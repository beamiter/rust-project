// src/backend/wayland/cursor.rs
use crate::backend::api::{CursorHandle, CursorProvider};
use crate::backend::common_define::StdCursorKind;

#[derive(Clone)]
pub struct CursorController;
impl CursorController { pub fn new() -> Self { Self } }

pub struct WaylandCursorProvider {
    _ctrl: CursorController,
}
impl WaylandCursorProvider {
    pub fn new(ctrl: CursorController) -> Self { Self { _ctrl: ctrl } }
}
impl CursorProvider for WaylandCursorProvider {
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn get(&mut self, _kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>> {
        Ok(CursorHandle(0))
    }
    fn apply(&mut self, _window_id: u64, _kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
}
