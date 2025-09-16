//! src/shared_ring_buffer.rs

// --- 修改点 1：导入 AnySyncBackend ---
use crate::backends::common::{AnySyncBackend, GenericHeader, SyncBackend, SyncStrategy};
use crate::shared_message::{SharedCommand, SharedMessage};

use log::{error, info, warn};
use shared_memory::{Shmem, ShmemConf};
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// --- 常量 ---
const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646;
const RING_BUFFER_VERSION: u64 = 8; // 版本号因结构调整而递增
const DEFAULT_BUFFER_SIZE: usize = 16;
const CMD_BUFFER_SIZE: usize = 16;
const DEFAULT_ADAPTIVE_POLL_SPINS: u32 = 400;

// --- 辅助函数 ---
#[inline]
fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

#[inline]
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// --- 内部数据结构 ---

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MessageSlot {
    timestamp: u64,
    checksum: u32,
    _padding: u32,
    message: SharedMessage,
}

// 确保在读取可能包含 padding 的结构时，只校验有效数据
fn calculate_message_checksum(m: &SharedMessage) -> u32 {
    let mut sum = 0u32;

    #[inline(always)]
    fn mix_u64(sum: &mut u32, v: u64) {
        *sum = sum.wrapping_add((v as u32) ^ ((v >> 32) as u32));
    }

    #[inline(always)]
    fn mix_i32(sum: &mut u32, v: i32) {
        *sum = sum.wrapping_add(v as u32);
    }

    // timestamp
    mix_u64(&mut sum, m.timestamp);

    let mi = &m.monitor_info;

    // scalar fields
    mix_i32(&mut sum, mi.monitor_num);
    mix_i32(&mut sum, mi.monitor_width);
    mix_i32(&mut sum, mi.monitor_height);
    mix_i32(&mut sum, mi.monitor_x);
    mix_i32(&mut sum, mi.monitor_y);

    // tag_status_vec：将 bool 压缩成位
    for ts in &mi.tag_status_vec {
        let bits: u8 = (ts.is_selected as u8)
            | ((ts.is_urg as u8) << 1)
            | ((ts.is_filled as u8) << 2)
            | ((ts.is_occ as u8) << 3);
        sum = sum.wrapping_add(bits as u32);
    }

    // client_name 和 ltsymbol 数组
    for &b in &mi.client_name {
        sum = sum.wrapping_add(b as u32);
    }
    for &b in &mi.ltsymbol {
        sum = sum.wrapping_add(b as u32);
    }

    sum
}

/// 基于共享内存的高性能 SPSC 环形缓冲区
pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut GenericHeader,
    message_slots: *mut MessageSlot,
    cmd_buffer_start: *mut SharedCommand,
    is_creator: bool,
    adaptive_poll_spins: u32,
    // --- 修改点 2：将 backend 类型改为 AnySyncBackend ---
    backend: AnySyncBackend,
}
impl std::hash::Hash for SharedRingBuffer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // 核心逻辑：我们通过 get_os_id() 获取共享内存的唯一标识符（一个字符串），
        // 然后对这个标识符进行哈希。
        // 这确保了所有指向同一块共享内存的 SharedRingBuffer 实例都具有相同的哈希值。
        self.shmem.get_os_id().hash(state);
    }
}
impl PartialEq for SharedRingBuffer {
    fn eq(&self, other: &Self) -> bool {
        // 保持与 Hash 一致：两个 SharedRingBuffer 相等，当且仅当
        // 它们引用的共享内存的唯一标识符相同。
        self.shmem.get_os_id() == other.shmem.get_os_id()
    }
}
// 因为 PartialEq 的实现满足了自反性、对称性和传递性，所以我们可以安全地实现 Eq。
impl Eq for SharedRingBuffer {}

unsafe impl Send for SharedRingBuffer {}
unsafe impl Sync for SharedRingBuffer {}

impl SharedRingBuffer {
    // --- 内部辅助方法 ---
    #[inline]
    fn buffer_size(&self) -> u32 {
        unsafe { (*self.header).buffer_size }
    }

    #[inline]
    fn buffer_mask(&self) -> u32 {
        self.buffer_size() - 1
    }

    #[inline]
    fn cmd_buffer_mask(&self) -> u32 {
        (CMD_BUFFER_SIZE as u32) - 1
    }

    // --- 构造与析构 ---

    /// 一个便捷的工厂函数，尝试打开一个已存在的缓冲区，如果失败则创建一个新的。
    pub fn create_shared_ring_buffer_aux(shared_path: &str) -> Option<Self> {
        return Self::create_shared_ring_buffer(shared_path, Self::get_default_strategy());
    }
    pub fn create_shared_ring_buffer(shared_path: &str, strategy: SyncStrategy) -> Option<Self> {
        if shared_path.is_empty() {
            warn!("No shared path provided, cannot use shared ring buffer.");
            return None;
        }
        match Self::open(shared_path, strategy, None) {
            Ok(shared_buffer) => {
                info!("Successfully opened shared ring buffer: {}", shared_path);
                Some(shared_buffer)
            }
            Err(e) => {
                warn!(
                    "Failed to open existing buffer ('{}'), attempting to create.",
                    e
                );
                match Self::create(shared_path, strategy, None, None) {
                    Ok(shared_buffer) => {
                        info!("Created new shared ring buffer: {}", shared_path);
                        Some(shared_buffer)
                    }
                    Err(create_err) => {
                        error!("Failed to create shared ring buffer: {}", create_err);
                        None
                    }
                }
            }
        }
    }

    /// 创建一个新的共享内存环形缓冲区。
    pub fn create_aux(
        path: &str,
        buffer_size: Option<usize>,
        adaptive_poll_spins: Option<u32>,
    ) -> Result<Self> {
        return Self::create(
            path,
            Self::get_default_strategy(),
            buffer_size,
            adaptive_poll_spins,
        );
    }
    pub fn create(
        path: &str,
        strategy: SyncStrategy,
        buffer_size: Option<usize>,
        adaptive_poll_spins: Option<u32>,
    ) -> Result<Self> {
        let buffer_size = buffer_size.unwrap_or(DEFAULT_BUFFER_SIZE);
        if !buffer_size.is_power_of_two() || !CMD_BUFFER_SIZE.is_power_of_two() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Buffer sizes must be powers of 2",
            ));
        }

        // 计算内存布局
        let generic_header_size = size_of::<GenericHeader>();
        let backend_header_size = strategy.backend_size();
        let backend_header_align = strategy.backend_align();

        let backend_offset = align_up(generic_header_size, backend_header_align);
        let messages_offset = align_up(
            backend_offset + backend_header_size,
            std::mem::align_of::<MessageSlot>(),
        );
        let messages_size = buffer_size * size_of::<MessageSlot>();
        let commands_offset = align_up(
            messages_offset + messages_size,
            std::mem::align_of::<SharedCommand>(),
        );
        let commands_size = CMD_BUFFER_SIZE * size_of::<SharedCommand>();
        let total_size = commands_offset + commands_size;

        let shmem = ShmemConf::new()
            .size(total_size)
            .flink(path)
            .force_create_flink()
            .create()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Failed to create shmem: {}", e)))?;

        let base_ptr = shmem.as_ptr();
        let header = base_ptr as *mut GenericHeader;
        let backend_ptr = unsafe { base_ptr.add(backend_offset) };
        let message_slots = unsafe { base_ptr.add(messages_offset) as *mut MessageSlot };
        let cmd_buffer_start = unsafe { base_ptr.add(commands_offset) as *mut SharedCommand };

        // 创建并初始化后端
        let mut backend = Self::new_backend(strategy);
        backend.init(true, backend_ptr)?;

        // 初始化通用 Header
        unsafe {
            // 先清零，再设置
            header.write_bytes(0, 1);
            (*header).magic.store(RING_BUFFER_MAGIC, Ordering::Release);
            (*header)
                .version
                .store(RING_BUFFER_VERSION, Ordering::Release);
            (*header).buffer_size = buffer_size as u32;
            (*header).is_destroyed.store(false, Ordering::Release);
        }

        Ok(Self {
            shmem,
            header,
            message_slots,
            cmd_buffer_start,
            is_creator: true,
            backend,
            adaptive_poll_spins: adaptive_poll_spins.unwrap_or(DEFAULT_ADAPTIVE_POLL_SPINS),
        })
    }

    /// 打开一个已存在的共享内存环形缓冲区。
    #[allow(unreachable_code)]
    fn get_default_strategy() -> SyncStrategy {
        #[cfg(feature = "use-eventfd")]
        {
            return SyncStrategy::EventFd;
        }
        #[cfg(feature = "use-futex")]
        {
            return SyncStrategy::Futex;
        }
        #[cfg(feature = "use-semaphore")]
        {
            return SyncStrategy::Semaphore;
        }
        return SyncStrategy::Futex;
    }

    pub fn open_aux(path: &str, adaptive_poll_spins: Option<u32>) -> Result<Self> {
        return Self::open(path, Self::get_default_strategy(), adaptive_poll_spins);
    }

    pub fn open(
        path: &str,
        strategy: SyncStrategy,
        adaptive_poll_spins: Option<u32>,
    ) -> Result<Self> {
        let shmem = ShmemConf::new()
            .flink(path)
            .open()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Failed to open shmem: {}", e)))?;

        let base_ptr = shmem.as_ptr();
        let header = base_ptr as *mut GenericHeader;
        let buffer_size;

        // 校验 Header
        unsafe {
            if (*header).magic.load(Ordering::Acquire) != RING_BUFFER_MAGIC {
                return Err(Error::new(ErrorKind::InvalidData, "Invalid magic number"));
            }
            if (*header).version.load(Ordering::Acquire) != RING_BUFFER_VERSION {
                return Err(Error::new(ErrorKind::InvalidData, "Incompatible version"));
            }
            buffer_size = (*header).buffer_size as usize;
        }

        // 根据 Header 信息计算偏移
        let generic_header_size = size_of::<GenericHeader>();
        let backend_header_align = strategy.backend_align();
        let backend_offset = align_up(generic_header_size, backend_header_align);
        let messages_offset = align_up(
            backend_offset + strategy.backend_size(),
            std::mem::align_of::<MessageSlot>(),
        );
        let messages_size = buffer_size * size_of::<MessageSlot>();
        let commands_offset = align_up(
            messages_offset + messages_size,
            std::mem::align_of::<SharedCommand>(),
        );

        let backend_ptr = unsafe { base_ptr.add(backend_offset) };
        let message_slots = unsafe { base_ptr.add(messages_offset) as *mut MessageSlot };
        let cmd_buffer_start = unsafe { base_ptr.add(commands_offset) as *mut SharedCommand };

        // 创建并初始化后端（作为打开者）
        let mut backend = Self::new_backend(strategy);
        backend.init(false, backend_ptr)?;

        Ok(Self {
            shmem,
            header,
            message_slots,
            cmd_buffer_start,
            is_creator: false,
            backend,
            adaptive_poll_spins: adaptive_poll_spins.unwrap_or(DEFAULT_ADAPTIVE_POLL_SPINS),
        })
    }

    // --- 修改点 3：修改 `new_backend` 的返回类型 ---
    /// 工厂方法，根据枚举创建对应的后端实例，并包装在 AnySyncBackend 枚举中。
    fn new_backend(strategy: SyncStrategy) -> AnySyncBackend {
        match strategy {
            #[cfg(feature = "futex")]
            SyncStrategy::Futex => {
                AnySyncBackend::Futex(crate::backends::futex::FutexBackend::new())
            }

            #[cfg(feature = "semaphore")]
            SyncStrategy::Semaphore => {
                AnySyncBackend::Semaphore(crate::backends::semaphore::SemaphoreBackend::new())
            }

            #[cfg(feature = "eventfd")]
            SyncStrategy::EventFd => {
                AnySyncBackend::EventFd(crate::backends::eventfd::EventFdBackend::new())
            }

            // 如果 SyncStrategy 是空枚举，这个 match 是详尽的，不需要通配符。
            // 否则，添加一个处理分支。
            #[cfg(not(any(feature = "futex", feature = "semaphore", feature = "eventfd")))]
            _ => unreachable!(), // 如果 SyncStrategy 为空，此分支永远不会到达
        }
    }

    // --- 消息 API ---
    pub fn try_write_message(&self, message: &SharedMessage) -> Result<bool> {
        if self.is_destroyed() {
            return Err(Error::new(ErrorKind::BrokenPipe, "Buffer is destroyed"));
        }

        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).read_idx.load(Ordering::Acquire);

            if write_idx.wrapping_sub(read_idx) >= self.buffer_size() {
                return Ok(false); // 缓冲区已满
            }

            let slot_idx = (write_idx & self.buffer_mask()) as usize;
            let slot = &mut *self.message_slots.add(slot_idx);

            // 构造 Slot 并写入
            *slot = MessageSlot {
                timestamp: now_millis(),
                checksum: calculate_message_checksum(message),
                _padding: 0,
                message: *message,
            };

            // 使用 Release 内存顺序确保写入内容对其他核心可见
            (*self.header)
                .last_timestamp
                .store(slot.timestamp, Ordering::Release);
            (*self.header)
                .write_idx
                .store(write_idx.wrapping_add(1), Ordering::Release);
        }

        self.backend.signal_message()?;
        Ok(true)
    }

    pub fn try_read_next_message(&self) -> Result<Option<SharedMessage>> {
        if self.is_destroyed() {
            return Ok(None);
        }

        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let read_idx = (*self.header).read_idx.load(Ordering::Relaxed);

            if read_idx == write_idx {
                return Ok(None);
            } // 缓冲区为空

            let slot_idx = (read_idx & self.buffer_mask()) as usize;
            let slot = &*self.message_slots.add(slot_idx);

            if calculate_message_checksum(&slot.message) != slot.checksum {
                // 清理并前进，避免卡死
                (*self.header)
                    .read_idx
                    .store(read_idx.wrapping_add(1), Ordering::Release);
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "Checksum mismatch on read",
                ));
            }

            let message = slot.message;
            (*self.header)
                .read_idx
                .store(read_idx.wrapping_add(1), Ordering::Release);
            Ok(Some(message))
        }
    }

    pub fn try_read_latest_message(&self) -> Result<Option<SharedMessage>> {
        if self.is_destroyed() {
            return Ok(None);
        }

        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let read_idx = (*self.header).read_idx.load(Ordering::Relaxed);

            if read_idx == write_idx {
                return Ok(None);
            }

            // 跳到最新的消息
            let new_read_idx = write_idx.wrapping_sub(1);

            let slot_idx = (new_read_idx & self.buffer_mask()) as usize;
            let slot = &*self.message_slots.add(slot_idx);

            if calculate_message_checksum(&slot.message) != slot.checksum {
                // 如果最新的也损坏了，就没办法了
                (*self.header).read_idx.store(write_idx, Ordering::Release); // 清空
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "Latest message checksum mismatch",
                ));
            }

            let message = slot.message;
            // 将 read_idx 更新到 write_idx，表示消费了所有消息
            (*self.header).read_idx.store(write_idx, Ordering::Release);
            Ok(Some(message))
        }
    }

    // --- 命令 API ---
    pub fn send_command(&self, command: SharedCommand) -> Result<bool> {
        if self.is_destroyed() {
            return Err(Error::new(ErrorKind::BrokenPipe, "Buffer is destroyed"));
        }

        unsafe {
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Acquire);

            if write_idx.wrapping_sub(read_idx) >= CMD_BUFFER_SIZE as u32 {
                return Ok(false); // 命令队列已满
            }

            let slot_idx = (write_idx & self.cmd_buffer_mask()) as usize;
            *self.cmd_buffer_start.add(slot_idx) = command;

            (*self.header)
                .cmd_write_idx
                .store(write_idx.wrapping_add(1), Ordering::Release);
        }

        self.backend.signal_command()?;
        Ok(true)
    }

    pub fn receive_command(&self) -> Option<SharedCommand> {
        if self.is_destroyed() {
            return None;
        }

        unsafe {
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Acquire);
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Relaxed);

            if read_idx == write_idx {
                return None;
            } // 命令队列为空

            let slot_idx = (read_idx & self.cmd_buffer_mask()) as usize;
            let command = *self.cmd_buffer_start.add(slot_idx);

            (*self.header)
                .cmd_read_idx
                .store(read_idx.wrapping_add(1), Ordering::Release);
            Some(command)
        }
    }

    // --- 等待与状态查询 API ---
    pub fn wait_for_message(&self, timeout: Option<Duration>) -> Result<bool> {
        if self.is_destroyed() {
            return Ok(false);
        }
        self.backend
            .wait_for_message(|| self.has_message(), self.adaptive_poll_spins, timeout)
    }

    pub fn wait_for_command(&self, timeout: Option<Duration>) -> Result<bool> {
        if self.is_destroyed() {
            return Ok(false);
        }
        self.backend
            .wait_for_command(|| self.has_command(), self.adaptive_poll_spins, timeout)
    }

    #[inline]
    pub fn is_destroyed(&self) -> bool {
        unsafe { (*self.header).is_destroyed.load(Ordering::Acquire) }
    }

    pub fn has_message(&self) -> bool {
        !self.is_destroyed() && self.available_messages() > 0
    }

    pub fn available_messages(&self) -> usize {
        if self.is_destroyed() {
            return 0;
        }
        unsafe {
            (*self.header)
                .write_idx
                .load(Ordering::Acquire)
                .wrapping_sub((*self.header).read_idx.load(Ordering::Acquire)) as usize
        }
    }

    pub fn has_command(&self) -> bool {
        !self.is_destroyed() && self.available_commands() > 0
    }

    pub fn available_commands(&self) -> usize {
        if self.is_destroyed() {
            return 0;
        }
        unsafe {
            (*self.header)
                .cmd_write_idx
                .load(Ordering::Acquire)
                .wrapping_sub((*self.header).cmd_read_idx.load(Ordering::Acquire))
                as usize
        }
    }
}

impl Drop for SharedRingBuffer {
    fn drop(&mut self) {
        // 由于 as_ptr() 可能在某些 shmem 版本中不存在，我们暂时假设它返回一个非空指针
        // 如果你的 shmem 库版本没有 as_ptr()，你可能需要另一种方式检查 shmem 的有效性
        // 或者直接执行清理逻辑。
        if !self.header.is_null() {
            // 使用 header 指针检查有效性
            // 标记为销毁状态
            if !self.is_destroyed() {
                unsafe {
                    (*self.header).is_destroyed.store(true, Ordering::Release);
                }
            }
            // 委托给后端进行清理，并唤醒所有等待者
            self.backend.cleanup(self.is_creator);
        }

        // 如果是创建者，负责删除共享内存的链接文件
        if self.is_creator {
            if let Some(path) = self.shmem.get_flink_path() {
                info!("(Creator) Removing shmem flink: {:?}", path);
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
