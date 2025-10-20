// src/backend/x11/window_ops.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::x11_utils::Serialize;

pub struct X11WindowOps<C: Connection> {
    conn: Arc<C>,
}

impl<C: Connection> X11WindowOps<C> {
    pub fn new(conn: Arc<C>) -> Self {
        Self { conn }
    }
}

impl<C: Connection + Send + Sync + 'static> X11WindowOps<C> {
    // 边框宽度
    pub fn set_border_width(
        &self,
        window: Window,
        border_width: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ConfigureWindowAux::new().border_width(border_width);
        self.conn.configure_window(window, &aux)?;
        Ok(())
    }

    // 边框颜色（X11 pixel）
    pub fn set_border_pixel(
        &self,
        window: Window,
        pixel: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ChangeWindowAttributesAux::new().border_pixel(pixel);
        self.conn.change_window_attributes(window, &aux)?;
        Ok(())
    }

    // 变更事件掩码
    pub fn change_event_mask(
        &self,
        window: Window,
        mask: EventMask,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let aux = ChangeWindowAttributesAux::new().event_mask(mask);
        self.conn.change_window_attributes(window, &aux)?;
        Ok(())
    }

    // 设置输入焦点到根窗口
    pub fn set_input_focus_root(
        &self,
        root: Window,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .set_input_focus(InputFocus::POINTER_ROOT, root, 0u32)?
            .check()?;
        Ok(())
    }

    // 发送 ClientMessage（如 WM_PROTOCOLS）
    pub fn send_client_message(
        &self,
        window: Window,
        message_type: Atom,
        data: [u32; 5],
        event_mask: EventMask,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let event = ClientMessageEvent::new(32, window, message_type, data);
        let raw = event.serialize();
        self.conn.send_event(false, window, event_mask, raw)?;
        Ok(())
    }

    // 属性写入（32位）
    pub fn change_property32(
        &self,
        window: Window,
        property: Atom,
        type_atom: Atom,
        data: &[u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::wrapper::ConnectionExt;
        self.conn
            .change_property32(PropMode::REPLACE, window, property, type_atom, data)?;
        Ok(())
    }

    // 删除属性
    pub fn delete_property(
        &self,
        window: Window,
        property: Atom,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.delete_property(window, property)?;
        Ok(())
    }

    // 将窗口置于某 sibling 之上（或无 sibling，直接 above）
    pub fn configure_stack_above(
        &self,
        window: Window,
        sibling: Option<Window>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut cfg = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
        if let Some(sib) = sibling {
            cfg = cfg.sibling(sib);
        }
        self.conn.configure_window(window, &cfg)?;
        Ok(())
    }

    // 统一配置 x/y/width/height/border
    pub fn configure_xywh_border(
        &self,
        window: Window,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        border: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut aux = ConfigureWindowAux::new();
        if let Some(v) = x {
            aux = aux.x(v);
        }
        if let Some(v) = y {
            aux = aux.y(v);
        }
        if let Some(v) = w {
            aux = aux.width(v);
        }
        if let Some(v) = h {
            aux = aux.height(v);
        }
        if let Some(v) = border {
            aux = aux.border_width(v);
        }
        self.conn.configure_window(window, &aux)?;
        Ok(())
    }

    // 取消所有按钮抓取（ANY）
    pub fn ungrab_all_buttons(&self, window: Window) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .ungrab_button(ButtonIndex::ANY, window, ModMask::ANY.into())?
            .check()?;
        Ok(())
    }

    // map 窗口
    pub fn map_window(&self, window: Window) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.map_window(window)?.check()?;
        Ok(())
    }

    // flush
    pub fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.flush()?;
        Ok(())
    }
}
