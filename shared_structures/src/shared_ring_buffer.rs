use crate::shared_message::{Command, CommandType};
use serde::{Deserialize, Serialize};
use shared_memory::{Shmem, ShmemConf};
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
/// 环形缓冲区头部结构
#[repr(C)]
struct RingBufferHeader {
    // 现有字段...
    magic: AtomicU64,
    version: AtomicU64,
    write_idx: AtomicUsize,
    read_idx: AtomicUsize,
    buffer_size: AtomicUsize,
    max_message_size: AtomicUsize,
    last_timestamp: AtomicU64,
    // 新增命令通道字段
    cmd_write_idx: AtomicUsize, // egui_bar 写入命令的索引
    cmd_read_idx: AtomicUsize,  // jwm 读取命令的索引
}

/// 消息头部结构
#[repr(C)]
struct MessageHeader {
    // 消息大小（不包括头部）
    size: u32,
    // 消息时间戳
    timestamp: u64,
    // 消息类型
    message_type: u32,
    // 校验和
    checksum: u32,
}

/// 命令槽结构，直接在共享内存中保存命令
#[repr(C)]
struct CommandSlot {
    cmd_type: u32,
    parameter: u32,
    monitor_id: i32,
    timestamp: u64,
    reserved: u32, // 保留字段，保证结构体字节对齐
}

// 常量定义
const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646; // "RINGBUFF" in hex
const RING_BUFFER_VERSION: u64 = 2; // 版本升级到2
const DEFAULT_BUFFER_SIZE: usize = 16;
const DEFAULT_MAX_MESSAGE_SIZE: usize = 4096;
const CMD_BUFFER_SIZE: usize = 16; // 命令缓冲区大小

/// 共享环形缓冲区
#[allow(unused)]
pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut RingBufferHeader,
    buffer_start: *mut u8,
    cmd_buffer_start: *mut CommandSlot,
}

// 实现 Send 和 Sync 特性，允许在线程间安全传递
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
        // 计算总大小，包括命令缓冲区
        let header_size = size_of::<RingBufferHeader>();
        let message_slot_size = size_of::<MessageHeader>() + max_message_size;
        let cmd_buffer_size = CMD_BUFFER_SIZE * size_of::<CommandSlot>();
        let total_size = header_size + buffer_size * message_slot_size + cmd_buffer_size;
        // 创建共享内存

        let shmem = ShmemConf::new()
            .size(total_size)
            .flink(path)
            .force_create_flink()
            .create()
            .map_err(|e| Error::new(ErrorKind::Other, format!("创建共享内存失败: {}", e)))?;

        // 初始化头部
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
            (*header).buffer_size.store(buffer_size, Ordering::Release);
            (*header)
                .max_message_size
                .store(max_message_size, Ordering::Release);
            (*header).last_timestamp.store(0, Ordering::Release);
            // 初始化命令缓冲区索引
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
        // 打开共享内存
        let shmem = ShmemConf::new()
            .flink(path)
            .open()
            .map_err(|e| Error::new(ErrorKind::Other, format!("打开共享内存失败: {}", e)))?;
        // 获取头部
        let header = shmem.as_ptr() as *mut RingBufferHeader;

        // 验证魔数和版本
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
            // 如果是旧版本，升级到新版本
            if version == 1 {
                (*header)
                    .version
                    .store(RING_BUFFER_VERSION, Ordering::Release);
                (*header).cmd_write_idx.store(0, Ordering::Release);
                (*header).cmd_read_idx.store(0, Ordering::Release);
            }
        }

        // 计算缓冲区起始位置
        let header_size = size_of::<RingBufferHeader>();
        let buffer_start = unsafe { shmem.as_ptr().add(header_size) };
        // 计算命令缓冲区起始位置
        let buffer_size = unsafe { (*header).buffer_size.load(Ordering::Relaxed) };
        let max_message_size = unsafe { (*header).max_message_size.load(Ordering::Relaxed) };
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

    /// 尝试写入消息
    pub fn try_write_message<T: Serialize>(&self, message: &T) -> Result<bool> {
        // 序列化消息
        let serialized = bincode::serialize(message)
            .map_err(|e| Error::new(ErrorKind::InvalidInput, format!("序列化失败: {}", e)))?;

        unsafe {
            let buffer_size = (*self.header).buffer_size.load(Ordering::Relaxed);
            let max_message_size = (*self.header).max_message_size.load(Ordering::Relaxed);

            // 检查消息大小
            if serialized.len() > max_message_size {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "消息太大: {} 字节, 最大允许: {} 字节",
                        serialized.len(),
                        max_message_size
                    ),
                ));
            }

            // 读取当前索引
            let write_idx = (*self.header).write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).read_idx.load(Ordering::Acquire);

            // 计算下一个写入位置
            let next_write_idx = (write_idx + 1) % buffer_size;

            // 检查缓冲区是否已满
            if next_write_idx == read_idx {
                return Ok(false); // 缓冲区已满
            }

            // 计算消息槽位置
            let message_slot_size = size_of::<MessageHeader>() + max_message_size;
            let slot_offset = write_idx * message_slot_size;

            // 获取消息头部指针
            let msg_header_ptr = self.buffer_start.add(slot_offset) as *mut MessageHeader;

            // 填充消息头部
            (*msg_header_ptr).size = serialized.len() as u32;
            (*msg_header_ptr).timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            (*msg_header_ptr).message_type = 0; // 默认消息类型
            (*msg_header_ptr).checksum = calculate_checksum(&serialized); // 计算校验和

            // 获取消息数据指针
            let msg_data_ptr = self
                .buffer_start
                .add(slot_offset + size_of::<MessageHeader>());

            // 复制消息数据
            std::ptr::copy_nonoverlapping(serialized.as_ptr(), msg_data_ptr, serialized.len());

            // 更新最后时间戳
            (*self.header)
                .last_timestamp
                .store((*msg_header_ptr).timestamp, Ordering::Release);

            // 更新写入索引（内存屏障确保所有数据写入完成后才更新索引）
            (*self.header)
                .write_idx
                .store(next_write_idx, Ordering::Release);

            Ok(true)
        }
    }

    /// 尝试读取最新消息（跳过旧消息）
    pub fn try_read_latest_message<T: for<'de> Deserialize<'de>>(&self) -> Result<Option<T>> {
        unsafe {
            let buffer_size = (*self.header).buffer_size.load(Ordering::Relaxed);
            let max_message_size = (*self.header).max_message_size.load(Ordering::Relaxed);
            let message_slot_size = size_of::<MessageHeader>() + max_message_size;

            // 读取当前索引
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let mut read_idx = (*self.header).read_idx.load(Ordering::Relaxed);

            // 检查是否有新消息
            if read_idx == write_idx {
                return Ok(None); // 没有新消息
            }

            // 如果有多条未读消息，直接跳到最新的
            let available = if write_idx > read_idx {
                write_idx - read_idx
            } else {
                buffer_size - read_idx + write_idx
            };

            if available > 1 {
                // 跳过旧消息，只读取最新的
                read_idx = if write_idx > 0 {
                    (write_idx - 1) % buffer_size
                } else {
                    buffer_size - 1
                };
            }

            // 计算消息槽位置
            let slot_offset = read_idx * message_slot_size;

            // 获取消息头部指针
            let msg_header_ptr = self.buffer_start.add(slot_offset) as *const MessageHeader;

            // 读取消息大小
            let msg_size = (*msg_header_ptr).size as usize;
            if msg_size > max_message_size {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("无效的消息大小: {} 字节", msg_size),
                ));
            }

            // 读取消息时间戳
            let _msg_timestamp = (*msg_header_ptr).timestamp;

            // 获取消息数据指针
            let msg_data_ptr = self
                .buffer_start
                .add(slot_offset + size_of::<MessageHeader>());

            // 读取消息数据
            let msg_data = std::slice::from_raw_parts(msg_data_ptr, msg_size);

            // 验证校验和
            let checksum = calculate_checksum(msg_data);
            if checksum != (*msg_header_ptr).checksum {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "校验和不匹配: 计算得到 {}, 期望 {}",
                        checksum,
                        (*msg_header_ptr).checksum
                    ),
                ));
            }

            // 反序列化消息
            let message: T = bincode::deserialize(msg_data)
                .map_err(|e| Error::new(ErrorKind::InvalidData, format!("反序列化失败: {}", e)))?;

            // 更新读取索引
            let next_read_idx = (read_idx + 1) % buffer_size;
            (*self.header)
                .read_idx
                .store(next_read_idx, Ordering::Release);

            Ok(Some(message))
        }
    }

    /// 获取最后更新时间戳
    pub fn get_last_timestamp(&self) -> u64 {
        unsafe { (*self.header).last_timestamp.load(Ordering::Acquire) }
    }

    /// 获取可用消息数量
    pub fn available_messages(&self) -> usize {
        unsafe {
            let buffer_size = (*self.header).buffer_size.load(Ordering::Relaxed);
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            let read_idx = (*self.header).read_idx.load(Ordering::Acquire);

            if write_idx >= read_idx {
                write_idx - read_idx
            } else {
                buffer_size - read_idx + write_idx
            }
        }
    }

    /// 重置读取索引（丢弃所有未读消息）
    pub fn reset_read_index(&self) {
        unsafe {
            let write_idx = (*self.header).write_idx.load(Ordering::Acquire);
            (*self.header).read_idx.store(write_idx, Ordering::Release);
        }
    }

    /// 发送命令（通常由 egui_bar 调用）
    pub fn send_command(&self, command: Command) -> Result<bool> {
        unsafe {
            // 读取当前索引
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Relaxed);
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Acquire);
            // 检查缓冲区是否已满
            if ((write_idx + 1) % CMD_BUFFER_SIZE) == read_idx {
                return Ok(false); // 缓冲区已满
            }
            // 获取命令槽并填充数据
            let cmd_slot = &mut *self.cmd_buffer_start.add(write_idx % CMD_BUFFER_SIZE);
            cmd_slot.cmd_type = command.cmd_type as u32;
            cmd_slot.parameter = command.parameter;
            cmd_slot.monitor_id = command.monitor_id;
            cmd_slot.timestamp = command.timestamp;
            // 更新写索引（内存屏障确保所有数据写入完成后才更新索引）
            (*self.header)
                .cmd_write_idx
                .store((write_idx + 1) % CMD_BUFFER_SIZE, Ordering::Release);
            Ok(true)
        }
    }

    /// 接收命令（通常由 jwm 调用）
    pub fn receive_command(&self) -> Option<Command> {
        unsafe {
            // 读取当前索引
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Relaxed);
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Acquire);
            // 检查是否有新命令
            if read_idx == write_idx {
                return None; // 没有新命令
            }
            // 获取命令数据
            let cmd_slot = &*self.cmd_buffer_start.add(read_idx % CMD_BUFFER_SIZE);
            let command = Command {
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

            // 更新读索引
            (*self.header)
                .cmd_read_idx
                .store((read_idx + 1) % CMD_BUFFER_SIZE, Ordering::Release);
            Some(command)
        }
    }

    /// 检查是否有可用命令
    pub fn has_command(&self) -> bool {
        unsafe {
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Relaxed);
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Acquire);
            read_idx != write_idx
        }
    }

    /// 获取可用命令数量
    pub fn available_commands(&self) -> usize {
        unsafe {
            let read_idx = (*self.header).cmd_read_idx.load(Ordering::Relaxed);
            let write_idx = (*self.header).cmd_write_idx.load(Ordering::Acquire);
            if write_idx >= read_idx {
                write_idx - read_idx
            } else {
                CMD_BUFFER_SIZE - read_idx + write_idx
            }
        }
    }

    /// 获取指针（用于直接访问，慎用）
    pub fn get_ptr(&self) -> *mut u8 {
        self.shmem.as_ptr()
    }
}

/// 计算简单的校验和
fn calculate_checksum(data: &[u8]) -> u32 {
    let mut checksum: u32 = 0;
    for &byte in data {
        checksum = checksum.wrapping_add(byte as u32);
    }
    checksum
}

#[test]
fn test_bidirectional_communication() {
    use crate::shared_message::{Command, CommandType, SharedMessage};
    use std::thread;
    use std::time::Duration;

    // 创建共享环形缓冲区
    let shared_path = "/tmp/test_bidirectional_buffer";
    let ring_buffer = match SharedRingBuffer::open(shared_path) {
        Ok(rb) => rb,
        Err(_) => {
            println!("创建新的共享环形缓冲区");
            SharedRingBuffer::create(shared_path, None, None).unwrap()
        }
    };

    // 启动模拟 egui_bar 的线程
    let egui_thread = thread::spawn(move || {
        // 打开相同的共享缓冲区
        let egui_buffer = SharedRingBuffer::open(shared_path).unwrap();
        for i in 1..=5 {
            // 读取 jwm 发来的状态
            match egui_buffer.try_read_latest_message::<SharedMessage>() {
                Ok(Some(message)) => {
                    println!("[egui] 收到状态更新: {:?}", message.monitor_info);
                    // 发送命令回 jwm
                    let command = Command::view_tag(1 << (i % 9), 0);
                    if let Err(e) = egui_buffer.send_command(command) {
                        eprintln!("[egui] 发送命令失败: {}", e);
                    } else {
                        println!("[egui] 发送查看标签 {} 命令", i % 9 + 1);
                    }
                }
                Ok(None) => println!("[egui] 没有新的状态更新"),
                Err(e) => eprintln!("[egui] 读取错误: {}", e),
            }
            thread::sleep(Duration::from_millis(150));
        }
    });

    // 模拟 jwm 的主线程
    for i in 0..10 {
        // 发送状态更新到 egui_bar
        let mut message = SharedMessage::default();
        message.monitor_info.monitor_num = 0;
        message.monitor_info.client_name = format!("窗口 {}", i);
        if let Err(e) = ring_buffer.try_write_message(&message) {
            eprintln!("[jwm] 写入状态失败: {}", e);
        } else {
            println!("[jwm] 发送状态更新: 窗口 {}", i);
        }
        // 检查来自 egui_bar 的命令
        while let Some(cmd) = ring_buffer.receive_command() {
            println!(
                "[jwm] 收到命令: {:?}, 参数: {}",
                cmd.cmd_type, cmd.parameter
            );
            // 模拟处理命令
            if let CommandType::ViewTag = cmd.cmd_type {
                println!("[jwm] 切换到标签 {}", cmd.parameter.trailing_zeros() + 1);
            }
        }
        thread::sleep(Duration::from_millis(100));
    }

    // 等待 egui 线程结束
    egui_thread.join().unwrap();
}
