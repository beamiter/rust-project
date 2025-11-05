use crate::backend::api::{Geometry, WindowAttributes, WindowId, WindowOps};

pub struct WaylandWindowOps {}

impl WaylandWindowOps {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl WindowOps for WaylandWindowOps {
    fn get_tree_child(&self, win: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }

    fn set_border_width(
        &self,
        win: WindowId,
        border: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn set_border_pixel(
        &self,
        win: WindowId,
        pixel: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn change_event_mask(
        &self,
        win: WindowId,
        mask: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn map_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn configure_xywh_border(
        &self,
        win: WindowId,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        border: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn configure_stack_above(
        &self,
        win: WindowId,
        sibling: Option<WindowId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn set_input_focus_root(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn send_client_message(
        &self,
        win: WindowId,
        type_atom: u32,
        data: [u32; 5],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn delete_property(&self, win: WindowId, atom: u32) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn change_property32(
        &self,
        win: WindowId,
        property: u32,
        ty: u32,
        data: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    // 新增：设置 8-bit STRING 属性
    fn change_property8(
        &self,
        win: WindowId,
        property: u32,
        ty: u32,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn kill_client(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn grab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn ungrab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get_window_attributes(
        &self,
        win: WindowId,
    ) -> Result<WindowAttributes, Box<dyn std::error::Error>> {
        Ok(WindowAttributes {
            override_redirect: false,
            map_state_viewable: true,
        })
    }

    fn get_geometry_translated(
        &self,
        win: WindowId,
    ) -> Result<Geometry, Box<dyn std::error::Error>> {
        Ok(Geometry {
            x: 0,
            y: 0,
            w: 960,
            h: 600,
            border: 0,
        })
    }

    fn ungrab_all_buttons(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn grab_button_any_anymod(
        &self,
        win: WindowId,
        event_mask_bits: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn grab_button(
        &self,
        win: WindowId,
        button: u8, // MouseButton::to_u8() 映射
        event_mask_bits: u32,
        mods_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn send_configure_notify(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        border: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn set_input_focus_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
