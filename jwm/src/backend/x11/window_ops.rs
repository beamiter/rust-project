// src/backend/x11/window_ops.rs
use crate::backend::api::{
    CloseResult, Geometry, Mods, Pixel, WindowAttributes, WindowId, WindowOps,
};
use crate::backend::api::{StackMode, WindowChanges};
use crate::backend::x11::Atoms;
use crate::backend::x11::WindowHandleExt;
use crate::backend::x11::adapter::{event_mask_from_generic, mods_to_x11};
use log::debug;
use std::sync::Arc;
use std::sync::Mutex;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

pub struct X11WindowOps<C: Connection> {
    conn: Arc<C>,
    atoms: Atoms,
    numlock_mask: Arc<Mutex<u16>>,
}

impl<C: Connection> X11WindowOps<C> {
    pub fn new(conn: Arc<C>, atoms: Atoms, numlock_mask: Arc<Mutex<u16>>) -> Self {
        Self {
            conn,
            atoms,
            numlock_mask,
        }
    }

    fn supports_delete_window(&self, win: u32) -> bool {
        let reply = match self.conn.get_property(
            false,
            win,
            self.atoms.WM_PROTOCOLS,
            AtomEnum::ATOM,
            0,
            1024,
        ) {
            Ok(c) => c.reply(),
            Err(_) => return false,
        };

        if let Ok(r) = reply {
            return r
                .value32()
                .into_iter()
                .flatten()
                .any(|a| a == self.atoms.WM_DELETE_WINDOW);
        }
        false
    }
}

impl<C: Connection + Send + Sync + 'static> WindowOps for X11WindowOps<C> {
    fn close_window(&self, win: WindowId) -> Result<CloseResult, Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        if self.supports_delete_window(w) {
            let event = ClientMessageEvent::new(
                32,
                w,
                self.atoms.WM_PROTOCOLS,
                [self.atoms.WM_DELETE_WINDOW, 0, 0, 0, 0],
            );
            use x11rb::x11_utils::Serialize;
            let data = event.serialize();
            self.conn
                .send_event(false, w, EventMask::NO_EVENT, data)?
                .check()?;
            return Ok(CloseResult::Graceful);
        }
        self.conn.kill_client(w)?.check()?;
        Ok(CloseResult::Forced)
    }

    fn change_event_mask(
        &self,
        win: WindowId,
        mask: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("[change_event_mask]");
        let w = win.to_x11_id()?;
        let x_mask = event_mask_from_generic(mask);
        let aux = ChangeWindowAttributesAux::new().event_mask(x_mask);
        self.conn.change_window_attributes(w, &aux)?;
        Ok(())
    }

    fn set_decoration_style(
        &self,
        win: WindowId,
        border_width: u32,
        border_color: Pixel,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux_attr = ChangeWindowAttributesAux::new().border_pixel(border_color.0);
        let w = win.to_x11_id()?;
        self.conn.change_window_attributes(w, &aux_attr)?;

        let aux_conf = ConfigureWindowAux::new().border_width(border_width);
        self.conn.configure_window(w, &aux_conf)?.check()?;

        Ok(())
    }

    fn grab_button_any_anymod(
        &self,
        win: WindowId,
        event_mask_bits: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let x_mask = event_mask_from_generic(event_mask_bits);
        let w = win.to_x11_id()?;
        self.conn
            .grab_button(
                false,
                w,
                x_mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                0u32,
                ButtonIndex::ANY,
                ModMask::ANY.into(),
            )?
            .check()?;
        Ok(())
    }

    fn grab_button(
        &self,
        win: WindowId,
        button: u8,
        event_mask_bits: u32,
        mods: Mods,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let x_mask = event_mask_from_generic(event_mask_bits);
        let bi = ButtonIndex::from(button);
        let numlock_val = *self.numlock_mask.lock().unwrap();
        let numlock_obj = KeyButMask::from(numlock_val);
        let x_mods = mods_to_x11(mods, numlock_obj);
        let mods_bits = ModMask::from(x_mods.bits());
        let w = win.to_x11_id()?;
        self.conn
            .grab_button(
                false,
                w,
                x_mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                0u32,
                bi,
                mods_bits,
            )?
            .check()?;
        Ok(())
    }

    fn send_configure_notify(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
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

    fn set_input_focus_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        self.conn
            .set_input_focus(InputFocus::NONE, w, 0u32)?
            .check()?;
        Ok(())
    }

    fn map_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        self.conn.map_window(w)?.check()?;
        Ok(())
    }

    fn apply_window_changes(
        &self,
        win: WindowId,
        changes: WindowChanges,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
        self.conn.configure_window(w, &aux)?.check()?;
        Ok(())
    }

    fn set_input_focus_root(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let r = root.to_x11_id()?;
        self.conn
            .set_input_focus(InputFocus::NONE, r, 0u32)?
            .check()?;
        Ok(())
    }

    fn send_client_message(
        &self,
        win: WindowId,
        type_atom: u32,
        data: [u32; 5],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let event = ClientMessageEvent::new(32, w, type_atom, data);
        use x11rb::x11_utils::Serialize;
        let buf = event.serialize();
        self.conn
            .send_event(false, w, EventMask::NO_EVENT, buf)?
            .check()?;
        Ok(())
    }

    fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.flush()?;
        Ok(())
    }

    fn kill_client(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        self.conn.kill_client(w)?.check()?;
        Ok(())
    }

    fn grab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.grab_server()?.check()?;
        Ok(())
    }

    fn ungrab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.ungrab_server()?;
        Ok(())
    }

    fn get_window_attributes(
        &self,
        win: WindowId,
    ) -> Result<WindowAttributes, Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let r = self.conn.get_window_attributes(w)?.reply()?;
        Ok(WindowAttributes {
            override_redirect: r.override_redirect,
            map_state_viewable: r.map_state == MapState::VIEWABLE,
        })
    }

    fn get_geometry_translated(
        &self,
        win: WindowId,
    ) -> Result<Geometry, Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let geom_reply = self.conn.get_geometry(w)?.reply()?;
        let tree_reply = self.conn.query_tree(w)?.reply()?;
        let trans_coord = self
            .conn
            .translate_coordinates(w, tree_reply.parent, geom_reply.x, geom_reply.y)?
            .reply()?;
        Ok(Geometry {
            x: trans_coord.dst_x,
            y: trans_coord.dst_y,
            w: geom_reply.width,
            h: geom_reply.height,
            border: geom_reply.border_width,
        })
    }

    fn get_tree_child(&self, win: WindowId) -> Result<Vec<WindowId>, Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        let tree_reply = self.conn.query_tree(w)?.reply()?;
        Ok(tree_reply
            .children
            .iter()
            .map(|c| WindowId::X11(*c as u64))
            .collect())
    }

    fn ungrab_all_buttons(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        self.conn
            .ungrab_button(ButtonIndex::ANY, w, ModMask::ANY.into())?
            .check()?;
        Ok(())
    }
}
