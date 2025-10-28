// src/backend/wayland/property_ops.rs
use super::window_ops::WaylandRegistry;
use crate::backend::api::{NormalHints, PropertyOps, WindowId, WmHints};
use std::sync::{Arc, Mutex};

use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State as XdgToplevelState;

pub struct WaylandPropertyOps {
    reg: Arc<Mutex<WaylandRegistry>>,
}
impl WaylandPropertyOps {
    pub fn new(reg: Arc<Mutex<WaylandRegistry>>) -> Self {
        Self { reg }
    }
}
impl PropertyOps for WaylandPropertyOps {
    fn is_popup_type(&self, _win: WindowId) -> bool {
        false
    }
    fn is_fullscreen(&self, win: WindowId) -> Result<bool, Box<dyn std::error::Error>> {
        let reg = self.reg.lock().unwrap();
        let f = reg
            .windows
            .get(&win.0)
            .and_then(|r| r.toplevel.as_ref())
            .map(|t| {
                // 使用 states.contains(Fullscreen) 判断全屏
                t.current_state()
                    .states
                    .contains(XdgToplevelState::Fullscreen)
            })
            .unwrap_or(false);
        Ok(f)
    }
    fn set_fullscreen_state(
        &self,
        win: WindowId,
        on: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let reg = self.reg.lock().unwrap();
        if let Some(tl) = reg.windows.get(&win.0).and_then(|r| r.toplevel.as_ref()) {
            tl.with_pending_state(|state| {
                if on {
                    state.states.set(XdgToplevelState::Fullscreen);
                } else {
                    state.states.unset(XdgToplevelState::Fullscreen);
                }
            });
            tl.send_configure();
        }
        Ok(())
    }
    // 其它接口保持 no-op 或最小仿真
    fn set_window_strut_top(
        &self,
        _: WindowId,
        _: u32,
        _: u32,
        _: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn clear_window_strut(&self, _: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn get_text_property_best_title(&self, win: WindowId) -> String {
        format!("window-0x{:x}", win.0)
    }
    fn get_wm_class(&self, _: WindowId) -> Option<(String, String)> {
        None
    }
    fn get_wm_hints(&self, _: WindowId) -> Option<WmHints> {
        None
    }
    fn set_urgent_hint(&self, _: WindowId, _: bool) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn transient_for(&self, _: WindowId) -> Option<WindowId> {
        None
    }
    fn fetch_normal_hints(
        &self,
        _: WindowId,
    ) -> Result<Option<NormalHints>, Box<dyn std::error::Error>> {
        Ok(None)
    }
    fn supports_delete_window(&self, _: WindowId) -> bool {
        false
    }
    fn send_delete_window(&self, _: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn set_client_info(
        &self,
        _: WindowId,
        _: u32,
        _: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn get_net_wm_state_atoms(&self, _: WindowId) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }
    fn has_net_wm_state(&self, _: WindowId, _: u32) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }
    fn get_window_types(&self, _: WindowId) -> Vec<u32> {
        vec![]
    }
    fn set_net_wm_state_atoms(
        &self,
        _: WindowId,
        _: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn add_net_wm_state_atom(&self, _: WindowId, _: u32) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn remove_net_wm_state_atom(
        &self,
        _: WindowId,
        _: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn get_wm_state(&self, _: WindowId) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(1)
    }
    fn set_wm_state(&self, _: WindowId, _: i64) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
