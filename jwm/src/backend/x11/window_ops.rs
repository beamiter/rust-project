// src/backend/x11/window_ops.rs
use crate::backend::api::{Geometry, WindowAttributes, WindowId, WindowOps};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

pub struct X11WindowOps<C: Connection> {
    conn: Arc<C>,
}

impl<C: Connection> X11WindowOps<C> {
    pub fn new(conn: Arc<C>) -> Self {
        Self { conn }
    }
}

impl<C: Connection + Send + Sync + 'static> WindowOps for X11WindowOps<C> {
    fn send_configure_notify(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
        w: u16,
        h: u16,
        border: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 构造并发送 ConfigureNotify
        let event = ConfigureNotifyEvent {
            response_type: CONFIGURE_NOTIFY_EVENT,
            sequence: 0,
            event: win.0 as u32,
            window: win.0 as u32,
            x,
            y,
            width: w,
            height: h,
            border_width: border,
            above_sibling: 0,
            override_redirect: false,
        };
        self.conn
            .send_event(false, win.0 as u32, EventMask::STRUCTURE_NOTIFY, event)?;
        self.conn.flush()?;
        Ok(())
    }

    fn set_input_focus_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, win.0 as u32, 0u32)?
            .check()?;
        Ok(())
    }

    fn set_border_width(
        &self,
        win: WindowId,
        border: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ConfigureWindowAux::new().border_width(border);
        self.conn.configure_window(win.0 as u32, &aux)?.check()?;
        Ok(())
    }

    fn set_border_pixel(
        &self,
        win: WindowId,
        pixel: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ChangeWindowAttributesAux::new().border_pixel(pixel);
        self.conn
            .change_window_attributes(win.0 as u32, &aux)?
            .check()?;
        Ok(())
    }

    fn change_event_mask(
        &self,
        win: WindowId,
        mask: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::from(mask));
        self.conn
            .change_window_attributes(win.0 as u32, &aux)?
            .check()?;
        Ok(())
    }

    fn map_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.map_window(win.0 as u32)?.check()?;
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
        let mut aux = ConfigureWindowAux::new();
        if let Some(x) = x {
            aux = aux.x(x);
        }
        if let Some(y) = y {
            aux = aux.y(y);
        }
        if let Some(w) = w {
            aux = aux.width(w);
        }
        if let Some(h) = h {
            aux = aux.height(h);
        }
        if let Some(b) = border {
            aux = aux.border_width(b);
        }
        self.conn.configure_window(win.0 as u32, &aux)?.check()?;
        Ok(())
    }

    fn configure_stack_above(
        &self,
        win: WindowId,
        sibling: Option<WindowId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        if let Some(s) = sibling {
            aux = aux.sibling(s.0 as u32);
        }
        self.conn.configure_window(win.0 as u32, &aux)?.check()?;
        Ok(())
    }

    fn set_input_focus_root(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, root.0 as u32, 0u32)?
            .check()?;
        Ok(())
    }

    fn send_client_message(
        &self,
        win: WindowId,
        type_atom: u32,
        data: [u32; 5],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let event = ClientMessageEvent::new(32, win.0 as u32, type_atom, data);
        use x11rb::x11_utils::Serialize;
        let buf = event.serialize();
        self.conn
            .send_event(false, win.0 as u32, EventMask::NO_EVENT, buf)?
            .check()?;
        Ok(())
    }

    fn delete_property(&self, win: WindowId, atom: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.delete_property(win.0 as u32, atom)?.check()?;
        Ok(())
    }

    fn change_property32(
        &self,
        win: WindowId,
        property: u32,
        ty: u32,
        data: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;
        self.conn
            .change_property32(PropMode::REPLACE, win.0 as u32, property, ty, data)?;
        Ok(())
    }

    fn change_property8(
        &self,
        win: WindowId,
        property: u32,
        ty: u32,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;
        self.conn
            .change_property8(PropMode::REPLACE, win.0 as u32, property, ty, data)?;
        Ok(())
    }

    fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.flush()?;
        Ok(())
    }

    fn kill_client(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.kill_client(win.0 as u32)?.check()?;
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
        let r = self.conn.get_window_attributes(win.0 as u32)?.reply()?;
        Ok(WindowAttributes {
            override_redirect: r.override_redirect,
            map_state_viewable: r.map_state == MapState::VIEWABLE,
        })
    }

    fn get_geometry_translated(
        &self,
        win: WindowId,
    ) -> Result<Geometry, Box<dyn std::error::Error>> {
        let geom_reply = self.conn.get_geometry(win.0 as u32)?.reply()?;
        let tree_reply = self.conn.query_tree(win.0 as u32)?.reply()?;
        let trans_coord = self
            .conn
            .translate_coordinates(win.0 as u32, tree_reply.parent, geom_reply.x, geom_reply.y)?
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
        let tree_reply = self.conn.query_tree(win.0 as u32)?.reply()?;
        Ok(tree_reply
            .children
            .iter()
            .map(|c| WindowId(*c as u64))
            .collect())
    }

    fn ungrab_all_buttons(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .ungrab_button(ButtonIndex::ANY, win.0 as u32, ModMask::ANY.into())?
            .check()?;
        Ok(())
    }

    fn grab_button_any_anymod(
        &self,
        win: WindowId,
        event_mask_bits: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .grab_button(
                false,
                win.0 as u32,
                EventMask::from(event_mask_bits),
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
        mods_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let bi = ButtonIndex::from(button);
        let mods = ModMask::from(mods_bits);
        self.conn
            .grab_button(
                false,
                win.0 as u32,
                EventMask::from(event_mask_bits),
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                0u32,
                bi,
                mods,
            )?
            .check()?;
        Ok(())
    }
}
