use prost::Message;
use std::time::{SystemTime, UNIX_EPOCH};

// Include the generated protobuf code
pub mod agent_message {
    include!(concat!(env!("OUT_DIR"), "/agent_swarm.rs"));
}

pub use agent_message::AgentMessage;

impl AgentMessage {
    /// Create a new AgentMessage with the current timestamp
    pub fn new(sender_id: String, content: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Self {
            sender_id,
            timestamp,
            content,
        }
    }

    /// Serialize the message to bytes using protobuf
    pub fn serialize(&self) -> Result<Vec<u8>, prost::EncodeError> {
        let mut buf = Vec::new();
        self.encode(&mut buf)?;
        Ok(buf)
    }

    /// Deserialize bytes to AgentMessage using protobuf
    pub fn deserialize(bytes: &[u8]) -> Result<Self, prost::DecodeError> {
        Self::decode(bytes)
    }

    /// Get the age of the message in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        (now - self.timestamp).max(0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_message_creation() {
        let message = AgentMessage::new("agent-1".to_string(), "Hello, world!".to_string());

        assert_eq!(message.sender_id, "agent-1");
        assert_eq!(message.content, "Hello, world!");
        assert!(message.timestamp > 0);
    }

    #[test]
    fn test_message_serialization_deserialization() {
        let original =
            AgentMessage::new("test-agent".to_string(), "Test message content".to_string());

        // Serialize the message
        let serialized = original.serialize().expect("Failed to serialize message");
        assert!(!serialized.is_empty());

        // Deserialize the message
        let deserialized =
            AgentMessage::deserialize(&serialized).expect("Failed to deserialize message");

        // Verify all fields match
        assert_eq!(deserialized.sender_id, original.sender_id);
        assert_eq!(deserialized.timestamp, original.timestamp);
        assert_eq!(deserialized.content, original.content);
    }

    #[test]
    fn test_message_serialization_with_empty_content() {
        let message = AgentMessage::new("agent-2".to_string(), "".to_string());

        let serialized = message
            .serialize()
            .expect("Failed to serialize empty message");
        let deserialized =
            AgentMessage::deserialize(&serialized).expect("Failed to deserialize empty message");

        assert_eq!(deserialized.sender_id, "agent-2");
        assert_eq!(deserialized.content, "");
    }

    #[test]
    fn test_message_serialization_with_unicode() {
        let message = AgentMessage::new("agent-unicode".to_string(), "Hello 世界! 🌍".to_string());

        let serialized = message
            .serialize()
            .expect("Failed to serialize unicode message");
        let deserialized =
            AgentMessage::deserialize(&serialized).expect("Failed to deserialize unicode message");

        assert_eq!(deserialized.content, "Hello 世界! 🌍");
    }

    #[test]
    fn test_invalid_deserialization() {
        let invalid_bytes = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let result = AgentMessage::deserialize(&invalid_bytes);

        assert!(result.is_err());
    }

    #[test]
    fn test_message_age_calculation() {
        let message = AgentMessage::new("agent-time".to_string(), "Time test".to_string());

        // Age should be very small (close to 0) for a newly created message
        let age = message.age_seconds();
        assert!(age <= 1); // Should be 0 or 1 second at most
    }

    #[test]
    fn test_message_with_custom_timestamp() {
        let custom_timestamp = 1640995200; // Jan 1, 2022 00:00:00 UTC
        let message = AgentMessage {
            sender_id: "agent-custom".to_string(),
            timestamp: custom_timestamp,
            content: "Custom timestamp test".to_string(),
        };

        let serialized = message
            .serialize()
            .expect("Failed to serialize custom timestamp message");
        let deserialized = AgentMessage::deserialize(&serialized)
            .expect("Failed to deserialize custom timestamp message");

        assert_eq!(deserialized.timestamp, custom_timestamp);
    }
}