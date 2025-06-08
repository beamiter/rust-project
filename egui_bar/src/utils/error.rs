//! Error handling for the egui_bar application

use std::fmt;

/// Application error types
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Audio system error: {message}")]
    Audio { message: String },

    #[error("System information error: {message}")]
    System { message: String },

    #[error("Configuration error: {message}")]
    Config { message: String },

    #[error("UI error: {message}")]
    Ui { message: String },

    #[error("IO error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    #[error("Serialization error: {source}")]
    Serialization {
        #[from]
        source: toml::de::Error,
    },

    #[error("Font loading error: {message}")]
    Font { message: String },

    #[error("Shared memory error: {message}")]
    SharedMemory { message: String },
}

/// Convenient Result type alias
pub type Result<T> = std::result::Result<T, AppError>;

/// Helper functions for creating specific error types
impl AppError {
    pub fn audio<S: Into<String>>(message: S) -> Self {
        Self::Audio {
            message: message.into(),
        }
    }

    pub fn system<S: Into<String>>(message: S) -> Self {
        Self::System {
            message: message.into(),
        }
    }

    pub fn config<S: Into<String>>(message: S) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    pub fn ui<S: Into<String>>(message: S) -> Self {
        Self::Ui {
            message: message.into(),
        }
    }

    pub fn font<S: Into<String>>(message: S) -> Self {
        Self::Font {
            message: message.into(),
        }
    }

    pub fn shared_memory<S: Into<String>>(message: S) -> Self {
        Self::SharedMemory {
            message: message.into(),
        }
    }
}

/// Convert common error types
impl From<font_kit::error::SelectionError> for AppError {
    fn from(err: font_kit::error::SelectionError) -> Self {
        Self::font(format!("Font selection failed: {}", err))
    }
}

impl From<font_kit::error::FontLoadingError> for AppError {
    fn from(err: font_kit::error::FontLoadingError) -> Self {
        Self::font(format!("Font loading failed: {}", err))
    }
}
