// src/backend/x11/property_ops.rs
use crate::backend::api::NormalHints;
use crate::backend::api::WmHints;
use crate::backend::api::{PropertyOps as PropertyOpsTrait, WindowId};
use crate::backend::x11::Atoms;
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
    pub fn get_property(
        &self,
        window: Window,
        property: Atom,
        req_type: AtomEnum,
        long_offset: u32,
        long_length: u32,
    ) -> Result<GetPropertyReply, Box<dyn std::error::Error>> {
        Ok(self
            .conn
            .get_property(false, window, property, req_type, long_offset, long_length)?
            .reply()?)
    }

    // 尝试获取文本属性并解析为 String
    fn get_text_property(&self, window: WindowId, atom: Atom) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window.0 as u32, atom, AtomEnum::ANY, 0, u32::MAX)
            .ok()?
            .reply()
            .ok()?;

        if reply.value.is_empty() || reply.format != 8 {
            return None;
        }

        let value = reply.value;

        // 根据类型解析
        let parsed = if reply.type_ == self.atoms.UTF8_STRING {
            Self::parse_utf8(&value)
        } else if reply.type_ == u32::from(AtomEnum::STRING) {
            Some(Self::parse_latin1(&value))
        } else if reply.type_ == self.atoms.COMPOUND_TEXT {
            // 先尝试UTF-8，失败回退Latin-1
            Self::parse_utf8(&value).or_else(|| Some(Self::parse_latin1(&value)))
        } else {
            // 回退：尝试UTF-8，再Latin-1
            Self::parse_utf8(&value).or_else(|| Some(Self::parse_latin1(&value)))
        };
        parsed
    }

    // 文本解析工具
    fn parse_utf8(value: &[u8]) -> Option<String> {
        String::from_utf8(value.to_vec()).ok()
    }
    fn parse_latin1(value: &[u8]) -> String {
        value.iter().map(|&b| b as char).collect()
    }
}

impl<C: Connection + Send + Sync + 'static> PropertyOpsTrait for X11PropertyOps<C> {
    fn set_window_strut_top(
        &self,
        win: WindowId,
        top: u32,
        start_x: u32,
        end_x: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let strut = [0, 0, top, 0];
        self.conn
            .change_property32(
                PropMode::REPLACE,
                win.0 as u32,
                self.atoms._NET_WM_STRUT,
                AtomEnum::CARDINAL,
                &strut,
            )?
            .check()?;
        let partial = [0, 0, top, 0, 0, 0, 0, 0, start_x, end_x, 0, 0];
        self.conn
            .change_property32(
                PropMode::REPLACE,
                win.0 as u32,
                self.atoms._NET_WM_STRUT_PARTIAL,
                AtomEnum::CARDINAL,
                &partial,
            )?
            .check()?;
        Ok(())
    }

    fn clear_window_strut(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self
            .conn
            .delete_property(win.0 as u32, self.atoms._NET_WM_STRUT);
        let _ = self
            .conn
            .delete_property(win.0 as u32, self.atoms._NET_WM_STRUT_PARTIAL);
        Ok(())
    }
    fn is_fullscreen(&self, win: WindowId) -> Result<bool, Box<dyn std::error::Error>> {
        let cookie = self.conn.get_property(
            false,
            win.0 as u32,
            self.atoms._NET_WM_STATE,
            AtomEnum::ATOM,
            0,
            u32::MAX,
        )?;
        let reply = cookie.reply()?;
        let has = reply
            .value32()
            .map(|mut v| v.any(|a| a == self.atoms._NET_WM_STATE_FULLSCREEN))
            .unwrap_or(false);
        Ok(has)
    }

    fn set_fullscreen_state(
        &self,
        win: WindowId,
        on: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 直接 add/remove _NET_WM_STATE_FULLSCREEN
        let mut atoms: Vec<u32> = Vec::new();
        if on {
            atoms.push(self.atoms._NET_WM_STATE_FULLSCREEN);
            self.conn
                .change_property32(
                    PropMode::APPEND,
                    win.0 as u32,
                    self.atoms._NET_WM_STATE,
                    AtomEnum::ATOM,
                    &atoms,
                )?
                .check()?;
        } else {
            // 读取现有状态，去掉 FULLSCREEN 后重写
            let current = self
                .conn
                .get_property(
                    false,
                    win.0 as u32,
                    self.atoms._NET_WM_STATE,
                    AtomEnum::ATOM,
                    0,
                    u32::MAX,
                )?
                .reply()?;
            let list: Vec<u32> = current
                .value32()
                .into_iter()
                .flatten()
                .filter(|&a| a != self.atoms._NET_WM_STATE_FULLSCREEN)
                .collect();
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    win.0 as u32,
                    self.atoms._NET_WM_STATE,
                    AtomEnum::ATOM,
                    &list,
                )?
                .check()?;
        }
        Ok(())
    }

    fn get_wm_hints(&self, win: WindowId) -> Option<WmHints> {
        let prop = self
            .conn
            .get_property(
                false,
                win.0 as u32,
                AtomEnum::WM_HINTS,
                AtomEnum::WM_HINTS,
                0,
                20,
            )
            .ok()?
            .reply()
            .ok()?;
        let mut it = prop.value32()?.into_iter();
        let flags = it.next()?;
        const X_URGENCY_HINT: u32 = 1 << 8;
        const INPUT_HINT: u32 = 1 << 0;
        let urgent = (flags & X_URGENCY_HINT) != 0;
        // 若 INPUT_HINT 位存在，则下一个字段是 input
        let input = if (flags & INPUT_HINT) != 0 {
            it.next().map(|v| v != 0)
        } else {
            None
        };
        Some(WmHints { urgent, input })
    }

    fn fetch_normal_hints(
        &self,
        win: WindowId,
    ) -> Result<Option<NormalHints>, Box<dyn std::error::Error>> {
        let reply_opt = WmSizeHints::get_normal_hints(&self.conn, win.0 as u32)?.reply()?;
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

    fn supports_delete_window(&self, win: WindowId) -> bool {
        let reply = match self.conn.get_property(
            false,
            win.0 as u32,
            self.atoms.WM_PROTOCOLS,
            AtomEnum::ATOM,
            0,
            1024,
        ) {
            Ok(c) => c.reply(),
            Err(_) => return false,
        };
        let reply = match reply {
            Ok(r) => r,
            Err(_) => return false,
        };
        let found = reply
            .value32()
            .into_iter()
            .flatten()
            .any(|a| a == self.atoms.WM_DELETE_WINDOW);
        found
    }

    fn send_delete_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let event = ClientMessageEvent::new(
            32,
            win.0 as u32,
            self.atoms.WM_PROTOCOLS,
            [self.atoms.WM_DELETE_WINDOW, 0, 0, 0, 0],
        );
        use x11rb::x11_utils::Serialize;
        let data = event.serialize();
        self.conn
            .send_event(false, win.0 as u32, EventMask::NO_EVENT, data)?
            .check()?;
        Ok(())
    }

    fn set_client_info(
        &self,
        win: WindowId,
        tags: u32,
        monitor_num: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let data = [tags, monitor_num];
        self.conn
            .change_property32(
                PropMode::REPLACE,
                win.0 as u32,
                self.atoms._NET_CLIENT_INFO,
                AtomEnum::CARDINAL,
                &data,
            )?
            .check()?;
        Ok(())
    }
    fn get_wm_state(&self, win: WindowId) -> Result<i64, Box<dyn std::error::Error>> {
        // 等价于原 jwm.get_wm_state
        let reply = self
            .conn
            .get_property(
                false,
                win.0 as u32,
                self.atoms.WM_STATE,
                self.atoms.WM_STATE,
                0,
                2,
            )?
            .reply()?;
        if reply.format != 32 {
            return Ok(-1);
        }
        let it = reply.value32();
        let x = Ok(it
            .into_iter()
            .flatten()
            .next()
            .map(|v| v as i64)
            .unwrap_or(-1));
        x
    }

    fn set_wm_state(&self, win: WindowId, state: i64) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;
        let data: [u32; 2] = [state as u32, 0];
        self.conn.change_property32(
            x11rb::protocol::xproto::PropMode::REPLACE,
            win.0 as u32,
            self.atoms.WM_STATE,
            self.atoms.WM_STATE,
            &data,
        )?;
        Ok(())
    }

    fn set_urgent_hint(
        &self,
        win: WindowId,
        urgent: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::protocol::xproto::{AtomEnum, PropMode};
        // 读取 WM_HINTS，修改 XUrgencyHint 位（1<<8）
        const X_URGENCY_HINT: u32 = 1 << 8;

        let cookie = self.conn.get_property(
            false,
            win.0 as u32,
            AtomEnum::WM_HINTS,
            AtomEnum::CARDINAL,
            0,
            20,
        )?;
        let reply = match cookie.reply() {
            Ok(r) => r,
            Err(_) => {
                // 不存在时，直接写 flags
                let flags = if urgent { X_URGENCY_HINT } else { 0 };
                self.conn.change_property32(
                    PropMode::REPLACE,
                    win.0 as u32,
                    AtomEnum::WM_HINTS,
                    AtomEnum::WM_HINTS,
                    &[flags],
                )?;
                return Ok(());
            }
        };
        let v = reply.value32();
        let mut data: Vec<u32> = v.into_iter().flatten().collect();
        // 至少要有 flags 字段
        if data.is_empty() {
            data.push(0);
        }
        if urgent {
            data[0] |= X_URGENCY_HINT;
        } else {
            data[0] &= !X_URGENCY_HINT;
        }
        use x11rb::wrapper::ConnectionExt;
        self.conn.change_property32(
            PropMode::REPLACE,
            win.0 as u32,
            AtomEnum::WM_HINTS,
            AtomEnum::WM_HINTS,
            &data,
        )?;
        Ok(())
    }

    fn is_popup_type(&self, win: WindowId) -> bool {
        let types = self.get_window_types(win);
        // X11: 看 EWMH 类型
        if types.iter().any(|&a| {
            a == self.atoms._NET_WM_WINDOW_TYPE_POPUP_MENU
                || a == self.atoms._NET_WM_WINDOW_TYPE_DROPDOWN_MENU
                || a == self.atoms._NET_WM_WINDOW_TYPE_MENU
                || a == self.atoms._NET_WM_WINDOW_TYPE_TOOLTIP
                || a == self.atoms._NET_WM_WINDOW_TYPE_COMBO
                || a == self.atoms._NET_WM_WINDOW_TYPE_NOTIFICATION
        }) {
            return true;
        }
        // fallback: transient_for 存在也可能是 popup-like
        self.transient_for(win).is_some()
    }

    fn transient_for(&self, win: WindowId) -> Option<WindowId> {
        // 读取 WM_TRANSIENT_FOR
        let r = self
            .conn
            .get_property(
                false,
                win.0 as u32,
                self.atoms.WM_TRANSIENT_FOR,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .ok()?
            .reply()
            .ok()?;
        if r.format == 32 {
            let it = r.value32();
            if let Some(t) = it?.next() {
                if t != 0 && t != win.0 as u32 {
                    return Some(WindowId(t as u64));
                }
            }
        }
        None
    }

    fn get_text_property_best_title(&self, win: WindowId) -> String {
        // 复用现有逻辑
        if let Some(title) = self.get_text_property(win, self.atoms._NET_WM_NAME) {
            return title;
        }
        if let Some(title) = self.get_text_property(win, AtomEnum::WM_NAME.into()) {
            return title;
        }
        format!("Window 0x{:x}", win.0)
    }

    fn get_wm_class(&self, win: WindowId) -> Option<(String, String)> {
        let reply = self
            .conn
            .get_property(
                false,
                win.0 as u32,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                0,
                256,
            )
            .ok()?
            .reply()
            .ok()?;
        if reply.type_ != u32::from(AtomEnum::STRING) || reply.format != 8 {
            return None;
        }
        let value = reply.value;
        if value.is_empty() {
            return None;
        }
        let mut parts = value.split(|&b| b == 0u8).filter(|s| !s.is_empty());
        let instance = parts
            .next()
            .and_then(|s| String::from_utf8(s.to_vec()).ok())?;
        let class = parts
            .next()
            .and_then(|s| String::from_utf8(s.to_vec()).ok())?;
        Some((instance.to_lowercase(), class.to_lowercase()))
    }

    fn has_net_wm_state(
        &self,
        win: WindowId,
        state_atom: u32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let states = self.get_net_wm_state_atoms(win)?;
        Ok(states.iter().any(|&a| a == state_atom))
    }

    fn get_window_types(&self, win: WindowId) -> Vec<u32> {
        if let Ok(reply) = self.conn.get_property(
            false,
            win.0 as u32,
            self.atoms._NET_WM_WINDOW_TYPE,
            AtomEnum::ATOM,
            0,
            u32::MAX,
        ) {
            if let Ok(rep) = reply.reply() {
                if rep.format == 32 {
                    return rep.value32().into_iter().flatten().collect::<Vec<_>>();
                }
            }
        }
        Vec::new()
    }

    fn set_net_wm_state_atoms(
        &self,
        win: WindowId,
        atoms: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;
        self.conn.change_property32(
            PropMode::REPLACE,
            win.0 as u32,
            self.atoms._NET_WM_STATE,
            AtomEnum::ATOM,
            atoms,
        )?;
        Ok(())
    }

    fn get_net_wm_state_atoms(
        &self,
        win: WindowId,
    ) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
        let reply = self
            .conn
            .get_property(
                false,
                win.0 as u32,
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
}
