// Reuse the existing compositor/WM glue used by the tty-udev backend.
//
// We use `include!` instead of duplicating ~1000 lines to keep the two backends in sync.
// The included file lives in the same crate and is maintained as part of this workspace.
include!("../udev/wayland.rs");
