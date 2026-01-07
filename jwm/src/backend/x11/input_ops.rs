// src/backend/x11/input_ops.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

use crate::backend::api::AllowMode;
use crate::backend::api::{InputOps as InputOpsTrait, WindowId};
use crate::backend::common_define::StdCursorKind;
use crate::backend::x11::WindowHandleExt;
use crate::backend::x11::adapter::event_mask_from_generic;

pub struct X11InputOps<C: Connection> {
    conn: Arc<C>,
    root: Window,
}

impl<C: Connection> Clone for X11InputOps<C> {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(), // 这里只是增加 Arc 的引用计数，非常廉价
            root: self.root,         // Window 本质是 u32/u64，是 Copy 的
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

    fn grab_pointer_raw(
        &self,
        event_mask: EventMask,
        cursor: Option<Cursor>,
    ) -> Result<GrabStatus, Box<dyn std::error::Error>> {
        let cursor_id = cursor.unwrap_or(0);
        let reply = self
            .conn
            .grab_pointer(
                false,
                self.root,
                event_mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                cursor_id,
                0u32,
            )?
            .reply()?;
        Ok(reply.status)
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
    fn set_cursor(&self, _kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn grab_pointer(
        &self,
        mask_bits: u32,
        cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let x_mask = event_mask_from_generic(mask_bits);
        let cursor_id = cursor.map(|c| c as u32);
        let status = self.grab_pointer_raw(x_mask, cursor_id)?;
        Ok(status == GrabStatus::SUCCESS)
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
