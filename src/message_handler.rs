use crate::message::AgentMessage;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, warn};

/// Message handler error types
#[derive(Error, Debug)]
pub enum MessageHandlerError {
    #[error("Channel send error: {0}")]
    ChannelSendError(String),

    #[error("Channel receive error: {0}")]
    ChannelReceiveError(String),

    #[error("Channel closed")]
    ChannelClosed,
}

/// Configuration for the message channel
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Buffer size for the MPSC channel
    pub buffer_size: usize,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1000, // Default buffer size for message queue
        }
    }
}

/// Message handler that manages MPSC channel communication between UDP intake and LLM processing
pub struct MessageHandler {
    /// Agent ID for filtering self-messages
    agent_id: String,
    /// Sender for UDP intake thread to send messages to LLM processing thread
    message_sender: mpsc::Sender<AgentMessage>,
    /// Receiver for LLM processing thread to receive messages
    message_receiver: Arc<Mutex<mpsc::Receiver<AgentMessage>>>,
    /// Channel configuration
    config: ChannelConfig,
}

impl MessageHandler {
    /// Create a new MessageHandler with the specified agent ID and configuration
    pub fn new(agent_id: String, config: ChannelConfig) -> Self {
        let (sender, receiver) = mpsc::channel(config.buffer_size);

        debug!(
            "Created message handler for agent '{}' with buffer size {}",
            agent_id, config.buffer_size
        );

        Self {
            agent_id,
            message_sender: sender,
            message_receiver: Arc::new(Mutex::new(receiver)),
            config,
        }
    }

    /// Create a new MessageHandler with default configuration
    pub fn new_default(agent_id: String) -> Self {
        Self::new(agent_id, ChannelConfig::default())
    }

    /// Get the agent ID
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Get the channel configuration
    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }

    /// Get a clone of the message sender for UDP intake thread
    /// This allows the UDP intake thread to send messages to the channel
    pub fn get_sender(&self) -> mpsc::Sender<AgentMessage> {
        self.message_sender.clone()
    }

    /// Send a message to the channel (used by UDP intake thread)
    pub async fn send_message(&self, message: AgentMessage) -> Result<(), MessageHandlerError> {
        match self.message_sender.send(message.clone()).await {
            Ok(()) => {
                debug!(
                    "Successfully sent message from '{}' to channel for agent '{}'",
                    message.sender_id, self.agent_id
                );
                Ok(())
            }
            Err(mpsc::error::SendError(msg)) => {
                let error_msg = format!(
                    "Failed to send message from '{}' to channel: receiver dropped",
                    msg.sender_id
                );
                error!("{}", error_msg);
                Err(MessageHandlerError::ChannelSendError(error_msg))
            }
        }
    }

    /// Try to send a message without blocking (used by UDP intake thread)
    pub fn try_send_message(&self, message: AgentMessage) -> Result<(), MessageHandlerError> {
        match self.message_sender.try_send(message.clone()) {
            Ok(()) => {
                debug!(
                    "Successfully sent message from '{}' to channel (non-blocking) for agent '{}'",
                    message.sender_id, self.agent_id
                );
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(msg)) => {
                let error_msg = format!(
                    "Channel buffer full, dropping message from '{}' for agent '{}'",
                    msg.sender_id, self.agent_id
                );
                warn!("{}", error_msg);
                Err(MessageHandlerError::ChannelSendError(error_msg))
            }
            Err(mpsc::error::TrySendError::Closed(msg)) => {
                let error_msg = format!(
                    "Channel closed, cannot send message from '{}' for agent '{}'",
                    msg.sender_id, self.agent_id
                );
                error!("{}", error_msg);
                Err(MessageHandlerError::ChannelClosed)
            }
        }
    }

    /// Receive a message from the channel (used by LLM processing thread)
    /// This method includes self-message filtering
    pub async fn receive_message(&self) -> Result<AgentMessage, MessageHandlerError> {
        let mut receiver = self.message_receiver.lock().await;

        loop {
            match receiver.recv().await {
                Some(message) => {
                    // Filter out self-messages to prevent self-replies
                    if message.sender_id == self.agent_id {
                        debug!(
                            "Filtered out self-message from agent '{}' with content: '{}'",
                            message.sender_id,
                            message.content.chars().take(50).collect::<String>()
                        );
                        continue; // Skip self-messages and continue receiving
                    }

                    debug!(
                        "Received message from '{}' for processing by agent '{}' with content: '{}'",
                        message.sender_id,
                        self.agent_id,
                        message.content.chars().take(50).collect::<String>()
                    );
                    return Ok(message);
                }
                None => {
                    let error_msg = format!(
                        "Message channel closed for agent '{}'",
                        self.agent_id
                    );
                    error!("{}", error_msg);
                    return Err(MessageHandlerError::ChannelClosed);
                }
            }
        }
    }

    /// Try to receive a message without blocking (used by LLM processing thread)
    /// This method includes self-message filtering
    pub async fn try_receive_message(&self) -> Result<Option<AgentMessage>, MessageHandlerError> {
        let mut receiver = self.message_receiver.lock().await;

        loop {
            match receiver.try_recv() {
                Ok(message) => {
                    // Filter out self-messages to prevent self-replies
                    if message.sender_id == self.agent_id {
                        debug!(
                            "Filtered out self-message from agent '{}' (non-blocking)",
                            message.sender_id
                        );
                        continue; // Skip self-messages and continue checking
                    }

                    debug!(
                        "Received message from '{}' for processing by agent '{}' (non-blocking)",
                        message.sender_id, self.agent_id
                    );
                    return Ok(Some(message));
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    return Ok(None); // No messages available
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    let error_msg = format!(
                        "Message channel disconnected for agent '{}'",
                        self.agent_id
                    );
                    error!("{}", error_msg);
                    return Err(MessageHandlerError::ChannelClosed);
                }
            }
        }
    }

    /// Get the current number of messages in the channel buffer
    /// This is useful for monitoring channel health and capacity
    pub fn channel_capacity(&self) -> usize {
        self.config.buffer_size
    }

    /// Check if the channel is closed
    pub fn is_closed(&self) -> bool {
        self.message_sender.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_message_handler_creation() {
        let config = ChannelConfig { buffer_size: 100 };
        let handler = MessageHandler::new("test-agent".to_string(), config.clone());

        assert_eq!(handler.agent_id(), "test-agent");
        assert_eq!(handler.config().buffer_size, 100);
        assert!(!handler.is_closed());
    }

    #[tokio::test]
    async fn test_message_handler_default_creation() {
        let handler = MessageHandler::new_default("test-agent-default".to_string());

        assert_eq!(handler.agent_id(), "test-agent-default");
        assert_eq!(handler.config().buffer_size, 1000);
        assert!(!handler.is_closed());
    }

    #[tokio::test]
    async fn test_send_and_receive_message() {
        let handler = MessageHandler::new_default("receiver-agent".to_string());
        let test_message = AgentMessage::new("sender-agent".to_string(), "Test message".to_string());

        // Send message
        let send_result = handler.send_message(test_message.clone()).await;
        assert!(send_result.is_ok());

        // Receive message
        let receive_result = timeout(Duration::from_secs(1), handler.receive_message()).await;
        assert!(receive_result.is_ok());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.sender_id, test_message.sender_id);
        assert_eq!(received_message.content, test_message.content);
        assert_eq!(received_message.timestamp, test_message.timestamp);
    }

    #[tokio::test]
    async fn test_self_message_filtering() {
        let handler = MessageHandler::new_default("self-filter-agent".to_string());
        
        // Send a self-message (should be filtered out)
        let self_message = AgentMessage::new("self-filter-agent".to_string(), "Self message".to_string());
        let send_result = handler.send_message(self_message).await;
        assert!(send_result.is_ok());

        // Send a message from another agent (should be received)
        let other_message = AgentMessage::new("other-agent".to_string(), "Other message".to_string());
        let send_result = handler.send_message(other_message.clone()).await;
        assert!(send_result.is_ok());

        // Receive message - should get the other agent's message, not the self message
        let receive_result = timeout(Duration::from_secs(1), handler.receive_message()).await;
        assert!(receive_result.is_ok());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.sender_id, "other-agent");
        assert_eq!(received_message.content, "Other message");
    }

    #[tokio::test]
    async fn test_try_send_message() {
        let handler = MessageHandler::new_default("try-send-agent".to_string());
        let test_message = AgentMessage::new("sender-agent".to_string(), "Try send test".to_string());

        // Try send message (non-blocking)
        let send_result = handler.try_send_message(test_message.clone());
        assert!(send_result.is_ok());

        // Receive message
        let receive_result = timeout(Duration::from_secs(1), handler.receive_message()).await;
        assert!(receive_result.is_ok());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.content, test_message.content);
    }

    #[tokio::test]
    async fn test_try_receive_message() {
        let handler = MessageHandler::new_default("try-receive-agent".to_string());
        let test_message = AgentMessage::new("sender-agent".to_string(), "Try receive test".to_string());

        // Initially, try_receive should return None (no messages)
        let empty_result = handler.try_receive_message().await;
        assert!(empty_result.is_ok());
        assert!(empty_result.unwrap().is_none());

        // Send a message
        let send_result = handler.send_message(test_message.clone()).await;
        assert!(send_result.is_ok());

        // Now try_receive should return the message
        let receive_result = handler.try_receive_message().await;
        assert!(receive_result.is_ok());
        assert!(receive_result.as_ref().unwrap().is_some());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.content, test_message.content);
    }

    #[tokio::test]
    async fn test_channel_buffer_overflow() {
        let small_config = ChannelConfig { buffer_size: 2 };
        let handler = MessageHandler::new("overflow-agent".to_string(), small_config);

        // Fill the buffer
        for i in 0..2 {
            let message = AgentMessage::new("sender".to_string(), format!("Message {}", i));
            let result = handler.try_send_message(message);
            assert!(result.is_ok());
        }

        // Try to send one more message - should fail due to buffer full
        let overflow_message = AgentMessage::new("sender".to_string(), "Overflow message".to_string());
        let overflow_result = handler.try_send_message(overflow_message);
        assert!(overflow_result.is_err());

        if let Err(MessageHandlerError::ChannelSendError(msg)) = overflow_result {
            assert!(msg.contains("Channel buffer full"));
        } else {
            panic!("Expected ChannelSendError for buffer overflow");
        }
    }

    #[tokio::test]
    async fn test_multiple_self_messages_filtering() {
        let handler = MessageHandler::new_default("multi-self-agent".to_string());

        // Send multiple self-messages
        for i in 0..3 {
            let self_message = AgentMessage::new("multi-self-agent".to_string(), format!("Self message {}", i));
            let send_result = handler.send_message(self_message).await;
            assert!(send_result.is_ok());
        }

        // Send one message from another agent
        let other_message = AgentMessage::new("other-agent".to_string(), "Valid message".to_string());
        let send_result = handler.send_message(other_message.clone()).await;
        assert!(send_result.is_ok());

        // Should receive only the message from the other agent
        let receive_result = timeout(Duration::from_secs(1), handler.receive_message()).await;
        assert!(receive_result.is_ok());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.sender_id, "other-agent");
        assert_eq!(received_message.content, "Valid message");
    }

    #[tokio::test]
    async fn test_sender_clone() {
        let handler = MessageHandler::new_default("clone-test-agent".to_string());
        let sender_clone = handler.get_sender();
        let test_message = AgentMessage::new("external-sender".to_string(), "Clone test".to_string());

        // Send message using the cloned sender
        let send_result = sender_clone.send(test_message.clone()).await;
        assert!(send_result.is_ok());

        // Receive message using the handler
        let receive_result = timeout(Duration::from_secs(1), handler.receive_message()).await;
        assert!(receive_result.is_ok());

        let received_message = receive_result.unwrap().unwrap();
        assert_eq!(received_message.content, test_message.content);
    }

    #[tokio::test]
    async fn test_channel_capacity() {
        let config = ChannelConfig { buffer_size: 500 };
        let handler = MessageHandler::new("capacity-agent".to_string(), config);

        assert_eq!(handler.channel_capacity(), 500);
    }
}