// src/backend/x11/cursor.rs
use std::collections::HashMap;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use crate::backend::traits::{CursorProvider, CursorHandle, StdCursorKind};
use crate::xcb_util::StandardCursor as X11StdCursor; // 复用现有的枚举和 create()

pub struct X11CursorProvider<C: Connection> {
    conn: C,
    cursor_font: Font,
    cache: HashMap<StdCursorKind, Cursor>,
}

impl<C: Connection + Clone> X11CursorProvider<C> {
    pub fn new(conn: C) -> Result<Self, Box<dyn std::error::Error>> {
        let font = conn.generate_id()?;
        conn.open_font(font, b"cursor")?;
        Ok(Self { conn, cursor_font: font, cache: HashMap::new() })
    }

    fn map_kind(kind: StdCursorKind) -> X11StdCursor {
        match kind {
            StdCursorKind::LeftPtr => X11StdCursor::LeftPtr,
            StdCursorKind::Hand => X11StdCursor::Hand1,
            StdCursorKind::XTerm => X11StdCursor::Xterm,
            StdCursorKind::Watch => X11StdCursor::Watch,
            StdCursorKind::Crosshair => X11StdCursor::Crosshair,
            StdCursorKind::Fleur => X11StdCursor::Fleur,
            StdCursorKind::HDoubleArrow => X11StdCursor::SbHDoubleArrow,
            StdCursorKind::VDoubleArrow => X11StdCursor::SbVDoubleArrow,
            StdCursorKind::TopLeftCorner => X11StdCursor::TopLeftCorner,
            StdCursorKind::TopRightCorner => X11StdCursor::TopRightCorner,
            StdCursorKind::BottomLeftCorner => X11StdCursor::BottomLeftCorner,
            StdCursorKind::BottomRightCorner => X11StdCursor::BottomRightCorner,
            StdCursorKind::Sizing => X11StdCursor::Sizing,
        }
    }
}

impl<C: Connection + Clone + Send + 'static> CursorProvider for X11CursorProvider<C> {
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 预创建常用光标
        for kind in [
            StdCursorKind::LeftPtr,
            StdCursorKind::Hand,
            StdCursorKind::XTerm,
            StdCursorKind::Watch,
            StdCursorKind::Crosshair,
            StdCursorKind::Fleur,
            StdCursorKind::HDoubleArrow,
            StdCursorKind::VDoubleArrow,
            StdCursorKind::TopLeftCorner,
            StdCursorKind::TopRightCorner,
            StdCursorKind::BottomLeftCorner,
            StdCursorKind::BottomRightCorner,
            StdCursorKind::Sizing,
        ] {
            let _ = self.get(kind)?;
        }
        Ok(())
    }

    fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>> {
        if let Some(&c) = self.cache.get(&kind) {
            return Ok(CursorHandle(c as u64));
        }
        let x11_cursor = Self::map_kind(kind);
        let cursor = x11_cursor.create(&self.conn, self.cursor_font)?;
        self.cache.insert(kind, cursor);
        Ok(CursorHandle(cursor as u64))
    }

    fn apply(&mut self, window_id: u64, kind: StdCursorKind) -> Result<(), Box<dyn std::error::Error>> {
        let c = match self.get(kind) {
            Ok(h) => h.0 as u32,
            Err(e) => return Err(e),
        };
        self.conn.change_window_attributes(window_id as u32, &ChangeWindowAttributesAux::new().cursor(c))?;
        Ok(())
    }

    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for &cursor in self.cache.values() {
            let _ = self.conn.free_cursor(cursor);
        }
        let _ = self.conn.close_font(self.cursor_font);
        Ok(())
    }
}
