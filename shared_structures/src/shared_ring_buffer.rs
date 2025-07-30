use crate::shared_message::{CommandType, SharedCommand};
use serde::{Deserialize, Serialize};
use shared_memory::{Shmem, ShmemConf};
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 缓存行大小（x86_64）
// const CACHE_LINE_SIZE: usize = 64;

/// 填充到缓存行的原子类型，避免伪共享
#[repr(C)]
struct PaddedAtomicU32 {
    value: AtomicU32,
    _pad: [u8; 60], // 64 - 4 = 60
}

impl PaddedAtomicU32 {
    #[allow(dead_code)]
    fn new(val: u32) -> Self {
        Self {
            value: AtomicU32::new(val),
            _pad: [0; 60],
        }
    }

    fn load(&self, order: Ordering) -> u32 {
        self.value.load(order)
    }

    fn store(&self, val: u32, order: Ordering) {
        self.value.store(val, order);
    }
}

/// 环形缓冲区头部结构（已对齐）
#[repr(C)]
struct RingBufferHeader {
    magic: AtomicU64,
    version: AtomicU64,
    _pad0: [u8; 48], // 对齐到缓存行

    write_idx: PaddedAtomicU32,
    read_idx: PaddedAtomicU32,

    buffer_size: u32, // 必须是 2^n
    max_message_size: u32,
    last_timestamp: AtomicU64,
    _pad1: [u8; 40],

    cmd_write_idx: PaddedAtomicU32,
    cmd_read_idx: PaddedAtomicU32,

    // 保留填充至完整缓存行边界（可选）
    _pad2: [u8; 48],
}

/// 消息头部结构
#[repr(C)]
struct MessageHeader {
    size: u32,
    timestamp: u64,
    message_type: u32,
    checksum: u32,
}

/// 命令槽结构
#[repr(C)]
struct CommandSlot {
    cmd_type: u32,
    parameter: u32,
    monitor_id: i32,
    timestamp: u64,
    reserved: u32,
}

// 常量定义
const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646; // "RINGBUFF"
const RING_BUFFER_VERSION: u64 = 2;
const DEFAULT_BUFFER_SIZE: usize = 16; // 必须是 2 的幂
const DEFAULT_MAX_MESSAGE_SIZE: usize = 4096;
const CMD_BUFFER_SIZE: usize = 16; // 必须是 2 的幂

// 掩码用于快速取模（仅当 size 是 2^n 时成立）
const BUFFER_MASK: u32 = (DEFAULT_BUFFER_SIZE as u32) - 1;
const CMD_BUFFER_MASK: u32 = (CMD_BUFFER_SIZE as u32) - 1;

/// 共享环形缓冲区
#[allow(unused)]
pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut RingBufferHeader,
    buffer_start: *mut u8,
    cmd_buffer_start: *mut CommandSlot,
}

// 实现 Send 和 Sync
unsafe impl Send for SharedRingBuffer {}
unsafe impl Sync for SharedRingBuffer {}

impl SharedRingBuffer {
    /// 创建新的共享环形缓冲区
    pub fn create(
        path: &str,
        buffer_size: Option<usize>,
        max_message_size: Option<usize>,
    ) -> Result<Self> {
        let buffer_size = buffer_size.unwrap_or(DEFAULT_BUFFER_SIZE);
        let max_message_size = max_message_size.unwrap_or(DEFAULT_MAX_MESSAGE_SIZE);

        // 检查是否为 2 的幂
        if !buffer_size.is_power_of_two() || buffer_size == 0 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "buffer_size 必须是 2 的正整数次幂",
            ));
        }
        if !CMD_BUFFER_SIZE.is_power_of_two() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "CMD_BUFFER_SIZE 必须是 2 的正整数次幂",
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
            .map_err(|e| Error::new(ErrorKind::Other, format!("创建共享内存失败: {}", e)))?;

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
        }

        Ok(SharedRingBuffer {
            shmem,
            header,
            buffer_start,
            cmd_buffer_start,
        })
    }

    /// 打开现有的共享环形缓冲区
    pub fn open(path: &str) -> Result<Self> {
        let shmem = ShmemConf::new()
            .flink(path)
            .open()
            .map_err(|e| Error::new(ErrorKind::Other, format!("打开共享内存失败: {}", e)))?;

        let header = shmem.as_ptr() as *mut RingBufferHeader;

        unsafe {
            let magic = (*header).magic.load(Ordering::Acquire);
            if magic != RING_BUFFER_MAGIC {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("无效的魔数: 0x{:X}, 期望: 0x{:X}", magic, RING_BUFFER_MAGIC),
                ));
            }

            let version = (*header).version.load(Ordering::Acquire);
            if version != RING_BUFFER_VERSION && version != 1 {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("不兼容的版本: {}, 期望: {}", version, RING_BUFFER_VERSION),
                ));
            }

            if version == 1 {
                (*header)
                    .version
                    .store(RING_BUFFER_VERSION, Ordering::Release);
                (*header).cmd_write_idx.store(0, Ordering::Release);
                (*header).cmd_read_idx.store(0, Ordering::Release);
            }
        }

        let header_size = size_of::<RingBufferHeader>();
        let buffer_start = unsafe { shmem.as_ptr().add(header_size) };

        let buffer_size = unsafe { (*header).buffer_size as usize };
        let max_message_size = unsafe { (*header).max_message_size as usize };
        let message_slot_size = size_of::<MessageHeader>() + max_message_size;

        let cmd_buffer_start = unsafe {
            shmem
                .as_ptr()
                .add(header_size + buffer_size * message_slot_size) as *mut CommandSlot
        };

        Ok(SharedRingBuffer {
            shmem,
            header,
            buffer_start,
            cmd_buffer_start,
        })
    }

    /// 尝试写入消息（使用位掩码，不再牺牲槽位）
    pub fn try_write_message<T: Serialize>(&self, message: &T) -> Result<bool> {
        let serialized = bincode::serialize(message)
            .map_err(|e| Error::new(ErrorKind::InvalidInput, format!("序列化失败: {}", e)))?;

        unsafe {
            let max_msg_size = (*self.header).max_message_size as usize;
            if serialized.len() > max_msg_size {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "消息太大: {} 字节, 最大允许: {} 字节",
                        serialized.len(),
                        max_msg_size
                    ),
                ));
            }

            let write_idx = (*self.header).write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).read_idx.load(Ordering::Acquire);

            // 使用差值判断是否满（支持 full buffer）
            let count = write_idx.wrapping_sub(read_idx);
            if count == (*self.header).buffer_size {
                return Ok(false); // 已满
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

            // 更新写指针（Release 确保数据先写入）
            let next_write = write_idx.wrapping_add(1);
            (*self.header)
                .write_idx
                .store(next_write, Ordering::Release);

            Ok(true)
        }
    }

    /// 尝试读取最新消息（跳过旧消息）
    pub fn try_read_latest_message<T: for<'de> Deserialize<'de>>(&self) -> Result<Option<T>> {
        unsafe {
            let max_msg_size = (*self.header).max_message_size as usize;
            let message_slot_size = size_of::<MessageHeader>() + max_msg_size;

            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let mut read_idx = (*self.header).read_idx.load(Ordering::Relaxed);

            if read_idx == write_idx {
                return Ok(None);
            }

            let count = write_idx.wrapping_sub(read_idx);
            if count > 1 {
                // 跳到最新一条（倒数第一条）
                read_idx = write_idx.wrapping_sub(1);
            }

            let slot_idx = (read_idx & BUFFER_MASK) as usize;
            let slot_offset = slot_idx * message_slot_size;

            let msg_header_ptr = self.buffer_start.add(slot_offset) as *const MessageHeader;
            let msg_size = (*msg_header_ptr).size as usize;

            if msg_size > max_msg_size {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("无效消息大小: {}", msg_size),
                ));
            }

            let msg_data_ptr = self
                .buffer_start
                .add(slot_offset + size_of::<MessageHeader>());
            let msg_data = std::slice::from_raw_parts(msg_data_ptr, msg_size);

            let checksum = calculate_checksum(msg_data);
            if checksum != (*msg_header_ptr).checksum {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "校验和错误: 计算={}, 期望={}",
                        checksum,
                        (*msg_header_ptr).checksum
                    ),
                ));
            }

            let message: T = bincode::deserialize(msg_data)
                .map_err(|e| Error::new(ErrorKind::InvalidData, format!("反序列化失败: {}", e)))?;

            let next_read = read_idx.wrapping_add(1);
            (*self.header).read_idx.store(next_read, Ordering::Release);

            Ok(Some(message))
        }
    }

    /// 获取最后时间戳
    pub fn get_last_timestamp(&self) -> u64 {
        unsafe { (*self.header).last_timestamp.load(Ordering::Acquire) }
    }

    /// 获取可用消息数
    pub fn available_messages(&self) -> usize {
        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let read_idx = (*self.header).read_idx.load(Ordering::Acquire);
            write_idx.wrapping_sub(read_idx) as usize
        }
    }

    /// 重置读索引
    pub fn reset_read_index(&self) {
        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            (*self.header).read_idx.store(write_idx, Ordering::Release);
        }
    }

    /// 发送命令（使用 CMD_BUFFER_MASK）
    pub fn send_command(&self, command: SharedCommand) -> Result<bool> {
        unsafe {
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Acquire);

            let count = write_idx.wrapping_sub(read_idx);
            if count == CMD_BUFFER_SIZE as u32 {
                return Ok(false); // 已满
            }

            let slot_idx = (write_idx & CMD_BUFFER_MASK) as usize;
            let cmd_slot = &mut *self.cmd_buffer_start.add(slot_idx);
            cmd_slot.cmd_type = command.cmd_type as u32;
            cmd_slot.parameter = command.parameter;
            cmd_slot.monitor_id = command.monitor_id;
            cmd_slot.timestamp = command.timestamp;

            let next_write = write_idx.wrapping_add(1);
            (*self.header)
                .cmd_write_idx
                .store(next_write, Ordering::Release);

            Ok(true)
        }
    }

    /// 接收命令
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

            let next_read = read_idx.wrapping_add(1);
            (*self.header)
                .cmd_read_idx
                .store(next_read, Ordering::Release);

            Some(command)
        }
    }

    /// 检查是否有命令
    pub fn has_command(&self) -> bool {
        self.available_commands() > 0
    }

    /// 获取可用命令数
    pub fn available_commands(&self) -> usize {
        unsafe {
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Acquire);
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Acquire);
            write_idx.wrapping_sub(read_idx) as usize
        }
    }

    /// 获取原始指针（慎用）
    pub fn get_ptr(&self) -> *mut u8 {
        self.shmem.as_ptr()
    }
}

/// 校验和计算
fn calculate_checksum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    for &b in data {
        sum = sum.wrapping_add(b as u32);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_message::SharedMessage;

    #[test]
    fn test_bidirectional_communication() {
        use std::thread;
        use std::time::Duration;

        let shared_path = "/tmp/test_optimized_buffer";
        let ring_buffer = match SharedRingBuffer::open(shared_path) {
            Ok(rb) => rb,
            Err(_) => SharedRingBuffer::create(shared_path, None, None).unwrap(),
        };

        let egui_thread = thread::spawn(move || {
            let egui_buffer = SharedRingBuffer::open(shared_path).unwrap();
            for i in 1..=5 {
                match egui_buffer.try_read_latest_message::<SharedMessage>() {
                    Ok(Some(message)) => {
                        println!("[egui] 收到状态: {:?}", message.monitor_info);
                        let command = SharedCommand::view_tag(1 << (i % 9), 0);
                        if let Err(e) = egui_buffer.send_command(command) {
                            eprintln!("[egui] 发送命令失败: {}", e);
                        } else {
                            println!("[egui] 发送切换标签命令 {}", i % 9 + 1);
                        }
                    }
                    Ok(None) => println!("[egui] 无新状态"),
                    Err(e) => eprintln!("[egui] 读取错误: {}", e),
                }
                thread::sleep(Duration::from_millis(150));
            }
        });

        for i in 0..10 {
            let mut message = SharedMessage::default();
            message.monitor_info.monitor_num = 0;
            message.monitor_info.client_name = format!("窗口 {}", i);
            if let Err(e) = ring_buffer.try_write_message(&message) {
                eprintln!("[jwm] 写入失败: {}", e);
            } else {
                println!("[jwm] 发送状态: 窗口 {}", i);
            }

            while let Some(cmd) = ring_buffer.receive_command() {
                println!(
                    "[jwm] 收到命令: {:?}, 参数: {}",
                    cmd.cmd_type, cmd.parameter
                );
                if let CommandType::ViewTag = cmd.cmd_type {
                    println!("[jwm] 切换到标签 {}", cmd.parameter.trailing_zeros() + 1);
                }
            }

            thread::sleep(Duration::from_millis(100));
        }

        egui_thread.join().unwrap();
    }
}
