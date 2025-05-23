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
