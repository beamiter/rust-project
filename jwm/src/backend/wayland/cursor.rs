// src/backend/wayland/cursor.rs
use crate::backend::api::{CursorHandle, CursorProvider};
use crate::backend::common_define::StdCursorKind;
use std::collections::HashMap;

pub struct WlCursorProvider {
    cache: HashMap<StdCursorKind, CursorHandle>,
}

impl WlCursorProvider {
    pub fn new() -> Self {
        Self { cache: HashMap::new() }
    }
}

impl CursorProvider for WlCursorProvider {
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Wayland seat 可设置主题光标，这里留空或记录kind
        Ok(())
    }

    fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>> {
        if let Some(h) = self.cache.get(&kind).copied() {
            return Ok(h);
        }
        // 使用 kind 作为 handle 的标识
        let h = CursorHandle(kind as u64);
        self.cache.insert(kind, h);
        Ok(h)
    }

    fn apply(&mut self, _window_id: u64, _kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>> {
        // Wayland: 通过 seat.set_cursor，在这里暂空
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.cache.clear();
        Ok(())
    }
}
