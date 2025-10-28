// src/backend/wayland/window_ops.rs
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::backend::api::{Geometry, WindowAttributes, WindowId, WindowOps};

use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::shell::xdg::ToplevelSurface;

use super::event_source::JwmWlState;
use smithay::desktop::{Space, Window as SWindow};
use smithay::utils::Size;

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
    seat: Arc<Mutex<smithay::input::Seat<JwmWlState>>>,
    keyboard: smithay::input::keyboard::KeyboardHandle<JwmWlState>,
}

impl WaylandWindowOps {
    pub fn new(
        reg: Arc<Mutex<WaylandRegistry>>,
        space: Arc<Mutex<Space<SWindow>>>,
        seat: Arc<Mutex<smithay::input::Seat<JwmWlState>>>,
        keyboard: smithay::input::keyboard::KeyboardHandle<JwmWlState>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            reg,
            space,
            seat,
            keyboard,
        })
    }
}

impl WindowOps for WaylandWindowOps {
    fn configure_xywh_border(
        &self,
        win: WindowId,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        border: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut reg = self.reg.lock().unwrap();
        if let Some(rec) = reg.windows.get_mut(&win.0) {
            if let Some(v) = x {
                rec.x = v;
            }
            if let Some(v) = y {
                rec.y = v;
            }
            if let Some(v) = w {
                rec.w = v as i32;
            }
            if let Some(v) = h {
                rec.h = v as i32;
            }
            if let Some(v) = border {
                rec.border = v as i32;
            }

            if let Some(ref window) = rec.handle {
                self.space
                    .lock()
                    .unwrap()
                    .map_element(window.clone(), (rec.x, rec.y), false);
            }
            if let Some(ref toplevel) = rec.toplevel {
                if w.is_some() || h.is_some() {
                    toplevel.with_pending_state(|state| {
                        state.size = Size::from((rec.w, rec.h)).into();
                    });
                    toplevel.send_pending_configure();
                }
            }
        }
        Ok(())
    }

    fn configure_stack_above(
        &self,
        win: WindowId,
        _sibling: Option<WindowId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let reg = self.reg.lock().unwrap();
        if let Some(rec) = reg.windows.get(&win.0) {
            if let Some(ref window) = rec.handle {
                self.space
                    .lock()
                    .unwrap()
                    .raise_element(&window.clone(), true);
            }
        }
        Ok(())
    }

    // Wayland: 焦点需在状态上下文中设置，这里保持 no-op
    fn set_input_focus_window(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    // Wayland: 清焦点亦需在状态上下文中，这里保持 no-op
    fn set_input_focus_root(&self, _root: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get_tree_child(&self, _root: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>> {
        let reg = self.reg.lock().unwrap();
        Ok(reg.windows.keys().map(|&id| WindowId(id)).collect())
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

    fn get_window_attributes(
        &self,
        win: WindowId,
    ) -> Result<WindowAttributes, Box<dyn std::error::Error>> {
        let reg = self.reg.lock().unwrap();
        let is_viewable = reg.windows.contains_key(&win.0);
        Ok(WindowAttributes {
            override_redirect: false,
            map_state_viewable: is_viewable,
        })
    }

    fn get_geometry_translated(
        &self,
        win: WindowId,
    ) -> Result<Geometry, Box<dyn std::error::Error>> {
        let reg = self.reg.lock().unwrap();
        if let Some(rec) = reg.windows.get(&win.0) {
            Ok(Geometry {
                x: rec.x as i16,
                y: rec.y as i16,
                w: rec.w as u16,
                h: rec.h as u16,
                border: rec.border as u16,
            })
        } else {
            Err("window not found".into())
        }
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
