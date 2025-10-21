// src/backend/x11/ewmh_facade.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, PropMode};
use x11rb::wrapper::ConnectionExt;

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
