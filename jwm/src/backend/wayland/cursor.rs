use crate::backend::api::{CursorHandle, CursorProvider, StdCursorKind};

pub struct WaylandCursorProvider {}

impl WaylandCursorProvider {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl CursorProvider for WaylandCursorProvider {
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>> {
        Ok(CursorHandle((0)))
    }

    fn apply(
        &mut self,
        window_id: u64,
        kind: StdCursorKind,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
