use crate::backend::api::{NormalHints, PropertyOps, WindowId, WmHints};

pub struct WaylandPropertyOps {}

impl WaylandPropertyOps {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl PropertyOps for WaylandPropertyOps {
    fn set_window_strut_top(
        &self,
        win: WindowId,
        top: u32,
        start_x: u32,
        end_x: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn clear_window_strut(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get_text_property_best_title(&self, win: WindowId) -> String {
        "none".to_string()
    }

    fn get_wm_class(&self, win: WindowId) -> Option<(String, String)> {
        None
    }

    fn is_popup_type(&self, win: WindowId) -> bool {
        false
    }

    fn is_fullscreen(&self, win: WindowId) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    fn set_fullscreen_state(
        &self,
        win: WindowId,
        on: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get_wm_hints(&self, win: WindowId) -> Option<WmHints> {
        None
    }

    fn set_urgent_hint(
        &self,
        win: WindowId,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn transient_for(&self, win: WindowId) -> Option<WindowId> {
        None
    }

    fn fetch_normal_hints(
        &self,
        win: WindowId,
    ) -> Result<Option<NormalHints>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    fn supports_delete_window(&self, win: WindowId) -> bool {
        false
    }

    fn send_delete_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn set_client_info(
        &self,
        win: WindowId,
        tags: u32,
        monitor_num: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get_net_wm_state_atoms(
        &self,
        win: WindowId,
    ) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }

    fn has_net_wm_state(
        &self,
        win: WindowId,
        state_atom: u32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    fn get_window_types(&self, win: WindowId) -> Vec<u32> {
        vec![]
    }

    fn set_net_wm_state_atoms(
        &self,
        win: WindowId,
        atoms: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn add_net_wm_state_atom(
        &self,
        win: WindowId,
        atom: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn remove_net_wm_state_atom(
        &self,
        win: WindowId,
        atom: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get_wm_state(&self, win: WindowId) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(0)
    }

    fn set_wm_state(&self, win: WindowId, state: i64) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
