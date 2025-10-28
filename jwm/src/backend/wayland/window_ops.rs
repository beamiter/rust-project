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
            screen_w: 1920,
            screen_h: 1080,
        }
    }
}

pub struct WaylandWindowOps {
    reg: Arc<Mutex<WaylandRegistry>>,
    space: Arc<Mutex<Space<SWindow>>>,
    seat: Arc<Mutex<smithay::input::Seat<JwmWlState>>>,
}

impl WaylandWindowOps {
    pub fn new(
        reg: Arc<Mutex<WaylandRegistry>>,
        space: Arc<Mutex<Space<SWindow>>>,
        seat: Arc<Mutex<smithay::input::Seat<JwmWlState>>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { reg, space, seat })
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
                    toplevel.send_configure();
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

    // Wayland 下暂时 no-op，避免 smithay 0.7 KeyboardHandle::set_focus 额外参数问题
    fn set_input_focus_window(&self, _win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

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

// 保留 no-op PropertyOps（未改）
pub fn no_op_property_ops() -> Box<dyn crate::backend::api::PropertyOps> {
    struct NoOpProps;
    impl crate::backend::api::PropertyOps for NoOpProps {
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
        fn is_popup_type(&self, _: WindowId) -> bool {
            false
        }
        fn is_fullscreen(&self, _: WindowId) -> Result<bool, Box<dyn std::error::Error>> {
            Ok(false)
        }
        fn set_fullscreen_state(
            &self,
            _: WindowId,
            _: bool,
        ) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
        fn get_wm_hints(&self, _: WindowId) -> Option<crate::backend::api::WmHints> {
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
        ) -> Result<Option<crate::backend::api::NormalHints>, Box<dyn std::error::Error>> {
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
        fn get_net_wm_state_atoms(
            &self,
            _: WindowId,
        ) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
            Ok(vec![])
        }
        fn has_net_wm_state(
            &self,
            _: WindowId,
            _: u32,
        ) -> Result<bool, Box<dyn std::error::Error>> {
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
        fn add_net_wm_state_atom(
            &self,
            _: WindowId,
            _: u32,
        ) -> Result<(), Box<dyn std::error::Error>> {
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
    Box::new(NoOpProps)
}
