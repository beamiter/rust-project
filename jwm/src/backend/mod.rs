// src/backend/mod.rs

pub mod common_input;
#[cfg(feature = "backend-x11")]
pub mod x11;

/// 仅抽象 EWMH 常用接口（PR6），后续 Wayland 可按需实现或桥接
pub trait Ewmh {
    type Window;
    type AtomSet;

    fn set_active_window<C>(&self, conn: &C, root: Self::Window, atoms: &Self::AtomSet, win: Self::Window) -> Result<(), Box<dyn std::error::Error>>;
    fn clear_active_window<C>(&self, conn: &C, root: Self::Window, atoms: &Self::AtomSet) -> Result<(), Box<dyn std::error::Error>>;
    fn set_client_list<C>(&self, conn: &C, root: Self::Window, atoms: &Self::AtomSet, list: &[Self::Window]) -> Result<(), Box<dyn std::error::Error>>;
    fn set_client_list_stacking<C>(&self, conn: &C, root: Self::Window, atoms: &Self::AtomSet, list: &[Self::Window]) -> Result<(), Box<dyn std::error::Error>>;
}
