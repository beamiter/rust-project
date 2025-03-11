use serde::{Deserialize, Serialize};
use shared_memory::{Shmem, ShmemConf};
use std::io::{Error, ErrorKind, Result};
use std::mem::size_of;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 环形缓冲区头部结构
#[repr(C)]
struct RingBufferHeader {
    // 魔数，用于验证共享内存格式
    magic: AtomicU64,
    // 版本号
    version: AtomicU64,
    // 写入索引
    write_idx: AtomicUsize,
    // 读取索引（每个读取者维护自己的）
    read_idx: AtomicUsize,
    // 缓冲区大小（消息数量）
    buffer_size: AtomicUsize,
    // 最大消息大小
    max_message_size: AtomicUsize,
    // 最后更新时间戳
    last_timestamp: AtomicU64,
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

// 魔数常量
const RING_BUFFER_MAGIC: u64 = 0x52494E47_42554646; // "RINGBUFF" in hex
                                                    // 当前版本
const RING_BUFFER_VERSION: u64 = 1;
// 默认缓冲区大小（消息数量）
const DEFAULT_BUFFER_SIZE: usize = 16;
// 默认最大消息大小
const DEFAULT_MAX_MESSAGE_SIZE: usize = 4096;

/// 共享环形缓冲区
#[allow(unused)]
pub struct SharedRingBuffer {
    shmem: Shmem,
    header: *mut RingBufferHeader,
    buffer_start: *mut u8,
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

        // 计算总大小
        let header_size = size_of::<RingBufferHeader>();
        let message_slot_size = size_of::<MessageHeader>() + max_message_size;
        let total_size = header_size + buffer_size * message_slot_size;

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
        }

        Ok(SharedRingBuffer {
            shmem,
            header,
            buffer_start,
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
            if version != RING_BUFFER_VERSION {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("不兼容的版本: {}, 期望: {}", version, RING_BUFFER_VERSION),
                ));
            }
        }

        // 计算缓冲区起始位置
        let header_size = size_of::<RingBufferHeader>();
        let buffer_start = unsafe { shmem.as_ptr().add(header_size) };

        Ok(SharedRingBuffer {
            shmem,
            header,
            buffer_start,
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
fn it_work() {
    use crate::shared_message::SharedMessage;
    use log::debug;
    use log::error;
    use log::info;
    use log::warn;
    use std::thread;
    use std::time::Duration;

    // 创建共享环形缓冲区
    let shared_path = "/tmp/my_shared_ring_buffer";
    let ring_buffer = match SharedRingBuffer::open(shared_path) {
        Ok(rb) => rb,
        Err(_) => {
            println!("创建新的共享环形缓冲区");
            SharedRingBuffer::create(shared_path, None, None).unwrap()
        }
    };

    thread::spawn(move || {
        // 创建或打开无锁环形缓冲区
        let ring_buffer: Option<SharedRingBuffer> = {
            if shared_path.is_empty() {
                None
            } else {
                match SharedRingBuffer::open(&shared_path) {
                    Ok(rb) => Some(rb),
                    Err(e) => {
                        error!("无法打开共享环形缓冲区: {}", e);
                        None
                    }
                }
            }
        };

        // 设置 panic 钩子
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            default_hook(panic_info);
            // 不需要发送任何消息，线程死亡会导致通道关闭
        }));

        let mut last_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut frame: u128 = 0;

        // 用于记录错误日志的计数器，避免日志过多
        let mut error_count = 0;
        let max_error_logs = 5;

        loop {
            let cur_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let mut need_request_repaint = false;

            if let Some(rb) = ring_buffer.as_ref() {
                // 尝试从环形缓冲区读取数据
                match rb.try_read_latest_message::<SharedMessage>() {
                    Ok(Some(message)) => {
                        println!("get message: {:?}", message);
                    }
                    Ok(None) => {
                        // 没有新数据，正常情况
                        if frame.wrapping_rem(1000) == 0 {
                            debug!("No new message in ring buffer");
                        }
                    }
                    Err(e) => {
                        // 限制错误日志数量
                        if error_count < max_error_logs {
                            error!("读取环形缓冲区错误: {}", e);
                            error_count += 1;
                        } else if error_count == max_error_logs {
                            error!("读取环形缓冲区持续出错，后续错误将不再记录");
                            error_count += 1;
                        }
                    }
                }
            } else if frame.wrapping_rem(100) == 0 {
                error!("环形缓冲区未初始化");
            }

            if frame.wrapping_rem(100) == 0 {
                info!("frame {frame}: {last_secs}, {cur_secs}");
            }

            if cur_secs != last_secs {
                need_request_repaint = true;
            }

            if need_request_repaint {
                warn!("request_repaint");
            }

            last_secs = cur_secs;
            frame = frame.wrapping_add(1).wrapping_rem(u128::MAX);

            thread::sleep(Duration::from_millis(10));
        }
    });

    // 定期写入消息
    loop {
        // 创建消息
        let message = SharedMessage::default();
        // 尝试写入消息
        match ring_buffer.try_write_message(&message) {
            Ok(true) => {
                println!("消息已写入: {:?}", message);
            }
            Ok(false) => {
                println!("缓冲区已满，等待空间...");
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
            Err(e) => {
                eprintln!("写入错误: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
