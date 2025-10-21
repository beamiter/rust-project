// src/backend/mod.rs

pub mod api;
pub mod common_input;
pub mod traits;

#[cfg(feature = "backend-x11")]
pub mod x11;
