use crate::backend::{
    api::{ColorAllocator, Pixel},
    common_define::{ArgbColor, ColorScheme, SchemeType},
};

pub struct WaylandColorAllocator {}

impl WaylandColorAllocator {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl ColorAllocator for WaylandColorAllocator {
    fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, Box<dyn std::error::Error>> {
        Ok(Pixel(0))
    }

    fn free_pixels(&mut self, pixels: &[Pixel]) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn set_scheme(&mut self, t: SchemeType, s: ColorScheme) {}
    fn get_scheme(&self, t: SchemeType) -> Option<ColorScheme> {
        None
    }

    fn ensure_pixel(&mut self, color: ArgbColor) -> Result<Pixel, Box<dyn std::error::Error>> {
        Ok(Pixel(0))
    }

    fn get_pixel_cached(&self, color: ArgbColor) -> Option<Pixel> {
        None
    }

    fn allocate_schemes_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn free_all_theme_pixels(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn get_border_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        Ok(Pixel(0))
    }

    fn get_fg_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        Ok(Pixel(0))
    }

    fn get_bg_pixel_of(&mut self, t: SchemeType) -> Result<Pixel, Box<dyn std::error::Error>> {
        Ok(Pixel(0))
    }
}
