// #![warn(dead_code, unused, unreachable_pub)]
// #![warn(clippy::all, clippy::pedantic)]

pub mod backend;
pub mod config;
pub mod jwm;
pub mod miscellaneous;
pub mod terminal_prober;

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
