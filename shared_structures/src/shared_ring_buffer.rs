use std::hint;
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// nix 用于 eventfd 和 poll
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::eventfd::EventFd;
use nix::unistd;
use std::os::unix::io::{AsRawFd, BorrowedFd};

use shared_memory::{Shmem, ShmemConf};

use crate::shared_message::{SharedCommand, SharedMessage};

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
}

// 消息槽结构体：移除 packed，使用自然对齐
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MessageSlot {
    timestamp: u64,
    checksum: u32,
    _padding: u32, // 确保 8 字节对齐
    message: SharedMessage,
}

const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646;
const RING_BUFFER_VERSION: u64 = 4;
const DEFAULT_BUFFER_SIZE: usize = 16;
const CMD_BUFFER_SIZE: usize = 16;
const BUFFER_MASK: u32 = (DEFAULT_BUFFER_SIZE as u32) - 1;
const CMD_BUFFER_MASK: u32 = (CMD_BUFFER_SIZE as u32) - 1;
const DEFAULT_ADAPTIVE_POLL_SPINS: u32 = 4000;

pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut RingBufferHeader,
    message_slots: *mut MessageSlot,
    cmd_buffer_start: *mut SharedCommand,
    is_creator: bool,
    message_event_fd: i32,
    command_event_fd: i32,
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

        // 计算内存布局，考虑对齐要求
        let header_size = size_of::<RingBufferHeader>();
        let message_slot_size = size_of::<MessageSlot>();
        let cmd_size = size_of::<SharedCommand>();

        // 确保各部分都正确对齐
        let messages_offset = align_up(header_size, std::mem::align_of::<MessageSlot>());
        let messages_size = buffer_size * message_slot_size;
        let commands_offset = align_up(
            messages_offset + messages_size,
            std::mem::align_of::<SharedCommand>(),
        );
        let commands_size = CMD_BUFFER_SIZE * cmd_size;
        let total_size = commands_offset + commands_size;

        println!("Creating shared memory layout:");
        println!("  Header: {} bytes at offset 0", header_size);
        println!(
            "  Messages: {} x {} = {} bytes at offset {}",
            buffer_size, message_slot_size, messages_size, messages_offset
        );
        println!(
            "  Commands: {} x {} = {} bytes at offset {}",
            CMD_BUFFER_SIZE, cmd_size, commands_size, commands_offset
        );
        println!("  Total: {} bytes", total_size);

        let shmem = ShmemConf::new()
            .size(total_size)
            .flink(path)
            .force_create_flink()
            .create()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Failed to create shmem: {}", e)))?;

        let msg_efd = EventFd::from_value(0)?;
        let cmd_efd = EventFd::from_value(0)?;

        let msg_fd_raw = msg_efd.as_raw_fd();
        let cmd_fd_raw = cmd_efd.as_raw_fd();

        std::mem::forget(msg_efd);
        std::mem::forget(cmd_efd);

        let header = shmem.as_ptr() as *mut RingBufferHeader;
        let message_slots = unsafe { shmem.as_ptr().add(messages_offset) as *mut MessageSlot };
        let cmd_buffer_start = unsafe { shmem.as_ptr().add(commands_offset) as *mut SharedCommand };

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
            (*header)
                .message_event_fd
                .store(msg_fd_raw, Ordering::Release);
            (*header)
                .command_event_fd
                .store(cmd_fd_raw, Ordering::Release);
        }

        Ok(Self {
            shmem,
            header,
            message_slots,
            cmd_buffer_start,
            is_creator: true,
            message_event_fd: msg_fd_raw,
            command_event_fd: cmd_fd_raw,
            adaptive_poll_spins,
        })
    }

    pub fn open(path: &str, adaptive_poll_spins: Option<u32>) -> Result<Self> {
        let shmem = ShmemConf::new()
            .flink(path)
            .open()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Failed to open shmem: {}", e)))?;

        let header = shmem.as_ptr() as *mut RingBufferHeader;
        let (message_event_fd, command_event_fd, buffer_size);

        unsafe {
            if (*header).magic.load(Ordering::Acquire) != RING_BUFFER_MAGIC {
                return Err(Error::new(ErrorKind::InvalidData, "Invalid magic number"));
            }
            if (*header).version.load(Ordering::Acquire) != RING_BUFFER_VERSION {
                return Err(Error::new(ErrorKind::InvalidData, "Incompatible version"));
            }
            message_event_fd = (*header).message_event_fd.load(Ordering::Acquire);
            command_event_fd = (*header).command_event_fd.load(Ordering::Acquire);
            buffer_size = (*header).buffer_size as usize;
        }

        // 重新计算偏移量（必须与创建时相同）
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

        Ok(Self {
            shmem,
            header,
            message_slots,
            cmd_buffer_start,
            is_creator: false,
            message_event_fd,
            command_event_fd,
            adaptive_poll_spins: adaptive_poll_spins.unwrap_or(DEFAULT_ADAPTIVE_POLL_SPINS),
        })
    }

    // 直接写入消息，无需序列化
    pub fn try_write_message(&self, message: &SharedMessage) -> Result<bool> {
        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).read_idx.load(Ordering::Acquire);

            if write_idx.wrapping_sub(read_idx) == (*self.header).buffer_size {
                return Ok(false); // 缓冲区满
            }

            let slot_idx = (write_idx & BUFFER_MASK) as usize;
            let slot = &mut *self.message_slots.add(slot_idx);

            // 计算校验和
            let message_bytes = std::slice::from_raw_parts(
                message as *const SharedMessage as *const u8,
                size_of::<SharedMessage>(),
            );
            let checksum = calculate_checksum(message_bytes);

            // 创建新的 MessageSlot
            let new_slot = MessageSlot {
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
                checksum,
                _padding: 0,
                message: *message,
            };

            // 一次性写入整个槽位
            *slot = new_slot;

            // 更新头部信息
            (*self.header)
                .last_timestamp
                .store(new_slot.timestamp, Ordering::Release);
            (*self.header)
                .write_idx
                .store(write_idx.wrapping_add(1), Ordering::Release);
        }

        self.signal_fd(self.message_event_fd)?;
        Ok(true)
    }

    // 直接从内存读取消息，无需反序列化
    pub fn try_read_latest_message(&self) -> Result<Option<SharedMessage>> {
        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let mut read_idx = (*self.header).read_idx.load(Ordering::Relaxed);

            if read_idx == write_idx {
                return Ok(None); // 没有新消息
            }

            // 如果有多条消息，直接跳到最新的
            if write_idx.wrapping_sub(read_idx) > 1 {
                read_idx = write_idx.wrapping_sub(1);
            }

            let slot_idx = (read_idx & BUFFER_MASK) as usize;
            let slot = &*self.message_slots.add(slot_idx);

            // 验证校验和
            let message_bytes = std::slice::from_raw_parts(
                &slot.message as *const SharedMessage as *const u8,
                size_of::<SharedMessage>(),
            );
            if calculate_checksum(message_bytes) != slot.checksum {
                return Err(Error::new(ErrorKind::InvalidData, "Checksum mismatch"));
            }

            // 直接返回消息副本
            let message = slot.message;

            // 更新读索引
            (*self.header)
                .read_idx
                .store(read_idx.wrapping_add(1), Ordering::Release);

            Ok(Some(message))
        }
    }

    pub fn send_command(&self, command: SharedCommand) -> Result<bool> {
        unsafe {
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Acquire);

            if write_idx.wrapping_sub(read_idx) == CMD_BUFFER_SIZE as u32 {
                return Ok(false); // 命令缓冲区满
            }

            let slot_idx = (write_idx & CMD_BUFFER_MASK) as usize;
            let cmd_slot = &mut *self.cmd_buffer_start.add(slot_idx);

            *cmd_slot = command; // 直接内存拷贝

            (*self.header)
                .cmd_write_idx
                .store(write_idx.wrapping_add(1), Ordering::Release);
        }

        self.signal_fd(self.command_event_fd)?;
        Ok(true)
    }

    pub fn receive_command(&self) -> Option<SharedCommand> {
        unsafe {
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Relaxed);
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Acquire);

            if read_idx == write_idx {
                return None; // 没有新命令
            }

            let slot_idx = (read_idx & CMD_BUFFER_MASK) as usize;
            let cmd_slot = &*self.cmd_buffer_start.add(slot_idx);
            let command = *cmd_slot; // 直接内存拷贝

            (*self.header)
                .cmd_read_idx
                .store(read_idx.wrapping_add(1), Ordering::Release);

            Some(command)
        }
    }

    pub fn wait_for_message(&self, timeout: Option<Duration>) -> Result<bool> {
        self.wait_on_fd(self.message_event_fd, timeout, || self.has_message())
    }

    pub fn wait_for_command(&self, timeout: Option<Duration>) -> Result<bool> {
        self.wait_on_fd(self.command_event_fd, timeout, || self.has_command())
    }

    pub fn has_command(&self) -> bool {
        self.available_commands() > 0
    }

    pub fn available_commands(&self) -> usize {
        unsafe {
            (*self.header)
                .cmd_write_idx
                .load(Ordering::Acquire)
                .wrapping_sub((*self.header).cmd_read_idx.load(Ordering::Acquire))
                as usize
        }
    }

    pub fn has_message(&self) -> bool {
        self.available_messages() > 0
    }

    pub fn available_messages(&self) -> usize {
        unsafe {
            (*self.header)
                .write_idx
                .load(Ordering::Acquire)
                .wrapping_sub((*self.header).read_idx.load(Ordering::Acquire)) as usize
        }
    }

    // 自适应轮询等待方法
    fn wait_on_fd(
        &self,
        fd: i32,
        timeout: Option<Duration>,
        has_data: impl Fn() -> bool,
    ) -> Result<bool> {
        // 1. 自适应轮询（Spinning）
        for _ in 0..self.adaptive_poll_spins {
            if has_data() {
                return Ok(true);
            }
            hint::spin_loop();
        }

        // 检查一次最终状态
        if has_data() {
            return Ok(true);
        }

        // 2. 回退到阻塞等待（poll）
        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let poll_fd = PollFd::new(borrowed_fd, PollFlags::POLLIN);
        let timeout_ms = timeout.map_or(0, |d| d.as_millis() as i32);

        let num_events = poll(&mut [poll_fd], timeout_ms as u16)
            .map_err(|e| Error::new(ErrorKind::Other, format!("poll failed: {}", e)))?;

        if num_events == 0 {
            return Ok(has_data());
        }

        // 清除 eventfd 的信号
        let mut buf = [0u8; 8];
        match unistd::read(borrowed_fd, &mut buf) {
            Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(true),
            Err(e) => Err(Error::new(
                ErrorKind::Other,
                format!("Failed to read from eventfd: {}", e),
            )),
        }
    }

    fn signal_fd(&self, fd: i32) -> Result<()> {
        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let signal_val: u64 = 1;
        match unistd::write(borrowed_fd, &signal_val.to_ne_bytes()) {
            Ok(_) | Err(nix::errno::Errno::EAGAIN) => Ok(()),
            Err(e) => Err(Error::new(
                ErrorKind::Other,
                format!("Failed to signal eventfd: {}", e),
            )),
        }
    }
}

impl Drop for SharedRingBuffer {
    fn drop(&mut self) {
        if self.is_creator {
            println!("(Creator) Cleaning up resources...");
            let _ = unistd::close(self.message_event_fd);
            let _ = unistd::close(self.command_event_fd);
            if let Some(path) = self.shmem.get_flink_path() {
                println!("(Creator) Removing shmem flink: {:?}", path);
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

// 辅助函数：向上对齐到指定边界
fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn calculate_checksum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |sum, &b| sum.wrapping_add(b as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_message::SharedMessage;
    use std::thread;

    #[test]
    fn test_alignment_calculation() {
        assert_eq!(align_up(1, 4), 4);
        assert_eq!(align_up(4, 4), 4);
        assert_eq!(align_up(5, 4), 8);
        assert_eq!(align_up(7, 8), 8);
        assert_eq!(align_up(9, 8), 16);
    }

    #[test]
    fn test_direct_memory_communication() {
        let shared_path = "/tmp/test_fixed_buffer";
        let _ = std::fs::remove_file(shared_path);

        let jwm_buffer = SharedRingBuffer::create(shared_path, None, Some(5000)).unwrap();
        println!("[JWM] SharedRingBuffer created at '{}'", shared_path);

        let egui_thread = thread::spawn(move || {
            let egui_buffer = SharedRingBuffer::open(shared_path, None).unwrap();
            println!("[EGUI] Opened SharedRingBuffer.");

            for i in 1..=5 {
                println!("\n[EGUI] Waiting for new message... (timeout 2s)");
                match egui_buffer.wait_for_message(Some(Duration::from_secs(2))) {
                    Ok(true) => {
                        println!("[EGUI] Event received! Reading message(s).");
                        if let Ok(Some(message)) = egui_buffer.try_read_latest_message() {
                            println!(
                                "[EGUI] Received State: client_name = '{}'",
                                message.get_monitor_info().get_client_name()
                            );
                            thread::sleep(Duration::from_millis(10));
                        }
                        let command = SharedCommand::view_tag(1 << (i % 9), 0);
                        println!("[EGUI] Sending command to switch to tag {}", i % 9 + 1);
                        if let Err(e) = egui_buffer.send_command(command) {
                            eprintln!("[EGUI] Failed to send command: {}", e);
                        }
                    }
                    Ok(false) => println!("[EGUI] Wait for message timed out."),
                    Err(e) => {
                        eprintln!("[EGUI] Wait for message failed: {}", e);
                        break;
                    }
                }
            }
        });

        for i in 0..5 {
            thread::sleep(Duration::from_millis(300));
            let mut message = SharedMessage::default();
            message.get_monitor_info_mut().monitor_num = 1;
            message
                .get_monitor_info_mut()
                .set_client_name(&format!("window-{}", i));
            println!(
                "\n[JWM] Writing state for '{}'",
                message.get_monitor_info().get_client_name()
            );
            if let Err(e) = jwm_buffer.try_write_message(&message) {
                eprintln!("[JWM] Failed to write message: {}", e);
            }

            println!("[JWM] Checking for commands... (timeout 10ms)");
            match jwm_buffer.wait_for_command(Some(Duration::from_millis(10))) {
                Ok(true) => {
                    println!("[JWM] Command event received! Processing command(s).");
                    while let Some(cmd) = jwm_buffer.receive_command() {
                        println!(
                            "[JWM] Processed Command: type={:?}, param={}",
                            cmd.get_command_type(),
                            cmd.get_parameter()
                        );
                        if let crate::shared_message::CommandType::ViewTag = cmd.get_command_type()
                        {
                            println!(
                                "[JWM] ACTION: Switched to tag {}",
                                cmd.get_parameter().trailing_zeros() + 1
                            );
                        }
                    }
                }
                Ok(false) => {}
                Err(e) => eprintln!("[JWM] Wait for command failed: {}", e),
            }
        }

        egui_thread.join().unwrap();
        println!("[JWM] EGUI thread finished. Test complete.");
    }

    #[test]
    fn test_message_size_and_layout() {
        println!("MessageSlot size: {}", size_of::<MessageSlot>());
        println!("MessageSlot align: {}", std::mem::align_of::<MessageSlot>());
        println!("SharedMessage size: {}", size_of::<SharedMessage>());
        println!("SharedCommand size: {}", size_of::<SharedCommand>());

        // 验证结构体是按预期对齐的
        assert!(size_of::<MessageSlot>() >= size_of::<SharedMessage>());

        // 验证对齐是合理的
        assert!(std::mem::align_of::<MessageSlot>() >= std::mem::align_of::<u64>());
        assert!(std::mem::align_of::<SharedCommand>() >= std::mem::align_of::<u64>());
    }
}
