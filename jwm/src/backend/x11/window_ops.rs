// src/backend/x11/window_ops.rs
use crate::backend::api::{CloseResult, Geometry, WindowAttributes, WindowOps};
use crate::backend::api::{StackMode, WindowChanges};
use crate::backend::common_define::{Mods, Pixel, WindowId};
use crate::backend::error::BackendError;
use crate::backend::x11::Atoms;
use crate::backend::x11::WindowHandleExt;
use crate::backend::x11::adapter::{event_mask_from_generic, mods_to_x11};
use log::debug;
use std::sync::Arc;
use std::sync::Mutex;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::x11_utils::Serialize;

pub struct X11WindowOps<C: Connection> {
    conn: Arc<C>,
    atoms: Atoms,
    numlock_mask: Arc<Mutex<u16>>,
    root: u32,
}

impl<C: Connection> X11WindowOps<C> {
    pub fn new(conn: Arc<C>, atoms: Atoms, numlock_mask: Arc<Mutex<u16>>, root: u32) -> Self {
        Self {
            conn,
            atoms,
            numlock_mask,
            root,
        }
    }

    fn send_configure_notify_internal(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border: u16,
    ) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        let event = ConfigureNotifyEvent {
            response_type: CONFIGURE_NOTIFY_EVENT,
            sequence: 0,
            event: w,
            window: w,
            x,
            y,
            width,
            height,
            border_width: border,
            above_sibling: 0,
            override_redirect: false,
        };
        self.conn
            .send_event(false, w, EventMask::STRUCTURE_NOTIFY, event)?;
        self.conn.flush()?;
        Ok(())
    }
}

impl<C: Connection + Send + Sync + 'static> WindowOps for X11WindowOps<C> {
    fn set_position(&self, win: WindowId, x: i32, y: i32) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        let aux = ConfigureWindowAux::new().x(x).y(y);
        self.conn.configure_window(w, &aux)?;
        Ok(())
    }

    fn configure(
        &self,
        win: WindowId,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        border: u32,
    ) -> Result<(), BackendError> {
        let wid = win.to_x11_id()?;

        // 1. 物理调整窗口
        let aux = ConfigureWindowAux::new()
            .x(x)
            .y(y)
            .width(w)
            .height(h)
            .border_width(border);
        self.conn.configure_window(wid, &aux)?;

        // 2. 发送 ConfigureNotify (ICCCM 要求)
        self.send_configure_notify_internal(
            win,
            x as i16,
            y as i16,
            w as u16,
            h as u16,
            border as u16,
        )?;

        Ok(())
    }

    fn set_decoration_style(
        &self,
        win: WindowId,
        border_width: u32,
        border_color: Pixel,
    ) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        // 设置边框颜色
        let aux_attr = ChangeWindowAttributesAux::new().border_pixel(border_color.0);
        self.conn.change_window_attributes(w, &aux_attr)?;
        // 设置边框宽度
        let aux_conf = ConfigureWindowAux::new().border_width(border_width);
        self.conn.configure_window(w, &aux_conf)?;
        Ok(())
    }

    fn raise_window(&self, win: WindowId) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        let aux = ConfigureWindowAux::new().stack_mode(x11rb::protocol::xproto::StackMode::ABOVE);
        self.conn.configure_window(w, &aux)?;
        Ok(())
    }

    fn close_window(&self, win: WindowId) -> Result<CloseResult, BackendError> {
        let w = win.to_x11_id()?;
        let supports_delete = {
            let reply = self
                .conn
                .get_property(false, w, self.atoms.WM_PROTOCOLS, AtomEnum::ATOM, 0, 1024)?
                .reply()?;
            reply
                .value32()
                .into_iter()
                .flatten()
                .any(|a| a == self.atoms.WM_DELETE_WINDOW)
        };

        if supports_delete {
            let event = ClientMessageEvent::new(
                32,
                w,
                self.atoms.WM_PROTOCOLS,
                [self.atoms.WM_DELETE_WINDOW, 0, 0, 0, 0],
            );
            self.conn.send_event(
                false,
                w,
                EventMask::NO_EVENT,
                event.serialize(), // 序列化为字节流
            )?;
            // 刷新请求队列，确保消息立即发出
            self.conn.flush()?;
            return Ok(CloseResult::Graceful);
        }

        self.conn.kill_client(w)?;
        Ok(CloseResult::Forced)
    }

    // 扫描窗口 (X11 必须实现，Wayland 可以返回空 Vec)
    fn scan_windows(&self) -> Result<Vec<WindowId>, BackendError> {
        let tree = self
            .conn
            .query_tree(self.conn.setup().roots[0].root)?
            .reply()?;
        Ok(tree
            .children
            .iter()
            .map(|&w| WindowId::X11(w as u64))
            .collect())
    }

    fn change_event_mask(&self, win: WindowId, mask: u32) -> Result<(), BackendError> {
        debug!("[change_event_mask]");
        let w = win.to_x11_id()?;
        let x_mask = event_mask_from_generic(mask);
        let aux = ChangeWindowAttributesAux::new().event_mask(x_mask);
        self.conn.change_window_attributes(w, &aux)?;
        Ok(())
    }

    fn grab_button_any_anymod(
        &self,
        win: WindowId,
        event_mask_bits: u32,
    ) -> Result<(), BackendError> {
        let x_mask = event_mask_from_generic(event_mask_bits);
        let w = win.to_x11_id()?;
        self.conn.grab_button(
            false,
            w,
            x_mask,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            0u32,
            0u32,
            ButtonIndex::ANY,
            ModMask::ANY.into(),
        )?;
        Ok(())
    }

    fn grab_button(
        &self,
        win: WindowId,
        button: u8,
        event_mask_bits: u32,
        mods: Mods,
    ) -> Result<(), BackendError> {
        let x_mask = event_mask_from_generic(event_mask_bits);
        let bi = ButtonIndex::from(button);
        let numlock_val = *self.numlock_mask.lock().unwrap();
        let numlock_obj = KeyButMask::from(numlock_val);
        let x_mods = mods_to_x11(mods, numlock_obj);
        let mods_bits = ModMask::from(x_mods.bits());
        let w = win.to_x11_id()?;
        self.conn.grab_button(
            false,
            w,
            x_mask,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            0u32,
            0u32,
            bi,
            mods_bits,
        )?;
        Ok(())
    }

    fn map_window(&self, win: WindowId) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        self.conn.map_window(w)?;
        Ok(())
    }

    fn apply_window_changes(
        &self,
        win: WindowId,
        changes: WindowChanges,
    ) -> Result<(), BackendError> {
        let mut aux = ConfigureWindowAux::new();
        if let Some(x) = changes.x {
            aux = aux.x(x);
        }
        if let Some(y) = changes.y {
            aux = aux.y(y);
        }
        if let Some(w) = changes.width {
            aux = aux.width(w);
        }
        if let Some(h) = changes.height {
            aux = aux.height(h);
        }
        if let Some(b) = changes.border_width {
            aux = aux.border_width(b);
        }
        if let Some(sibling) = changes.sibling {
            aux = aux.sibling(sibling.to_x11_id().unwrap());
        }
        if let Some(mode) = changes.stack_mode {
            let x_mode = match mode {
                StackMode::Above => x11rb::protocol::xproto::StackMode::ABOVE,
                StackMode::Below => x11rb::protocol::xproto::StackMode::BELOW,
                StackMode::TopIf => x11rb::protocol::xproto::StackMode::TOP_IF,
                StackMode::BottomIf => x11rb::protocol::xproto::StackMode::BOTTOM_IF,
                StackMode::Opposite => x11rb::protocol::xproto::StackMode::OPPOSITE,
            };
            aux = aux.stack_mode(x_mode);
        }

        let w = win.to_x11_id()?;
        self.conn.configure_window(w, &aux)?;
        Ok(())
    }

    fn set_input_focus_root(&self) -> Result<(), BackendError> {
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, self.root, x11rb::CURRENT_TIME)?;
        Ok(())
    }

    fn unmap_window(&self, win: WindowId) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        self.conn.unmap_window(w)?;
        Ok(())
    }

    fn set_input_focus(&self, win: WindowId) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        self.conn
            .set_input_focus(InputFocus::PARENT, w, x11rb::CURRENT_TIME)?;
        Ok(())
    }

    fn get_geometry(&self, win: WindowId) -> Result<Geometry, BackendError> {
        let w = win.to_x11_id()?;
        let reply = self.conn.get_geometry(w)?.reply()?;
        Ok(Geometry {
            x: reply.x as i32,
            y: reply.y as i32,
            w: reply.width as u32,
            h: reply.height as u32,
            border: reply.border_width as u32,
        })
    }

    fn flush(&self) -> Result<(), BackendError> {
        self.conn.flush()?;
        Ok(())
    }

    fn kill_client(&self, win: WindowId) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        self.conn.kill_client(w)?;
        Ok(())
    }

    fn get_window_attributes(&self, win: WindowId) -> Result<WindowAttributes, BackendError> {
        let w = win.to_x11_id()?;
        let r = self.conn.get_window_attributes(w)?.reply()?;
        Ok(WindowAttributes {
            override_redirect: r.override_redirect,
            map_state_viewable: r.map_state == MapState::VIEWABLE,
        })
    }

    fn get_tree_child(&self, win: WindowId) -> Result<Vec<WindowId>, BackendError> {
        let w = win.to_x11_id()?;
        let tree_reply = self.conn.query_tree(w)?.reply()?;
        Ok(tree_reply
            .children
            .iter()
            .map(|c| WindowId::X11(*c as u64))
            .collect())
    }

    fn ungrab_all_buttons(&self, win: WindowId) -> Result<(), BackendError> {
        let w = win.to_x11_id()?;
        self.conn
            .ungrab_button(ButtonIndex::ANY, w, ModMask::ANY.into())?;
        Ok(())
    }
}
