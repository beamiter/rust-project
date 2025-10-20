// src/backend/mod.rs

pub mod common_input;
pub mod traits;
#[cfg(feature = "backend-x11")]
pub mod x11;

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
