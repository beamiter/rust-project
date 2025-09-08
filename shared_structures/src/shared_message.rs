use serde::Serialize;
use serde_big_array::BigArray;
use std::time::{SystemTime, UNIX_EPOCH};

// 常量定义
pub const MAX_CLIENT_NAME_LEN: usize = 128;
pub const MAX_LT_SYMBOL_LEN: usize = 32;
pub const MAX_TAGS: usize = 9;

// 移除 packed，使用合理的对齐
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TagStatus {
    pub is_selected: bool,
    pub is_urg: bool,
    pub is_filled: bool,
    pub is_occ: bool,
}

impl Default for TagStatus {
    fn default() -> Self {
        Self {
            is_selected: false,
            is_urg: false,
            is_filled: false,
            is_occ: false,
        }
    }
}

impl TagStatus {
    pub fn new(is_selected: bool, is_urg: bool, is_filled: bool, is_occ: bool) -> Self {
        Self {
            is_selected,
            is_urg,
            is_filled,
            is_occ,
        }
    }
}

// 使用更合理的对齐策略
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MonitorInfo {
    // 将较大的字段放在前面，确保对齐
    pub monitor_num: i32,
    pub monitor_width: i32,
    pub monitor_height: i32,
    pub monitor_x: i32,
    pub monitor_y: i32,
    // 固定大小的数组
    pub tag_status_vec: [TagStatus; MAX_TAGS],
    #[serde(with = "BigArray")]
    pub client_name: [u8; MAX_CLIENT_NAME_LEN],
    pub ltsymbol: [u8; MAX_LT_SYMBOL_LEN],
}

impl Default for MonitorInfo {
    fn default() -> Self {
        Self {
            client_name: [0; MAX_CLIENT_NAME_LEN],
            tag_status_vec: [TagStatus::default(); MAX_TAGS],
            monitor_num: 0,
            monitor_width: 0,
            monitor_height: 0,
            monitor_x: 0,
            monitor_y: 0,
            ltsymbol: [0; MAX_LT_SYMBOL_LEN],
        }
    }
}

impl MonitorInfo {
    // 辅助方法：设置客户端名称
    pub fn set_client_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(MAX_CLIENT_NAME_LEN - 1);
        self.client_name[..len].copy_from_slice(&bytes[..len]);
        // 清零剩余部分
        for i in len..MAX_CLIENT_NAME_LEN {
            self.client_name[i] = 0;
        }
    }

    // 辅助方法：获取客户端名称
    pub fn get_client_name(&self) -> String {
        let null_pos = self
            .client_name
            .iter()
            .position(|&x| x == 0)
            .unwrap_or(MAX_CLIENT_NAME_LEN);
        String::from_utf8_lossy(&self.client_name[..null_pos]).to_string()
    }

    // 辅助方法：设置布局符号
    pub fn set_ltsymbol(&mut self, symbol: &str) {
        let bytes = symbol.as_bytes();
        let len = bytes.len().min(MAX_LT_SYMBOL_LEN - 1);
        self.ltsymbol[..len].copy_from_slice(&bytes[..len]);
        // 清零剩余部分
        for i in len..MAX_LT_SYMBOL_LEN {
            self.ltsymbol[i] = 0;
        }
    }

    // 辅助方法：获取布局符号
    pub fn get_ltsymbol(&self) -> String {
        let null_pos = self
            .ltsymbol
            .iter()
            .position(|&x| x == 0)
            .unwrap_or(MAX_LT_SYMBOL_LEN);
        String::from_utf8_lossy(&self.ltsymbol[..null_pos]).to_string()
    }

    // 辅助方法：设置标签状态
    pub fn set_tag_status(&mut self, index: usize, status: TagStatus) {
        if index < MAX_TAGS {
            self.tag_status_vec[index] = status;
        }
    }

    // 辅助方法：获取标签状态
    pub fn get_tag_status(&self, index: usize) -> Option<TagStatus> {
        if index < MAX_TAGS {
            Some(self.tag_status_vec[index])
        } else {
            None
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SharedMessage {
    pub timestamp: u64,
    pub monitor_info: MonitorInfo,
}

impl Default for SharedMessage {
    fn default() -> Self {
        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            monitor_info: MonitorInfo::default(),
        }
    }
}

impl SharedMessage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_monitor_info(monitor_info: MonitorInfo) -> Self {
        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            monitor_info,
        }
    }

    // 更新时间戳
    pub fn update_timestamp(&mut self) {
        self.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
    }

    // 提供安全的字段访问方法
    pub fn get_timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn get_monitor_info(&self) -> &MonitorInfo {
        &self.monitor_info
    }

    pub fn get_monitor_info_mut(&mut self) -> &mut MonitorInfo {
        &mut self.monitor_info
    }
}

// 命令相关定义
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandType {
    None = 0,
    ViewTag = 1,
    ToggleTag = 2,
    SetLayout = 3,
}

impl Default for CommandType {
    fn default() -> Self {
        CommandType::None
    }
}

// 实现从 u32 到 CommandType 的转换
impl From<u32> for CommandType {
    fn from(value: u32) -> Self {
        match value {
            1 => CommandType::ViewTag,
            2 => CommandType::ToggleTag,
            3 => CommandType::SetLayout,
            _ => CommandType::None, // 对于未知值，默认返回 None
        }
    }
}

// 实现从 CommandType 到 u32 的转换
impl From<CommandType> for u32 {
    fn from(cmd_type: CommandType) -> Self {
        cmd_type as u32
    }
}

// 使用合理的对齐，移除 packed
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SharedCommand {
    pub cmd_type: u32,
    pub parameter: u32,
    pub monitor_id: i32,
    pub timestamp: u64,
}

impl SharedCommand {
    pub fn new(cmd_type: CommandType, parameter: u32, monitor_id: i32) -> Self {
        Self {
            cmd_type: cmd_type.into(),
            parameter,
            monitor_id,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }

    pub fn get_command_type(&self) -> CommandType {
        match self.cmd_type {
            1 => CommandType::ViewTag,
            2 => CommandType::ToggleTag,
            3 => CommandType::SetLayout,
            _ => CommandType::None,
        }
    }

    pub fn view_tag(tag_bit: u32, monitor_id: i32) -> Self {
        Self::new(CommandType::ViewTag, tag_bit, monitor_id)
    }

    pub fn toggle_tag(tag_bit: u32, monitor_id: i32) -> Self {
        Self::new(CommandType::ToggleTag, tag_bit, monitor_id)
    }

    pub fn set_layout(layout_idx: u32, monitor_id: i32) -> Self {
        Self::new(CommandType::SetLayout, layout_idx, monitor_id)
    }

    // 提供安全的字段访问方法
    pub fn get_parameter(&self) -> u32 {
        self.parameter
    }

    pub fn get_monitor_id(&self) -> i32 {
        self.monitor_id
    }

    pub fn get_timestamp(&self) -> u64 {
        self.timestamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_struct_alignment() {
        println!(
            "SharedMessage size: {}",
            std::mem::size_of::<SharedMessage>()
        );
        println!(
            "SharedMessage align: {}",
            std::mem::align_of::<SharedMessage>()
        );
        println!("MonitorInfo size: {}", std::mem::size_of::<MonitorInfo>());
        println!("MonitorInfo align: {}", std::mem::align_of::<MonitorInfo>());
        println!("TagStatus size: {}", std::mem::size_of::<TagStatus>());
        println!("TagStatus align: {}", std::mem::align_of::<TagStatus>());
        println!(
            "SharedCommand size: {}",
            std::mem::size_of::<SharedCommand>()
        );
        println!(
            "SharedCommand align: {}",
            std::mem::align_of::<SharedCommand>()
        );

        // 验证结构体大小是合理的
        assert!(std::mem::size_of::<SharedMessage>() > 0);
        assert!(std::mem::size_of::<SharedCommand>() > 0);
    }

    #[test]
    fn test_monitor_info_methods() {
        let mut info = MonitorInfo::default();

        // 测试客户端名称
        info.set_client_name("test_client");
        assert_eq!(info.get_client_name(), "test_client");

        // 测试长字符串截断
        let long_name = "a".repeat(200);
        info.set_client_name(&long_name);
        let result = info.get_client_name();
        assert!(result.len() < MAX_CLIENT_NAME_LEN);

        // 测试布局符号
        info.set_ltsymbol("[]=");
        assert_eq!(info.get_ltsymbol(), "[]=");

        // 测试标签状态
        let status = TagStatus::new(true, false, true, false);
        info.set_tag_status(0, status);
        assert_eq!(info.get_tag_status(0), Some(status));

        // 测试边界情况
        assert_eq!(info.get_tag_status(MAX_TAGS), None);
    }

    #[test]
    fn test_shared_command() {
        let cmd = SharedCommand::view_tag(1 << 2, 0);
        assert_eq!(cmd.get_command_type(), CommandType::ViewTag);
        // 使用安全的访问方法
        assert_eq!(cmd.get_parameter(), 1 << 2);
        assert_eq!(cmd.get_monitor_id(), 0);
        assert!(cmd.get_timestamp() > 0);
    }

    #[test]
    fn test_shared_message() {
        let mut message = SharedMessage::new();
        assert!(message.get_timestamp() > 0);

        // 测试修改 monitor_info
        let monitor_info = message.get_monitor_info_mut();
        monitor_info.set_client_name("test");
        monitor_info.monitor_num = 42;

        assert_eq!(message.get_monitor_info().get_client_name(), "test");
        assert_eq!(message.get_monitor_info().monitor_num, 42);
    }

    #[test]
    fn test_memory_layout() {
        // 确保结构体可以安全地进行内存拷贝
        let message1 = SharedMessage::default();
        let message2 = message1; // 应该能够复制
        assert_eq!(message1, message2);

        let cmd1 = SharedCommand::view_tag(1, 0);
        let cmd2 = cmd1; // 应该能够复制
        assert_eq!(cmd1.get_parameter(), cmd2.get_parameter());
    }
}
