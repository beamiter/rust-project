// Shared Smithay Wayland compositor state (used by udev/KMS and windowed X11 backend).
//
// Keep this as a small shim so the real implementation can live under `wayland_udev/`
// while preserving the public path `crate::backend::wayland::state`.

#[path = "wayland_udev/state.rs"]
pub mod state;
