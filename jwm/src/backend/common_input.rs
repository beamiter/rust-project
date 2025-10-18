// src/backend/common_input.rs
use bitflags::bitflags;

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
