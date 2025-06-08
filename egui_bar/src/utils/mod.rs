//! Utility modules

pub mod error;
pub mod metrics;

pub use error::{AppError, Result};
pub use metrics::{PerformanceMetrics, RollingAverage};
