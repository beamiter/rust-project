// src/backend/x11/property_ops.rs
use crate::xcb_util::Atoms;
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

pub struct X11PropertyOps<C: Connection> {
    conn: Arc<C>,
}

impl<C: Connection> X11PropertyOps<C> {
    pub fn new(conn: Arc<C>) -> Self {
        Self { conn }
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

    // 解析 WM_CLASS -> (instance, class)
    pub fn get_wm_class(&self, window: Window) -> Option<(String, String)> {
        let reply = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
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

    // 获取单个Atom属性（value32第一个值）
    pub fn get_atom_property_first(&self, window: Window, property: Atom) -> Atom {
        let cookie = match self
            .conn
            .get_property(false, window, property, AtomEnum::ATOM, 0, 1)
        {
            Ok(c) => c,
            Err(_) => return 0,
        };
        let reply = match cookie.reply() {
            Ok(r) => r,
            Err(_) => return 0,
        };
        let mut values = match reply.value32() {
            Some(v) => v,
            None => return 0,
        };
        values.next().unwrap_or(0)
    }

    // 获取 _NET_WM_STATE 列表
    pub fn get_net_wm_state(
        &self,
        window: Window,
        atoms: &Atoms,
    ) -> Result<Vec<Atom>, Box<dyn std::error::Error>> {
        let reply = self
            .conn
            .get_property(
                false,
                window,
                atoms._NET_WM_STATE,
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

    pub fn has_net_wm_state(
        &self,
        window: Window,
        atoms: &Atoms,
        state_atom: Atom,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let states = self.get_net_wm_state(window, atoms)?;
        Ok(states.iter().any(|&a| a == state_atom))
    }

    // 获取 _NET_WM_WINDOW_TYPE 列表
    pub fn get_window_types(&self, window: Window, atoms: &Atoms) -> Vec<Atom> {
        if let Ok(reply) = self.conn.get_property(
            false,
            window,
            atoms._NET_WM_WINDOW_TYPE,
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

    // 尝试获取文本属性并解析为 String
    pub fn get_text_property(&self, window: Window, atom: Atom, atoms: &Atoms) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window, atom, AtomEnum::ANY, 0, u32::MAX)
            .ok()?
            .reply()
            .ok()?;

        if reply.value.is_empty() || reply.format != 8 {
            return None;
        }

        let value = reply.value;

        // 根据类型解析
        let parsed = if reply.type_ == atoms.UTF8_STRING {
            Self::parse_utf8(&value)
        } else if reply.type_ == u32::from(AtomEnum::STRING) {
            Some(Self::parse_latin1(&value))
        } else if reply.type_ == atoms.COMPOUND_TEXT {
            // 先尝试UTF-8，失败回退Latin-1
            Self::parse_utf8(&value).or_else(|| Some(Self::parse_latin1(&value)))
        } else {
            // 回退：尝试UTF-8，再Latin-1
            Self::parse_utf8(&value).or_else(|| Some(Self::parse_latin1(&value)))
        };
        parsed
    }

    // 获取最佳窗口标题（优先 _NET_WM_NAME，然后 WM_NAME）
    pub fn get_best_window_title(&self, window: Window, atoms: &Atoms) -> String {
        if let Some(title) = self.get_text_property(window, atoms._NET_WM_NAME, atoms) {
            return title;
        }
        if let Some(title) = self.get_text_property(window, AtomEnum::WM_NAME.into(), atoms) {
            return title;
        }
        format!("Window 0x{:x}", window)
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
