// src/backend/mod.rs

pub mod api;
pub mod common_define;
pub mod error;

#[cfg(feature = "backend-x11")]
pub mod x11;

#[cfg(feature = "backend-udev")]
pub mod udev;
