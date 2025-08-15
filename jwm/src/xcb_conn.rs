// src/xcb_conn.rs
use xcb::{ConnError, Connection, Xid};

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
