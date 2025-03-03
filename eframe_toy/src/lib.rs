#![warn(clippy::all, rust_2018_idioms)]

mod ssh_commander;
pub use ssh_commander::SSHCommander;

mod image_concatenator;
pub use image_concatenator::ImageProcessor;

mod screen_selection;
pub use screen_selection::ScreenSelection;

mod deprecated;
