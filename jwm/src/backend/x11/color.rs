// src/backend/x11/color.rs
use crate::backend::api::ColorAllocator;
use crate::backend::common_define::{ArgbColor, ColorScheme, Pixel, SchemeType};
use std::collections::HashMap;
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::Colormap;

pub struct X11ColorAllocator<C: Connection> {
    conn: Arc<C>,
    colormap: Colormap,

    pixel_cache: HashMap<u32, Pixel>,
    schemes: HashMap<SchemeType, ColorScheme>,
}

impl<C: Connection> X11ColorAllocator<C> {
    pub fn new(conn: Arc<C>, colormap: Colormap) -> Self {
        Self {
            conn,
            colormap,
            pixel_cache: HashMap::new(),
            schemes: HashMap::new(),
        }
    }
}

impl<C: Connection + Send + Sync + 'static> ColorAllocator for X11ColorAllocator<C> {
    fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, Box<dyn std::error::Error>> {
        use x11rb::protocol::xproto::ConnectionExt;
        let reply = (*self.conn)
            .alloc_color(
                self.colormap,
                (r as u16) << 8,
                (g as u16) << 8,
                (b as u16) << 8,
            )?
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

    fn set_scheme(&mut self, t: SchemeType, s: ColorScheme) {
        self.schemes.insert(t, s);
    }
    fn get_scheme(&self, t: SchemeType) -> Option<ColorScheme> {
        self.schemes.get(&t).cloned()
    }

    fn ensure_pixel(&mut self, color: ArgbColor) -> Result<Pixel, Box<dyn std::error::Error>> {
        if let Some(p) = self.pixel_cache.get(&color.value).copied() {
            return Ok(p);
        }
        let (_, r, g, b) = color.components();
        let pix = self.alloc_rgb(r, g, b)?;
        self.pixel_cache.insert(color.value, pix);
        Ok(pix)
    }

    fn get_pixel_cached(&self, color: ArgbColor) -> Option<Pixel> {
        self.pixel_cache.get(&color.value).copied()
    }

    fn allocate_schemes_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut colors: Vec<ArgbColor> = Vec::new();
        for s in self.schemes.values() {
            colors.push(s.fg);
            colors.push(s.bg);
            colors.push(s.border);
        }
        colors.sort_by_key(|c| c.value);
        colors.dedup();
        for c in colors {
            let _ = self.ensure_pixel(c)?;
        }
        Ok(())
    }

    fn free_all_theme_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.pixel_cache.is_empty() {
            return Ok(());
        }
        let pixels: Vec<Pixel> = self.pixel_cache.values().copied().collect();
        self.free_pixels(&pixels)?;
        self.pixel_cache.clear();
        Ok(())
    }
}
