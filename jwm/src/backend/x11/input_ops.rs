// src/backend/x11/input_ops.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

use crate::backend::api::AllowMode;
use crate::backend::api::InputOps as InputOpsTrait;
use crate::backend::common_define::StdCursorKind;
use crate::backend::common_define::WindowId;
use crate::backend::x11::WindowHandleExt;

pub struct X11InputOps<C: Connection> {
    conn: Arc<C>,
    root: Window,
}

impl<C: Connection> Clone for X11InputOps<C> {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            root: self.root,
        }
    }
}

impl<C: Connection + Send + Sync + 'static> X11InputOps<C> {
    pub fn new(conn: Arc<C>, root: Window) -> Self {
        Self { conn, root }
    }

    fn map_allow_mode(mode: AllowMode) -> Allow {
        match mode {
            AllowMode::AsyncPointer => Allow::ASYNC_POINTER,
            AllowMode::ReplayPointer => Allow::REPLAY_POINTER,
            AllowMode::SyncPointer => Allow::SYNC_POINTER,
            AllowMode::AsyncKeyboard => Allow::ASYNC_KEYBOARD,
            AllowMode::SyncKeyboard => Allow::SYNC_KEYBOARD,
            AllowMode::ReplayKeyboard => Allow::REPLAY_KEYBOARD,
            AllowMode::AsyncBoth => Allow::ASYNC_BOTH,
            AllowMode::SyncBoth => Allow::SYNC_BOTH,
        }
    }

    pub fn allow_events_raw(
        &self,
        mode: Allow,
        time: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.allow_events(mode, time)?;
        Ok(())
    }

    pub fn query_pointer(&self) -> Result<QueryPointerReply, Box<dyn std::error::Error>> {
        Ok(self.conn.query_pointer(self.root)?.reply()?)
    }

    pub fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.flush()?;
        Ok(())
    }
}

impl<C: Connection + Send + Sync + 'static> InputOpsTrait for X11InputOps<C> {
    fn get_pointer_position(&self) -> Result<(f64, f64), Box<dyn std::error::Error>> {
        let reply = self.query_pointer()?;
        // X11 是整数坐标，转换为 f64
        Ok((reply.root_x as f64, reply.root_y as f64))
    }

    fn grab_pointer(
        &self,
        _mask: u32,
        cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let cursor_id = cursor.map(|c| c as u32).unwrap_or(0);
        // 通常 Grab Pointer 需要监听 ButtonRelease 和 Motion
        let mask = EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;

        let reply = self
            .conn
            .grab_pointer(
                false,
                self.root,
                mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32, // None confine_to
                cursor_id,
                0u32, // Current time
            )?
            .reply()?;

        Ok(reply.status == GrabStatus::SUCCESS)
    }

    fn set_cursor(&self, _kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.ungrab_pointer(0u32)?;
        Ok(())
    }

    fn allow_events(&self, mode: AllowMode, time: u32) -> Result<(), Box<dyn std::error::Error>> {
        let allow = Self::map_allow_mode(mode);
        self.allow_events_raw(allow, time)
    }

    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>> {
        let reply = self.query_pointer()?;
        Ok((
            reply.root_x as i32,
            reply.root_y as i32,
            reply.mask.bits() as u16,
            0,
        ))
    }

    fn warp_pointer_to_window(
        &self,
        win: WindowId,
        x: i16,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let w = win.to_x11_id()?;
        self.conn.warp_pointer(0u32, w, 0, 0, 0, 0, x, y)?;
        Ok(())
    }
}
