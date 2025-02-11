use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub client_name: String,
    pub tag_status_vec: Vec<TagStatus>,
    pub monitor_num: u8,
    pub monitor_width: u32,
    pub monitor_height: u32,
}
impl Default for MonitorInfo {
    fn default() -> Self {
        Self {
            client_name: String::new(),
            tag_status_vec: Vec::new(),
            monitor_num: 0,
            monitor_width: 0,
            monitor_height: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMessage {
    pub timestamp: u128,
    pub monitor_infos: Vec<MonitorInfo>,
}

impl Default for SharedMessage {
    fn default() -> Self {
        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            monitor_infos: Vec::new(),
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
