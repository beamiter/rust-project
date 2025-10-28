// src/backend/wayland/window_ops.rs
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::event_source::CompositorCommand;
use crate::backend::api::{Geometry, WindowAttributes, WindowId, WindowOps};
use smithay::desktop::{Space, Window as SWindow};
use smithay::reexports::calloop::channel::Sender as CommandSender;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State as XdgToplevelState;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::shell::xdg::ToplevelSurface;

#[derive(Clone)]
pub struct WindowRecord {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub border: i32,
    pub handle: Option<SWindow>,
    pub wl_surface: Option<WlSurface>,
    pub toplevel: Option<ToplevelSurface>,
}

#[derive(Clone)]
pub struct WaylandRegistry {
    pub windows: HashMap<u64, WindowRecord>,
    pub screen_w: i32,
    pub screen_h: i32,
}
impl WaylandRegistry {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            screen_w: 1280,
            screen_h: 800,
        }
    }
}

pub struct WaylandWindowOps {
    reg: Arc<Mutex<WaylandRegistry>>,
    space: Arc<Mutex<Space<SWindow>>>,
    command_tx: CommandSender<CompositorCommand>,
}

impl WaylandWindowOps {
    pub fn new(
        reg: Arc<Mutex<WaylandRegistry>>,
        space: Arc<Mutex<Space<SWindow>>>,
        command_tx: CommandSender<CompositorCommand>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            reg,
            space,
            command_tx,
        })
    }
}

impl WindowOps for WaylandWindowOps {
    fn set_input_focus_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.command_tx
            .send(CompositorCommand::SetFocus(Some(win)))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    fn set_input_focus_root(&self, _root: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.command_tx
            .send(CompositorCommand::SetFocus(None))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    fn configure_xywh_border(
        &self,
        win: WindowId,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _border: Option<u32>, // Wayland后端可以先忽略border
    ) -> Result<(), Box<dyn std::error::Error>> {
        let reg = self.reg.lock().unwrap();
        if let Some(record) = reg.windows.get(&win.0) {
            // --- 位置变更 ---
            if let (Some(x_val), Some(y_val)) = (x, y) {
                if let Some(handle) = &record.handle {
                    // JWM 核心计算的位置是绝对位置，直接映射
                    let mut space = self.space.lock().unwrap();
                    space.map_element(handle.clone(), (x_val, y_val), true);
                }
            }

            // --- 尺寸变更 ---
            if let (Some(w_val), Some(h_val)) = (w, h) {
                if let Some(toplevel) = &record.toplevel {
                    // 检查窗口是否处于最大化或全屏状态，如果是，则先取消
                    let current_state = toplevel.current_state();
                    let mut should_unmaximize = false;
                    if current_state.states.contains(XdgToplevelState::Maximized) {
                        should_unmaximize = true;
                    }
                    // Wayland 中，不能直接为全屏窗口设置大小，需要先退出全屏
                    if current_state.states.contains(XdgToplevelState::Fullscreen) {
                        // 忽略尺寸变更请求或先退出全屏，这里先选择忽略
                        return Ok(());
                    }

                    toplevel.with_pending_state(|state| {
                        if should_unmaximize {
                            state.states.unset(XdgToplevelState::Maximized);
                        }
                        state.size = Some((w_val as i32, h_val as i32).into());
                    });
                    toplevel.send_configure();
                }
            }
        }
        Ok(())
    }

    fn configure_stack_above(
        &self,
        win: WindowId,
        _sibling: Option<WindowId>, // 简单的 raise to top 实现可以先忽略 sibling
    ) -> Result<(), Box<dyn std::error::Error>> {
        let reg = self.reg.lock().unwrap();
        if let Some(record) = reg.windows.get(&win.0) {
            if let Some(handle) = &record.handle {
                let mut space = self.space.lock().unwrap();
                // true 参数表示同时提升其父窗口（如果有）
                space.raise_element(handle, true);
            }
        }
        Ok(())
    }

    fn get_tree_child(&self, _root: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }
    fn get_window_attributes(
        &self,
        _win: WindowId,
    ) -> Result<WindowAttributes, Box<dyn std::error::Error>> {
        Ok(WindowAttributes {
            override_redirect: false,
            map_state_viewable: true,
        })
    }
    fn get_geometry_translated(
        &self,
        _win: WindowId,
    ) -> Result<Geometry, Box<dyn std::error::Error>> {
        Ok(Geometry {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            border: 0,
        })
    }
    fn set_border_width(
        &self,
        _win: WindowId,
        _border: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn set_border_pixel(
        &self,
        _win: WindowId,
        _pixel: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn change_event_mask(
        &self,
        _win: WindowId,
        _mask: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn map_window(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn send_client_message(
        &self,
        _win: WindowId,
        _type_atom: u32,
        _data: [u32; 5],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn delete_property(
        &self,
        _win: WindowId,
        _atom: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn change_property32(
        &self,
        _win: WindowId,
        _property: u32,
        _ty: u32,
        _data: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn change_property8(
        &self,
        _win: WindowId,
        _property: u32,
        _ty: u32,
        _data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn kill_client(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn grab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn ungrab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn ungrab_all_buttons(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn grab_button_any_anymod(
        &self,
        _win: WindowId,
        _event_mask_bits: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn grab_button(
        &self,
        _win: WindowId,
        _button: u8,
        _event_mask_bits: u32,
        _mods_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn send_configure_notify(
        &self,
        _win: WindowId,
        _x: i16,
        _y: i16,
        _w: u16,
        _h: u16,
        _border: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
