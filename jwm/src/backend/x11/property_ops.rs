// src/backend/x11/property_ops.rs
use crate::backend::api::NormalHints;
use crate::backend::api::WmHints;
use crate::backend::api::{PropertyOps as PropertyOpsTrait, WindowType};
use crate::backend::common_define::WindowId;
use crate::backend::x11::Atoms;
use crate::backend::x11::WindowHandleExt;
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto::*;
use x11rb::wrapper::ConnectionExt as _;

pub struct X11PropertyOps<C: Connection> {
    conn: Arc<C>,
    atoms: Atoms,
}

impl<C: Connection> X11PropertyOps<C> {
    pub fn new(conn: Arc<C>, atoms: Atoms) -> Self {
        Self { conn, atoms }
    }
}

impl<C: Connection + Send + Sync + 'static> X11PropertyOps<C> {
    fn get_text_property(&self, win: WindowId, atom: Atom) -> Option<String> {
        let w = win.to_x11_id().ok()?;
        let reply = self
            .conn
            .get_property(false, w, atom, AtomEnum::ANY, 0, u32::MAX)
            .ok()?
            .reply()
            .ok()?;

        if reply.value.is_empty() || reply.format != 8 {
            return None;
        }

        let value = reply.value;
        if reply.type_ == self.atoms.UTF8_STRING {
            Self::parse_utf8(&value)
        } else if reply.type_ == u32::from(AtomEnum::STRING) {
            Some(Self::parse_latin1(&value))
        } else {
            Self::parse_utf8(&value).or_else(|| Some(Self::parse_latin1(&value)))
        }
    }

    fn parse_utf8(value: &[u8]) -> Option<String> {
        String::from_utf8(value.to_vec()).ok()
    }
    fn parse_latin1(value: &[u8]) -> String {
        value.iter().map(|&b| b as char).collect()
    }

    fn get_net_wm_state_atoms(
        &self,
        win: WindowId,
    ) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let reply = self
            .conn
            .get_property(
                false,
                w,
                self.atoms._NET_WM_STATE,
                AtomEnum::ATOM,
                0,
                u32::MAX,
            )?
            .reply()?;
        if reply.format != 32 {
            return Ok(Vec::new());
        }
        Ok(reply.value32().into_iter().flatten().collect())
    }

    fn set_net_wm_state_atoms(
        &self,
        win: WindowId,
        atoms: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        self.conn.change_property32(
            PropMode::REPLACE,
            w,
            self.atoms._NET_WM_STATE,
            AtomEnum::ATOM,
            atoms,
        )?;
        Ok(())
    }

    fn add_net_wm_state_atom(
        &self,
        win: WindowId,
        atom: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut states = self.get_net_wm_state_atoms(win)?;
        if !states.iter().any(|&a| a == atom) {
            states.push(atom);
            self.set_net_wm_state_atoms(win, &states)?;
        }
        Ok(())
    }

    fn remove_net_wm_state_atom(
        &self,
        win: WindowId,
        atom: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut states = self.get_net_wm_state_atoms(win)?;
        let len_before = states.len();
        states.retain(|&a| a != atom);
        if states.len() != len_before {
            self.set_net_wm_state_atoms(win, &states)?;
        }
        Ok(())
    }

    fn atom_to_window_type(&self, atom: u32) -> WindowType {
        if atom == self.atoms._NET_WM_WINDOW_TYPE_DESKTOP {
            WindowType::Desktop
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_DOCK {
            WindowType::Dock
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_TOOLBAR {
            WindowType::Toolbar
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_MENU {
            WindowType::Menu
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_UTILITY {
            WindowType::Utility
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_SPLASH {
            WindowType::Splash
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_DIALOG {
            WindowType::Dialog
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_DROPDOWN_MENU {
            WindowType::DropdownMenu
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_POPUP_MENU {
            WindowType::PopupMenu
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_TOOLTIP {
            WindowType::Tooltip
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_NOTIFICATION {
            WindowType::Notification
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE_COMBO {
            WindowType::Combo
        }
        // else if atom == self.atoms._NET_WM_WINDOW_TYPE_DND { WindowType::Dnd }
        else {
            WindowType::Unknown
        }
    }
}

impl<C: Connection + Send + Sync + 'static> PropertyOpsTrait for X11PropertyOps<C> {
    fn get_title(&self, win: WindowId) -> String {
        if let Some(title) = self.get_text_property(win, self.atoms._NET_WM_NAME) {
            return title;
        }
        if let Some(title) = self.get_text_property(win, AtomEnum::WM_NAME.into()) {
            return title;
        }
        "".to_string()
    }

    fn get_class(&self, win: WindowId) -> (String, String) {
        let w = win.to_x11_id().unwrap();
        let reply =
            match self
                .conn
                .get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            {
                Ok(cookie) => cookie.reply().ok(),
                Err(_) => None,
            };

        if let Some(reply) = reply {
            if reply.type_ == u32::from(AtomEnum::STRING) && reply.format == 8 {
                let value = reply.value;
                if !value.is_empty() {
                    let mut parts = value.split(|&b| b == 0u8).filter(|s| !s.is_empty());
                    let instance = parts
                        .next()
                        .and_then(|s| String::from_utf8(s.to_vec()).ok())
                        .unwrap_or_default();
                    let class = parts
                        .next()
                        .and_then(|s| String::from_utf8(s.to_vec()).ok())
                        .unwrap_or_default();
                    return (instance.to_lowercase(), class.to_lowercase());
                }
            }
        }
        (String::new(), String::new())
    }

    fn get_window_types(&self, win: WindowId) -> Vec<WindowType> {
        let w = win.to_x11_id().unwrap();
        let mut result = Vec::new();
        if let Ok(reply) = self.conn.get_property(
            false,
            w,
            self.atoms._NET_WM_WINDOW_TYPE,
            AtomEnum::ATOM,
            0,
            u32::MAX,
        ) {
            if let Ok(rep) = reply.reply() {
                if rep.format == 32 {
                    for atom in rep.value32().into_iter().flatten() {
                        let wt = self.atom_to_window_type(atom);
                        if wt != WindowType::Unknown {
                            result.push(wt);
                        }
                    }
                }
            }
        }
        if result.is_empty() {
            if self.transient_for(win).is_some() {
                result.push(WindowType::Dnd);
            } else {
                result.push(WindowType::Normal);
            }
        }
        result
    }

    fn is_fullscreen(&self, win: WindowId) -> bool {
        let states = self.get_net_wm_state_atoms(win).unwrap_or_default();
        states
            .iter()
            .any(|&a| a == self.atoms._NET_WM_STATE_FULLSCREEN)
    }

    fn set_fullscreen_state(
        &self,
        win: WindowId,
        on: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if on {
            self.add_net_wm_state_atom(win, self.atoms._NET_WM_STATE_FULLSCREEN)
        } else {
            self.remove_net_wm_state_atom(win, self.atoms._NET_WM_STATE_FULLSCREEN)
        }
    }

    fn get_wm_hints(&self, win: WindowId) -> Option<WmHints> {
        let w = win.to_x11_id().ok()?;
        let prop = self
            .conn
            .get_property(false, w, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, 0, 20)
            .ok()?
            .reply()
            .ok()?;

        let mut it = prop.value32()?.into_iter();
        let flags = it.next()?;
        const X_URGENCY_HINT: u32 = 1 << 8;
        const INPUT_HINT: u32 = 1 << 0;

        let urgent = (flags & X_URGENCY_HINT) != 0;
        let input = if (flags & INPUT_HINT) != 0 {
            it.next().map(|v| v != 0)
        } else {
            None
        };
        Some(WmHints { urgent, input })
    }

    fn set_urgent_hint(
        &self,
        win: WindowId,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        const X_URGENCY_HINT: u32 = 1 << 8;
        let w = win.to_x11_id()?;
        let cookie =
            self.conn
                .get_property(false, w, AtomEnum::WM_HINTS, AtomEnum::WM_HINTS, 0, 20)?;

        let mut data = Vec::new();
        if let Ok(reply) = cookie.reply() {
            data = reply.value32().into_iter().flatten().collect();
        }
        if data.is_empty() {
            data.push(0);
        }

        if urgent {
            data[0] |= X_URGENCY_HINT;
        } else {
            data[0] &= !X_URGENCY_HINT;
        }

        self.conn.change_property32(
            PropMode::REPLACE,
            w,
            AtomEnum::WM_HINTS,
            AtomEnum::WM_HINTS,
            &data,
        )?;
        Ok(())
    }

    fn transient_for(&self, win: WindowId) -> Option<WindowId> {
        let w = win.to_x11_id().ok()?;
        let r = self
            .conn
            .get_property(
                false,
                w,
                self.atoms.WM_TRANSIENT_FOR,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .ok()?
            .reply()
            .ok()?;

        if r.format == 32 {
            if let Some(t) = r.value32()?.next() {
                if t != 0 && t != w {
                    return Some(WindowId::X11(t as u64));
                }
            }
        }
        None
    }

    fn fetch_normal_hints(
        &self,
        win: WindowId,
    ) -> Result<Option<NormalHints>, Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let reply_opt = WmSizeHints::get_normal_hints(&self.conn, w)?.reply()?;
        if let Some(r) = reply_opt {
            let (mut base_w, mut base_h) = (0, 0);
            let (mut inc_w, mut inc_h) = (0, 0);
            let (mut max_w, mut max_h) = (0, 0);
            let (mut min_w, mut min_h) = (0, 0);
            let (mut min_aspect, mut max_aspect) = (0.0, 0.0);

            if let Some((w, h)) = r.base_size {
                base_w = w;
                base_h = h;
            }
            if let Some((w, h)) = r.size_increment {
                inc_w = w;
                inc_h = h;
            }
            if let Some((w, h)) = r.max_size {
                max_w = w;
                max_h = h;
            }
            if let Some((w, h)) = r.min_size {
                min_w = w;
                min_h = h;
            }
            if let Some((min, max)) = r.aspect {
                min_aspect = min.numerator as f32 / min.denominator as f32;
                max_aspect = max.numerator as f32 / max.denominator as f32;
            }
            Ok(Some(NormalHints {
                base_w,
                base_h,
                inc_w,
                inc_h,
                max_w,
                max_h,
                min_w,
                min_h,
                min_aspect,
                max_aspect,
            }))
        } else {
            Ok(None)
        }
    }

    fn set_window_strut_top(
        &self,
        win: WindowId,
        top: u32,
        start_x: u32,
        end_x: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let strut = [0, 0, top, 0];
        self.conn.change_property32(
            PropMode::REPLACE,
            w,
            self.atoms._NET_WM_STRUT,
            AtomEnum::CARDINAL,
            &strut,
        )?;
        let partial = [0, 0, top, 0, 0, 0, 0, 0, start_x, end_x, 0, 0];
        self.conn.change_property32(
            PropMode::REPLACE,
            w,
            self.atoms._NET_WM_STRUT_PARTIAL,
            AtomEnum::CARDINAL,
            &partial,
        )?;
        Ok(())
    }

    fn clear_window_strut(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let _ = self.conn.delete_property(w, self.atoms._NET_WM_STRUT);
        let _ = self
            .conn
            .delete_property(w, self.atoms._NET_WM_STRUT_PARTIAL);
        Ok(())
    }

    fn set_client_info_props(
        &self,
        win: WindowId,
        tags: u32,
        monitor_num: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let data = [tags, monitor_num];
        self.conn.change_property32(
            PropMode::REPLACE,
            w,
            self.atoms._NET_CLIENT_INFO,
            AtomEnum::CARDINAL,
            &data,
        )?;
        Ok(())
    }

    fn get_wm_state(&self, win: WindowId) -> Result<i64, Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let reply = self
            .conn
            .get_property(false, w, self.atoms.WM_STATE, self.atoms.WM_STATE, 0, 2)?
            .reply()?;
        if reply.format != 32 {
            return Ok(-1);
        }
        Ok(reply
            .value32()
            .into_iter()
            .flatten()
            .next()
            .map(|v| v as i64)
            .unwrap_or(-1))
    }

    fn set_wm_state(&self, win: WindowId, state: i64) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let data: [u32; 2] = [state as u32, 0];
        self.conn.change_property32(
            PropMode::REPLACE,
            w,
            self.atoms.WM_STATE,
            self.atoms.WM_STATE,
            &data,
        )?;
        Ok(())
    }
}
