// src/backend/wayland/color.rs
use crate::backend::api::ColorAllocator;
use crate::backend::common_define::{ArgbColor, ColorScheme, Pixel, SchemeType};

use std::collections::HashMap;

pub struct WaylandColorAllocator {
    pixel_cache: HashMap<u32, Pixel>,
    schemes: HashMap<SchemeType, ColorScheme>,
}
impl WaylandColorAllocator {
    pub fn new() -> Self {
        Self {
            pixel_cache: HashMap::new(),
            schemes: HashMap::new(),
        }
    }
}
impl ColorAllocator for WaylandColorAllocator {
    fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, Box<dyn std::error::Error>> {
        let v = 0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        Ok(Pixel(v))
    }
    fn free_pixels(&mut self, _pixels: &[Pixel]) -> Result<(), Box<dyn std::error::Error>> {
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
        let (r, g, b) = color.rgb();
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
        self.pixel_cache.clear();
        Ok(())
    }
}
