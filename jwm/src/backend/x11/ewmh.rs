// src/backend/x11/ewmh.rs
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, PropMode, Window};
use x11rb::wrapper::ConnectionExt;

use crate::backend::Ewmh;
use crate::xcb_util::Atoms;

/// X11 上的 EWMH 实现
pub struct X11Ewmh;

impl Ewmh for X11Ewmh {
    type Window = Window;
    type AtomSet = Atoms;

    fn set_active_window<C>(
        &self,
        conn: &C,
        root: Window,
        atoms: &Atoms,
        win: Window,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        C: Connection,
    {
        conn.change_property32(
            PropMode::REPLACE,
            root,
            atoms._NET_ACTIVE_WINDOW,
            AtomEnum::WINDOW,
            &[win],
        )?;
        Ok(())
    }

    fn clear_active_window<C>(
        &self,
        conn: &C,
        root: Window,
        atoms: &Atoms,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        C: Connection,
    {
        use x11rb::protocol::xproto::ConnectionExt;
        conn.delete_property(root, atoms._NET_ACTIVE_WINDOW)?;
        Ok(())
    }

    fn set_client_list<C>(
        &self,
        conn: &C,
        root: Window,
        atoms: &Atoms,
        list: &[Window],
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        C: Connection,
    {
        conn.change_property32(
            PropMode::REPLACE,
            root,
            atoms._NET_CLIENT_LIST,
            AtomEnum::WINDOW,
            list,
        )?;
        Ok(())
    }

    fn set_client_list_stacking<C>(
        &self,
        conn: &C,
        root: Window,
        atoms: &Atoms,
        list: &[Window],
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        C: Connection,
    {
        conn.change_property32(
            PropMode::REPLACE,
            root,
            atoms._NET_CLIENT_LIST_STACKING,
            AtomEnum::WINDOW,
            list,
        )?;
        Ok(())
    }
}
