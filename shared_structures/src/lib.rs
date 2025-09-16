// src/lib.rs

// 消息结构体定义，保持不变
mod shared_message;
pub use shared_message::{
    CommandType, MonitorInfo, SharedCommand, SharedMessage, TagStatus, MAX_CLIENT_NAME_LEN,
    MAX_LT_SYMBOL_LEN, MAX_TAGS,
};

// 核心环形缓冲区实现
mod shared_ring_buffer;
pub use shared_ring_buffer::SharedRingBuffer;

// 引入后端模块，并导出公共的 Strategy 枚举
mod backends;
pub use backends::common::SyncStrategy;
