// src/backend/wayland/window_ops.rs
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::event_source::CompositorCommand;
use crate::backend::api::{Geometry, WindowAttributes, WindowId, WindowOps};
use smithay::desktop::{Space, Window as SWindow};
use smithay::reexports::calloop::channel::Sender as CommandSender;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::Size;
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
    command_tx: CommandSender<CompositorCommand>,
}

impl WaylandWindowOps {
    pub fn new(
        command_tx: CommandSender<CompositorCommand>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { command_tx })
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

    // --- 其他函数保持不变 ---
    // 注意：这里的 configure_xywh_border 等函数现在不再直接访问 space 或 registry
    // 在一个更完整的实现中，这些操作也应该通过 CompositorCommand 发送给事件循环线程处理。
    // 但为了保持简单和让你先跑起来，我们暂时保留它们为空实现或只操作本地数据。
    // 你需要把这些函数的逻辑也迁移到 JwmWlState::process_command 中去。

    fn configure_xywh_border(
        &self,
        _win: WindowId,
        _x: Option<i32>,
        _y: Option<i32>,
        _w: Option<u32>,
        _h: Option<u32>,
        _border: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: This should be a CompositorCommand
        Ok(())
    }
    fn configure_stack_above(
        &self,
        _win: WindowId,
        _sibling: Option<WindowId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: This should be a CompositorCommand
        Ok(())
    }

    // ... 其他 no-op 函数 ...
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
