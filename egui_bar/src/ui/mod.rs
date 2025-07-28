//! UI components module

pub mod system_info;
pub mod controller_info;

pub mod volume_control;
pub mod debug_display;

pub mod workspace_info;

pub use system_info::SystemInfoPanel;

pub use volume_control::VolumeControlWindow;
pub use debug_display::DebugDisplayWindow;

pub use workspace_info::WorkspacePanel;
