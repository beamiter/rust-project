// src/xcb_conn.rs
use xcb::{ConnError, Connection, x};

use crate::Jwm;

#[derive(Debug)]
pub struct AtomCache {
    pub wm_protocols: x::Atom,
    pub wm_delete_window: x::Atom,
    pub wm_state: x::Atom,
    pub wm_take_focus: x::Atom,
    pub net_active_window: x::Atom,
    pub net_supported: x::Atom,
    pub net_wm_name: x::Atom,
    pub net_wm_state: x::Atom,
    pub net_wm_check: x::Atom,
    pub net_wm_fullscreen: x::Atom,
    pub net_wm_window_type: x::Atom,
    pub net_wm_window_type_dialog: x::Atom,
    pub net_client_list: x::Atom,
    pub net_client_info: x::Atom,
    pub utf8_string: x::Atom,
}

impl AtomCache {
    pub fn new(conn: &Connection) -> Self {
        Self {
            wm_protocols: Jwm::intern_atom(conn, "WM_PROTOCOLS"),
            wm_delete_window: Jwm::intern_atom(conn, "WM_DELETE_WINDOW"),
            wm_state: Jwm::intern_atom(conn, "WM_STATE"),
            wm_take_focus: Jwm::intern_atom(conn, "WM_TAKE_FOCUS"),

            net_active_window: Jwm::intern_atom(conn, "_NET_ACTIVE_WINDOW"),
            net_supported: Jwm::intern_atom(conn, "_NET_SUPPORTED"),
            net_wm_name: Jwm::intern_atom(conn, "_NET_WM_NAME"),
            net_wm_state: Jwm::intern_atom(conn, "_NET_WM_STATE"),
            net_wm_check: Jwm::intern_atom(conn, "_NET_SUPPORTING_WM_CHECK"),
            net_wm_fullscreen: Jwm::intern_atom(conn, "_NET_WM_STATE_FULLSCREEN"),
            net_wm_window_type: Jwm::intern_atom(conn, "_NET_WM_WINDOW_TYPE"),
            net_wm_window_type_dialog: Jwm::intern_atom(conn, "_NET_WM_WINDOW_TYPE_DIALOG"),
            net_client_list: Jwm::intern_atom(conn, "_NET_CLIENT_LIST"),
            net_client_info: Jwm::intern_atom(conn, "_NET_CLIENT_INFO"),
            utf8_string: Jwm::intern_atom(conn, "UTF8_STRING"),
        }
    }
}

pub struct XcbConnection {
    conn: Connection,
    screen_num: i32,
}

impl XcbConnection {
    pub fn connect() -> Result<Self, xcb::Error> {
        let (conn, screen_num) = Connection::connect(None)?;
        Ok(Self { conn, screen_num })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn screen_num(&self) -> i32 {
        self.screen_num
    }

    pub fn flush(&self) -> Result<(), ConnError> {
        self.conn.flush()
    }
}
