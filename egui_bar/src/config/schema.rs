use serde::{Deserialize, Serialize};

/// Top-level bar configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BarConfig {
    pub general: GeneralConfig,
    pub modules: ModulesConfig,
    pub theme: ThemeConfig,
    pub animation: AnimationConfig,
    pub fonts: FontConfig,
}

/// Bar position
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    Top,
    Bottom,
}

/// Bar width mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarWidth {
    Full,
    Fixed(f32),
}

/// General bar settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub position: BarPosition,
    pub height: f32,
    pub width: BarWidth,
    pub transparent: bool,
    pub monitor: Option<u32>,
    pub scale_factor: f32,
}

/// Module layout configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModulesConfig {
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
}

/// Theme mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Theme configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub mode: ThemeMode,
    pub bg: Option<String>,
    pub text: Option<String>,
    pub accent: Option<String>,
    pub corner_radius: f32,
    pub tag_colors: Option<Vec<String>>,
}

/// Easing function name
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EasingName {
    Linear,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,
}

/// Animation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationConfig {
    pub enabled: bool,
    pub duration_ms: u64,
    pub easing: EasingName,
    pub hover_duration_ms: u64,
}

/// Font configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub families: Vec<String>,
    pub size: f32,
}

// === Default implementations ===
// All defaults match the current hardcoded values exactly

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            modules: ModulesConfig::default(),
            theme: ThemeConfig::default(),
            animation: AnimationConfig::default(),
            fonts: FontConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            position: BarPosition::Top,
            height: 40.0,
            width: BarWidth::Full,
            transparent: false,
            monitor: None,
            scale_factor: 1.0,
        }
    }
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            left: vec!["workspaces".into(), "layout".into()],
            center: vec![],
            right: vec![
                "tray".into(),
                "cpu".into(),
                "memory".into(),
                "battery".into(),
                "audio".into(),
                "clock".into(),
            ],
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Dark,
            bg: None,
            text: None,
            accent: None,
            corner_radius: 8.0,
            tag_colors: None,
        }
    }
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            duration_ms: 200,
            easing: EasingName::EaseOutQuad,
            hover_duration_ms: 150,
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            families: vec![
                "Noto Sans CJK SC".into(),
                "Noto Sans CJK TC".into(),
                "SauceCodeProNerdFont".into(),
            ],
            size: 18.0,
        }
    }
}
