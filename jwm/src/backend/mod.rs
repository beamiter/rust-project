// src/backend/mod.rs

pub mod api;
pub mod common_define;
pub mod error;

// Shared Smithay Wayland compositor state (used by udev/KMS and windowed X11 backend).
#[cfg(any(
	feature = "backend-udev",
	feature = "backend-wayland-x11",
	feature = "backend-wayland-winit"
))]
pub mod wayland;

// Shared xkbcommon-based key mapping used by Smithay-backed backends.
#[cfg(any(
	feature = "backend-udev",
	feature = "backend-wayland-x11",
	feature = "backend-wayland-winit"
))]
pub mod wayland_key_ops;

// Shared dummy ops used by Smithay-backed backends.
#[cfg(any(
	feature = "backend-udev",
	feature = "backend-wayland-x11",
	feature = "backend-wayland-winit"
))]
pub mod wayland_dummy_ops;

#[cfg(feature = "backend-x11")]
pub mod x11;

#[cfg(feature = "backend-udev")]
#[path = "wayland_udev/mod.rs"]
pub mod wayland_udev;

// Backwards-compat alias for older module paths.
#[cfg(feature = "backend-udev")]
pub mod udev {
	pub mod backend {
		pub use crate::backend::wayland_udev::backend::*;
	}
}

#[cfg(feature = "backend-wayland-x11")]
pub mod wayland_x11;

#[cfg(feature = "backend-wayland-winit")]
pub mod wayland_winit;
