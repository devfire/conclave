use crate::message::{AgentMessage, CompressedAgentMessage};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr};

use thiserror::Error;
use tokio::net::UdpSocket;

/// Network-related error types
#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Failed to create socket: {0}")]
    SocketCreation(#[from] std::io::Error),

    #[error("Failed to join multicast group: {0}")]
    MulticastJoin(String),

    #[error("Failed to send message: {0}")]
    SendError(String),

    #[error("Failed to receive message: {0}")]
    ReceiveError(String),

    #[error("Message serialization error: {0}")]
    SerializationError(#[from] prost::EncodeError),

    #[error("Message deserialization error: {0}")]
    DeserializationError(#[from] prost::DecodeError),

    #[error("Invalid network configuration: {0}")]
    ConfigError(String),
}

/// Configuration for network operations
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Multicast address for agent communication
    pub multicast_address: SocketAddr,
    /// Network interface to bind to (None for default)
    pub interface: Option<String>,
    /// Size of the receive buffer in bytes
    pub buffer_size: usize,
    /// Message size threshold in bytes above which compression will be applied
    /// Messages larger than this threshold will be compressed using gzip before transmission
    pub compression_threshold: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            buffer_size: 65536,          // 64KB buffer
            compression_threshold: 1024, // Compress messages larger than 1KB
        }
    }
}

/// Manages UDP multicast networking for agent communication
pub struct NetworkManager {
    socket: UdpSocket,
    multicast_addr: SocketAddr,
    agent_id: String,
    config: NetworkConfig,
}

impl NetworkManager {
    /// Create a new NetworkManager with the specified configuration
    pub async fn new(config: NetworkConfig, agent_id: String) -> Result<Self, NetworkError> {
        // Validate multicast address
        if !config.multicast_address.ip().is_multicast() {
            return Err(NetworkError::ConfigError(format!(
                "Address {} is not a valid multicast address",
                config.multicast_address.ip()
            )));
        }

        // Create the UDP socket using socket2 for advanced configuration
        let socket = Self::create_multicast_socket(&config)?;

        // Convert to tokio UdpSocket
        let tokio_socket = UdpSocket::from_std(socket)?;

        let manager = Self {
            socket: tokio_socket,
            multicast_addr: config.multicast_address,
            agent_id,
            config,
        };

        Ok(manager)
    }

    /// Create and configure a UDP socket for multicast operations
    fn create_multicast_socket(
        config: &NetworkConfig,
    ) -> Result<std::net::UdpSocket, NetworkError> {
        // Create socket with socket2 for advanced configuration
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(NetworkError::SocketCreation)?;

        // Enable SO_REUSEADDR to allow multiple agents on the same machine
        socket
            .set_reuse_address(true)
            .map_err(NetworkError::SocketCreation)?;

        // On Unix systems, also set SO_REUSEPORT if available
        #[cfg(unix)]
        {
            if let Err(e) = socket.set_reuse_port(true) {
                tracing::warn!("Failed to set SO_REUSEPORT: {}", e);
            }
        }

        // Bind to the multicast address
        let bind_addr = SocketAddr::new(
            std::net::Ipv4Addr::UNSPECIFIED.into(),
            config.multicast_address.port(),
        );
        socket
            .bind(&bind_addr.into())
            .map_err(NetworkError::SocketCreation)?;

        // Join the multicast group
        if let SocketAddr::V4(multicast_v4) = config.multicast_address {
            let multicast_ip = *multicast_v4.ip();

            // Determine the interface to use
            let interface_ip = if let Some(ref interface_str) = config.interface {
                // Try to parse as IP address first
                interface_str
                    .parse::<Ipv4Addr>()
                    .unwrap_or(Ipv4Addr::UNSPECIFIED)
            } else {
                Ipv4Addr::UNSPECIFIED
            };

            socket
                .join_multicast_v4(&multicast_ip, &interface_ip)
                .map_err(|e| {
                    NetworkError::MulticastJoin(format!(
                        "Failed to join multicast group {}:{} on interface {}: {}",
                        multicast_ip,
                        multicast_v4.port(),
                        interface_ip,
                        e
                    ))
                })?;

            tracing::info!(
                "Joined multicast group {}:{} on interface {}",
                multicast_ip,
                multicast_v4.port(),
                interface_ip
            );
        } else {
            return Err(NetworkError::ConfigError(
                "IPv6 multicast not currently supported".to_string(),
            ));
        }

        // Set socket to non-blocking mode for tokio compatibility
        socket
            .set_nonblocking(true)
            .map_err(NetworkError::SocketCreation)?;

        // Convert to std::net::UdpSocket
        Ok(socket.into())
    }

    /// Send a message to the multicast group
    pub async fn send_message(&self, message: &AgentMessage) -> Result<(), NetworkError> {
        // Convert to compressed message based on threshold
        let compressed_message = message
            .to_compressed(self.config.compression_threshold)
            .map_err(|e| {
                NetworkError::ConfigError(format!("Failed to compress message: {}", e))
            })?;

        // Serialize the compressed message using protobuf
        let serialized = compressed_message
            .serialize()
            .map_err(NetworkError::SerializationError)?;

        // Send the serialized message to the multicast address
        match self.socket.send_to(&serialized, self.multicast_addr).await {
            Ok(bytes_sent) => {
                tracing::debug!(
                    "Sent {} bytes to multicast group {} from agent {} (compressed: {}, original size: {})",
                    bytes_sent,
                    self.multicast_addr,
                    self.agent_id,
                    compressed_message.is_compressed,
                    compressed_message.original_size
                );
                Ok(())
            }
            Err(e) => {
                let error_msg = format!(
                    "Failed to send message from agent {} to {}: {}",
                    self.agent_id, self.multicast_addr, e
                );
                tracing::error!("{}", error_msg);
                Err(NetworkError::SendError(error_msg))
            }
        }
    }

    /// Receive a single message from the multicast group
    pub async fn receive_message(&self) -> Result<AgentMessage, NetworkError> {
        let mut buffer = vec![0u8; self.config.buffer_size];

        match self.socket.recv_from(&mut buffer).await {
            Ok((bytes_received, sender_addr)) => {
                tracing::debug!(
                    "Received {} bytes from {} on agent {}",
                    bytes_received,
                    sender_addr,
                    self.agent_id
                );

                // Trim buffer to actual message size
                buffer.truncate(bytes_received);

                // First deserialize as AgentMessage to check if content is compressed
                match AgentMessage::deserialize(&buffer) {
                    Ok(temp_message) => {
                        // Check if message content is base64 encoded (compressed)
                        let is_compressed = temp_message.content.starts_with("H4") || // gzip base64 starts with H4
                                           temp_message.content.starts_with("eJ"); // zlib base64 starts with eJ

                        // Convert to CompressedAgentMessage and then to regular AgentMessage
                        let compressed_message = CompressedAgentMessage::deserialize(
                            &buffer,
                            is_compressed,
                            temp_message.content.len(),
                        )
                        .map_err(|e| {
                            NetworkError::DeserializationError(prost::DecodeError::new(
                                format!("Failed to deserialize compressed message: {}", e),
                            ))
                        })?;

                        let message = compressed_message.to_agent_message().map_err(|e| {
                            NetworkError::DeserializationError(prost::DecodeError::new(
                                format!("Failed to decompress message: {}", e),
                            ))
                        })?;

                        tracing::debug!(
                            "Successfully deserialized message from agent {} (compressed: {}, original size: {}) with content: '{}'",
                            message.sender_id,
                            compressed_message.is_compressed,
                            compressed_message.original_size,
                            message.content.chars().take(50).collect::<String>()
                        );
                        Ok(message)
                    }
                    Err(e) => {
                        let error_msg =
                            format!("Failed to deserialize message from {}: {}", sender_addr, e);
                        tracing::warn!("{}", error_msg);
                        Err(NetworkError::DeserializationError(e))
                    }
                }
            }
            Err(e) => {
                let error_msg = format!(
                    "Failed to receive message on agent {}: {}",
                    self.agent_id, e
                );
                tracing::error!("{}", error_msg);
                Err(NetworkError::ReceiveError(error_msg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::default();
        assert!(config.multicast_address.ip().is_multicast());
        assert_eq!(config.multicast_address.port(), 8080);
        assert_eq!(config.buffer_size, 65536);
        assert_eq!(config.compression_threshold, 1024);
    }

    #[tokio::test]
    async fn test_network_manager_creation_valid_multicast() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        let result = NetworkManager::new(config, "test-agent".to_string()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_network_manager_creation_invalid_multicast() {
        let config = NetworkConfig {
            multicast_address: "192.168.1.1:8080".parse().unwrap(), // Not multicast
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        let result = NetworkManager::new(config, "test-agent".to_string()).await;
        assert!(result.is_err());

        if let Err(NetworkError::ConfigError(msg)) = result {
            assert!(msg.contains("is not a valid multicast address"));
        } else {
            panic!("Expected ConfigError");
        }
    }

    #[test]
    fn test_create_multicast_socket_valid_config() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        let result = NetworkManager::create_multicast_socket(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_multicast_socket_with_interface() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: Some("127.0.0.1".to_string()),
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        let result = NetworkManager::create_multicast_socket(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_network_error_display() {
        let error = NetworkError::ConfigError("Test error".to_string());
        assert_eq!(
            error.to_string(),
            "Invalid network configuration: Test error"
        );

        let error = NetworkError::SendError("Send failed".to_string());
        assert_eq!(error.to_string(), "Failed to send message: Send failed");
    }

    #[tokio::test]
    async fn test_send_message_success() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        let manager = NetworkManager::new(config, "test-sender".to_string())
            .await
            .unwrap();
        let message = crate::message::AgentMessage::new(
            "test-sender".to_string(),
            "Hello, multicast world!".to_string(),
        );

        let result = manager.send_message(&message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_message_with_empty_content() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8081".parse().unwrap(), // Different port
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        let manager = NetworkManager::new(config, "test-sender-empty".to_string())
            .await
            .unwrap();
        let message =
            crate::message::AgentMessage::new("test-sender-empty".to_string(), "".to_string());

        let result = manager.send_message(&message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_message_with_unicode() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8082".parse().unwrap(), // Different port
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        let manager = NetworkManager::new(config, "test-sender-unicode".to_string())
            .await
            .unwrap();
        let message = crate::message::AgentMessage::new(
            "test-sender-unicode".to_string(),
            "Hello 世界! 🌍".to_string(),
        );

        let result = manager.send_message(&message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_and_receive_message() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8083".parse().unwrap(),
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        // Create sender and receiver
        let sender = NetworkManager::new(config.clone(), "test-sender".to_string())
            .await
            .unwrap();
        let receiver = NetworkManager::new(config, "test-receiver".to_string())
            .await
            .unwrap();

        let test_message = crate::message::AgentMessage::new(
            "test-sender".to_string(),
            "Test message for send/receive".to_string(),
        );

        // Send message in a separate task
        let send_message = test_message.clone();
        let send_task = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            sender.send_message(&send_message).await
        });

        // Receive message with timeout
        let receive_task = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            receiver.receive_message(),
        );

        // Wait for both operations
        let (send_result, receive_result) = tokio::join!(send_task, receive_task);

        // Verify results
        assert!(send_result.unwrap().is_ok());
        assert!(receive_result.is_ok());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.sender_id, test_message.sender_id);
        assert_eq!(received_message.content, test_message.content);
    }

    #[tokio::test]
    async fn test_receive_message_with_malformed_data() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8084".parse().unwrap(),
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1024,
        };

        let manager = NetworkManager::new(config, "test-malformed".to_string())
            .await
            .unwrap();

        // Send malformed data directly to the socket
        let malformed_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let send_result = manager
            .socket
            .send_to(&malformed_data, manager.multicast_addr)
            .await;
        assert!(send_result.is_ok());

        // Try to receive the malformed message
        let receive_result = tokio::time::timeout(
            tokio::time::Duration::from_millis(500),
            manager.receive_message(),
        )
        .await;

        // Should get a timeout or deserialization error
        match receive_result {
            Ok(Err(NetworkError::DeserializationError(_))) => {
                // This is expected for malformed data
            }
            Err(_) => {
                // Timeout is also acceptable since malformed data might not be received
            }
            _ => {
                // Any other result is unexpected
                panic!("Expected deserialization error or timeout");
            }
        }
    }

    #[tokio::test]
    async fn test_compression_functionality() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8085".parse().unwrap(),
            interface: None,
            buffer_size: 1024,
            compression_threshold: 100, // Low threshold to force compression
        };

        // Create sender and receiver
        let sender = NetworkManager::new(config.clone(), "test-sender-compress".to_string())
            .await
            .unwrap();
        let receiver = NetworkManager::new(config, "test-receiver-compress".to_string())
            .await
            .unwrap();

        // Create a message that should be compressed (longer than threshold)
        let long_content = "This is a very long message that should definitely be compressed because it exceeds the compression threshold of 100 bytes. ".repeat(5);
        let test_message = crate::message::AgentMessage::new(
            "test-sender-compress".to_string(),
            long_content,
        );

        // Send message in a separate task
        let send_message = test_message.clone();
        let send_task = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            sender.send_message(&send_message).await
        });

        // Receive message with timeout
        let receive_task = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            receiver.receive_message(),
        );

        // Wait for both operations
        let (send_result, receive_result) = tokio::join!(send_task, receive_task);

        // Verify results
        assert!(send_result.unwrap().is_ok());
        assert!(receive_result.is_ok());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.sender_id, test_message.sender_id);
        assert_eq!(received_message.content, test_message.content);
        assert_eq!(received_message.timestamp, test_message.timestamp);
    }

    #[tokio::test]
    async fn test_no_compression_for_small_messages() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8086".parse().unwrap(),
            interface: None,
            buffer_size: 1024,
            compression_threshold: 1000, // High threshold to avoid compression
        };

        // Create sender and receiver
        let sender = NetworkManager::new(config.clone(), "test-sender-no-compress".to_string())
            .await
            .unwrap();
        let receiver = NetworkManager::new(config, "test-receiver-no-compress".to_string())
            .await
            .unwrap();

        // Create a short message that should not be compressed
        let test_message = crate::message::AgentMessage::new(
            "test-sender-no-compress".to_string(),
            "Short message".to_string(),
        );

        // Send message in a separate task
        let send_message = test_message.clone();
        let send_task = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            sender.send_message(&send_message).await
        });

        // Receive message with timeout
        let receive_task = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            receiver.receive_message(),
        );

        // Wait for both operations
        let (send_result, receive_result) = tokio::join!(send_task, receive_task);

        // Verify results
        assert!(send_result.unwrap().is_ok());
        assert!(receive_result.is_ok());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.sender_id, test_message.sender_id);
        assert_eq!(received_message.content, test_message.content);
        assert_eq!(received_message.timestamp, test_message.timestamp);
    }
}
