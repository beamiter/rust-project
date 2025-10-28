// src/backend/wayland/output_ops.rs
use super::window_ops::WaylandRegistry;
use crate::backend::api::{OutputInfo, OutputOps, ScreenInfo};
use smithay::desktop::{Space, Window};
use std::sync::{Arc, Mutex};

pub struct WaylandOutputOps {
    reg: Arc<Mutex<WaylandRegistry>>,
    space: Arc<Mutex<Space<Window>>>,
}
impl WaylandOutputOps {
    pub fn new(
        reg: Arc<Mutex<WaylandRegistry>>,
        space: Arc<Mutex<Space<Window>>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { reg, space })
    }
}
impl OutputOps for WaylandOutputOps {
    fn screen_info(&self) -> ScreenInfo {
        let sp = self.space.lock().unwrap();
        if let Some(out) = sp.outputs().next() {
            if let Some(geo) = sp.output_geometry(out) {
                return ScreenInfo {
                    width: geo.size.w,
                    height: geo.size.h,
                };
            }
        }
        let reg = self.reg.lock().unwrap();
        ScreenInfo {
            width: reg.screen_w,
            height: reg.screen_h,
        }
    }
    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        let sp = self.space.lock().unwrap();
        let mut vec = Vec::new();
        for (i, out) in sp.outputs().enumerate() {
            if let Some(geo) = sp.output_geometry(out) {
                vec.push(OutputInfo {
                    id: i as i32,
                    x: geo.loc.x,
                    y: geo.loc.y,
                    width: geo.size.w,
                    height: geo.size.h,
                });
            }
        }
        if vec.is_empty() {
            let reg = self.reg.lock().unwrap();
            vec.push(OutputInfo {
                id: 0,
                x: 0,
                y: 0,
                width: reg.screen_w,
                height: reg.screen_h,
            });
        }
        vec
    }
}
