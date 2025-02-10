use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagStatus {
    pub is_selected: bool,
    pub is_urg: bool,
    pub is_filled: bool,
}
impl TagStatus {
    pub fn new(is_selected: bool, is_urg: bool, is_filled: bool) -> Self {
        Self {
            is_selected,
            is_urg,
            is_filled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMessage {
    pub client_name: String,
    pub timestamp: u128,
    pub tag_status_vec: Vec<TagStatus>,
}

impl SharedMessage {
    pub fn new() -> Self {
        Self {
            client_name: String::new(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            tag_status_vec: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_message() {
        let message = SharedMessage::new();
        assert!(message.timestamp > 0);
    }
}
