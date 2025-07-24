//! Application constants and configuration values

pub use egui::Color32;

/// UI constants
pub mod ui {
    pub const DEFAULT_FONT_SIZE: f32 = 18.0;
    pub const DEFAULT_SCALE_FACTOR: f32 = 1.0;
}

/// Update intervals in milliseconds
pub mod intervals {
    pub const SYSTEM_UPDATE: u64 = 1000;
    pub const AUDIO_UPDATE: u64 = 500;
    pub const UI_REFRESH: u64 = 16; // ~60 FPS
    pub const VOLUME_DEBOUNCE: u64 = 50;
}

/// Color scheme
pub mod colors {
    use super::Color32;

    // Primary colors
    pub const RED: Color32 = Color32::from_rgb(255, 99, 71);
    pub const ORANGE: Color32 = Color32::from_rgb(255, 165, 0);
    pub const YELLOW: Color32 = Color32::from_rgb(255, 215, 0);
    pub const GREEN: Color32 = Color32::from_rgb(60, 179, 113);
    pub const BLUE: Color32 = Color32::from_rgb(100, 149, 237);
    pub const INDIGO: Color32 = Color32::from_rgb(75, 0, 130);
    pub const VIOLET: Color32 = Color32::from_rgb(138, 43, 226);
    pub const BROWN: Color32 = Color32::from_rgb(165, 42, 42);
    pub const GOLD: Color32 = Color32::from_rgb(255, 215, 0);
    pub const MAGENTA: Color32 = Color32::from_rgb(255, 0, 255);
    pub const CYAN: Color32 = Color32::from_rgb(0, 206, 209);
    pub const SILVER: Color32 = Color32::from_rgb(192, 192, 192);
    pub const OLIVE_GREEN: Color32 = Color32::from_rgb(128, 128, 0);
    pub const ROYALBLUE: Color32 = Color32::from_rgb(65, 105, 225);
    pub const WHEAT: Color32 = Color32::from_rgb(245, 222, 179);

    // System status colors
    pub const CPU_LOW: Color32 = GREEN;
    pub const CPU_MEDIUM: Color32 = YELLOW;
    pub const CPU_HIGH: Color32 = ORANGE;
    pub const CPU_CRITICAL: Color32 = RED;

    pub const MEMORY_AVAILABLE: Color32 = CYAN;
    pub const MEMORY_USED: Color32 = SILVER;

    // Tag colors for workspace indicators
    pub const TAG_COLORS: [Color32; 9] = [
        Color32::from_rgb(0xFF, 0x6B, 0x6B), // 红色
        Color32::from_rgb(0x4E, 0xCD, 0xC4), // 青色
        Color32::from_rgb(0x45, 0xB7, 0xD1), // 蓝色
        Color32::from_rgb(0x96, 0xCE, 0xB4), // 绿色
        Color32::from_rgb(0xFE, 0xCA, 0x57), // 黄色
        Color32::from_rgb(0xFF, 0x9F, 0xF3), // 粉色
        Color32::from_rgb(0x54, 0xA0, 0xFF), // 淡蓝色
        Color32::from_rgb(0x5F, 0x27, 0xCD), // 紫色
        Color32::from_rgb(0x00, 0xD2, 0xD3), // 青绿色
    ];

    // UI accent colors
    pub const ACCENT_PRIMARY: Color32 = BLUE;
    pub const ACCENT_SECONDARY: Color32 = CYAN;
    pub const WARNING: Color32 = ORANGE;
    pub const ERROR: Color32 = RED;
    pub const SUCCESS: Color32 = GREEN;

    // 新增电池相关颜色
    pub const BATTERY_HIGH: Color32 = Color32::from_rgb(76, 175, 80); // 绿色
    pub const BATTERY_MEDIUM: Color32 = Color32::from_rgb(255, 193, 7); // 黄色
    pub const BATTERY_LOW: Color32 = Color32::from_rgb(244, 67, 54); // 红色
    pub const CHARGING: Color32 = Color32::from_rgb(33, 150, 243); // 蓝色
    pub const UNAVAILABLE: Color32 = Color32::from_rgb(158, 158, 158); // 灰色
}

/// Icons and symbols
pub mod icons {
    // Workspace tag icons
    pub const TAG_ICONS: [&str; 9] = ["🏠", "💻", "🌐", "🎵", "📁", "🎮", "📧", "🔧", "📊"];

    // Audio icons
    pub const VOLUME_MUTED: &str = "🔇";
    pub const VOLUME_LOW: &str = "🔈";
    pub const VOLUME_MEDIUM: &str = "🔉";
    pub const VOLUME_HIGH: &str = "🔊";

    // System icons
    pub const CPU_ICON: &str = "🔥";
    pub const MEMORY_ICON: &str = "💾";
    pub const SCREENSHOT_ICON: &str = "📸";
    pub const SETTINGS_ICON: &str = "⚙️";

    // Monitor numbers
    pub const MONITOR_NUMBERS: [&str; 2] = ["󰎡", "󰎤"];
}

/// Font families to try loading
pub const FONT_FAMILIES: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans CJK TC",
    // "Noto Color Emoji",
    // "Noto Emoji",
    "SauceCodeProNerdFont",
    // "DejaVuSansMonoNerdFont",
    // "JetBrainsMonoNerdFont",
];

/// Application metadata
pub mod app {
    pub const DEFAULT_LOG_LEVEL: &str = "info";
    pub const LOG_FILE_MAX_SIZE: u64 = 10_000_000; // 10MB
    pub const LOG_FILE_MAX_COUNT: usize = 5;
    pub const HEARTBEAT_TIMEOUT_SECS: u64 = 5;
}
