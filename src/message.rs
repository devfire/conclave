use prost::Message;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};

// Include the generated protobuf code
pub mod agent_message {
    include!(concat!(env!("OUT_DIR"), "/agent_swarm.rs"));
}

pub use agent_message::AgentMessage;

/// Errors that can occur while (de)serializing an [`AgentMessage`].
#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("protobuf encode failed: {0}")]
    Encode(#[from] prost::EncodeError),
    #[error("protobuf decode failed: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("compression failed: {0}")]
    Compress(#[source] std::io::Error),
    #[error("decompression failed: {0}")]
    Decompress(#[source] std::io::Error),
    #[error("base64 decode failed: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("original size mismatch: expected {expected} bytes, decompressed to {actual}")]
    SizeMismatch { expected: usize, actual: usize },
}

/// Compression utilities for message content.
///
/// Every message is compressed on the wire, so these helpers are the only path
/// — there is no uncompressed variant.
pub mod compression {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    use tracing::debug;

    /// Gzip-compress `content` into a new byte buffer.
    pub fn compress_content(content: &str) -> Result<Vec<u8>, std::io::Error> {
        debug!("Compressing {} bytes", content.len());

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(content.as_bytes())?;
        encoder.finish()
    }

    /// Gunzip `compressed_data` back into a UTF-8 string.
    pub fn decompress_content(compressed_data: &[u8]) -> Result<String, std::io::Error> {
        use flate2::read::GzDecoder;
        use std::io::Read;

        let mut decoder = GzDecoder::new(compressed_data);
        let mut decompressed_string = String::new();
        decoder.read_to_string(&mut decompressed_string)?;

        debug!("Decompressed to {decompressed_string}");

        Ok(decompressed_string)
    }
}

impl AgentMessage {
    /// Create a new `AgentMessage` with the current timestamp.
    pub fn new(sender_id: String, content: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect(
                "Failed to get current time. Time to panic because this is a basic machine right.",
            )
            .as_secs()
            .cast_signed();

        Self {
            sender_id,
            timestamp,
            content,
            original_size: 0,
        }
    }

    /// Serialize the message to wire bytes.
    ///
    /// The `content` field is **always** gzip-compressed and base64-encoded, so
    /// the wire format is unambiguous: [`deserialize`](Self::deserialize) can
    /// decompress unconditionally and never has to sniff or guess.
    pub fn serialize(&self) -> Result<Vec<u8>, MessageError> {
        let compressed =
            compression::compress_content(&self.content).map_err(MessageError::Compress)?;

        let wire = AgentMessage {
            sender_id: self.sender_id.clone(),
            timestamp: self.timestamp,
            content: STANDARD.encode(&compressed),
            original_size: i64::try_from(self.content.len()).unwrap_or(i64::MAX),
        };

        let mut buf = Vec::new();
        wire.encode(&mut buf)?;
        Ok(buf)
    }

    /// Deserialize wire bytes into a plaintext [`AgentMessage`].
    ///
    /// Reverses [`serialize`](Self::serialize): decode the protobuf, base64-decode
    /// `content`, gunzip it, and verify the result against `original_size`.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, MessageError> {
        let wire = Self::decode(bytes)?;

        let compressed = STANDARD.decode(&wire.content)?;
        let content =
            compression::decompress_content(&compressed).map_err(MessageError::Decompress)?;

        let expected = usize::try_from(wire.original_size).unwrap_or_default();
        if expected != 0 && content.len() != expected {
            return Err(MessageError::SizeMismatch {
                expected,
                actual: content.len(),
            });
        }

        Ok(Self {
            sender_id: wire.sender_id,
            timestamp: wire.timestamp,
            content,
            original_size: 0,
        })
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
        let message = AgentMessage::new("agent-2".to_string(), String::new());

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
        let custom_timestamp = 1_640_995_200; // Jan 1, 2022 00:00:00 UTC
        let message = AgentMessage {
            sender_id: "agent-custom".to_string(),
            timestamp: custom_timestamp,
            content: "Custom timestamp test".to_string(),
            original_size: 0,
        };

        let serialized = message
            .serialize()
            .expect("Failed to serialize custom timestamp message");
        let deserialized = AgentMessage::deserialize(&serialized)
            .expect("Failed to deserialize custom timestamp message");

        assert_eq!(deserialized.timestamp, custom_timestamp);
    }

    #[test]
    fn test_compression_round_trip() {
        let original_content =
            "This is a test message that should be compressed because it's quite long and exceeds the compression threshold for testing purposes. "
                .repeat(10);

        let compressed =
            compression::compress_content(&original_content).expect("Failed to compress");
        assert!(!compressed.is_empty());
        assert!(compressed.len() < original_content.len());

        let decompressed =
            compression::decompress_content(&compressed).expect("Failed to decompress");
        assert_eq!(decompressed, original_content);
    }

    #[test]
    fn test_serialize_always_compresses_content() {
        // Even a tiny message must be compressed on the wire — there is no
        // uncompressed path anymore.
        let message = AgentMessage::new("agent".to_string(), "tiny".to_string());

        let serialized = message.serialize().expect("Failed to serialize");
        let wire = String::from_utf8_lossy(&serialized);

        assert!(
            !wire.contains("tiny"),
            "plaintext content must not appear on the wire"
        );

        // Round-trip still yields the original plaintext.
        let back = AgentMessage::deserialize(&serialized).expect("Failed to deserialize");
        assert_eq!(back.content, "tiny");
        assert_eq!(back.sender_id, "agent");
    }

    #[test]
    fn test_deserialize_rejects_size_mismatch() {
        // Build a valid wire message, then tamper with original_size to confirm
        // the receive-side integrity check fires.
        let message = AgentMessage::new("agent".to_string(), "hello world".to_string());
        let wire_bytes = message.serialize().expect("serialize");
        let wire = AgentMessage::decode(&wire_bytes[..]).expect("decode");

        let tampered = AgentMessage {
            sender_id: wire.sender_id,
            timestamp: wire.timestamp,
            content: wire.content,
            original_size: wire.original_size + 1000,
        };
        let mut bytes = Vec::new();
        prost::Message::encode(&tampered, &mut bytes).expect("encode");

        let result = AgentMessage::deserialize(&bytes);
        assert!(matches!(result, Err(MessageError::SizeMismatch { .. })));
    }
}
