// src/xcb_util.rs

use crate::backend::traits::{ColorAllocator, Pixel};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ThemeManager {
    schemes: HashMap<SchemeType, ColorScheme>,
    pixel_cache: HashMap<u32, Pixel>,
}

impl ThemeManager {
    pub fn new() -> Self {
        Self {
            schemes: HashMap::new(),
            pixel_cache: HashMap::new(),
        }
    }

    pub fn set_scheme(&mut self, t: SchemeType, s: ColorScheme) {
        self.schemes.insert(t, s);
    }
    pub fn get_scheme(&self, t: SchemeType) -> Option<&ColorScheme> {
        self.schemes.get(&t)
    }
    pub fn get_fg(&self, t: SchemeType) -> Option<ArgbColor> {
        self.get_scheme(t).map(|s| s.fg)
    }
    pub fn get_bg(&self, t: SchemeType) -> Option<ArgbColor> {
        self.get_scheme(t).map(|s| s.bg)
    }
    pub fn get_border(&self, t: SchemeType) -> Option<ArgbColor> {
        self.get_scheme(t).map(|s| s.border)
    }

    pub fn get_pixel(&self, color: ArgbColor) -> Option<Pixel> {
        self.pixel_cache.get(&color.value).copied()
    }

    pub fn allocate_pixels<A: ColorAllocator>(
        &mut self,
        allocator: &mut A,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut colors = Vec::new();
        for s in self.schemes.values() {
            colors.push(s.fg);
            colors.push(s.bg);
            colors.push(s.border);
        }
        colors.sort_by_key(|c| c.value);
        colors.dedup();
        for c in colors {
            if self.pixel_cache.contains_key(&c.value) {
                continue;
            }
            let (_, r, g, b) = c.components();
            let pix = allocator.alloc_rgb(r, g, b)?;
            self.pixel_cache.insert(c.value, pix);
        }
        Ok(())
    }

    pub fn free_pixels<A: ColorAllocator>(
        &mut self,
        allocator: &mut A,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pixels: Vec<Pixel> = self.pixel_cache.values().copied().collect();
        if !pixels.is_empty() {
            allocator.free_pixels(&pixels)?;
            self.pixel_cache.clear();
        }
        Ok(())
    }
}

// 从配置创建
impl ThemeManager {
    pub fn create_from_config<A: ColorAllocator>(
        mut allocator: A,
    ) -> Result<(Self, A), Box<dyn std::error::Error>> {
        let mut theme = Self::new();
        let colors = crate::config::CONFIG.colors();

        let normal = ColorScheme::new(
            ArgbColor::from_hex(&colors.dark_sea_green1, colors.opaque)?,
            ArgbColor::from_hex(&colors.light_sky_blue1, colors.opaque)?,
            ArgbColor::from_hex(&colors.light_sky_blue1, colors.opaque)?,
        );
        let selected = ColorScheme::new(
            ArgbColor::from_hex(&colors.dark_sea_green2, colors.opaque)?,
            ArgbColor::from_hex(&colors.pale_turquoise1, colors.opaque)?,
            ArgbColor::from_hex(&colors.cyan, colors.opaque)?,
        );
        theme.set_scheme(SchemeType::Norm, normal);
        theme.set_scheme(SchemeType::Sel, selected);

        theme.allocate_pixels(&mut allocator)?;
        Ok((theme, allocator))
    }
}

/// ARGB颜色结构，支持Alpha通道
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArgbColor {
    pub value: u32, // ARGB格式: 0xAARRGGBB
}

impl ArgbColor {
    /// 从ARGB分量创建颜色
    pub fn new(alpha: u8, red: u8, green: u8, blue: u8) -> Self {
        let value =
            ((alpha as u32) << 24) | ((red as u32) << 16) | ((green as u32) << 8) | (blue as u32);
        Self { value }
    }

    /// 从RGB创建不透明颜色
    pub fn from_rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::new(255, red, green, blue)
    }

    /// 从十六进制字符串创建颜色
    pub fn from_hex(hex: &str, alpha: u8) -> Result<Self, Box<dyn std::error::Error>> {
        let (r, g, b) = parse_hex_color(hex)?;
        Ok(Self::new(alpha, r, g, b))
    }

    /// 提取ARGB分量
    pub fn components(&self) -> (u8, u8, u8, u8) {
        let alpha = (self.value >> 24) as u8;
        let red = (self.value >> 16) as u8;
        let green = (self.value >> 8) as u8;
        let blue = self.value as u8;
        (alpha, red, green, blue)
    }

    /// 获取RGB分量（不包含alpha）
    pub fn rgb(&self) -> (u8, u8, u8) {
        let (_, r, g, b) = self.components();
        (r, g, b)
    }

    /// 获取alpha值
    pub fn alpha(&self) -> u8 {
        (self.value >> 24) as u8
    }

    /// 设置alpha值
    pub fn with_alpha(&self, alpha: u8) -> Self {
        let (_, r, g, b) = self.components();
        Self::new(alpha, r, g, b)
    }

    /// 转换为浮点RGBA（用于Cairo等）
    pub fn to_rgba_f64(&self) -> (f64, f64, f64, f64) {
        let (a, r, g, b) = self.components();
        (
            r as f64 / 255.0,
            g as f64 / 255.0,
            b as f64 / 255.0,
            a as f64 / 255.0,
        )
    }

    /// 获取X11像素值（去除alpha）
    pub fn to_x11_pixel(&self) -> u32 {
        self.value & 0x00FFFFFF
    }
}

/// 颜色方案
#[derive(Debug, Clone)]
pub struct ColorScheme {
    pub fg: ArgbColor,     // 前景色
    pub bg: ArgbColor,     // 背景色
    pub border: ArgbColor, // 边框色
}

impl ColorScheme {
    /// 创建新的颜色方案
    pub fn new(fg: ArgbColor, bg: ArgbColor, border: ArgbColor) -> Self {
        Self { fg, bg, border }
    }

    /// 从十六进制字符串创建颜色方案
    pub fn from_hex(
        fg_hex: &str,
        bg_hex: &str,
        border_hex: &str,
        alpha: u8,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::new(
            ArgbColor::from_hex(fg_hex, alpha)?,
            ArgbColor::from_hex(bg_hex, alpha)?,
            ArgbColor::from_hex(border_hex, alpha)?,
        ))
    }

    /// 获取前景色
    pub fn foreground(&self) -> ArgbColor {
        self.fg
    }

    /// 获取背景色
    pub fn background(&self) -> ArgbColor {
        self.bg
    }

    /// 获取边框色
    pub fn border_color(&self) -> ArgbColor {
        self.border
    }
}

/// 方案类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemeType {
    Norm = 0,    // 普通状态
    Sel = 1,     // 选中状态
    Urgent = 2,  // 紧急状态
    Warning = 3, // 警告状态
    Error = 4,   // 错误状态
}

/// 辅助函数
fn parse_hex_color(hex: &str) -> Result<(u8, u8, u8), Box<dyn std::error::Error>> {
    let hex = if hex.starts_with('#') { &hex[1..] } else { hex };

    match hex.len() {
        3 => {
            // #RGB -> #RRGGBB
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16)?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16)?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16)?;
            Ok((r, g, b))
        }
        6 => {
            // #RRGGBB
            let r = u8::from_str_radix(&hex[0..2], 16)?;
            let g = u8::from_str_radix(&hex[2..4], 16)?;
            let b = u8::from_str_radix(&hex[4..6], 16)?;
            Ok((r, g, b))
        }
        _ => Err("Invalid hex color format".into()),
    }
}

/// 预定义颜色常量
impl ArgbColor {
    pub const TRANSPARENT: ArgbColor = ArgbColor { value: 0x00000000 };
    pub const BLACK: ArgbColor = ArgbColor { value: 0xFF000000 };
    pub const WHITE: ArgbColor = ArgbColor { value: 0xFFFFFFFF };
    pub const RED: ArgbColor = ArgbColor { value: 0xFFFF0000 };
    pub const GREEN: ArgbColor = ArgbColor { value: 0xFF00FF00 };
    pub const BLUE: ArgbColor = ArgbColor { value: 0xFF0000FF };
    pub const YELLOW: ArgbColor = ArgbColor { value: 0xFFFFFF00 };
    pub const CYAN: ArgbColor = ArgbColor { value: 0xFF00FFFF };
    pub const MAGENTA: ArgbColor = ArgbColor { value: 0xFFFF00FF };
}
