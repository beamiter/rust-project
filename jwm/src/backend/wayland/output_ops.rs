// src/backend/wayland/output_ops.rs
use std::sync::{Arc, Mutex};
use crate::backend::api::{OutputInfo, OutputOps, ScreenInfo};
use super::window_ops::WaylandRegistry;

pub struct WaylandOutputOps {
    reg: Arc<Mutex<WaylandRegistry>>,
}
impl WaylandOutputOps {
    pub fn new(reg: Arc<Mutex<WaylandRegistry>>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { reg })
    }
}
impl OutputOps for WaylandOutputOps {
    fn screen_info(&self) -> ScreenInfo {
        let reg = self.reg.lock().unwrap();
        ScreenInfo { width: reg.screen_w, height: reg.screen_h }
    }
    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        let reg = self.reg.lock().unwrap();
        vec![OutputInfo {
            id: 0, x: 0, y: 0, width: reg.screen_w, height: reg.screen_h,
        }]
    }
}
