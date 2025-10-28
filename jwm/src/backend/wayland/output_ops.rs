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
    pub fn new(reg: Arc<Mutex<WaylandRegistry>>, space: Arc<Mutex<Space<Window>>>) -> Self {
        Self { reg, space }
    }
}

impl OutputOps for WaylandOutputOps {
    fn screen_info(&self) -> ScreenInfo {
        // 优先从 Smithay 的 Space 中获取当前输出的几何信息
        // 这更准确，因为它反映了 compositor 内部的实际布局
        let space_guard = self.space.lock().unwrap();
        if let Some(output) = space_guard.outputs().next() {
            if let Some(geo) = space_guard.output_geometry(output) {
                return ScreenInfo {
                    width: geo.size.w,
                    height: geo.size.h,
                };
            }
        }

        // 如果 Space 中没有输出（例如初始化早期），则回退到 registry 中缓存的尺寸
        let reg_guard = self.reg.lock().unwrap();
        ScreenInfo {
            width: reg_guard.screen_w,
            height: reg_guard.screen_h,
        }
    }

    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        let space_guard = self.space.lock().unwrap();
        let mut outputs_vec = Vec::new();

        // 遍历所有由 Smithay管理的输出
        for (i, output) in space_guard.outputs().enumerate() {
            if let Some(geo) = space_guard.output_geometry(output) {
                // 将 Smithay 的输出信息转换为 jwm 的通用 OutputInfo 结构
                outputs_vec.push(OutputInfo {
                    id: i as i32, // 使用枚举索引作为临时 ID
                    x: geo.loc.x,
                    y: geo.loc.y,
                    width: geo.size.w,
                    height: geo.size.h,
                });
            }
        }

        if outputs_vec.is_empty() {
            let reg_guard = self.reg.lock().unwrap();
            outputs_vec.push(OutputInfo {
                id: 0,
                x: 0,
                y: 0,
                width: reg_guard.screen_w,
                height: reg_guard.screen_h,
            });
        }

        outputs_vec
    }
}
