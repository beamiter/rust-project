use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMessage {
    pub id: u64,
    pub content: String,
    pub timestamp: u128,
}

impl SharedMessage {
    /// 创建一个新的 `SharedMessage` 实例
    pub fn new(id: u64, content: String) -> Self {
        Self {
            id,
            content,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_message() {
        let message = SharedMessage::new(1, "Test message".to_string());
        assert_eq!(message.id, 1);
        assert_eq!(message.content, "Test message");
        assert!(message.timestamp > 0);
    }
}
