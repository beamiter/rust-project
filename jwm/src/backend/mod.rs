// src/backend/mod.rs

pub mod common_input;
pub mod traits;
#[cfg(feature = "backend-x11")]
pub mod x11;

pub use traits::Ewmh;
