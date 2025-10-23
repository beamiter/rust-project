// src/backend/wayland/output_ops.rs
use crate::backend::api::{OutputInfo, OutputOps, ScreenInfo};

pub struct WlOutputOps {
    pub width: i32,
    pub height: i32,
}

impl WlOutputOps {
    pub fn new(width: i32, height: i32) -> Self { Self { width, height } }
}

impl OutputOps for WlOutputOps {
    fn screen_info(&self) -> ScreenInfo {
        ScreenInfo { width: self.width, height: self.height }
    }

    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        vec![OutputInfo { id: 0, x: 0, y: 0, width: self.width, height: self.height }]
    }
}
