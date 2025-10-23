// src/backend/wayland/compositor.rs
use crate::backend::api::{BackendEvent, Geometry};
use crate::backend::common_define::WindowId;
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::backend::wayland::input_ops::DragRuntime;

pub struct CompositorHandle; // 占位，后续接入 smithay 再扩展

pub struct CompositorState {
    pub windows: Arc<Mutex<HashMap<u64, Geometry>>>,
    pub drag_rt: Arc<Mutex<DragRuntime>>,
    pub event_tx: Sender<BackendEvent>,
    pub next_wid: Arc<Mutex<u64>>,
}

impl CompositorState {
    pub fn new(event_tx: Sender<BackendEvent>) -> Self {
        Self {
            windows: Arc::new(Mutex::new(HashMap::new())),
            drag_rt: Arc::new(Mutex::new(DragRuntime {
                in_drag: false,
                last_root_x: 0,
                last_root_y: 0,
                last_time: 0,
                mouse_down: false,
            })),
            event_tx,
            next_wid: Arc::new(Mutex::new(10)),
        }
    }

    pub fn spawn(self) -> Result<CompositorHandle, Box<dyn std::error::Error>> {
        // 生成两个“窗口”，通知 JWM 管理
        {
            let mut wid = self.next_wid.lock().unwrap();
            let id0 = *wid;
            *wid += 1;
            let id1 = *wid;
            *wid += 1;
            self.windows.lock().unwrap().insert(
                id0,
                Geometry {
                    x: 100,
                    y: 100,
                    w: 800,
                    h: 600,
                    border: 0,
                },
            );
            self.windows.lock().unwrap().insert(
                id1,
                Geometry {
                    x: 500,
                    y: 200,
                    w: 400,
                    h: 300,
                    border: 0,
                },
            );

            let _ = self.event_tx.send(BackendEvent::MapRequest {
                window: WindowId(id0),
            });
            let _ = self.event_tx.send(BackendEvent::MapRequest {
                window: WindowId(id1),
            });
        }

        // 可选：后台线程（未来放 smithay seat/pointer 事件）
        std::thread::spawn(move || {
            loop {
                // 这里后续用 smithay 的指针/键盘回调更新 drag_rt & 发送 BackendEvent
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        });

        Ok(CompositorHandle)
    }
}
