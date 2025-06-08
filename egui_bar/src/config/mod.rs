//! Configuration management

use crate::constants::{intervals, ui};
use crate::utils::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub ui: UiConfig,
    pub audio: AudioConfig,
    pub system: SystemConfig,
    pub logging: LoggingConfig,
}

/// UI-related configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub font_size: f32,
    pub scale_factor: f32,
    pub show_seconds: bool,
    pub theme: String,
    pub window_opacity: f32,
    pub auto_hide: bool,
    pub update_interval_ms: u64,
}

/// Audio system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub update_interval_ms: u64,
    pub volume_step: i32,
    pub default_device: Option<String>,
    pub show_all_devices: bool,
}

/// System monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub update_interval_ms: u64,
    pub cpu_history_length: usize,
    pub memory_warning_threshold: f32,
    pub cpu_warning_threshold: f32,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub log_to_file: bool,
    pub log_dir: Option<PathBuf>,
    pub max_file_size: u64,
    pub max_files: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ui: UiConfig::default(),
            audio: AudioConfig::default(),
            system: SystemConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            font_size: ui::DEFAULT_FONT_SIZE,
            scale_factor: ui::DEFAULT_SCALE_FACTOR,
            show_seconds: false,
            theme: "dark".to_string(),
            window_opacity: 0.95,
            auto_hide: false,
            update_interval_ms: intervals::UI_REFRESH,
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            update_interval_ms: intervals::AUDIO_UPDATE,
            volume_step: 5,
            default_device: None,
            show_all_devices: true,
        }
    }
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            update_interval_ms: intervals::SYSTEM_UPDATE,
            cpu_history_length: 60,
            memory_warning_threshold: 0.8,
            cpu_warning_threshold: 0.8,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            log_to_file: true,
            log_dir: None,
            max_file_size: 10_000_000, // 10MB
            max_files: 5,
        }
    }
}

impl AppConfig {
    /// Load configuration from file or create default
    pub fn load() -> Result<Self> {
        let config_path = Self::config_file_path()?;

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| AppError::config(format!("Failed to read config file: {}", e)))?;

            let config: AppConfig = toml::from_str(&content)
                .map_err(|e| AppError::config(format!("Failed to parse config: {}", e)))?;

            log::info!("Loaded configuration from {:?}", config_path);
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            log::info!("Created default configuration at {:?}", config_path);
            Ok(config)
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_file_path()?;

        // Create config directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::config(format!("Failed to create config directory: {}", e))
            })?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| AppError::config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(&config_path, content)
            .map_err(|e| AppError::config(format!("Failed to write config file: {}", e)))?;

        log::info!("Saved configuration to {:?}", config_path);
        Ok(())
    }

    /// Get the config file path
    fn config_file_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| AppError::config("Cannot determine config directory"))?;

        Ok(config_dir.join("egui_bar").join("config.toml"))
    }

    /// Validate configuration values
    pub fn validate(&mut self) -> Result<()> {
        // Clamp UI values
        self.ui.font_size = self.ui.font_size.clamp(8.0, 48.0);
        self.ui.scale_factor = self
            .ui
            .scale_factor
            .clamp(ui::MIN_SCALE_FACTOR, ui::MAX_SCALE_FACTOR);
        self.ui.window_opacity = self.ui.window_opacity.clamp(0.1, 1.0);

        // Validate intervals
        self.ui.update_interval_ms = self.ui.update_interval_ms.max(16); // At least 60 FPS
        self.audio.update_interval_ms = self.audio.update_interval_ms.max(100);
        self.system.update_interval_ms = self.system.update_interval_ms.max(100);

        // Validate audio settings
        self.audio.volume_step = self.audio.volume_step.clamp(1, 20);

        // Validate system settings
        self.system.cpu_history_length = self.system.cpu_history_length.clamp(10, 1000);
        self.system.memory_warning_threshold = self.system.memory_warning_threshold.clamp(0.1, 1.0);
        self.system.cpu_warning_threshold = self.system.cpu_warning_threshold.clamp(0.1, 1.0);

        // Validate logging settings
        self.logging.max_file_size = self.logging.max_file_size.max(1_000_000); // At least 1MB
        self.logging.max_files = self.logging.max_files.clamp(1, 20);

        Ok(())
    }
}
