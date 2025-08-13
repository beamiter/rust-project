use cfg_if::cfg_if;
use std::hint;
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::shared_message::{SharedCommand, SharedMessage};
use shared_memory::{Shmem, ShmemConf};

// =============================================================================
// 使用 cfg_if! 统一定义同步原语
// =============================================================================

cfg_if! {
    if #[cfg(all(feature = "use-eventfd", feature = "use-semaphore"))] {
        compile_error!("Cannot enable both 'use-eventfd' and 'use-semaphore'. Please choose one.");
    } else if #[cfg(feature = "use-eventfd")] {
        // =============================
        // 使用 eventfd 的实现
        // =============================

        use nix::poll::{poll, PollFd, PollFlags};
        use nix::sys::eventfd::EventFd;
        use nix::unistd;
        use std::os::unix::io::{AsRawFd, BorrowedFd};
        use std::sync::atomic::AtomicI32;

        struct SyncPrimitiveWrapper {
            event_fd: EventFd,
        }

        impl SyncPrimitiveWrapper {
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

        type SyncPrimitive = Arc<SyncPrimitiveWrapper>;

        #[repr(C, align(128))]
        #[derive(Debug)]
        pub struct RingBufferHeader {
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

    } else if #[cfg(feature = "use-semaphore")] {
        // =============================
        // 使用 semaphore 的实现
        // =============================

        use libc::{sem_destroy, sem_init, sem_post, sem_t, sem_timedwait, sem_wait};

        struct SyncPrimitiveWrapper {
            sem: *mut sem_t,
            _owned: bool,
        }

        unsafe impl Send for SyncPrimitiveWrapper {}
        unsafe impl Sync for SyncPrimitiveWrapper {}

        impl SyncPrimitiveWrapper {
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
                                    Some(libc::EINTR) => Ok(false),
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
                                    Ok(false)
                                } else {
                                    Err(err)
                                }
                            }
                        }
                    }
                }
            }
        }

        impl Drop for SyncPrimitiveWrapper {
            fn drop(&mut self) {
                if self._owned && !self.sem.is_null() {
                    unsafe {
                        sem_destroy(self.sem);
                        libc::free(self.sem as *mut libc::c_void);
                    }
                }
            }
        }

        type SyncPrimitive = Arc<SyncPrimitiveWrapper>;

        #[repr(C, align(128))]
        #[derive(Debug)]
        pub struct RingBufferHeader {
            magic: AtomicU64,
            version: AtomicU64,
            write_idx: AtomicU32,
            read_idx: AtomicU32,
            buffer_size: u32,
            message_size: u32,
            last_timestamp: AtomicU64,
            message_sem: sem_t,
            command_sem: sem_t,
            cmd_write_idx: AtomicU32,
            cmd_read_idx: AtomicU32,
            is_destroyed: AtomicBool,
            _padding: [u8; 8],
        }

    } else {
        // 默认启用 eventfd
        compile_error!(
            "One of the features 'use-eventfd' or 'use-semaphore' must be enabled."
        );
    }
}

// =============================================================================
// 通用常量定义（不依赖 feature）
// =============================================================================

const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646;
const RING_BUFFER_VERSION: u64 = 5;
const DEFAULT_BUFFER_SIZE: usize = 16;
const CMD_BUFFER_SIZE: usize = 16;
const BUFFER_MASK: u32 = (DEFAULT_BUFFER_SIZE as u32) - 1;
const CMD_BUFFER_MASK: u32 = (CMD_BUFFER_SIZE as u32) - 1;
const DEFAULT_ADAPTIVE_POLL_SPINS: u32 = 4000;

// =============================================================================
// 主结构体定义（统一接口）
// =============================================================================

pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut RingBufferHeader,
    message_slots: *mut MessageSlot,
    cmd_buffer_start: *mut SharedCommand,
    is_creator: bool,

    // 同步原语（根据 feature 实际类型不同）
    message_sync: Option<SyncPrimitive>,
    command_sync: Option<SyncPrimitive>,

    adaptive_poll_spins: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MessageSlot {
    timestamp: u64,
    checksum: u32,
    _padding: u32,
    message: SharedMessage,
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

        // 创建同步原语（feature-specific）
        let (message_sync, command_sync) = {
            cfg_if! {
                if #[cfg(feature = "use-eventfd")] {
                    let message_efd = Arc::new(SyncPrimitiveWrapper::new()?);
                    let command_efd = Arc::new(SyncPrimitiveWrapper::new()?);

                    unsafe {
                        (*header)
                            .message_event_fd
                            .store(message_efd.as_raw_fd(), Ordering::Release);
                        (*header)
                            .command_event_fd
                            .store(command_efd.as_raw_fd(), Ordering::Release);
                    }

                    (Some(message_efd), Some(command_efd))
                } else if #[cfg(feature = "use-semaphore")] {
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

                        let message_wrapper = Arc::new(SyncPrimitiveWrapper::from_shared(message_sem_ptr));
                        let command_wrapper = Arc::new(SyncPrimitiveWrapper::from_shared(command_sem_ptr));

                        (Some(message_wrapper), Some(command_wrapper))
                    }
                }
            }
        };

        // 初始化通用 header 字段
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
            message_sync,
            command_sync,
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

        // 连接到已有同步原语
        let (message_sync, command_sync) = {
            cfg_if! {
                if #[cfg(feature = "use-eventfd")] {
                    (None, None) // 非创建者不持有 EventFd
                } else if #[cfg(feature = "use-semaphore")] {
                    unsafe {
                        let message_sem_ptr = &mut (*header).message_sem as *mut sem_t;
                        let command_sem_ptr = &mut (*header).command_sem as *mut sem_t;

                        let message_wrapper = Arc::new(SyncPrimitiveWrapper::from_shared(message_sem_ptr));
                        let command_wrapper = Arc::new(SyncPrimitiveWrapper::from_shared(command_sem_ptr));

                        (Some(message_wrapper), Some(command_wrapper))
                    }
                }
            }
        };

        Ok(Self {
            shmem,
            header,
            message_slots,
            cmd_buffer_start,
            is_creator: false,
            message_sync,
            command_sync,
            adaptive_poll_spins: adaptive_poll_spins.unwrap_or(DEFAULT_ADAPTIVE_POLL_SPINS),
        })
    }

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

        self.signal_message()?;
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

        self.signal_command()?;
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

    fn wait_with_adaptive_polling(
        &self,
        is_message: bool,
        timeout: Option<Duration>,
        has_data: impl Fn() -> bool,
    ) -> Result<bool> {
        for _ in 0..self.adaptive_poll_spins {
            if has_data() || self.is_destroyed() {
                return Ok(has_data());
            }
            hint::spin_loop();
        }

        if has_data() || self.is_destroyed() {
            return Ok(has_data());
        }

        cfg_if! {
            if #[cfg(feature = "use-eventfd")] {
                self.wait_on_eventfd(is_message, timeout, has_data)
            } else if #[cfg(feature = "use-semaphore")] {
                self.wait_on_semaphore(is_message, timeout, has_data)
            }
        }
    }

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

    #[cfg(feature = "use-semaphore")]
    fn wait_on_semaphore(
        &self,
        is_message: bool,
        timeout: Option<Duration>,
        has_data: impl Fn() -> bool,
    ) -> Result<bool> {
        let sem = if is_message {
            &self.message_sync
        } else {
            &self.command_sync
        };

        if let Some(sem) = sem {
            match sem.wait_timeout(timeout) {
                Ok(true) => Ok(true),
                Ok(false) => Ok(has_data()),
                Err(_) => Ok(has_data()),
            }
        } else {
            Ok(has_data())
        }
    }

    fn signal_message(&self) -> Result<()> {
        if let Some(ref sync) = self.message_sync {
            sync.signal()
        } else {
            Ok(())
        }
    }

    fn signal_command(&self) -> Result<()> {
        if let Some(ref sync) = self.command_sync {
            sync.signal()
        } else {
            Ok(())
        }
    }
}

impl Drop for SharedRingBuffer {
    fn drop(&mut self) {
        if self.is_creator {
            println!("(Creator) Cleaning up resources...");

            unsafe {
                (*self.header).is_destroyed.store(true, Ordering::Release);
            }

            cfg_if! {
                if #[cfg(feature = "use-eventfd")] {
                    self.message_sync = None;
                    self.command_sync = None;
                } else if #[cfg(feature = "use-semaphore")] {
                    unsafe {
                        sem_destroy(&mut (*self.header).message_sem);
                        sem_destroy(&mut (*self.header).command_sem);
                    }
                    self.message_sync = None;
                    self.command_sync = None;
                }
            }

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

#[cfg(test)]
mod thread_safety_tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_ring_buffer_concurrent_access_latest_read() {
        let shared_path = "/tmp/thread_safety_test_latest";
        let _ = std::fs::remove_file(shared_path);

        let buffer = Arc::new(SharedRingBuffer::create(shared_path, Some(16), Some(100)).unwrap());

        let mut handles = vec![];
        let write_duration = Duration::from_secs(2);

        // 启动持续写入线程
        for thread_id in 0..4 {
            let buffer_clone = Arc::clone(&buffer);
            let duration = write_duration;
            let handle = thread::spawn(move || {
                let start = std::time::Instant::now();
                let mut count = 0;

                while start.elapsed() < duration {
                    let mut message = SharedMessage::default();
                    message.get_monitor_info_mut().monitor_num = thread_id * 10000 + count;

                    if buffer_clone.try_write_message(&message).unwrap() {
                        count += 1;
                    }

                    thread::sleep(Duration::from_millis(10)); // 控制写入频率
                }
                println!("Writer thread {} wrote {} messages", thread_id, count);
            });
            handles.push(handle);
        }

        // 启动读取线程 - 适应"读最新"的语义
        let buffer_reader = Arc::clone(&buffer);
        let read_duration = write_duration + Duration::from_millis(500);
        let read_handle = thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut total_read = 0;
            let mut last_monitor_num = -1;

            while start.elapsed() < read_duration {
                if let Ok(Some(message)) = buffer_reader.try_read_latest_message() {
                    let monitor_num = message.get_monitor_info().monitor_num;

                    // 确保读到的是新消息
                    if monitor_num != last_monitor_num {
                        total_read += 1;
                        last_monitor_num = monitor_num;

                        if total_read % 10 == 0 {
                            println!(
                                "Read message #{}: monitor_num = {}",
                                total_read, monitor_num
                            );
                        }
                    }
                }

                thread::sleep(Duration::from_millis(20)); // 控制读取频率
            }
            println!("Reader completed: {} unique messages", total_read);
        });

        // 等待所有线程完成
        for handle in handles {
            handle.join().unwrap();
        }
        read_handle.join().unwrap();

        println!("Latest-read thread safety test passed!");
    }

    #[test]
    fn test_atomic_operations_safety() {
        let shared_path = "/tmp/atomic_safety_test";
        let _ = std::fs::remove_file(shared_path);

        let buffer = Arc::new(SharedRingBuffer::create(shared_path, Some(8), Some(50)).unwrap());

        let mut handles = vec![];

        // 多线程测试原子操作
        for _ in 0..8 {
            let buffer_clone = Arc::clone(&buffer);
            let handle = thread::spawn(move || {
                // 测试状态查询原子操作
                for _ in 0..1000 {
                    let _ = buffer_clone.available_messages();
                    let _ = buffer_clone.has_message();
                    let _ = buffer_clone.available_commands();
                    let _ = buffer_clone.has_command();
                    thread::yield_now();
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        println!("Atomic operations safety test passed!");
    }
}

#[cfg(test)]
mod sanitizer_tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_ring_buffer_thread_safety() {
        let shared_path = "/tmp/sanitizer_test_buffer";
        let _ = std::fs::remove_file(shared_path);

        let writer = Arc::new(SharedRingBuffer::create(shared_path, Some(16), Some(100)).unwrap());
        let reader = Arc::new(SharedRingBuffer::open(shared_path, Some(100)).unwrap());

        let writer_clone = Arc::clone(&writer);
        let reader_clone = Arc::clone(&reader);

        let write_finished = Arc::new(AtomicBool::new(false));
        let write_finished_clone = Arc::clone(&write_finished);

        // 写入线程
        let write_handle = thread::spawn(move || {
            for i in 0..1000 {
                let mut message = SharedMessage::default();
                message.get_monitor_info_mut().monitor_num = i;

                while !writer_clone.try_write_message(&message).unwrap() {
                    thread::yield_now();
                }

                if i % 100 == 0 {
                    thread::sleep(Duration::from_micros(1));
                }
            }
            write_finished_clone.store(true, Ordering::Release);
            println!("Write thread finished");
        });

        // 读取线程 - 修改逻辑以适应"读最新"语义
        let read_handle = thread::spawn(move || {
            let mut last_monitor_num = -1;
            let mut unique_reads = 0;
            let start_time = std::time::Instant::now();

            loop {
                // 检查写入是否完成且没有新消息
                if write_finished.load(Ordering::Acquire) && !reader_clone.has_message() {
                    println!("No more messages to read, breaking");
                    break;
                }

                // 超时保护
                if start_time.elapsed().as_secs() > 10 {
                    println!("Timeout reached, breaking");
                    break;
                }

                if let Ok(Some(message)) = reader_clone.try_read_latest_message() {
                    let monitor_num = message.get_monitor_info().monitor_num;

                    // 只计算真正的新消息
                    if monitor_num != last_monitor_num {
                        last_monitor_num = monitor_num;
                        unique_reads += 1;

                        if unique_reads % 50 == 0 {
                            println!(
                                "Read {} unique messages, latest: {}",
                                unique_reads, monitor_num
                            );
                        }
                    }
                } else {
                    thread::yield_now();
                }
            }

            println!(
                "Read thread finished: {} unique messages read",
                unique_reads
            );
        });

        write_handle.join().unwrap();
        read_handle.join().unwrap();

        println!("Ring buffer thread safety test passed!");
    }

    #[test]
    fn test_atomic_operations_safety() {
        let shared_path = "/tmp/atomic_test_buffer";
        let _ = std::fs::remove_file(shared_path);

        let buffer = SharedRingBuffer::create(shared_path, Some(8), Some(50)).unwrap();
        let buffer = Arc::new(buffer);

        let mut handles = vec![];

        // 多个线程同时进行原子操作
        for thread_id in 0..4 {
            let buffer_clone = Arc::clone(&buffer);
            let handle = thread::spawn(move || {
                for i in 0..250 {
                    let mut message = SharedMessage::default();
                    message.get_monitor_info_mut().monitor_num = thread_id * 1000 + i;

                    // 测试原子写入
                    while !buffer_clone.try_write_message(&message).unwrap() {
                        thread::yield_now();
                    }

                    // 测试原子读取
                    let _ = buffer_clone.try_read_latest_message();

                    // 测试状态查询（涉及原子操作）
                    let _ = buffer_clone.available_messages();
                    let _ = buffer_clone.has_message();
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        println!("Atomic operations safety test passed!");
    }
}

#[cfg(test)]
mod memory_safety_tests {
    use super::*;
    use crate::CommandType;

    #[test]
    fn test_shared_command_memory_safety() {
        // 创建命令并检查所有字节都被初始化
        let cmd = SharedCommand::view_tag(1, 0);

        // 读取整个结构体的所有字节
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &cmd as *const SharedCommand as *const u8,
                std::mem::size_of::<SharedCommand>(),
            )
        };

        // 在 MemorySanitizer 下，如果有未初始化字节，这里会报错
        for (i, &byte) in bytes.iter().enumerate() {
            let _ = byte; // 强制读取每个字节
            if i % 8 == 0 {
                println!("Checking byte {}", i);
            }
        }

        // 测试所有构造函数
        let _cmd1 = SharedCommand::view_tag(1, 0);
        let _cmd2 = SharedCommand::toggle_tag(2, 1);
        let _cmd3 = SharedCommand::set_layout(0, 0);
        let _cmd4 = SharedCommand::new(CommandType::None, 0, 0);
    }

    #[test]
    fn test_shared_message_memory_safety() {
        let msg = SharedMessage::new();

        // 检查整个消息结构体的字节
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &msg as *const SharedMessage as *const u8,
                std::mem::size_of::<SharedMessage>(),
            )
        };

        // 强制读取所有字节以触发 MemorySanitizer 检查
        let _checksum: u32 = bytes.iter().map(|&b| b as u32).sum();

        println!("SharedMessage memory safety test passed");
    }
}
