// src/backend/x11/property_ops.rs
use crate::backend::api::{PropertyOps as PropertyOpsTrait, WindowId};
use crate::xcb_util::Atoms;
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

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

    // 截断到字符数（非字节数）上限
    pub fn truncate_chars(input: String, max_chars: usize) -> String {
        if input.is_empty() {
            return input;
        }
        let mut count = 0usize;
        let mut truncate_at = input.len();
        for (idx, _) in input.char_indices() {
            if count >= max_chars {
                truncate_at = idx;
                break;
            }
            count += 1;
        }
        let mut s = input;
        s.truncate(truncate_at);
        s
    }
}

impl<C: Connection + Send + Sync + 'static> PropertyOpsTrait for X11PropertyOps<C> {
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
