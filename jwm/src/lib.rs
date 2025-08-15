pub mod config;
pub mod drw;
pub mod jwm;
pub mod miscellaneous;
pub mod terminal_prober;
pub mod xproto;
pub mod xcb_conn;

pub use jwm::Jwm;

// Xnest and Xephyr is all you need!
// Xnest:
// Xnest :2 -geometry 1024x768 &
// export DISPLAY=:2
// exec jwm

// Xephyr:
// Xephyr :2 -screen 1024x768 &
// DISPLAY=:2 jwm

// For dual monitor:
// xrandr --output HDMI-1 --rotate normal --left-of eDP-1 --auto &
