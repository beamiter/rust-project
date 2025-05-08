#![warn(clippy::all)]

mod ssh_commander;
pub use ssh_commander::SSHCommander;

mod image_concatenator;
pub use image_concatenator::ImageProcessor;

mod screen_selection;
pub use screen_selection::ScreenSelection;

mod deprecated;

mod filer;
pub use filer::configure_text_styles;
pub use filer::Filer;

mod correlation_stitcher;

mod direct_stitcher;

mod image_viewer;
pub use image_viewer::ImageViewerApp;
