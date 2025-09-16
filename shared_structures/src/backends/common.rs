//! src/backends/common.rs

use std::io::Result;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::time::Duration;

/// 运行时选择同步策略的枚举
/// 只有在 Cargo.toml 中启用了对应的 feature，才能使用该选项
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStrategy {
    #[cfg(feature = "futex")]
    Futex,
    #[cfg(feature = "semaphore")]
    Semaphore,
    #[cfg(feature = "eventfd")]
    EventFd,
}

// 如果一个 feature 也没启用，提供一个空的枚举，避免编译错误
#[cfg(not(any(feature = "futex", feature = "semaphore", feature = "eventfd")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStrategy {}

impl SyncStrategy {
    /// 返回此策略在共享内存中需要的后端专属 Header 的大小
    pub fn backend_size(&self) -> usize {
        match self {
            #[cfg(feature = "futex")]
            SyncStrategy::Futex => std::mem::size_of::<super::futex::FutexHeader>(),
            #[cfg(feature = "semaphore")]
            SyncStrategy::Semaphore => std::mem::size_of::<super::semaphore::SemaphoreHeader>(),
            #[cfg(feature = "eventfd")]
            SyncStrategy::EventFd => std::mem::size_of::<super::eventfd::EventFdHeader>(),
        }
    }

    /// 返回此策略的内存对齐要求
    pub fn backend_align(&self) -> usize {
        match self {
            #[cfg(feature = "futex")]
            SyncStrategy::Futex => std::mem::align_of::<super::futex::FutexHeader>(),
            #[cfg(feature = "semaphore")]
            SyncStrategy::Semaphore => std::mem::align_of::<super::semaphore::SemaphoreHeader>(),
            #[cfg(feature = "eventfd")]
            SyncStrategy::EventFd => std::mem::align_of::<super::eventfd::EventFdHeader>(),
        }
    }
}

/// 环形缓冲区在共享内存中的通用头部
#[repr(C, align(128))] // 保持高对齐以避免 false sharing
#[derive(Debug)]
pub struct GenericHeader {
    // 基础元数据
    pub magic: AtomicU64,
    pub version: AtomicU64,

    // 消息环形缓冲区指针
    pub write_idx: AtomicU32,
    pub read_idx: AtomicU32,
    pub buffer_size: u32,

    // 命令环形缓冲区指针
    pub cmd_write_idx: AtomicU32,
    pub cmd_read_idx: AtomicU32,

    // 状态
    pub is_destroyed: AtomicBool,
    pub last_timestamp: AtomicU64,

    // 占位符，确保后端 Header 的起始地址是可预测的
    _padding: [u8; 32],
}

/// 同步后端必须实现的 Trait
pub trait SyncBackend: Send + Sync {
    /// 初始化后端。
    /// is_creator: true 表示创建并初始化共享内存，false 表示打开已有的。
    /// backend_ptr: 指向为后端分配的专属内存区域的指针。
    fn init(&mut self, is_creator: bool, backend_ptr: *mut u8) -> Result<()>;

    /// 等待消息
    fn wait_for_message(
        &self,
        has_data: impl Fn() -> bool,
        adaptive_poll_spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool>;

    /// 等待命令
    fn wait_for_command(
        &self,
        has_data: impl Fn() -> bool,
        adaptive_poll_spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool>;

    /// 唤醒正在等待消息的进程
    fn signal_message(&self) -> Result<()>;

    /// 唤醒正在等待命令的进程
    fn signal_command(&self) -> Result<()>;

    /// 在 SharedRingBuffer 被销毁时执行清理工作
    fn cleanup(&mut self, is_creator: bool);
}

// ========================================================================= //
// ==========================   新增修复代码   ============================== //
// ========================================================================= //

// 1. 导入所有可能的后端实现
#[cfg(feature = "eventfd")]
use crate::backends::eventfd::EventFdBackend;
#[cfg(feature = "futex")]
use crate::backends::futex::FutexBackend;
#[cfg(feature = "semaphore")]
use crate::backends::semaphore::SemaphoreBackend;

/// 一个包装了所有可用 SyncBackend 实现的枚举，用于取代 `Box<dyn SyncBackend>`
pub enum AnySyncBackend {
    #[cfg(feature = "futex")]
    Futex(FutexBackend),
    #[cfg(feature = "semaphore")]
    Semaphore(SemaphoreBackend),
    #[cfg(feature = "eventfd")]
    EventFd(EventFdBackend),
    /// 用于处理没有启用任何 feature 的情况
    #[doc(hidden)]
    _Unsupported,
}

// 2. 为这个枚举实现 SyncBackend trait，通过 match 将调用分发到具体的后端
impl SyncBackend for AnySyncBackend {
    fn init(&mut self, is_creator: bool, backend_ptr: *mut u8) -> Result<()> {
        match self {
            #[cfg(feature = "futex")]
            Self::Futex(b) => b.init(is_creator, backend_ptr),
            #[cfg(feature = "semaphore")]
            Self::Semaphore(b) => b.init(is_creator, backend_ptr),
            #[cfg(feature = "eventfd")]
            Self::EventFd(b) => b.init(is_creator, backend_ptr),
            Self::_Unsupported => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "No backend feature enabled",
            )),
        }
    }

    fn wait_for_message(
        &self,
        has_data: impl Fn() -> bool,
        adaptive_poll_spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        match self {
            #[cfg(feature = "futex")]
            Self::Futex(b) => b.wait_for_message(has_data, adaptive_poll_spins, timeout),
            #[cfg(feature = "semaphore")]
            Self::Semaphore(b) => b.wait_for_message(has_data, adaptive_poll_spins, timeout),
            #[cfg(feature = "eventfd")]
            Self::EventFd(b) => b.wait_for_message(has_data, adaptive_poll_spins, timeout),
            Self::_Unsupported => Ok(has_data()),
        }
    }

    fn wait_for_command(
        &self,
        has_data: impl Fn() -> bool,
        adaptive_poll_spins: u32,
        timeout: Option<Duration>,
    ) -> Result<bool> {
        match self {
            #[cfg(feature = "futex")]
            Self::Futex(b) => b.wait_for_command(has_data, adaptive_poll_spins, timeout),
            #[cfg(feature = "semaphore")]
            Self::Semaphore(b) => b.wait_for_command(has_data, adaptive_poll_spins, timeout),
            #[cfg(feature = "eventfd")]
            Self::EventFd(b) => b.wait_for_command(has_data, adaptive_poll_spins, timeout),
            Self::_Unsupported => Ok(has_data()),
        }
    }

    fn signal_message(&self) -> Result<()> {
        match self {
            #[cfg(feature = "futex")]
            Self::Futex(b) => b.signal_message(),
            #[cfg(feature = "semaphore")]
            Self::Semaphore(b) => b.signal_message(),
            #[cfg(feature = "eventfd")]
            Self::EventFd(b) => b.signal_message(),
            Self::_Unsupported => Ok(()),
        }
    }

    fn signal_command(&self) -> Result<()> {
        match self {
            #[cfg(feature = "futex")]
            Self::Futex(b) => b.signal_command(),
            #[cfg(feature = "semaphore")]
            Self::Semaphore(b) => b.signal_command(),
            #[cfg(feature = "eventfd")]
            Self::EventFd(b) => b.signal_command(),
            Self::_Unsupported => Ok(()),
        }
    }

    fn cleanup(&mut self, is_creator: bool) {
        match self {
            #[cfg(feature = "futex")]
            Self::Futex(b) => b.cleanup(is_creator),
            #[cfg(feature = "semaphore")]
            Self::Semaphore(b) => b.cleanup(is_creator),
            #[cfg(feature = "eventfd")]
            Self::EventFd(b) => b.cleanup(is_creator),
            Self::_Unsupported => {}
        }
    }
}
