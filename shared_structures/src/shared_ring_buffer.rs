use cfg_if::cfg_if;
use log::{error, info, warn};
use std::hint;
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::shared_message::{SharedCommand, SharedMessage};
use shared_memory::{Shmem, ShmemConf};

// =============================================================================
// 同步原语特性选择（互斥）
// =============================================================================
cfg_if! {
    if #[cfg(all(feature = "use-eventfd", any(feature = "use-semaphore", feature = "use-futex")))] {
        compile_error!("Enable only one of 'use-eventfd' OR 'use-semaphore' OR 'use-futex'.");
    } else if #[cfg(all(feature = "use-semaphore", feature = "use-futex"))] {
        compile_error!("Enable only one of 'use-eventfd' OR 'use-semaphore' OR 'use-futex'.");
    }
}

// =============================================================================
// 使用 cfg_if! 定义各分支的 header 与同步包装
// =============================================================================

cfg_if! {
    if #[cfg(feature = "use-eventfd")] {
        use nix::poll::{poll, PollFd, PollFlags};
        use nix::sys::eventfd::{EventFd, EfdFlags};
        use nix::unistd;
        use std::os::unix::io::AsRawFd;
        use std::os::fd::BorrowedFd;
        use std::sync::atomic::AtomicI32;
        use std::sync::Arc;

        struct SyncPrimitiveWrapper {
            event_fd: EventFd,
        }

        impl SyncPrimitiveWrapper {
            fn new() -> Result<Self> {
                // 非阻塞 + CLOEXEC，避免阻塞与 exec 继承
                let efd = EventFd::from_flags(EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_CLOEXEC)
                    .map_err(|e| Error::new(ErrorKind::Other, format!("EventFd::new: {}", e)))?;
                Ok(Self { event_fd: efd })
            }

            fn as_raw_fd(&self) -> i32 {
                self.event_fd.as_raw_fd()
            }

            fn signal(&self) -> Result<()> {
                // 写入 1，非阻塞
                match self.event_fd.write(1) {
                    Ok(_) => Ok(()),
                    Err(nix::errno::Errno::EAGAIN) => Ok(()), // 计数溢出时忽略
                    Err(e) => Err(Error::new(ErrorKind::Other, format!("eventfd write: {}", e))),
                }
            }

            #[allow(dead_code)]
            fn clear(&self) -> Result<()> {
                let mut buf = [0u8; 8];
                use std::os::fd::AsFd;
                match unistd::read(self.event_fd.as_fd(), &mut buf) {
                    Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(()),
                    Err(e) => Err(Error::new(ErrorKind::Other, format!("eventfd read: {}", e))),
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
            message_event_fd: AtomicI32, // 创建者写入fd整数，其他进程仅“看到数字”
            command_event_fd: AtomicI32,
            cmd_write_idx: AtomicU32,
            cmd_read_idx: AtomicU32,
            is_destroyed: AtomicBool,
        }

    } else if #[cfg(feature = "use-semaphore")] {
        use libc::{sem_destroy, sem_init, sem_post, sem_t, sem_timedwait, sem_wait};
        use std::sync::Arc;

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
                        return Err(Error::new(ErrorKind::OutOfMemory, "Failed to allocate semaphore"));
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
                Self { sem: sem_ptr, _owned: false }
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
                            let deadline_since_epoch = deadline.duration_since(UNIX_EPOCH)
                                .map_err(|_| Error::new(ErrorKind::InvalidInput, "Invalid time"))?;
                            let ts = libc::timespec {
                                tv_sec: deadline_since_epoch.as_secs() as libc::time_t,
                                tv_nsec: deadline_since_epoch.subsec_nanos() as libc::c_long,
                            };
                            let result = sem_timedwait(self.sem, &ts);
                            if result == 0 { Ok(true) } else {
                                let err = Error::last_os_error();
                                match err.raw_os_error() {
                                    Some(libc::ETIMEDOUT) | Some(libc::EINTR) => Ok(false),
                                    _ => Err(err),
                                }
                            }
                        }
                        None => {
                            if sem_wait(self.sem) == 0 { Ok(true) } else {
                                let err = Error::last_os_error();
                                if err.raw_os_error() == Some(libc::EINTR) { Ok(false) } else { Err(err) }
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

    } else if #[cfg(feature = "use-futex")] {
        use libc::timespec;

        #[repr(C, align(128))]
        #[derive(Debug)]
        pub struct RingBufferHeader {
            // 热点字段（生产/消费相关）
            magic: AtomicU64,
            version: AtomicU64,
            write_idx: AtomicU32,
            read_idx: AtomicU32,
            buffer_size: u32,
            message_size: u32,
            last_timestamp: AtomicU64,

            // futex 等待/唤醒相关字段，尽量与热点隔离，降低 false sharing
            message_seq: AtomicU32,
            message_waiters: AtomicU32,

            command_seq: AtomicU32,
            command_waiters: AtomicU32,

            cmd_write_idx: AtomicU32,
            cmd_read_idx: AtomicU32,
            is_destroyed: AtomicBool,
            _padding: [u8; 16],
        }

        #[inline]
        fn futex_wait(addr: &AtomicU32, expected: u32, timeout: Option<Duration>) -> std::io::Result<bool> {
            let mut ts = timespec { tv_sec: 0, tv_nsec: 0 };
            let ts_ptr = if let Some(dur) = timeout {
                ts.tv_sec = dur.as_secs() as libc::time_t;
                ts.tv_nsec = dur.subsec_nanos() as libc::c_long;
                &mut ts as *mut timespec
            } else {
                std::ptr::null_mut()
            };
            let uaddr = addr as *const AtomicU32 as *const u32 as *const i32;
            let ret = unsafe {
                libc::syscall(
                    libc::SYS_futex,
                    uaddr,
                    libc::FUTEX_WAIT, // FUTEX_WAIT: 当 *uaddr == expected 时才睡眠
                    expected as i32,
                    ts_ptr,
                    std::ptr::null::<libc::c_void>(),
                    0,
                )
            };
            if ret == 0 {
                Ok(true) // 被唤醒或超时返回 0 的语义取决于内核；这里统一为 true 表示唤醒/返回
            } else {
                let err = std::io::Error::last_os_error();
                match err.raw_os_error() {
                    Some(libc::EAGAIN) | Some(libc::EINTR) | Some(libc::ETIMEDOUT) => Ok(false),
                    _ => Err(err),
                }
            }
        }

        // futex 分支下不需要本地 SyncPrimitive 对象
        type SyncPrimitive = ();

        #[inline]
        fn futex_wake(addr: &AtomicU32, n: i32) -> std::io::Result<i32> {
            let uaddr = addr as *const AtomicU32 as *const u32 as *const i32;
            let ret = unsafe {
                libc::syscall(
                    libc::SYS_futex,
                    uaddr,
                    libc::FUTEX_WAKE,
                    n,
                    std::ptr::null::<libc::c_void>(),
                    std::ptr::null::<libc::c_void>(),
                    0,
                )
            };
            if ret < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(ret as i32)
            }
        }
    } else {
        compile_error!("Enable one of the features: 'use-eventfd', 'use-semaphore', or 'use-futex'.");
    }
}

// =============================================================================
// 通用常量与工具
// =============================================================================

const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646;
const RING_BUFFER_VERSION: u64 = 7; // 升级版本，避免旧段误用
const DEFAULT_BUFFER_SIZE: usize = 16;
const CMD_BUFFER_SIZE: usize = 16; // 命令环固定大小
const DEFAULT_ADAPTIVE_POLL_SPINS: u32 = 400;

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

// SharedMessage 的安全校验和（避免读取 padding）
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
    mix_u64(&mut sum, m.timestamp as u64);

    let mi = &m.monitor_info;

    // scalar fields
    mix_i32(&mut sum, mi.monitor_num);
    mix_i32(&mut sum, mi.monitor_width);
    mix_i32(&mut sum, mi.monitor_height);
    mix_i32(&mut sum, mi.monitor_x);
    mix_i32(&mut sum, mi.monitor_y);

    // tag_status_vec：把 bool 压成位
    for ts in &mi.tag_status_vec {
        let bits: u8 = (ts.is_selected as u8)
            | ((ts.is_urg as u8) << 1)
            | ((ts.is_filled as u8) << 2)
            | ((ts.is_occ as u8) << 3);
        sum = sum.wrapping_add(bits as u32);
    }

    // client_name
    for &b in &mi.client_name {
        sum = sum.wrapping_add(b as u32);
    }
    // ltsymbol
    for &b in &mi.ltsymbol {
        sum = sum.wrapping_add(b as u32);
    }

    sum
}

// =============================================================================
// 主结构体定义
// =============================================================================

#[allow(dead_code)]
pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut RingBufferHeader,
    message_slots: *mut MessageSlot,
    cmd_buffer_start: *mut SharedCommand,
    is_creator: bool,

    // 特性相关的同步原语
    message_sync: Option<SyncPrimitive>,
    command_sync: Option<SyncPrimitive>,

    adaptive_poll_spins: u32,

    // eventfd 有效性日志标志（仅在 eventfd 分支使用）
    #[cfg(feature = "use-eventfd")]
    eventfd_msg_warned: std::sync::atomic::AtomicBool,
    #[cfg(feature = "use-eventfd")]
    eventfd_cmd_warned: std::sync::atomic::AtomicBool,
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
    // 动态 buffer_mask：根据 header.buffer_size 计算掩码
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

    pub fn create_shared_ring_buffer(shared_path: &String) -> Option<SharedRingBuffer> {
        if shared_path.is_empty() {
            warn!("No shared path provided, running without shared memory");
            return None;
        }
        match SharedRingBuffer::open(&shared_path, Some(0)) {
            Ok(shared_buffer) => {
                info!("Successfully opened shared ring buffer: {}", shared_path);
                Some(shared_buffer)
            }
            Err(e) => {
                warn!(
                    "Failed to open shared ring buffer: {}, attempting to create a new one",
                    e
                );
                match SharedRingBuffer::create(&shared_path, None, Some(0)) {
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

        // 创建同步原语（特性分支）
        let (message_sync, command_sync) = {
            cfg_if! {
                if #[cfg(feature = "use-eventfd")] {
                    let message_efd = std::sync::Arc::new(SyncPrimitiveWrapper::new()?);
                    let command_efd = std::sync::Arc::new(SyncPrimitiveWrapper::new()?);

                    unsafe {
                        (*header).message_event_fd.store(message_efd.as_raw_fd(), Ordering::Release);
                        (*header).command_event_fd.store(command_efd.as_raw_fd(), Ordering::Release);
                    }

                    (Some(message_efd), Some(command_efd))
                } else if #[cfg(feature = "use-semaphore")] {
                    unsafe {
                        let message_sem_ptr = &mut (*header).message_sem as *mut libc::sem_t;
                        let command_sem_ptr = &mut (*header).command_sem as *mut libc::sem_t;

                        if libc::sem_init(message_sem_ptr, 1, 0) != 0 {
                            return Err(Error::last_os_error());
                        }
                        if libc::sem_init(command_sem_ptr, 1, 0) != 0 {
                            libc::sem_destroy(message_sem_ptr);
                            return Err(Error::last_os_error());
                        }

                        let message_wrapper = std::sync::Arc::new(SyncPrimitiveWrapper::from_shared(message_sem_ptr));
                        let command_wrapper = std::sync::Arc::new(SyncPrimitiveWrapper::from_shared(command_sem_ptr));

                        (Some(message_wrapper), Some(command_wrapper))
                    }
                } else if #[cfg(feature = "use-futex")] {
                    (None, None)
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
            #[cfg(feature = "use-futex")]
            {
                (*header).message_seq.store(0, Ordering::Release);
                (*header).command_seq.store(0, Ordering::Release);
                (*header).message_waiters.store(0, Ordering::Release);
                (*header).command_waiters.store(0, Ordering::Release);
            }
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
            #[cfg(feature = "use-eventfd")]
            eventfd_msg_warned: std::sync::atomic::AtomicBool::new(false),
            #[cfg(feature = "use-eventfd")]
            eventfd_cmd_warned: std::sync::atomic::AtomicBool::new(false),
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
            // 校验 message_size 以防结构体不匹配
            if (*header).message_size as usize != size_of::<MessageSlot>() {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "Incompatible message slot size",
                ));
            }
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

        // 连接已有同步原语
        let (message_sync, command_sync) = {
            cfg_if! {
                if #[cfg(feature = "use-eventfd")] {
                    // 非创建者仅能读到整数fd，可能在本进程无效；等待处会检测并降级
                    (None, None)
                } else if #[cfg(feature = "use-semaphore")] {
                    unsafe {
                        let message_sem_ptr = &mut (*header).message_sem as *mut libc::sem_t;
                        let command_sem_ptr = &mut (*header).command_sem as *mut libc::sem_t;

                        let message_wrapper = std::sync::Arc::new(SyncPrimitiveWrapper::from_shared(message_sem_ptr));
                        let command_wrapper = std::sync::Arc::new(SyncPrimitiveWrapper::from_shared(command_sem_ptr));

                        (Some(message_wrapper), Some(command_wrapper))
                    }
                } else if #[cfg(feature = "use-futex")] {
                    (None, None)
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
            #[cfg(feature = "use-eventfd")]
            eventfd_msg_warned: std::sync::atomic::AtomicBool::new(false),
            #[cfg(feature = "use-eventfd")]
            eventfd_cmd_warned: std::sync::atomic::AtomicBool::new(false),
        })
    }

    #[inline]
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

            if write_idx.wrapping_sub(read_idx) == self.buffer_size() {
                return Ok(false);
            }

            let slot_idx = (write_idx & self.buffer_mask()) as usize;
            let slot = &mut *self.message_slots.add(slot_idx);

            let checksum = calculate_message_checksum(message);

            let new_slot = MessageSlot {
                timestamp: now_millis(),
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
            let slot_idx = (read_idx & self.buffer_mask()) as usize;
            let slot = &*self.message_slots.add(slot_idx);

            if calculate_message_checksum(&slot.message) != slot.checksum {
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
            let slot_idx = (read_idx & self.buffer_mask()) as usize;
            let slot = &*self.message_slots.add(slot_idx);

            if calculate_message_checksum(&slot.message) != slot.checksum {
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

            let slot_idx = (write_idx & self.cmd_buffer_mask()) as usize;
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

            let slot_idx = (read_idx & self.cmd_buffer_mask()) as usize;
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
            } else if #[cfg(feature = "use-futex")] {
                self.wait_on_futex(is_message, timeout, has_data)
            }
        }
    }

    #[cfg(feature = "use-eventfd")]
    fn fd_is_valid(fd: i32) -> bool {
        use nix::fcntl::{fcntl, FcntlArg};
        use std::os::fd::BorrowedFd;
        unsafe { fcntl(BorrowedFd::borrow_raw(fd), FcntlArg::F_GETFD).is_ok() }
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

        if fd < 0 || !Self::fd_is_valid(fd) {
            let warned = if is_message {
                self.eventfd_msg_warned.swap(true, Ordering::AcqRel)
            } else {
                self.eventfd_cmd_warned.swap(true, Ordering::AcqRel)
            };
            if !warned {
                warn!(
                    "eventfd {} is not valid in this process; falling back to polling. \
                     This usually means the fd was not inherited or not passed via SCM_RIGHTS.",
                    fd
                );
            }
            // 直接退化为轮询路径
            return Ok(has_data());
        }

        // 有效 fd
        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let mut poll_fd = PollFd::new(borrowed_fd, PollFlags::POLLIN);

        // nix 0.30: poll<T: Into<PollTimeout>>(fds, timeout)
        // 传 Option<u16>：None 表示无限等待
        let poll_timeout: Option<u16> =
            timeout.map(|d| u16::try_from(d.as_millis()).unwrap_or(u16::MAX));

        match poll(std::slice::from_mut(&mut poll_fd), poll_timeout) {
            Ok(0) => Ok(has_data()),
            Ok(_) => {
                // drain 一次，防止计数粘连
                let mut buf = [0u8; 8];
                let _ = nix::unistd::read(borrowed_fd, &mut buf);
                Ok(true)
            }
            Err(e) => {
                warn!("poll(eventfd) error: {}. Fallback to polling path", e);
                Ok(has_data())
            }
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
                Err(e) => {
                    warn!("semaphore wait error: {}. Fallback to polling path", e);
                    Ok(has_data())
                }
            }
        } else {
            Ok(has_data())
        }
    }

    #[cfg(feature = "use-futex")]
    fn wait_on_futex(
        &self,
        is_message: bool,
        timeout: Option<Duration>,
        has_data: impl Fn() -> bool,
    ) -> Result<bool> {
        unsafe {
            let (seq, waiters) = if is_message {
                (&(*self.header).message_seq, &(*self.header).message_waiters)
            } else {
                (&(*self.header).command_seq, &(*self.header).command_waiters)
            };

            // 快路径再检查
            if has_data() || self.is_destroyed() {
                return Ok(has_data());
            }

            // 标记等待者
            waiters.fetch_add(1, Ordering::AcqRel);

            // 再次检查，避免丢唤醒
            if has_data() || self.is_destroyed() {
                waiters.fetch_sub(1, Ordering::AcqRel);
                return Ok(has_data());
            }

            // 记录快照后进入 futex_wait
            let snapshot = seq.load(Ordering::Acquire);
            let res = futex_wait(seq, snapshot, timeout);

            // 撤销等待者标记
            waiters.fetch_sub(1, Ordering::AcqRel);

            match res {
                Ok(_) => Ok(has_data()),
                Err(e) => {
                    warn!("futex_wait error: {}. Fallback to polling path", e);
                    Ok(has_data())
                }
            }
        }
    }

    fn signal_message(&self) -> Result<()> {
        cfg_if! {
            if #[cfg(feature = "use-eventfd")] {
                if let Some(ref sync) = self.message_sync {
                    sync.signal()
                } else {
                    // 非创建者无法 signal，记录一次
                    if !self.eventfd_msg_warned.swap(true, Ordering::AcqRel) {
                        warn!("message_sync not available in this process (eventfd). \
                               Writer cannot signal peer; relying on polling or other wakeups.");
                    }
                    Ok(())
                }
            } else if #[cfg(feature = "use-semaphore")] {
                if let Some(ref sync) = self.message_sync { sync.signal() } else { Ok(()) }
            } else if #[cfg(feature = "use-futex")] {
                unsafe {
                    // 仅当确实有等待者时才唤醒，避免高吞吐场景下的无用系统调用
                    if (*self.header).message_waiters.load(Ordering::Acquire) > 0 {
                        let _ = (*self.header).message_seq.fetch_add(1, Ordering::Release);
                        let _ = futex_wake(&(*self.header).message_seq, 1);
                    }
                }
                Ok(())
            }
        }
    }

    fn signal_command(&self) -> Result<()> {
        cfg_if! {
            if #[cfg(feature = "use-eventfd")] {
                if let Some(ref sync) = self.command_sync {
                    sync.signal()
                } else {
                    if !self.eventfd_cmd_warned.swap(true, Ordering::AcqRel) {
                        warn!("command_sync not available in this process (eventfd). \
                               Writer cannot signal peer; relying on polling or other wakeups.");
                    }
                    Ok(())
                }
            } else if #[cfg(feature = "use-semaphore")] {
                if let Some(ref sync) = self.command_sync { sync.signal() } else { Ok(()) }
            } else if #[cfg(feature = "use-futex")] {
                unsafe {
                    if (*self.header).command_waiters.load(Ordering::Acquire) > 0 {
                        let _ = (*self.header).command_seq.fetch_add(1, Ordering::Release);
                        let _ = futex_wake(&(*self.header).command_seq, 1);
                    }
                }
                Ok(())
            }
        }
    }
}

impl Drop for SharedRingBuffer {
    fn drop(&mut self) {
        // 标记销毁，尝试唤醒对端
        unsafe {
            (*self.header).is_destroyed.store(true, Ordering::Release);
        }

        cfg_if! {
            if #[cfg(feature = "use-eventfd")] {
                // 释放本地持有的 eventfd
                self.message_sync = None;
                self.command_sync = None;
            } else if #[cfg(feature = "use-semaphore")] {
                self.message_sync = None;
                self.command_sync = None;
            } else if #[cfg(feature = "use-futex")] {
                unsafe {
                    let _ = (*self.header).message_seq.fetch_add(1, Ordering::Release);
                    let _ = (*self.header).command_seq.fetch_add(1, Ordering::Release);
                    let _ = futex_wake(&(*self.header).message_seq, i32::MAX);
                    let _ = futex_wake(&(*self.header).command_seq, i32::MAX);
                }
            }
        }

        if self.is_creator {
            if let Some(path) = self.shmem.get_flink_path() {
                info!("(Creator) Removing shmem flink: {:?}", path);
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

// =============================================================================
// 测试
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_message::SharedMessage;
    use std::sync::Arc;
    use std::thread;

    fn mk_path(name: &str) -> String {
        format!("/tmp/{}_{}", name, std::process::id())
    }

    #[test]
    fn test_feature_selected() {
        println!(
            "Testing with feature: {}",
            if cfg!(feature = "use-semaphore") {
                "semaphore"
            } else if cfg!(feature = "use-eventfd") {
                "eventfd"
            } else if cfg!(feature = "use-futex") {
                "futex"
            } else {
                "unknown"
            }
        );
    }

    #[test]
    fn test_variable_buffer_sizes() {
        for &sz in &[8usize, 16usize, 32usize] {
            let shared_path = mk_path(&format!("buf_size_{}", sz));
            let _ = std::fs::remove_file(&shared_path);
            let rb = SharedRingBuffer::create(&shared_path, Some(sz), Some(0)).unwrap();

            for i in 0..(sz * 3) {
                let mut msg = SharedMessage::default();
                msg.get_monitor_info_mut().monitor_num = i as i32;
                // 尽力写入
                if !rb.try_write_message(&msg).unwrap() {
                    // 若满，尝试读一条丢弃
                    let _ = rb.try_read_next_message().unwrap();
                }
            }
            // 读取一条确认可用
            let _ = rb.try_read_latest_message().unwrap();
        }
    }

    #[test]
    fn test_spsc_concurrency_latest() {
        let shared_path = mk_path("spsc_latest");
        let _ = std::fs::remove_file(&shared_path);

        let rb = Arc::new(SharedRingBuffer::create(&shared_path, Some(16), Some(0)).unwrap());

        let writer = {
            let rb = rb.clone();
            thread::spawn(move || {
                for i in 0..500 {
                    let mut msg = SharedMessage::default();
                    msg.get_monitor_info_mut().monitor_num = i as i32;
                    while !rb.try_write_message(&msg).unwrap() {
                        thread::yield_now();
                    }
                }
            })
        };

        let reader = {
            let rb = rb.clone();
            thread::spawn(move || {
                let mut last = -1i32;
                let mut unique = 0;
                let start = std::time::Instant::now();
                loop {
                    if let Ok(Some(m)) = rb.try_read_latest_message() {
                        let n = m.get_monitor_info().monitor_num;
                        if n != last {
                            unique += 1;
                            last = n;
                        }
                        if n >= 499 {
                            break;
                        }
                    } else {
                        if start.elapsed().as_secs() > 5 {
                            break;
                        }
                        thread::yield_now();
                    }
                }
                assert!(unique > 0);
            })
        };

        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn test_checksum_safe() {
        let mut m = SharedMessage::default();
        m.get_monitor_info_mut().set_client_name("x");
        m.get_monitor_info_mut().set_ltsymbol("[]=");
        let _ = calculate_message_checksum(&m);
    }

    #[test]
    fn test_command_ring() {
        let shared_path = mk_path("cmd_ring");
        let _ = std::fs::remove_file(&shared_path);
        let rb = SharedRingBuffer::create(&shared_path, Some(16), Some(0)).unwrap();

        // 发送接收
        let cmd = SharedCommand {
            cmd_type: 1,
            parameter: 7,
            monitor_id: 2,
            timestamp: now_millis(),
        };
        assert!(rb.send_command(cmd).unwrap());
        assert!(rb.has_command());
        let got = rb.receive_command().unwrap();
        assert_eq!(got.parameter, 7);
    }

    #[cfg(feature = "use-eventfd")]
    #[test]
    fn test_eventfd_validity_logging() {
        // 创建端可用
        let shared_path = mk_path("eventfd_validity");
        let _ = std::fs::remove_file(&shared_path);
        let rb_creator = SharedRingBuffer::create(&shared_path, Some(8), Some(0)).unwrap();

        // open 端拿不到有效本地 fd（除非继承），wait 时会记录 warning 并走 polling
        let rb_open = SharedRingBuffer::open(&shared_path, Some(0)).unwrap();

        // 基本写读，至少保证功能不受影响
        let mut msg = SharedMessage::default();
        msg.get_monitor_info_mut().monitor_num = 42;
        assert!(rb_creator.try_write_message(&msg).unwrap());
        let _ = rb_open.try_read_latest_message().unwrap();
    }

    // ===========================
    // 性能统计用测试
    // ===========================

    #[test]
    fn test_spsc_stats_busy_poll() {
        let shared_path = mk_path("spsc_stats_busy");
        let _ = std::fs::remove_file(&shared_path);
        // 使用较大的环和0自旋（纯忙等场景更明确）
        let rb = Arc::new(SharedRingBuffer::create(&shared_path, Some(1024), Some(0)).unwrap());

        let n: usize = 200_000;
        let rb_w = rb.clone();
        let writer = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut sent = 0usize;
            let mut msg = SharedMessage::default();
            while sent < n {
                msg.get_monitor_info_mut().monitor_num = sent as i32;
                msg.update_timestamp(); // 用于端到端延迟统计
                if rb_w.try_write_message(&msg).unwrap() {
                    sent += 1;
                } else {
                    std::thread::yield_now();
                }
            }
            let elapsed = start.elapsed();
            let per_msg_ns = (elapsed.as_nanos() as f64) / (n as f64);
            (elapsed, per_msg_ns)
        });

        let rb_r = rb.clone();
        let reader = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut got = 0usize;
            let mut last = -1i32;
            let mut sum_lat_ns: u128 = 0;

            while got < n {
                if let Ok(Some(m)) = rb_r.try_read_next_message() {
                    let now = now_millis();
                    // 将消息内部 timestamp 视为发送时间（毫秒），这里粗略换算为纳秒统计
                    let sent_ms = m.get_timestamp() as u128;
                    let now_ms = now as u128;
                    let lat_ns = (now_ms.saturating_sub(sent_ms)) * 1_000_000;
                    sum_lat_ns += lat_ns;

                    let cur = m.get_monitor_info().monitor_num;
                    if cur != last {
                        got += 1;
                        last = cur;
                    }
                } else {
                    std::thread::yield_now();
                }
            }
            let elapsed = start.elapsed();
            let per_msg_ns = (elapsed.as_nanos() as f64) / (n as f64);
            let avg_lat_ns = (sum_lat_ns as f64) / (got.max(1) as f64);
            (elapsed, per_msg_ns, avg_lat_ns)
        });

        let (w_elapsed, w_per_ns) = writer.join().unwrap();
        let (r_elapsed, r_per_ns, avg_lat_ns) = reader.join().unwrap();

        println!(
            "[BUSY] feature={} | N={} | writer: {:?}, {:.0} ns/msg | reader: {:?}, {:.0} ns/msg | avg e2e latency ≈ {:.0} ns",
            if cfg!(feature="use-semaphore") {"semaphore"}
            else if cfg!(feature="use-eventfd") {"eventfd"}
            else if cfg!(feature="use-futex") {"futex"} else {"unknown"},
            n, w_elapsed, w_per_ns, r_elapsed, r_per_ns, avg_lat_ns
        );
    }

    #[test]
    fn test_spsc_stats_waiting() {
        let shared_path = mk_path("spsc_stats_wait");
        let _ = std::fs::remove_file(&shared_path);
        // 保留一定自旋后再等待，覆盖 wait 路径
        let rb = Arc::new(SharedRingBuffer::create(&shared_path, Some(1024), Some(200)).unwrap());

        let n: usize = 50_000;
        let rb_w = rb.clone();
        let writer = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut sent = 0usize;
            let mut msg = SharedMessage::default();
            while sent < n {
                msg.get_monitor_info_mut().monitor_num = sent as i32;
                msg.update_timestamp();

                // 队列满了就稍微让一下 CPU
                while !rb_w.try_write_message(&msg).unwrap() {
                    std::thread::yield_now();
                }
                sent += 1;
            }
            start.elapsed()
        });

        let rb_r = rb.clone();
        let reader = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut got = 0usize;
            let mut sum_lat_ns: u128 = 0;

            while got < n {
                // 使用等待接口，考察睡眠/唤醒路径
                if rb_r
                    .wait_for_message(Some(std::time::Duration::from_millis(50)))
                    .unwrap()
                {
                    while let Ok(Some(m)) = rb_r.try_read_next_message() {
                        let now = now_millis() as u128;
                        let sent_ms = m.get_timestamp() as u128;
                        sum_lat_ns += (now.saturating_sub(sent_ms)) * 1_000_000;
                        got += 1;
                        if got >= n {
                            break;
                        }
                    }
                }
            }
            let elapsed = start.elapsed();
            let avg_lat_ns = (sum_lat_ns as f64) / (got.max(1) as f64);
            (elapsed, avg_lat_ns)
        });

        let w_elapsed = writer.join().unwrap();
        let (r_elapsed, avg_lat_ns) = reader.join().unwrap();

        let per_msg_writer = (w_elapsed.as_nanos() as f64) / (n as f64);
        let per_msg_reader = (r_elapsed.as_nanos() as f64) / (n as f64);

        println!(
            "[WAIT] feature={} | N={} | writer: {:?}, {:.0} ns/msg | reader: {:?}, {:.0} ns/msg | avg e2e latency ≈ {:.0} ns",
            if cfg!(feature="use-semaphore") {"semaphore"}
            else if cfg!(feature="use-eventfd") {"eventfd"}
            else if cfg!(feature="use-futex") {"futex"} else {"unknown"},
            n, w_elapsed, per_msg_writer, r_elapsed, per_msg_reader, avg_lat_ns
        );
    }
}
