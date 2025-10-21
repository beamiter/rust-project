// src/backend/x11/window_ops.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::*;

pub struct X11WindowOps<C: Connection> {
    conn: Arc<C>,
}

impl<C: Connection> X11WindowOps<C> {
    pub fn new(conn: Arc<C>) -> Self {
        Self { conn }
    }

    pub fn set_border_width(
        &self,
        win: u32,
        border: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ConfigureWindowAux::new().border_width(border);
        self.conn.configure_window(win, &aux)?.check()?;
        Ok(())
    }

    pub fn set_border_pixel(&self, win: u32, pixel: u32) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ChangeWindowAttributesAux::new().border_pixel(pixel);
        self.conn.change_window_attributes(win, &aux)?.check()?;
        Ok(())
    }

    pub fn change_event_mask(&self, win: u32, mask: u32) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::from(mask));
        self.conn.change_window_attributes(win, &aux)?.check()?;
        Ok(())
    }

    pub fn map_window(&self, win: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.map_window(win)?.check()?;
        Ok(())
    }

    pub fn configure_xywh_border(
        &self,
        win: u32,
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
        self.conn.configure_window(win, &aux)?.check()?;
        Ok(())
    }

    pub fn configure_stack_above(
        &self,
        win: u32,
        sibling: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        if let Some(s) = sibling {
            aux = aux.sibling(s);
        }
        self.conn.configure_window(win, &aux)?.check()?;
        Ok(())
    }

    pub fn set_input_focus_root(&self, root: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, root, 0u32)?
            .check()?;
        Ok(())
    }

    pub fn send_client_message(
        &self,
        win: u32,
        type_atom: u32,
        data: [u32; 5],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let event = ClientMessageEvent::new(32, win, type_atom, data);
        use x11rb::x11_utils::Serialize;
        let buf = event.serialize();
        self.conn
            .send_event(false, win, EventMask::NO_EVENT, buf)?
            .check()?;
        Ok(())
    }

    pub fn delete_property(&self, win: u32, atom: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.delete_property(win, atom)?.check()?;
        Ok(())
    }

    pub fn change_property32(
        &self,
        win: u32,
        property: u32,
        ty: u32,
        data: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;
        self.conn
            .change_property32(PropMode::REPLACE, win, property, ty, data)?;
        Ok(())
    }

    pub fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.flush()?;
        Ok(())
    }

    pub fn kill_client(&self, win: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.kill_client(win)?.check()?;
        Ok(())
    }

    pub fn grab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.grab_server()?;
        Ok(())
    }

    pub fn ungrab_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.ungrab_server()?;
        Ok(())
    }

    pub fn get_window_attributes(&self, win: u32) -> Result<GetWindowAttributesReply, ReplyError> {
        self.conn.get_window_attributes(win)?.reply()
    }

    // 简化：内部自行获取 parent（query_tree）
    pub fn get_geometry_translated(&self, win: u32) -> Result<GetGeometryReply, ReplyError> {
        let geom = self.conn.get_geometry(win)?.reply()?;
        let tree = self.conn.query_tree(win)?.reply()?;
        let trans = self
            .conn
            .translate_coordinates(win, tree.parent, geom.x, geom.y)?
            .reply()?;
        let mut g = geom.clone();
        g.x = trans.dst_x;
        g.y = trans.dst_y;
        Ok(g)
    }

    pub fn ungrab_all_buttons(&self, win: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .ungrab_button(ButtonIndex::ANY, win, ModMask::ANY.into())?
            .check()?;
        Ok(())
    }

    // 额外：抓取 ButtonIndex::ANY + ModMask::ANY（JWM 在未聚焦时启用）
    pub fn grab_button_any_anymod(
        &self,
        win: u32,
        event_mask: EventMask,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .grab_button(
                false,
                win,
                event_mask,
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

    // 常规抓取某个按钮与修饰
    pub fn grab_button(
        &self,
        win: u32,
        button: ButtonIndex,
        event_mask: EventMask,
        mods_bits: u16,
        pointer_mode: GrabMode,
        keyboard_mode: GrabMode,
        confine_to: u32,
        cursor: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mods = ModMask::from(mods_bits);
        self.conn
            .grab_button(
                false,
                win,
                event_mask,
                pointer_mode,
                keyboard_mode,
                confine_to,
                cursor,
                button,
                mods,
            )?
            .check()?;
        Ok(())
    }
}
