// src/backend/x11/ewmh_facade.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::CreateWindowAux;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xproto::{AtomEnum, PropMode};
use x11rb::wrapper::ConnectionExt as _;

use crate::backend::api::{EwmhFacade, WindowId};
use crate::backend::x11::Atoms;

pub struct X11EwmhFacade<C: Connection> {
    conn: Arc<C>,
    root: WindowId,
    atoms: Atoms,
}

impl<C: Connection> X11EwmhFacade<C> {
    pub fn new(conn: Arc<C>, root: WindowId, atoms: Atoms) -> Self {
        Self { conn, root, atoms }
    }
}

impl<C: Connection + Send + Sync + 'static> EwmhFacade for X11EwmhFacade<C> {
    fn setup_supporting_wm_check(
        &self,
        wm_name: &str,
    ) -> Result<WindowId, Box<dyn std::error::Error>> {
        // 创建 1x1 supporting window
        let frame_win = self.conn.generate_id()?;
        let aux = CreateWindowAux::new().event_mask(EventMask::EXPOSURE | EventMask::KEY_PRESS);
        self.conn
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                frame_win,
                self.root.0 as u32,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &aux,
            )?
            .check()?;
        // 设置 _NET_SUPPORTING_WM_CHECK on root and frame_win
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root.0 as u32,
            self.atoms._NET_SUPPORTING_WM_CHECK,
            AtomEnum::WINDOW,
            &[frame_win],
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            frame_win,
            self.atoms._NET_SUPPORTING_WM_CHECK,
            AtomEnum::WINDOW,
            &[frame_win],
        )?;
        // WM_NAME (STRING)
        x11rb::wrapper::ConnectionExt::change_property8(
            &*self.conn,
            PropMode::REPLACE,
            frame_win,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            wm_name.as_bytes(),
        )?;
        Ok(WindowId(frame_win as u64))
    }

    fn set_supported_atoms(&self, supported: &[u32]) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root.0 as u32,
            self.atoms._NET_SUPPORTED,
            AtomEnum::ATOM,
            supported,
        )?;
        Ok(())
    }

    fn set_active_window(&self, win: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root.0 as u32,
            self.atoms._NET_ACTIVE_WINDOW,
            AtomEnum::WINDOW,
            &[win.0 as u32],
        )?;
        Ok(())
    }

    fn clear_active_window(&self) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::protocol::xproto::ConnectionExt as RawExt;
        self.conn
            .delete_property(self.root.0 as u32, self.atoms._NET_ACTIVE_WINDOW)?;
        Ok(())
    }

    fn set_client_list(&self, list: &[WindowId]) -> Result<(), Box<dyn std::error::Error>> {
        let raw: Vec<u32> = list.iter().map(|w| w.0 as u32).collect();
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root.0 as u32,
            self.atoms._NET_CLIENT_LIST,
            AtomEnum::WINDOW,
            &raw,
        )?;
        Ok(())
    }

    fn set_client_list_stacking(
        &self,
        list: &[WindowId],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw: Vec<u32> = list.iter().map(|w| w.0 as u32).collect();
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root.0 as u32,
            self.atoms._NET_CLIENT_LIST_STACKING,
            AtomEnum::WINDOW,
            &raw,
        )?;
        Ok(())
    }
}
