// src/backends/mod.rs

// 模块声明，根据 feature 条件编译
#[cfg(feature = "futex")]
pub mod futex;

#[cfg(feature = "semaphore")]
pub mod semaphore;

#[cfg(feature = "eventfd")]
pub mod eventfd;

// 公共的 Trait 和 Header 定义
pub mod common;
