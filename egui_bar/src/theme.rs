use egui::{Color32, FontFamily, FontId, Margin, TextStyle};
use log::info;
use std::collections::BTreeMap;

use anyhow::Result;
use crate::config::CONFIG;

/// UI constants
pub mod ui {
    pub const DEFAULT_FONT_SIZE: f32 = 18.0;
    pub const DEFAULT_SCALE_FACTOR: f32 = 1.0;
}

/// Color scheme
#[allow(dead_code)]
pub mod colors {
    use egui::Color32;

    // Core UI palette (dark)
    pub const BG: Color32 = Color32::from_rgb(0x0F, 0x13, 0x1A);
    pub const BG_ELEVATED: Color32 = Color32::from_rgb(0x14, 0x1A, 0x24);
    pub const BG_HOVER: Color32 = Color32::from_rgb(0x1C, 0x24, 0x31);
    pub const BG_ACTIVE: Color32 = Color32::from_rgb(0x23, 0x2D, 0x3D);

    pub const STROKE_SUBTLE: Color32 = Color32::from_rgb(0x2A, 0x34, 0x45);
    pub const TEXT: Color32 = Color32::from_rgb(0xE9, 0xEE, 0xF5);
    pub const TEXT_SUBTLE: Color32 = Color32::from_rgb(0xA8, 0xB3, 0xC3);

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
        Color32::from_rgb(0xFF, 0x6B, 0x6B), // Red
        Color32::from_rgb(0x4E, 0xCD, 0xC4), // Cyan
        Color32::from_rgb(0x45, 0xB7, 0xD1), // Blue
        Color32::from_rgb(0x96, 0xCE, 0xB4), // Green
        Color32::from_rgb(0xFE, 0xCA, 0x57), // Yellow
        Color32::from_rgb(0xFF, 0x9F, 0xF3), // Pink
        Color32::from_rgb(0x54, 0xA0, 0xFF), // Light Blue
        Color32::from_rgb(0x5F, 0x27, 0xCD), // Purple
        Color32::from_rgb(0x00, 0xD2, 0xD3), // Teal
    ];

    // UI accent colors
    pub const ACCENT_PRIMARY: Color32 = BLUE;
    pub const ACCENT_SECONDARY: Color32 = CYAN;
    pub const WARNING: Color32 = ORANGE;
    pub const ERROR: Color32 = RED;
    pub const SUCCESS: Color32 = GREEN;

    // Battery related colors
    pub const BATTERY_HIGH: Color32 = Color32::from_rgb(76, 175, 80);    // Green
    pub const BATTERY_MEDIUM: Color32 = Color32::from_rgb(255, 193, 7);  // Yellow
    pub const BATTERY_LOW: Color32 = Color32::from_rgb(244, 67, 54);     // Red
    pub const CHARGING: Color32 = Color32::from_rgb(33, 150, 243);       // Blue
    pub const UNAVAILABLE: Color32 = Color32::from_rgb(158, 158, 158);   // Gray
}

/// Icons and symbols
#[allow(dead_code)]
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

/// Default font families to try loading (used when config has no overrides)
pub const FONT_FAMILIES: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans CJK TC",
    "SauceCodeProNerdFont",
];

/// Apply theme to egui context
pub fn apply_theme(ctx: &egui::Context) {
    use crate::config::ThemeMode;

    let cfg = CONFIG.load();
    // Env var overrides config
    let theme = std::env::var("EGUI_BAR_THEME").unwrap_or_else(|_| {
        match cfg.theme.mode {
            ThemeMode::Light => "light".to_string(),
            ThemeMode::Dark => "dark".to_string(),
        }
    });

    let mut style = (*ctx.style()).clone();
    let mut visuals = if theme.eq_ignore_ascii_case("light") {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };

    visuals.window_corner_radius = egui::CornerRadius::same(10);
    visuals.menu_corner_radius = egui::CornerRadius::same(10);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.open.corner_radius = egui::CornerRadius::same(8);

    if !theme.eq_ignore_ascii_case("light") {
        visuals.panel_fill = colors::BG;
        visuals.window_fill = colors::BG_ELEVATED;
        visuals.override_text_color = Some(colors::TEXT);

        visuals.widgets.noninteractive.bg_fill = colors::BG;
        visuals.widgets.noninteractive.fg_stroke.color = colors::TEXT;

        visuals.widgets.inactive.bg_fill = colors::BG_ELEVATED;
        visuals.widgets.inactive.bg_stroke.color = colors::STROKE_SUBTLE;
        visuals.widgets.inactive.fg_stroke.color = colors::TEXT;

        visuals.widgets.hovered.bg_fill = colors::BG_HOVER;
        visuals.widgets.hovered.bg_stroke.color = colors::STROKE_SUBTLE;
        visuals.widgets.hovered.fg_stroke.color = colors::TEXT;

        visuals.widgets.active.bg_fill = colors::BG_ACTIVE;
        visuals.widgets.active.bg_stroke.color = colors::STROKE_SUBTLE;
        visuals.widgets.active.fg_stroke.color = colors::TEXT;

        visuals.widgets.open.bg_fill = colors::BG_ELEVATED;
        visuals.widgets.open.bg_stroke.color = colors::STROKE_SUBTLE;
        visuals.widgets.open.fg_stroke.color = colors::TEXT;

        visuals.selection.bg_fill = colors::ACCENT_PRIMARY.gamma_multiply(0.35);
        visuals.selection.stroke.color = colors::ACCENT_PRIMARY;
    }

    style.visuals = visuals;

    style.spacing.item_spacing = egui::vec2(10.0, 0.0);
    style.spacing.button_padding = egui::vec2(10.0, 3.0);
    style.spacing.interact_size = egui::vec2(34.0, 26.0);
    style.spacing.menu_margin = Margin::symmetric(10, 8);
    style.spacing.window_margin = Margin::symmetric(12, 10);
    style.interaction.tooltip_delay = 0.25;

    ctx.set_style(style);
}

/// Setup custom fonts from system
pub fn setup_custom_fonts(ctx: &egui::Context) -> Result<()> {
    use font_kit::family_name::FamilyName;
    use font_kit::properties::Properties;
    use font_kit::source::SystemSource;
    use std::collections::HashSet;

    info!("Loading system fonts...");
    let mut fonts = egui::FontDefinitions::default();
    let system_source = SystemSource::new();

    let original_proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let original_monospace = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    let mut loaded_fonts = Vec::new();
    let mut seen_fonts = HashSet::new();

    // Use config font families, falling back to hardcoded defaults
    let cfg = CONFIG.load();
    let font_families: Vec<String> = if cfg.fonts.families.is_empty() {
        FONT_FAMILIES.iter().map(|s| s.to_string()).collect()
    } else {
        cfg.fonts.families.clone()
    };

    for font_name in &font_families {
        if fonts.font_data.contains_key(font_name.as_str()) || seen_fonts.contains(font_name.as_str()) {
            info!("Font {} already loaded, skipping", font_name);
            continue;
        }

        info!("Attempting to load font: {}", font_name);

        let font_result = system_source
            .select_best_match(
                &[FamilyName::Title(font_name.clone())],
                &Properties::new(),
            )
            .and_then(|handle| {
                handle
                    .load()
                    .map_err(|_| font_kit::error::SelectionError::NotFound)
            })
            .and_then(|font| {
                font.copy_font_data()
                    .ok_or(font_kit::error::SelectionError::NotFound)
            });

        match font_result {
            Ok(font_data) => {
                let font_key = font_name.clone();
                fonts.font_data.insert(
                    font_key.clone(),
                    egui::FontData::from_owned(font_data.to_vec()).into(),
                );
                loaded_fonts.push(font_key);
                seen_fonts.insert(font_name.clone());
                info!("Successfully loaded font: {}", font_name);
            }
            Err(e) => {
                info!("Failed to load font {}: {}", font_name, e);
            }
        }
    }

    if !loaded_fonts.is_empty() {
        update_font_families(
            &mut fonts,
            loaded_fonts,
            original_proportional,
            original_monospace,
        );
        info!(
            "Font setup completed with {} custom fonts",
            fonts.font_data.len() - 2
        );
    } else {
        info!("No custom fonts loaded, using default configuration");
    }

    ctx.set_fonts(fonts);
    Ok(())
}

/// Update font families with loaded custom fonts
fn update_font_families(
    fonts: &mut egui::FontDefinitions,
    loaded_fonts: Vec<String>,
    original_proportional: Vec<String>,
    original_monospace: Vec<String>,
) {
    let new_proportional = [loaded_fonts.clone(), original_proportional].concat();
    let new_monospace = [loaded_fonts.clone(), original_monospace].concat();

    fonts
        .families
        .insert(FontFamily::Proportional, new_proportional);
    fonts.families.insert(FontFamily::Monospace, new_monospace);

    info!("Updated font families:");
    info!(
        "  Proportional: {:?}",
        fonts.families.get(&FontFamily::Proportional)
    );
    info!(
        "  Monospace: {:?}",
        fonts.families.get(&FontFamily::Monospace)
    );
}

/// Configure text styles for egui context
pub fn configure_text_styles(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        let cfg = CONFIG.load();
        let base_font_size = if cfg.fonts.size > 0.0 {
            cfg.fonts.size
        } else {
            ui::DEFAULT_FONT_SIZE
        };
        let text_styles: BTreeMap<TextStyle, FontId> = [
            (
                TextStyle::Small,
                FontId::new(base_font_size * 0.8, FontFamily::Monospace),
            ),
            (
                TextStyle::Body,
                FontId::new(base_font_size, FontFamily::Monospace),
            ),
            (
                TextStyle::Monospace,
                FontId::new(base_font_size, FontFamily::Monospace),
            ),
            (
                TextStyle::Button,
                FontId::new(base_font_size, FontFamily::Monospace),
            ),
            (
                TextStyle::Heading,
                FontId::new(base_font_size * 1.5, FontFamily::Monospace),
            ),
        ]
        .into();

        style.text_styles = text_styles;
    });
}

/// Create a color with specified alpha
pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}
