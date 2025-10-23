// src/backend/wayland/color.rs
use std::collections::HashMap;

use crate::backend::api::ColorAllocator;
use crate::backend::common_define::{ArgbColor, ColorScheme, Pixel, SchemeType};

pub struct WlColorAllocator {
    schemes: HashMap<SchemeType, ColorScheme>,
    cache: HashMap<u32, Pixel>, // key = ARGB value
}

impl WlColorAllocator {
    pub fn new() -> Self {
        Self {
            schemes: HashMap::new(),
            cache: HashMap::new(),
        }
    }
}

impl ColorAllocator for WlColorAllocator {
    fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, Box<dyn std::error::Error>> {
        // Wayland: 无需分配，直接返回 ARGB
        let argb = 0xFF000000u32 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        Ok(Pixel(argb))
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
        if let Some(p) = self.cache.get(&color.value).copied() {
            return Ok(p);
        }
        let (_, r, g, b) = color.components();
        let p = self.alloc_rgb(r, g, b)?;
        self.cache.insert(color.value, p);
        Ok(p)
    }

    fn get_pixel_cached(&self, color: ArgbColor) -> Option<Pixel> {
        self.cache.get(&color.value).copied()
    }

    fn allocate_schemes_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut colors = Vec::new();
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
        self.cache.clear();
        Ok(())
    }
}
