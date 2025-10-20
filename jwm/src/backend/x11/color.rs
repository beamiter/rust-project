// src/backend/x11/color.rs
use crate::backend::traits::{ColorAllocator, Pixel};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::Colormap;

pub struct X11ColorAllocator<C: Connection> {
    conn: Arc<C>,
    colormap: Colormap,
}

impl<C: Connection> X11ColorAllocator<C> {
    pub fn new(conn: Arc<C>, colormap: Colormap) -> Self {
        Self { conn, colormap }
    }
}

impl<C: Connection + Send + Sync + 'static> ColorAllocator for X11ColorAllocator<C> {
    fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, Box<dyn std::error::Error>> {
        use x11rb::protocol::xproto::ConnectionExt;
        let reply = (*self.conn)
            .alloc_color(self.colormap, (r as u16) << 8, (g as u16) << 8, (b as u16) << 8)?
            .reply()?;
        Ok(Pixel(reply.pixel))
    }

    fn free_pixels(&mut self, pixels: &[Pixel]) -> Result<(), Box<dyn std::error::Error>> {
        if pixels.is_empty() {
            return Ok(());
        }
        use x11rb::protocol::xproto::ConnectionExt;
        let raw: Vec<u32> = pixels.iter().map(|p| p.0).collect();
        (*self.conn).free_colors(self.colormap, 0, &raw)?;
        Ok(())
    }
}
