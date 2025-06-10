use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TagStatus {
    pub is_selected: bool,
    pub is_urg: bool,
    pub is_filled: bool,
    pub is_occ: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct MonitorInfo {
    pub client_name: String,
    pub tag_status_vec: Vec<TagStatus>,
    pub monitor_num: i32,
    pub monitor_width: i32,
    pub monitor_height: i32,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub showbar0: bool,
    pub ltsymbol: String,
    pub border_w: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedMessage {
    pub timestamp: u128,
    pub monitor_info: MonitorInfo,
}

impl Default for SharedMessage {
    fn default() -> Self {
        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            monitor_info: MonitorInfo::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_message() {
        let message = SharedMessage::default();
        assert!(message.timestamp > 0);
    }
}

// 新增命令相关定义
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommandType {
    None = 0,
    ViewTag = 1,
    ToggleTag = 2,
    SetLayout = 3,
    // 可以添加更多命令类型...
}

impl Default for CommandType {
    fn default() -> Self {
        CommandType::None
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SharedCommand {
    pub cmd_type: CommandType,
    pub parameter: u32,
    pub monitor_id: i32,
    pub timestamp: u64,
}

impl SharedCommand {
    pub fn new(cmd_type: CommandType, parameter: u32, monitor_id: i32) -> Self {
        Self {
            cmd_type,
            parameter,
            monitor_id,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
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
}
