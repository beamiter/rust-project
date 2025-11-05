use crate::backend::api::{EwmhFacade, EwmhFeature, WindowId};

pub struct WaylandEwmhFacade {}

impl WaylandEwmhFacade {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl EwmhFacade for WaylandEwmhFacade {
    fn set_active_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn clear_active_window(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn set_client_list(&self, list: &[WindowId]) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn set_client_list_stacking(
        &self,
        list: &[WindowId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn setup_supporting_wm_check(
        &self,
        wm_name: &str,
    ) -> Result<WindowId, Box<dyn std::error::Error>> {
        Ok(WindowId(0))
    }

    fn set_supported_atoms(&self, supported: &[u32]) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn declare_supported(
        &self,
        features: &[EwmhFeature],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
