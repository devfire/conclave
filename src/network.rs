use crate::message::{AgentMessage, MessageError};
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

    #[error("Message (de)serialization error: {0}")]
    Message(#[from] MessageError),

    #[error("Invalid network configuration: {0}")]
    ConfigError(String),
}

impl From<NetworkError> for String {
    fn from(error: NetworkError) -> Self {
        error.to_string()
    }
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
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            buffer_size: 65536, // 64KB buffer
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
    /// Create a new `NetworkManager` with the specified configuration
    pub fn new(config: NetworkConfig, agent_id: String) -> Result<Self, NetworkError> {
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
        // serialize() always gzip-compresses + base64-encodes the content.
        let serialized = message.serialize()?;

        // Send the serialized message to the multicast address
        match self.socket.send_to(&serialized, self.multicast_addr).await {
            Ok(bytes_sent) => {
                tracing::debug!(
                    "Sent {} bytes to multicast group {} from agent {} (original size: {})",
                    bytes_sent,
                    self.multicast_addr,
                    self.agent_id,
                    message.content.len()
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

    /// Receive a single message from the multicast group.
    ///
    /// # Concurrency
    /// This method is safe to call concurrently from multiple tasks:
    /// - `&self` receiver: no mutable aliasing of `NetworkManager`
    /// - `UdpSocket::recv_from` takes `&self` and is internally synchronized by tokio
    /// - The receive buffer is allocated fresh on each call (task-local), so there
    ///   is no shared mutable state and no possibility of a data race.
    pub async fn receive_message(&self) -> Result<AgentMessage, NetworkError> {
        // Task-local buffer: each concurrent call gets its own allocation.
        let mut buffer = vec![0u8; self.config.buffer_size];

        let (bytes_received, sender_addr) =
            self.socket.recv_from(&mut buffer).await.map_err(|e| {
                let error_msg = format!(
                    "Failed to receive message on agent {}: {}",
                    self.agent_id, e
                );
                tracing::error!("{}", error_msg);
                NetworkError::ReceiveError(error_msg)
            })?;

        tracing::debug!(
            "Received {} bytes from {} on agent {}",
            bytes_received,
            sender_addr,
            self.agent_id
        );

        // Work on a slice of exactly the bytes received — no truncation of a
        // shared buffer, no extra copies.
        let datagram = &buffer[..bytes_received];

        // Every message is compressed on the wire; deserialize handles
        // base64-decode + gunzip + size verification deterministically.
        let message = AgentMessage::deserialize(datagram).map_err(|e| {
            let error_msg = format!("Failed to deserialize message from {sender_addr}: {e}");
            tracing::warn!("{}", error_msg);
            NetworkError::Message(e)
        })?;

        tracing::debug!(
            "Successfully deserialized message from agent {} with content: '{}'",
            message.sender_id,
            message.content.chars().take(50).collect::<String>()
        );

        Ok(message)
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
    }

    #[tokio::test]
    async fn test_network_manager_creation_valid_multicast() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            buffer_size: 1024,
        };

        let result = NetworkManager::new(config, "test-agent".to_string());
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_network_manager_creation_invalid_multicast() {
        let config = NetworkConfig {
            multicast_address: "192.168.1.1:8080".parse().unwrap(), // Not multicast
            interface: None,
            buffer_size: 1024,
        };

        let result = NetworkManager::new(config, "test-agent".to_string());
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
        };

        let manager = NetworkManager::new(config, "test-sender".to_string()).unwrap();
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
        };

        let manager = NetworkManager::new(config, "test-sender-empty".to_string()).unwrap();
        let message =
            crate::message::AgentMessage::new("test-sender-empty".to_string(), String::new());

        let result = manager.send_message(&message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_message_with_unicode() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8082".parse().unwrap(), // Different port
            interface: None,
            buffer_size: 1024,
        };

        let manager = NetworkManager::new(config, "test-sender-unicode".to_string()).unwrap();
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
        };

        // Create sender and receiver
        let sender = NetworkManager::new(config.clone(), "test-sender".to_string()).unwrap();
        let receiver = NetworkManager::new(config, "test-receiver".to_string()).unwrap();

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
        };

        let manager = NetworkManager::new(config, "test-malformed".to_string()).unwrap();

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
            Ok(Err(NetworkError::Message(_))) | Err(_) => {
                // Malformed data or receive timeout are both acceptable here.
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
        };

        // Create sender and receiver
        let sender =
            NetworkManager::new(config.clone(), "test-sender-compress".to_string()).unwrap();
        let receiver = NetworkManager::new(config, "test-receiver-compress".to_string()).unwrap();

        // Create a large message.
        let long_content = "This is a very long message that should definitely be compressed because it exceeds the compression threshold of 100 bytes. ".repeat(5);
        let test_message =
            crate::message::AgentMessage::new("test-sender-compress".to_string(), long_content);

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
    async fn test_small_message_is_compressed_on_wire() {
        let config = NetworkConfig {
            multicast_address: "239.255.255.250:8086".parse().unwrap(),
            interface: None,
            buffer_size: 4096,
        };

        // Create sender and receiver
        let sender = NetworkManager::new(config.clone(), "sender".to_string()).unwrap();
        let receiver = NetworkManager::new(config, "receiver".to_string()).unwrap();

        // Small payload that, under the old threshold design, would have been
        // sent uncompressed. With always-compress it must never appear as
        // plaintext on the wire.
        let plaintext = "PLAINTEXT_SECRET_42";
        let test_message =
            crate::message::AgentMessage::new("sender".to_string(), plaintext.to_string());

        // Send and capture the raw datagram before it is decompressed.
        let send_message = test_message.clone();
        let send_task = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            sender.send_message(&send_message).await
        });

        let mut buf = vec![0u8; 4096];
        let recv_task = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            receiver.socket.recv_from(&mut buf),
        );

        let (send_result, recv_result) = tokio::join!(send_task, recv_task);
        assert!(send_result.unwrap().is_ok());

        let (n, _) = recv_result.unwrap().unwrap();
        let datagram = String::from_utf8_lossy(&buf[..n]);
        assert!(
            !datagram.contains(plaintext),
            "plaintext content must not appear on the wire, got: {datagram}"
        );
    }
}
