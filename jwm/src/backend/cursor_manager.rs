use crate::backend::traits::{CursorProvider, StdCursorKind};

pub struct CursorManager {
    provider: Box<dyn CursorProvider>,
}

impl CursorManager {
    pub fn new(mut provider: Box<dyn CursorProvider>) -> Result<Self, Box<dyn std::error::Error>> {
        provider.preload_common()?;
        Ok(Self { provider })
    }

    pub fn apply_cursor(
        &mut self,
        window: u64,
        kind: StdCursorKind,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.provider.apply(window, kind)
    }

    pub fn get_cursor(&mut self, kind: StdCursorKind) -> Result<u32, Box<dyn std::error::Error>> {
        let h = self.provider.get(kind)?;
        Ok(h.0 as u32)
    }

    pub fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.provider.cleanup()
    }
}
