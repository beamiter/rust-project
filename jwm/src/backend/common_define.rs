// src/backend/common_define.rs
use bitflags::bitflags;

// 通用后端窗口ID（X11: Window; Wayland: 自定义句柄）
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pixel(pub u32); // 通用像素句柄（X11=像素ID，Wayland=ABGR/RGBA值或纹理句柄）

// 通用光标类型与接口
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CursorHandle(pub u64); // X11=Cursor id, Wayland=内部id

// 标准光标的语义标识（与 xcb_util::StandardCursor 对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StdCursorKind {
    LeftPtr,
    Hand,
    XTerm,
    Watch,
    Crosshair,
    Fleur,
    HDoubleArrow,
    VDoubleArrow,
    TopLeftCorner,
    TopRightCorner,
    BottomLeftCorner,
    BottomRightCorner,
    Sizing,
}

/// 与后端无关的 KeySym（使用 xkbcommon 的 keysym 值，Wayland/X11 通用）
pub type KeySym = u32;

/// 与后端无关的鼠标按钮
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Other(u8),
}

impl MouseButton {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => MouseButton::Left,
            2 => MouseButton::Middle,
            3 => MouseButton::Right,
            x => MouseButton::Other(x),
        }
    }
    pub fn to_u8(self) -> u8 {
        match self {
            MouseButton::Left => 1,
            MouseButton::Middle => 2,
            MouseButton::Right => 3,
            MouseButton::Other(x) => x,
        }
    }
}

bitflags! {
    /// 与后端无关的修饰键集合
    #[derive(Debug, Clone, PartialEq, Eq, Copy)]
    pub struct Mods: u16 {
        const NONE    = 0;
        const SHIFT   = 1 << 0;
        const CONTROL = 1 << 1;
        const ALT     = 1 << 2;  // 通常对应 Mod1
        const MOD2    = 1 << 3;
        const MOD3    = 1 << 4;
        const SUPER   = 1 << 5;  // 通常对应 Mod4
        const MOD5    = 1 << 6;
        const CAPS    = 1 << 7;
        const NUMLOCK = 1 << 8;
    }
}

/// 常用 keysym 常量（xkbcommon）
pub mod keys {
    pub use xkbcommon::xkb::keysyms::*;
    pub use xkbcommon::xkb::*;
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
