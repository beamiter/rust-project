use std::hint;
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(feature = "use-eventfd")]
use nix::poll::{poll, PollFd, PollFlags};
#[cfg(feature = "use-eventfd")]
use nix::sys::eventfd::EventFd;
#[cfg(feature = "use-eventfd")]
use nix::unistd;
#[cfg(feature = "use-eventfd")]
use std::os::unix::io::{AsRawFd, BorrowedFd};
#[cfg(feature = "use-eventfd")]
use std::sync::atomic::AtomicI32;

#[cfg(feature = "use-semaphore")]
use libc::{sem_destroy, sem_init, sem_post, sem_t, sem_timedwait, sem_wait};

use crate::shared_message::{SharedCommand, SharedMessage};
use shared_memory::{Shmem, ShmemConf};

// =============================================================================
// 条件编译的同步原语定义
// =============================================================================

#[cfg(feature = "use-eventfd")]
struct EventFdWrapper {
    event_fd: EventFd,
}

#[cfg(feature = "use-eventfd")]
impl EventFdWrapper {
    fn new() -> Result<Self> {
        Ok(Self {
            event_fd: EventFd::from_value(0)?,
        })
    }

    fn as_raw_fd(&self) -> i32 {
        self.event_fd.as_raw_fd()
    }

    fn signal(&self) -> Result<()> {
        match unistd::write(&self.event_fd, &1u64.to_ne_bytes()) {
            Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(()),
            Err(e) => Err(Error::new(
                ErrorKind::Other,
                format!("Failed to signal eventfd: {}", e),
            )),
        }
    }

    #[allow(dead_code)]
    fn clear(&self) -> Result<()> {
        let mut buf = [0u8; 8];
        match unistd::read(&self.event_fd, &mut buf) {
            Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(()),
            Err(e) => Err(Error::new(
                ErrorKind::Other,
                format!("Failed to read from eventfd: {}", e),
            )),
        }
    }
}

#[cfg(feature = "use-semaphore")]
struct SemaphoreWrapper {
    sem: *mut sem_t,
    _owned: bool,
}

#[cfg(feature = "use-semaphore")]
unsafe impl Send for SemaphoreWrapper {}
#[cfg(feature = "use-semaphore")]
unsafe impl Sync for SemaphoreWrapper {}

#[cfg(feature = "use-semaphore")]
impl SemaphoreWrapper {
    #[allow(dead_code)]
    fn new() -> Result<Self> {
        let sem = unsafe {
            let sem_ptr = libc::malloc(size_of::<sem_t>()) as *mut sem_t;
            if sem_ptr.is_null() {
                return Err(Error::new(
                    ErrorKind::OutOfMemory,
                    "Failed to allocate semaphore",
                ));
            }

            // 初始化为进程间共享信号量，初始值为0
            if sem_init(sem_ptr, 1, 0) != 0 {
                libc::free(sem_ptr as *mut libc::c_void);
                return Err(Error::last_os_error());
            }

            sem_ptr
        };

        Ok(Self { sem, _owned: true })
    }

    fn from_shared(sem_ptr: *mut sem_t) -> Self {
        Self {
            sem: sem_ptr,
            _owned: false,
        }
    }

    #[allow(dead_code)]
    fn as_ptr(&self) -> *mut sem_t {
        self.sem
    }

    fn signal(&self) -> Result<()> {
        unsafe {
            if sem_post(self.sem) != 0 {
                let err = Error::last_os_error();
                // EOVERFLOW 表示信号量值已达到最大值，可以忽略
                if err.raw_os_error() == Some(libc::EOVERFLOW) {
                    return Ok(());
                }
                return Err(err);
            }
        }
        Ok(())
    }

    fn wait_timeout(&self, timeout: Option<Duration>) -> Result<bool> {
        unsafe {
            match timeout {
                Some(duration) => {
                    let deadline = SystemTime::now() + duration;
                    let deadline_since_epoch = deadline
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| Error::new(ErrorKind::InvalidInput, "Invalid time"))?;

                    let ts = libc::timespec {
                        tv_sec: deadline_since_epoch.as_secs() as libc::time_t,
                        tv_nsec: deadline_since_epoch.subsec_nanos() as libc::c_long,
                    };

                    let result = sem_timedwait(self.sem, &ts);
                    if result == 0 {
                        Ok(true)
                    } else {
                        let err = Error::last_os_error();
                        match err.raw_os_error() {
                            Some(libc::ETIMEDOUT) => Ok(false),
                            Some(libc::EINTR) => Ok(false), // 被中断，视为超时
                            _ => Err(err),
                        }
                    }
                }
                None => {
                    if sem_wait(self.sem) == 0 {
                        Ok(true)
                    } else {
                        let err = Error::last_os_error();
                        if err.raw_os_error() == Some(libc::EINTR) {
                            Ok(false) // 被中断，返回false让调用者重试
                        } else {
                            Err(err)
                        }
                    }
                }
            }
        }
    }
}

#[cfg(feature = "use-semaphore")]
impl Drop for SemaphoreWrapper {
    fn drop(&mut self) {
        if self._owned && !self.sem.is_null() {
            unsafe {
                sem_destroy(self.sem);
                libc::free(self.sem as *mut libc::c_void);
            }
        }
    }
}

// =============================================================================
// 条件编译类型别名
// =============================================================================

#[cfg(feature = "use-eventfd")]
type SyncPrimitive = Arc<EventFdWrapper>;
#[cfg(feature = "use-semaphore")]
type SyncPrimitive = Arc<SemaphoreWrapper>;

// =============================================================================
// 条件编译的 Header 结构
// =============================================================================

#[cfg(feature = "use-eventfd")]
#[repr(C, align(128))]
#[derive(Debug)]
struct RingBufferHeader {
    magic: AtomicU64,
    version: AtomicU64,
    write_idx: AtomicU32,
    read_idx: AtomicU32,
    buffer_size: u32,
    message_size: u32,
    last_timestamp: AtomicU64,
    message_event_fd: AtomicI32,
    command_event_fd: AtomicI32,
    cmd_write_idx: AtomicU32,
    cmd_read_idx: AtomicU32,
    is_destroyed: AtomicBool,
}

#[cfg(feature = "use-semaphore")]
#[repr(C, align(128))]
#[derive(Debug)]
struct RingBufferHeader {
    magic: AtomicU64,
    version: AtomicU64,
    write_idx: AtomicU32,
    read_idx: AtomicU32,
    buffer_size: u32,
    message_size: u32,
    last_timestamp: AtomicU64,
    // 直接嵌入信号量到共享内存中
    message_sem: sem_t,
    command_sem: sem_t,
    cmd_write_idx: AtomicU32,
    cmd_read_idx: AtomicU32,
    is_destroyed: AtomicBool,
    _padding: [u8; 8], // 确保对齐
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MessageSlot {
    timestamp: u64,
    checksum: u32,
    _padding: u32,
    message: SharedMessage,
}

const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646;
const RING_BUFFER_VERSION: u64 = 5; // 增加版本号表示支持两种模式
const DEFAULT_BUFFER_SIZE: usize = 16;
const CMD_BUFFER_SIZE: usize = 16;
const BUFFER_MASK: u32 = (DEFAULT_BUFFER_SIZE as u32) - 1;
const CMD_BUFFER_MASK: u32 = (CMD_BUFFER_SIZE as u32) - 1;
const DEFAULT_ADAPTIVE_POLL_SPINS: u32 = 4000;

// =============================================================================
// 主要结构体
// =============================================================================

pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut RingBufferHeader,
    message_slots: *mut MessageSlot,
    cmd_buffer_start: *mut SharedCommand,
    is_creator: bool,

    // 条件编译的同步原语
    #[cfg(feature = "use-eventfd")]
    message_event_fd: Option<SyncPrimitive>,
    #[cfg(feature = "use-eventfd")]
    command_event_fd: Option<SyncPrimitive>,

    #[cfg(feature = "use-semaphore")]
    message_sem: Option<SyncPrimitive>,
    #[cfg(feature = "use-semaphore")]
    command_sem: Option<SyncPrimitive>,

    adaptive_poll_spins: u32,
}

unsafe impl Send for SharedRingBuffer {}
unsafe impl Sync for SharedRingBuffer {}

impl SharedRingBuffer {
    pub fn create(
        path: &str,
        buffer_size: Option<usize>,
        adaptive_poll_spins: Option<u32>,
    ) -> Result<Self> {
        let buffer_size = buffer_size.unwrap_or(DEFAULT_BUFFER_SIZE);
        let adaptive_poll_spins = adaptive_poll_spins.unwrap_or(DEFAULT_ADAPTIVE_POLL_SPINS);

        if !buffer_size.is_power_of_two() || !CMD_BUFFER_SIZE.is_power_of_two() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "buffer_size and CMD_BUFFER_SIZE must be powers of 2",
            ));
        }

        // 计算内存布局
        let header_size = size_of::<RingBufferHeader>();
        let message_slot_size = size_of::<MessageSlot>();
        let cmd_size = size_of::<SharedCommand>();

        let messages_offset = align_up(header_size, std::mem::align_of::<MessageSlot>());
        let messages_size = buffer_size * message_slot_size;
        let commands_offset = align_up(
            messages_offset + messages_size,
            std::mem::align_of::<SharedCommand>(),
        );
        let commands_size = CMD_BUFFER_SIZE * cmd_size;
        let total_size = commands_offset + commands_size;

        let shmem = ShmemConf::new()
            .size(total_size)
            .flink(path)
            .force_create_flink()
            .create()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Failed to create shmem: {}", e)))?;

        let header = shmem.as_ptr() as *mut RingBufferHeader;
        let message_slots = unsafe { shmem.as_ptr().add(messages_offset) as *mut MessageSlot };
        let cmd_buffer_start = unsafe { shmem.as_ptr().add(commands_offset) as *mut SharedCommand };

        // 条件编译：创建同步原语
        #[cfg(feature = "use-eventfd")]
        let (message_sync, command_sync) = {
            let message_efd = Arc::new(EventFdWrapper::new()?);
            let command_efd = Arc::new(EventFdWrapper::new()?);

            // 初始化header
            unsafe {
                (*header)
                    .message_event_fd
                    .store(message_efd.as_raw_fd(), Ordering::Release);
                (*header)
                    .command_event_fd
                    .store(command_efd.as_raw_fd(), Ordering::Release);
            }

            (Some(message_efd), Some(command_efd))
        };

        #[cfg(feature = "use-semaphore")]
        let (message_sync, command_sync) = {
            // 初始化共享内存中的信号量
            unsafe {
                let message_sem_ptr = &mut (*header).message_sem as *mut sem_t;
                let command_sem_ptr = &mut (*header).command_sem as *mut sem_t;

                if sem_init(message_sem_ptr, 1, 0) != 0 {
                    return Err(Error::last_os_error());
                }
                if sem_init(command_sem_ptr, 1, 0) != 0 {
                    sem_destroy(message_sem_ptr);
                    return Err(Error::last_os_error());
                }

                let message_wrapper = Arc::new(SemaphoreWrapper::from_shared(message_sem_ptr));
                let command_wrapper = Arc::new(SemaphoreWrapper::from_shared(command_sem_ptr));

                (Some(message_wrapper), Some(command_wrapper))
            }
        };

        // 初始化通用header字段
        unsafe {
            (*header).magic.store(RING_BUFFER_MAGIC, Ordering::Release);
            (*header)
                .version
                .store(RING_BUFFER_VERSION, Ordering::Release);
            (*header).write_idx.store(0, Ordering::Release);
            (*header).read_idx.store(0, Ordering::Release);
            (*header).buffer_size = buffer_size as u32;
            (*header).message_size = message_slot_size as u32;
            (*header).last_timestamp.store(0, Ordering::Release);
            (*header).cmd_write_idx.store(0, Ordering::Release);
            (*header).cmd_read_idx.store(0, Ordering::Release);
            (*header).is_destroyed.store(false, Ordering::Release);
        }

        Ok(Self {
            shmem,
            header,
            message_slots,
            cmd_buffer_start,
            is_creator: true,

            #[cfg(feature = "use-eventfd")]
            message_event_fd: message_sync,
            #[cfg(feature = "use-eventfd")]
            command_event_fd: command_sync,

            #[cfg(feature = "use-semaphore")]
            message_sem: message_sync,
            #[cfg(feature = "use-semaphore")]
            command_sem: command_sync,

            adaptive_poll_spins,
        })
    }

    pub fn open(path: &str, adaptive_poll_spins: Option<u32>) -> Result<Self> {
        let shmem = ShmemConf::new()
            .flink(path)
            .open()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Failed to open shmem: {}", e)))?;

        let header = shmem.as_ptr() as *mut RingBufferHeader;
        let buffer_size;

        unsafe {
            if (*header).magic.load(Ordering::Acquire) != RING_BUFFER_MAGIC {
                return Err(Error::new(ErrorKind::InvalidData, "Invalid magic number"));
            }
            if (*header).version.load(Ordering::Acquire) != RING_BUFFER_VERSION {
                return Err(Error::new(ErrorKind::InvalidData, "Incompatible version"));
            }
            buffer_size = (*header).buffer_size as usize;
        }

        // 重新计算偏移量
        let header_size = size_of::<RingBufferHeader>();
        let message_slot_size = size_of::<MessageSlot>();
        let messages_offset = align_up(header_size, std::mem::align_of::<MessageSlot>());
        let messages_size = buffer_size * message_slot_size;
        let commands_offset = align_up(
            messages_offset + messages_size,
            std::mem::align_of::<SharedCommand>(),
        );

        let message_slots = unsafe { shmem.as_ptr().add(messages_offset) as *mut MessageSlot };
        let cmd_buffer_start = unsafe { shmem.as_ptr().add(commands_offset) as *mut SharedCommand };

        // 条件编译：连接到现有同步原语
        #[cfg(feature = "use-eventfd")]
        let (message_sync, command_sync) = (None, None); // 非创建者不拥有EventFd

        #[cfg(feature = "use-semaphore")]
        let (message_sync, command_sync) = {
            unsafe {
                let message_sem_ptr = &mut (*header).message_sem as *mut sem_t;
                let command_sem_ptr = &mut (*header).command_sem as *mut sem_t;

                let message_wrapper = Arc::new(SemaphoreWrapper::from_shared(message_sem_ptr));
                let command_wrapper = Arc::new(SemaphoreWrapper::from_shared(command_sem_ptr));

                (Some(message_wrapper), Some(command_wrapper))
            }
        };

        Ok(Self {
            shmem,
            header,
            message_slots,
            cmd_buffer_start,
            is_creator: false,

            #[cfg(feature = "use-eventfd")]
            message_event_fd: message_sync,
            #[cfg(feature = "use-eventfd")]
            command_event_fd: command_sync,

            #[cfg(feature = "use-semaphore")]
            message_sem: message_sync,
            #[cfg(feature = "use-semaphore")]
            command_sem: command_sync,

            adaptive_poll_spins: adaptive_poll_spins.unwrap_or(DEFAULT_ADAPTIVE_POLL_SPINS),
        })
    }

    // 检查是否已被销毁
    fn is_destroyed(&self) -> bool {
        unsafe { (*self.header).is_destroyed.load(Ordering::Acquire) }
    }

    pub fn try_write_message(&self, message: &SharedMessage) -> Result<bool> {
        if self.is_destroyed() {
            return Err(Error::new(
                ErrorKind::BrokenPipe,
                "Buffer has been destroyed",
            ));
        }

        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).read_idx.load(Ordering::Acquire);

            if write_idx.wrapping_sub(read_idx) == (*self.header).buffer_size {
                return Ok(false);
            }

            let slot_idx = (write_idx & BUFFER_MASK) as usize;
            let slot = &mut *self.message_slots.add(slot_idx);

            let message_bytes = std::slice::from_raw_parts(
                message as *const SharedMessage as *const u8,
                size_of::<SharedMessage>(),
            );
            let checksum = calculate_checksum(message_bytes);

            let new_slot = MessageSlot {
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                checksum,
                _padding: 0,
                message: *message,
            };

            *slot = new_slot;

            (*self.header)
                .last_timestamp
                .store(new_slot.timestamp, Ordering::Release);
            (*self.header)
                .write_idx
                .store(write_idx.wrapping_add(1), Ordering::Release);
        }

        self.signal_message_event()?;
        Ok(true)
    }

    pub fn try_read_latest_message(&self) -> Result<Option<SharedMessage>> {
        if self.is_destroyed() {
            return Ok(None);
        }

        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let mut read_idx = (*self.header).read_idx.load(Ordering::Relaxed);

            if read_idx == write_idx {
                return Ok(None);
            }

            if write_idx.wrapping_sub(read_idx) > 1 {
                read_idx = write_idx.wrapping_sub(1);
            }

            let slot_idx = (read_idx & BUFFER_MASK) as usize;
            let slot = &*self.message_slots.add(slot_idx);

            let message_bytes = std::slice::from_raw_parts(
                &slot.message as *const SharedMessage as *const u8,
                size_of::<SharedMessage>(),
            );
            if calculate_checksum(message_bytes) != slot.checksum {
                return Err(Error::new(ErrorKind::InvalidData, "Checksum mismatch"));
            }

            let message = slot.message;

            (*self.header)
                .read_idx
                .store(read_idx.wrapping_add(1), Ordering::Release);

            Ok(Some(message))
        }
    }

    pub fn send_command(&self, command: SharedCommand) -> Result<bool> {
        if self.is_destroyed() {
            return Err(Error::new(
                ErrorKind::BrokenPipe,
                "Buffer has been destroyed",
            ));
        }

        unsafe {
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Acquire);

            if write_idx.wrapping_sub(read_idx) == CMD_BUFFER_SIZE as u32 {
                return Ok(false);
            }

            let slot_idx = (write_idx & CMD_BUFFER_MASK) as usize;
            let cmd_slot = &mut *self.cmd_buffer_start.add(slot_idx);

            *cmd_slot = command;

            (*self.header)
                .cmd_write_idx
                .store(write_idx.wrapping_add(1), Ordering::Release);
        }

        self.signal_command_event()?;
        Ok(true)
    }

    pub fn receive_command(&self) -> Option<SharedCommand> {
        if self.is_destroyed() {
            return None;
        }

        unsafe {
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Relaxed);
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Acquire);

            if read_idx == write_idx {
                return None;
            }

            let slot_idx = (read_idx & CMD_BUFFER_MASK) as usize;
            let cmd_slot = &*self.cmd_buffer_start.add(slot_idx);
            let command = *cmd_slot;

            (*self.header)
                .cmd_read_idx
                .store(read_idx.wrapping_add(1), Ordering::Release);

            Some(command)
        }
    }

    pub fn wait_for_message(&self, timeout: Option<Duration>) -> Result<bool> {
        if self.is_destroyed() {
            return Ok(false);
        }
        self.wait_with_adaptive_polling(true, timeout, || self.has_message())
    }

    pub fn wait_for_command(&self, timeout: Option<Duration>) -> Result<bool> {
        if self.is_destroyed() {
            return Ok(false);
        }
        self.wait_with_adaptive_polling(false, timeout, || self.has_command())
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

    // 统一的等待方法，处理两种同步原语
    fn wait_with_adaptive_polling(
        &self,
        is_message: bool,
        timeout: Option<Duration>,
        has_data: impl Fn() -> bool,
    ) -> Result<bool> {
        // 1. 自适应轮询阶段
        for _ in 0..self.adaptive_poll_spins {
            if has_data() || self.is_destroyed() {
                return Ok(has_data());
            }
            hint::spin_loop();
        }

        if has_data() || self.is_destroyed() {
            return Ok(has_data());
        }

        // 2. 阻塞等待阶段 - 条件编译
        #[cfg(feature = "use-eventfd")]
        {
            self.wait_on_eventfd(is_message, timeout, has_data)
        }

        #[cfg(feature = "use-semaphore")]
        {
            self.wait_on_semaphore(is_message, timeout, has_data)
        }
    }

    // EventFd 等待实现
    #[cfg(feature = "use-eventfd")]
    fn wait_on_eventfd(
        &self,
        is_message: bool,
        timeout: Option<Duration>,
        has_data: impl Fn() -> bool,
    ) -> Result<bool> {
        let fd = unsafe {
            if is_message {
                (*self.header).message_event_fd.load(Ordering::Acquire)
            } else {
                (*self.header).command_event_fd.load(Ordering::Acquire)
            }
        };

        if fd < 0 {
            return Ok(has_data());
        }

        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let poll_fd = PollFd::new(borrowed_fd, PollFlags::POLLIN);
        let timeout_ms = timeout.map_or(0, |d| d.as_millis() as i32);

        match poll(&mut [poll_fd], timeout_ms as u16) {
            Ok(num_events) => {
                if num_events == 0 {
                    return Ok(has_data());
                }

                let mut buf = [0u8; 8];
                let _ = unistd::read(borrowed_fd, &mut buf);
                Ok(true)
            }
            Err(_) => Ok(has_data()),
        }
    }

    // Semaphore 等待实现
    #[cfg(feature = "use-semaphore")]
    fn wait_on_semaphore(
        &self,
        is_message: bool,
        timeout: Option<Duration>,
        has_data: impl Fn() -> bool,
    ) -> Result<bool> {
        let sem = if is_message {
            &self.message_sem
        } else {
            &self.command_sem
        };

        if let Some(sem) = sem {
            match sem.wait_timeout(timeout) {
                Ok(true) => Ok(true),
                Ok(false) => Ok(has_data()), // 超时或被中断
                Err(_) => Ok(has_data()),    // 错误时返回当前状态
            }
        } else {
            Ok(has_data())
        }
    }

    // 条件编译的信号发送
    fn signal_message_event(&self) -> Result<()> {
        #[cfg(feature = "use-eventfd")]
        {
            if let Some(ref efd) = self.message_event_fd {
                efd.signal()
            } else {
                Ok(())
            }
        }

        #[cfg(feature = "use-semaphore")]
        {
            if let Some(ref sem) = self.message_sem {
                sem.signal()
            } else {
                Ok(())
            }
        }
    }

    fn signal_command_event(&self) -> Result<()> {
        #[cfg(feature = "use-eventfd")]
        {
            if let Some(ref efd) = self.command_event_fd {
                efd.signal()
            } else {
                Ok(())
            }
        }

        #[cfg(feature = "use-semaphore")]
        {
            if let Some(ref sem) = self.command_sem {
                sem.signal()
            } else {
                Ok(())
            }
        }
    }
}

impl Drop for SharedRingBuffer {
    fn drop(&mut self) {
        if self.is_creator {
            println!("(Creator) Cleaning up resources...");

            // 标记为已销毁
            unsafe {
                (*self.header).is_destroyed.store(true, Ordering::Release);
            }

            // 条件编译的清理
            #[cfg(feature = "use-eventfd")]
            {
                self.message_event_fd = None;
                self.command_event_fd = None;
            }

            #[cfg(feature = "use-semaphore")]
            {
                // 销毁共享内存中的信号量
                unsafe {
                    sem_destroy(&mut (*self.header).message_sem);
                    sem_destroy(&mut (*self.header).command_sem);
                }
                self.message_sem = None;
                self.command_sem = None;
            }

            // 删除共享内存文件
            if let Some(path) = self.shmem.get_flink_path() {
                println!("(Creator) Removing shmem flink: {:?}", path);
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn calculate_checksum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |sum, &b| sum.wrapping_add(b as u32))
}

// =============================================================================
// 测试代码
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_message::SharedMessage;

    #[test]
    fn test_conditional_compilation() {
        println!(
            "Testing with feature: {}",
            if cfg!(feature = "use-semaphore") {
                "semaphore"
            } else {
                "eventfd"
            }
        );

        let shared_path = "/tmp/test_conditional_buffer";
        let _ = std::fs::remove_file(shared_path);

        let buffer = SharedRingBuffer::create(shared_path, None, Some(100)).unwrap();

        // 基本功能测试
        let mut message = SharedMessage::default();
        message.get_monitor_info_mut().monitor_num = 42;

        assert!(buffer.try_write_message(&message).unwrap());
        assert!(buffer.has_message());

        let received = buffer.try_read_latest_message().unwrap().unwrap();
        assert_eq!(received.get_monitor_info().monitor_num, 42);

        println!("Conditional compilation test passed!");
    }

    #[test]
    fn test_performance_comparison() {
        use std::time::Instant;

        let iterations = 1000;
        let shared_path = "/tmp/perf_test_buffer";
        let _ = std::fs::remove_file(shared_path);

        let buffer = SharedRingBuffer::create(shared_path, None, Some(0)).unwrap();

        let start = Instant::now();
        for i in 0..iterations {
            let mut message = SharedMessage::default();
            message.get_monitor_info_mut().monitor_num = i;
            buffer.try_write_message(&message).unwrap();

            let _ = buffer.try_read_latest_message().unwrap();
        }
        let duration = start.elapsed();

        println!(
            "Feature: {}",
            if cfg!(feature = "use-semaphore") {
                "semaphore"
            } else {
                "eventfd"
            }
        );
        println!("Time for {} iterations: {:?}", iterations, duration);
        println!(
            "Average per operation: {:?}",
            duration / iterations.try_into().unwrap()
        );
    }
}
