use serde::{Deserialize, Serialize};
use shared_memory::{Shmem, ShmemConf};
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// nix 用于 eventfd 和 poll
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::eventfd::EventFd;
use nix::unistd;
use std::os::unix::io::{AsRawFd, BorrowedFd};

use crate::shared_message::{CommandType, SharedCommand};

#[repr(C)]
#[derive(Debug)]
struct PaddedAtomicU32 {
    value: AtomicU32,
    _pad: [u8; 60],
}

impl PaddedAtomicU32 {
    fn load(&self, order: Ordering) -> u32 {
        self.value.load(order)
    }
    fn store(&self, val: u32, order: Ordering) {
        self.value.store(val, order);
    }
}

#[repr(C)]
#[derive(Debug)]
struct RingBufferHeader {
    magic: AtomicU64,
    version: AtomicU64,
    _pad0: [u8; 48],
    write_idx: PaddedAtomicU32,
    read_idx: PaddedAtomicU32,
    buffer_size: u32,
    max_message_size: u32,
    last_timestamp: AtomicU64,
    message_event_fd: AtomicI32,
    command_event_fd: AtomicI32,
    _pad1: [u8; 32],
    cmd_write_idx: PaddedAtomicU32,
    cmd_read_idx: PaddedAtomicU32,
    _pad2: [u8; 48],
}

#[repr(C)]
struct MessageHeader {
    size: u32,
    timestamp: u64,
    message_type: u32,
    checksum: u32,
}
#[repr(C)]
struct CommandSlot {
    cmd_type: u32,
    parameter: u32,
    monitor_id: i32,
    timestamp: u64,
    reserved: u32,
}

const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646;
const RING_BUFFER_VERSION: u64 = 3;
const DEFAULT_BUFFER_SIZE: usize = 16;
const DEFAULT_MAX_MESSAGE_SIZE: usize = 4096;
const CMD_BUFFER_SIZE: usize = 16;
const BUFFER_MASK: u32 = (DEFAULT_BUFFER_SIZE as u32) - 1;
const CMD_BUFFER_MASK: u32 = (CMD_BUFFER_SIZE as u32) - 1;

pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut RingBufferHeader,
    buffer_start: *mut u8,
    cmd_buffer_start: *mut CommandSlot,
    is_creator: bool,
    message_event_fd: i32,
    command_event_fd: i32,
}

unsafe impl Send for SharedRingBuffer {}
unsafe impl Sync for SharedRingBuffer {}

impl SharedRingBuffer {
    pub fn create(
        path: &str,
        buffer_size: Option<usize>,
        max_message_size: Option<usize>,
    ) -> Result<Self> {
        let buffer_size = buffer_size.unwrap_or(DEFAULT_BUFFER_SIZE);
        let max_message_size = max_message_size.unwrap_or(DEFAULT_MAX_MESSAGE_SIZE);

        if !buffer_size.is_power_of_two() || !CMD_BUFFER_SIZE.is_power_of_two() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "buffer_size and CMD_BUFFER_SIZE must be powers of 2",
            ));
        }

        let header_size = size_of::<RingBufferHeader>();
        let message_slot_size = size_of::<MessageHeader>() + max_message_size;
        let cmd_buffer_bytes = CMD_BUFFER_SIZE * size_of::<CommandSlot>();
        let total_size = header_size + buffer_size * message_slot_size + cmd_buffer_bytes;

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
        let buffer_start = unsafe { shmem.as_ptr().add(header_size) };
        let cmd_buffer_start = unsafe {
            shmem
                .as_ptr()
                .add(header_size + buffer_size * message_slot_size) as *mut CommandSlot
        };

        unsafe {
            (*header).magic.store(RING_BUFFER_MAGIC, Ordering::Release);
            (*header)
                .version
                .store(RING_BUFFER_VERSION, Ordering::Release);
            (*header).write_idx.store(0, Ordering::Release);
            (*header).read_idx.store(0, Ordering::Release);
            (*header).buffer_size = buffer_size as u32;
            (*header).max_message_size = max_message_size as u32;
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
            buffer_start,
            cmd_buffer_start,
            is_creator: true,
            message_event_fd: msg_fd_raw,
            command_event_fd: cmd_fd_raw,
        })
    }

    pub fn open(path: &str) -> Result<Self> {
        let shmem = ShmemConf::new()
            .flink(path)
            .open()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Failed to open shmem: {}", e)))?;

        let header = shmem.as_ptr() as *mut RingBufferHeader;
        let (message_event_fd, command_event_fd);

        unsafe {
            if (*header).magic.load(Ordering::Acquire) != RING_BUFFER_MAGIC {
                return Err(Error::new(ErrorKind::InvalidData, "Invalid magic number"));
            }
            if (*header).version.load(Ordering::Acquire) != RING_BUFFER_VERSION {
                return Err(Error::new(ErrorKind::InvalidData, "Incompatible version"));
            }
            message_event_fd = (*header).message_event_fd.load(Ordering::Acquire);
            command_event_fd = (*header).command_event_fd.load(Ordering::Acquire);
        }

        let header_size = size_of::<RingBufferHeader>();
        let buffer_start = unsafe { shmem.as_ptr().add(header_size) };
        let (buffer_size, max_message_size) = unsafe {
            (
                (*header).buffer_size as usize,
                (*header).max_message_size as usize,
            )
        };
        let message_slot_size = size_of::<MessageHeader>() + max_message_size;
        let cmd_buffer_start = unsafe {
            shmem
                .as_ptr()
                .add(header_size + buffer_size * message_slot_size) as *mut CommandSlot
        };

        Ok(Self {
            shmem,
            header,
            buffer_start,
            cmd_buffer_start,
            is_creator: false,
            message_event_fd,
            command_event_fd,
        })
    }

    pub fn try_write_message<T: Serialize>(&self, message: &T) -> Result<bool> {
        let serialized = bincode::serialize(message).map_err(|e| {
            Error::new(
                ErrorKind::InvalidInput,
                format!("Serialization failed: {}", e),
            )
        })?;
        unsafe {
            let max_msg_size = (*self.header).max_message_size as usize;
            if serialized.len() > max_msg_size {
                return Err(Error::new(ErrorKind::InvalidInput, "Message too large"));
            }
            let write_idx = (*self.header).write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).read_idx.load(Ordering::Acquire);
            if write_idx.wrapping_sub(read_idx) == (*self.header).buffer_size {
                return Ok(false);
            }
            let slot_idx = (write_idx & BUFFER_MASK) as usize;
            let message_slot_size = size_of::<MessageHeader>() + max_msg_size;
            let slot_offset = slot_idx * message_slot_size;
            let msg_header_ptr = self.buffer_start.add(slot_offset) as *mut MessageHeader;
            (*msg_header_ptr).size = serialized.len() as u32;
            (*msg_header_ptr).timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            (*msg_header_ptr).message_type = 0;
            (*msg_header_ptr).checksum = calculate_checksum(&serialized);
            let msg_data_ptr = self
                .buffer_start
                .add(slot_offset + size_of::<MessageHeader>());
            std::ptr::copy_nonoverlapping(serialized.as_ptr(), msg_data_ptr, serialized.len());
            (*self.header)
                .last_timestamp
                .store((*msg_header_ptr).timestamp, Ordering::Release);
            (*self.header)
                .write_idx
                .store(write_idx.wrapping_add(1), Ordering::Release);
        }
        self.signal_fd(self.message_event_fd)?;
        Ok(true)
    }

    pub fn send_command(&self, command: SharedCommand) -> Result<bool> {
        unsafe {
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Acquire);
            if write_idx.wrapping_sub(read_idx) == CMD_BUFFER_SIZE as u32 {
                return Ok(false);
            }
            let slot_idx = (write_idx & CMD_BUFFER_MASK) as usize;
            let cmd_slot = &mut *self.cmd_buffer_start.add(slot_idx);
            *cmd_slot = CommandSlot {
                cmd_type: command.cmd_type as u32,
                parameter: command.parameter,
                monitor_id: command.monitor_id,
                timestamp: command.timestamp,
                reserved: 0,
            };
            (*self.header)
                .cmd_write_idx
                .store(write_idx.wrapping_add(1), Ordering::Release);
        }
        self.signal_fd(self.command_event_fd)?;
        Ok(true)
    }

    pub fn wait_for_message(&self, timeout: Option<Duration>) -> Result<bool> {
        self.wait_on_fd(self.message_event_fd, timeout)
    }
    pub fn wait_for_command(&self, timeout: Option<Duration>) -> Result<bool> {
        self.wait_on_fd(self.command_event_fd, timeout)
    }

    // (Other methods like try_read_latest_message, receive_command, etc. remain unchanged)
    pub fn try_read_latest_message<T: for<'de> Deserialize<'de>>(&self) -> Result<Option<T>> {
        unsafe {
            let max_msg_size = (*self.header).max_message_size as usize;
            let message_slot_size = size_of::<MessageHeader>() + max_msg_size;
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let mut read_idx = (*self.header).read_idx.load(Ordering::Relaxed);
            if read_idx == write_idx {
                return Ok(None);
            }
            if write_idx.wrapping_sub(read_idx) > 1 {
                read_idx = write_idx.wrapping_sub(1);
            }
            let slot_idx = (read_idx & BUFFER_MASK) as usize;
            let slot_offset = slot_idx * message_slot_size;
            let msg_header_ptr = self.buffer_start.add(slot_offset) as *const MessageHeader;
            let msg_size = (*msg_header_ptr).size as usize;
            if msg_size > max_msg_size {
                return Err(Error::new(ErrorKind::InvalidData, "Invalid message size"));
            }
            let msg_data_ptr = self
                .buffer_start
                .add(slot_offset + size_of::<MessageHeader>());
            let msg_data = std::slice::from_raw_parts(msg_data_ptr, msg_size);
            if calculate_checksum(msg_data) != (*msg_header_ptr).checksum {
                return Err(Error::new(ErrorKind::InvalidData, "Checksum mismatch"));
            }
            let message: T = bincode::deserialize(msg_data).map_err(|e| {
                Error::new(
                    ErrorKind::InvalidData,
                    format!("Deserialization failed: {}", e),
                )
            })?;
            (*self.header)
                .read_idx
                .store(read_idx.wrapping_add(1), Ordering::Release);
            Ok(Some(message))
        }
    }
    pub fn receive_command(&self) -> Option<SharedCommand> {
        unsafe {
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Relaxed);
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Acquire);
            if read_idx == write_idx {
                return None;
            }
            let slot_idx = (read_idx & CMD_BUFFER_MASK) as usize;
            let cmd_slot = &*self.cmd_buffer_start.add(slot_idx);
            let command = SharedCommand {
                cmd_type: match cmd_slot.cmd_type {
                    1 => CommandType::ViewTag,
                    2 => CommandType::ToggleTag,
                    3 => CommandType::SetLayout,
                    _ => CommandType::None,
                },
                parameter: cmd_slot.parameter,
                monitor_id: cmd_slot.monitor_id,
                timestamp: cmd_slot.timestamp,
            };
            (*self.header)
                .cmd_read_idx
                .store(read_idx.wrapping_add(1), Ordering::Release);
            Some(command)
        }
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

    fn wait_on_fd(&self, fd: i32, timeout: Option<Duration>) -> Result<bool> {
        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let poll_fd = PollFd::new(borrowed_fd, PollFlags::POLLIN);
        let timeout_ms = timeout.map_or(-1, |d| d.as_millis() as i32);
        let num_events = poll(&mut [poll_fd], timeout_ms as u16)
            .map_err(|e| Error::new(ErrorKind::Other, format!("poll failed: {}", e)))?;
        if num_events == 0 {
            return Ok(false);
        }
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

fn calculate_checksum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |sum, &b| sum.wrapping_add(b as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_message::{MonitorInfo, SharedMessage};
    use std::thread;

    #[test]
    fn test_bidirectional_communication_with_eventfd() {
        let shared_path = "/tmp/test_eventfd_buffer";
        let _ = std::fs::remove_file(shared_path);

        let jwm_buffer = SharedRingBuffer::create(shared_path, None, None).unwrap();
        println!("[JWM] SharedRingBuffer created at '{}'", shared_path);

        let egui_thread = thread::spawn(move || {
            let egui_buffer = SharedRingBuffer::open(shared_path).unwrap();
            println!("[EGUI] Opened SharedRingBuffer.");

            for i in 1..=5 {
                println!("\n[EGUI] Waiting for new message... (timeout 2s)");
                match egui_buffer.wait_for_message(Some(Duration::from_secs(2))) {
                    Ok(true) => {
                        println!("[EGUI] Event received! Reading message(s).");
                        if let Ok(Some(message)) =
                            egui_buffer.try_read_latest_message::<SharedMessage>()
                        {
                            println!(
                                "[EGUI] Received State: client_name = '{}'",
                                message.monitor_info.client_name
                            );
                            // 添加一个小延迟
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
            message.monitor_info.monitor_num = 1;
            message.monitor_info.client_name = format!("window-{}", i);
            println!(
                "\n[JWM] Writing state for '{}'",
                message.monitor_info.client_name
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
                            cmd.cmd_type, cmd.parameter
                        );
                        if let CommandType::ViewTag = cmd.cmd_type {
                            println!(
                                "[JWM] ACTION: Switched to tag {}",
                                cmd.parameter.trailing_zeros() + 1
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
}
