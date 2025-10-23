// src/backend/wayland/property_ops.rs
use crate::backend::api::{
    NormalHints, PropertyOps, WmHints, WindowId,
};

pub struct WlPropertyOps;

impl WlPropertyOps {
    pub fn new() -> Self { Self }
}

impl PropertyOps for WlPropertyOps {
    fn set_window_strut_top(&self, _win: WindowId, _top: u32, _start_x: u32, _end_x: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn clear_window_strut(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn get_text_property_best_title(&self, win: WindowId) -> String { format!("Window 0x{:x}", win.0) }
    fn get_wm_class(&self, _win: WindowId) -> Option<(String, String)> { None }

    fn is_popup_type(&self, _win: WindowId) -> bool { false }
    fn is_fullscreen(&self, _win: WindowId) -> Result<bool, Box<dyn std::error::Error>> { Ok(false) }
    fn set_fullscreen_state(&self, _win: WindowId, _on: bool) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn get_wm_hints(&self, _win: WindowId) -> Option<WmHints> { None }
    fn set_urgent_hint(&self, _win: WindowId, _urgent: bool) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn transient_for(&self, _win: WindowId) -> Option<WindowId> { None }
    fn fetch_normal_hints(&self, _win: WindowId) -> Result<Option<NormalHints>, Box<dyn std::error::Error>> { Ok(None) }

    fn supports_delete_window(&self, _win: WindowId) -> bool { true } // 统一调用 close
    fn send_delete_window(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn set_client_info(&self, _win: WindowId, _tags: u32, _monitor_num: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn get_net_wm_state_atoms(&self, _win: WindowId) -> Result<Vec<u32>, Box<dyn std::error::Error>> { Ok(vec![]) }
    fn has_net_wm_state(&self, _win: WindowId, _state_atom: u32) -> Result<bool, Box<dyn std::error::Error>> { Ok(false) }
    fn get_window_types(&self, _win: WindowId) -> Vec<u32> { vec![] }

    fn set_net_wm_state_atoms(&self, _win: WindowId, _atoms: &[u32]) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn add_net_wm_state_atom(&self, _win: WindowId, _atom: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    fn remove_net_wm_state_atom(&self, _win: WindowId, _atom: u32) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }

    fn get_wm_state(&self, _win: WindowId) -> Result<i64, Box<dyn std::error::Error>> { Ok(-1) }
    fn set_wm_state(&self, _win: WindowId, _state: i64) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
}
