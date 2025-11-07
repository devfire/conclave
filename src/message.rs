use prost::Message;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};

// Include the generated protobuf code
pub mod agent_message {
    include!(concat!(env!("OUT_DIR"), "/agent_swarm.rs"));
}

pub use agent_message::AgentMessage;

/// Compression utilities for message content
pub mod compression {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    use tracing::debug;

    /// Compress message content using gzip
    pub fn compress_content(content: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        debug!("Compressing message");

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(content.as_bytes())?;
        Ok(encoder.finish()?)
    }

    /// Decompress message content using gzip
    pub fn decompress_content(
        compressed_data: &[u8],
    ) -> Result<String, Box<dyn std::error::Error>> {
        use flate2::read::GzDecoder;
        use std::io::Read;

        // Create a cursor to make the data readable
        let cursor = std::io::Cursor::new(compressed_data);
        let mut decoder = GzDecoder::new(cursor);
        let mut decompressed_string = String::new();
        decoder.read_to_string(&mut decompressed_string)?;

        debug!("Decompressed input to {decompressed_string}");

        Ok(decompressed_string)
    }

    /// Check if content should be compressed (if larger than threshold)
    pub fn should_compress(content: &str, threshold: usize) -> bool {
        content.len() > threshold
    }
}

impl AgentMessage {
    /// Create a new AgentMessage with the current timestamp
    pub fn new(sender_id: String, content: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect(
                "Failed to get current time. Time to panic because this is a basic machine right.",
            )
            .as_secs() as i64;

        Self {
            sender_id,
            timestamp,
            content,
        }
    }

    /// Create a compressed version of this message
    pub fn to_compressed(
        &self,
        compression_threshold: usize,
    ) -> Result<CompressedAgentMessage, Box<dyn std::error::Error>> {
        if compression::should_compress(&self.content, compression_threshold) {
            let compressed_data = compression::compress_content(&self.content)?;
            Ok(CompressedAgentMessage {
                sender_id: self.sender_id.clone(),
                timestamp: self.timestamp,
                compressed_data,
                is_compressed: true,
                original_size: self.content.len(),
            })
        } else {
            // Return uncompressed version if below threshold
            Ok(CompressedAgentMessage {
                sender_id: self.sender_id.clone(),
                timestamp: self.timestamp,
                compressed_data: self.content.as_bytes().to_vec(),
                is_compressed: false,
                original_size: self.content.len(),
            })
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

    #[cfg(test)]
    mod compression_tests {
        use super::*;

        #[test]
        fn test_compression_decompression() {
            let original_content = "This is a test message that should be compressed because it's quite long and exceeds the compression threshold for testing purposes. ".repeat(10);

            // Test compression
            let compressed =
                compression::compress_content(&original_content).expect("Failed to compress");
            assert!(!compressed.is_empty());
            assert!(compressed.len() < original_content.len());

            // Test decompression
            let decompressed =
                compression::decompress_content(&compressed).expect("Failed to decompress");
            assert_eq!(decompressed, original_content);
        }

        #[test]
        fn test_should_compress() {
            assert!(!compression::should_compress("short", 100));
            assert!(compression::should_compress(
                "this is a longer message that exceeds the threshold",
                50
            ));
        }

        #[test]
        fn test_compressed_agent_message() {
            let original_message =
                AgentMessage::new("test-agent".to_string(), "Test content".to_string());

            // Test compression
            let compressed_msg = original_message
                .to_compressed(50)
                .expect("Failed to create compressed message");
            assert!(!compressed_msg.is_compressed);
            assert_eq!(compressed_msg.original_size, 12);

            // Test decompression back to regular message
            let decompressed_msg = compressed_msg
                .to_agent_message()
                .expect("Failed to decompress message");
            assert_eq!(decompressed_msg.sender_id, original_message.sender_id);
            assert_eq!(decompressed_msg.content, original_message.content);
        }

        #[test]
        fn test_uncompressed_agent_message() {
            let original_message = AgentMessage::new("test-agent".to_string(), "Short".to_string());

            // Test that short message is not compressed
            let compressed_msg = original_message
                .to_compressed(50)
                .expect("Failed to create compressed message");
            assert!(!compressed_msg.is_compressed);
            assert_eq!(compressed_msg.original_size, 5);

            // Test decompression back to regular message
            let decompressed_msg = compressed_msg
                .to_agent_message()
                .expect("Failed to decompress message");
            assert_eq!(decompressed_msg.content, original_message.content);
        }
    }
}

/// A message that can be either compressed or uncompressed
pub struct CompressedAgentMessage {
    pub sender_id: String,
    pub timestamp: i64,
    pub compressed_data: Vec<u8>,
    pub is_compressed: bool,
    pub original_size: usize,
}

impl CompressedAgentMessage {
    /// Convert back to regular AgentMessage (decompress if needed)
    pub fn to_agent_message(&self) -> Result<AgentMessage, Box<dyn std::error::Error>> {
        let content = if self.is_compressed {
            compression::decompress_content(&self.compressed_data)?
        } else {
            String::from_utf8(self.compressed_data.clone())?
        };

        Ok(AgentMessage {
            sender_id: self.sender_id.clone(),
            timestamp: self.timestamp,
            content,
        })
    }

    /// Serialize the compressed message
    pub fn serialize(&self) -> Result<Vec<u8>, prost::EncodeError> {
        // Create a temporary AgentMessage for serialization
        let temp_message = AgentMessage {
            sender_id: self.sender_id.clone(),
            timestamp: self.timestamp,
            content: if self.is_compressed {
                // Base64 encode compressed data for safe transmission
                STANDARD.encode(&self.compressed_data)
            } else {
                String::from_utf8_lossy(&self.compressed_data).to_string()
            },
        };
        temp_message.serialize()
    }

    /// Deserialize and create CompressedAgentMessage
    pub fn deserialize(
        bytes: &[u8],
        is_compressed: bool,
        original_size: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let agent_message = AgentMessage::deserialize(bytes)?;

        let compressed_data = if is_compressed {
            STANDARD.decode(&agent_message.content)?
        } else {
            agent_message.content.as_bytes().to_vec()
        };

        Ok(CompressedAgentMessage {
            sender_id: agent_message.sender_id,
            timestamp: agent_message.timestamp,
            compressed_data,
            is_compressed,
            original_size,
        })
    }
}
