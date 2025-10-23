// src/backend/wayland/window_ops.rs
use crate::backend::api::{Geometry, WindowAttributes, WindowId, WindowOps};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct WlWindowOps {
    // 维护 surface/toplevel 的虚拟树与几何
    pub(crate) windows: Arc<Mutex<HashMap<u64, Geometry>>>,
}

impl WlWindowOps {
    pub fn new(windows: Arc<Mutex<HashMap<u64, Geometry>>>) -> Self {
        Self { windows }
    }
}

impl WindowOps for WlWindowOps {
    fn get_tree_child(&self, _win: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>> {
        // Wayland：返回已知窗口列表
        let map = self.windows.lock().unwrap();
        Ok(map.keys().map(|k| WindowId(*k)).collect())
    }

    fn set_border_width(&self, _win: WindowId, _border: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn set_border_pixel(&self, _win: WindowId, _pixel: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn change_event_mask(&self, _win: WindowId, _mask: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn map_window(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn configure_xywh_border(
        &self,
        win: WindowId,
        x: Option<i32>, y: Option<i32>, w: Option<u32>, h: Option<u32>, _border: Option<u32>
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut map = self.windows.lock().unwrap();
        let e = map.entry(win.0).or_insert(Geometry { x:0, y:0, w:1, h:1, border:0 });
        if let Some(v) = x { e.x = v as i16; }
        if let Some(v) = y { e.y = v as i16; }
        if let Some(v) = w { e.w = v as u16; }
        if let Some(v) = h { e.h = v as u16; }
        Ok(())
    }

    fn configure_stack_above(&self, _win: WindowId, _sibling: Option<WindowId>) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn set_input_focus_root(&self, _root: WindowId) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn send_client_message(&self, _win: WindowId, _type_atom: u32, _data: [u32; 5]) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn delete_property(&self, _win: WindowId, _atom: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn change_property32(&self, _win: WindowId, _property: u32, _ty: u32, _data: &[u32]) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn change_property8(&self, _win: WindowId, _property: u32, _ty: u32, _data: &[u8]) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn flush(&self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn kill_client(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn grab_server(&self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn ungrab_server(&self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn get_window_attributes(&self, win: WindowId) -> Result<WindowAttributes, Box<dyn std::error::Error>> {
        let map = self.windows.lock().unwrap();
        Ok(WindowAttributes {
            override_redirect: false,
            map_state_viewable: map.contains_key(&win.0),
        })
    }

    fn get_geometry_translated(&self, win: WindowId) -> Result<Geometry, Box<dyn std::error::Error>> {
        let map = self.windows.lock().unwrap();
        Ok(map.get(&win.0).cloned().unwrap_or(Geometry { x:0, y:0, w:800, h:600, border:0 }))
    }

    fn ungrab_all_buttons(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn grab_button_any_anymod(&self, _win: WindowId, _event_mask_bits: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn grab_button(&self, _win: WindowId, _button: u8, _event_mask_bits: u32, _mods_bits: u16) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn send_configure_notify(
        &self, _win: WindowId, _x: i16, _y: i16, _w: u16, _h: u16, _border: u16
    ) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn set_input_focus_window(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
}
