// src/backend/traits.rs
use std::fmt::Debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pixel(pub u32); // 通用像素句柄（X11=像素ID，Wayland=ABGR/RGBA值或纹理句柄）

// 通用颜色分配接口：ThemeManager 使用这个接口，不关心后端细节
pub trait ColorAllocator: Send {
    // 分配一个 RGB 颜色，返回通用 Pixel 句柄
    fn alloc_rgb(&mut self, r: u8, g: u8, b: u8) -> Result<Pixel, Box<dyn std::error::Error>>;
    // 释放多个像素（可选）
    fn free_pixels(&mut self, pixels: &[Pixel]) -> Result<(), Box<dyn std::error::Error>>;
}

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
    // 可继续扩展...
}

pub trait CursorProvider {
    // 预创建常用光标
    fn preload_common(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    // 获取（或创建）某种标准光标
    fn get(&mut self, kind: StdCursorKind) -> Result<CursorHandle, Box<dyn std::error::Error>>;
    // 应用光标到窗口
    // 这里用通用 WindowId: u64 表示，X11=Window，Wayland=surface 或 seat/cursor surface 逻辑自行映射
    fn apply(
        &mut self,
        window_id: u64,
        kind: StdCursorKind,
    ) -> Result<(), Box<dyn std::error::Error>>;
    // 清理资源
    fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait Ewmh {
    type Window;
    type AtomSet;
    type Conn;

    fn set_active_window(
        &self,
        conn: &Self::Conn,
        root: Self::Window,
        atoms: &Self::AtomSet,
        win: Self::Window,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn clear_active_window(
        &self,
        conn: &Self::Conn,
        root: Self::Window,
        atoms: &Self::AtomSet,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn set_client_list(
        &self,
        conn: &Self::Conn,
        root: Self::Window,
        atoms: &Self::AtomSet,
        list: &[Self::Window],
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn set_client_list_stacking(
        &self,
        conn: &Self::Conn,
        root: Self::Window,
        atoms: &Self::AtomSet,
        list: &[Self::Window],
    ) -> Result<(), Box<dyn std::error::Error>>;
}
