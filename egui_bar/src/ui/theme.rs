//! Theme management for the UI

use crate::constants::colors;
use egui::{Color32, Style, Visuals};

/// Available themes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeType {
    Dark,
    Light,
    Auto, // Follow system theme
}

impl Default for ThemeType {
    fn default() -> Self {
        ThemeType::Auto
    }
}

impl std::str::FromStr for ThemeType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dark" => Ok(Self::Dark),
            "light" => Ok(Self::Light),
            "auto" => Ok(Self::Auto),
            _ => Err(format!("Unknown theme: {}", s)),
        }
    }
}

impl std::fmt::Display for ThemeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dark => write!(f, "dark"),
            Self::Light => write!(f, "light"),
            Self::Auto => write!(f, "auto"),
        }
    }
}

/// Color scheme for the application
#[derive(Debug, Clone)]
pub struct ColorScheme {
    pub background: Color32,
    pub foreground: Color32,
    pub accent_primary: Color32,
    pub accent_secondary: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub success: Color32,
    pub muted: Color32,
}

/// Theme manager
#[derive(Debug)]
pub struct ThemeManager {
    current_theme: ThemeType,
    dark_scheme: ColorScheme,
    light_scheme: ColorScheme,
}

impl ThemeManager {
    pub fn new(theme_type: ThemeType) -> Self {
        Self {
            current_theme: theme_type,
            dark_scheme: Self::dark_color_scheme(),
            light_scheme: Self::light_color_scheme(),
        }
    }

    /// Get dark color scheme
    fn dark_color_scheme() -> ColorScheme {
        ColorScheme {
            background: Color32::from_gray(25),
            foreground: Color32::from_gray(220),
            accent_primary: colors::BLUE,
            accent_secondary: colors::CYAN,
            warning: colors::ORANGE,
            error: colors::RED,
            success: colors::GREEN,
            muted: Color32::from_gray(128),
        }
    }

    /// Get light color scheme
    fn light_color_scheme() -> ColorScheme {
        ColorScheme {
            background: Color32::from_gray(248),
            foreground: Color32::from_gray(30),
            accent_primary: colors::BLUE,
            accent_secondary: Color32::from_rgb(0, 150, 180),
            warning: Color32::from_rgb(200, 120, 0),
            error: Color32::from_rgb(180, 40, 40),
            success: Color32::from_rgb(40, 140, 80),
            muted: Color32::from_gray(128),
        }
    }

    /// Get current color scheme
    pub fn current_scheme(&self) -> &ColorScheme {
        match self.current_theme {
            ThemeType::Dark => &self.dark_scheme,
            ThemeType::Light => &self.light_scheme,
            ThemeType::Auto => {
                // TODO: Detect system theme
                &self.dark_scheme
            }
        }
    }

    /// Apply theme to egui context
    pub fn apply_to_context(&self, ctx: &egui::Context) {
        let scheme = self.current_scheme();

        let visuals = match self.current_theme {
            ThemeType::Light => Visuals::light(),
            _ => Visuals::dark(),
        };

        ctx.set_visuals(visuals);

        // Customize style
        ctx.style_mut(|style| {
            self.apply_colors_to_style(style, scheme);
        });
    }

    /// Apply color scheme to style
    fn apply_colors_to_style(&self, style: &mut Style, scheme: &ColorScheme) {
        // Window colors
        style.visuals.window_fill = scheme.background;
        style.visuals.panel_fill = scheme.background;

        // Text colors
        style.visuals.override_text_color = Some(scheme.foreground);

        // Widget colors
        style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.bg_fill = scheme.muted.gamma_multiply(0.3);
        style.visuals.widgets.hovered.bg_fill = scheme.accent_primary.gamma_multiply(0.3);
        style.visuals.widgets.active.bg_fill = scheme.accent_primary.gamma_multiply(0.5);

        // Borders
        style.visuals.widgets.noninteractive.bg_stroke.color = scheme.muted.gamma_multiply(0.5);
        style.visuals.widgets.inactive.bg_stroke.color = scheme.muted;
        style.visuals.widgets.hovered.bg_stroke.color = scheme.accent_primary;
        style.visuals.widgets.active.bg_stroke.color = scheme.accent_primary;
    }

    /// Change theme
    pub fn set_theme(&mut self, theme: ThemeType) {
        self.current_theme = theme;
    }

    /// Get current theme type
    pub fn current_theme(&self) -> &ThemeType {
        &self.current_theme
    }

    /// Toggle between dark and light themes
    pub fn toggle_theme(&mut self) {
        self.current_theme = match self.current_theme {
            ThemeType::Dark => ThemeType::Light,
            ThemeType::Light => ThemeType::Dark,
            ThemeType::Auto => ThemeType::Dark, // Default to dark when toggling from auto
        };
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new(ThemeType::Light)
    }
}
