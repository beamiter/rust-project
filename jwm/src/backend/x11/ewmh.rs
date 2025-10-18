// src/backend/x11/ewmh.rs
use x11rb::protocol::xproto::{AtomEnum, PropMode, Window};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt;

use crate::backend::Ewmh;
use crate::xcb_util::Atoms;

/// X11 上的 EWMH 实现
pub struct X11Ewmh;

impl Ewmh for X11Ewmh {
    type Window = Window;
    type AtomSet = Atoms;
    type Conn = RustConnection;

    fn set_active_window(
        &self,
        conn: &Self::Conn,
        root: Self::Window,
        atoms: &Self::AtomSet,
        win: Self::Window,
    ) -> Result<(), Box<dyn std::error::Error>> {
        conn.change_property32(
            PropMode::REPLACE,
            root,
            atoms._NET_ACTIVE_WINDOW,
            AtomEnum::WINDOW,
            &[win],
        )?;
        Ok(())
    }

    fn clear_active_window(
        &self,
        conn: &Self::Conn,
        root: Self::Window,
        atoms: &Self::AtomSet,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use x11rb::protocol::xproto::ConnectionExt;
        conn.delete_property(root, atoms._NET_ACTIVE_WINDOW)?;
        Ok(())
    }

    fn set_client_list(
        &self,
        conn: &Self::Conn,
        root: Self::Window,
        atoms: &Self::AtomSet,
        list: &[Self::Window],
    ) -> Result<(), Box<dyn std::error::Error>> {
        conn.change_property32(
            PropMode::REPLACE,
            root,
            atoms._NET_CLIENT_LIST,
            AtomEnum::WINDOW,
            list,
        )?;
        Ok(())
    }

    fn set_client_list_stacking(
        &self,
        conn: &Self::Conn,
        root: Self::Window,
        atoms: &Self::AtomSet,
        list: &[Self::Window],
    ) -> Result<(), Box<dyn std::error::Error>> {
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
