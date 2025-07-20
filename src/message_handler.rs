use crate::message::AgentMessage;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, warn};

/// Message handler error types
#[derive(Error, Debug)]
pub enum MessageHandlerError {
    #[error("Channel send error: {0}")]
    ChannelSendError(String),

    #[error("Channel closed")]
    ChannelClosed,
}

/// Message handler that manages MPSC channel communication between UDP intake and LLM processing
pub struct MessageHandler {
    /// Agent ID for filtering self-messages
    agent_id: String,
    /// Sender for UDP intake thread to send messages to LLM processing thread
    message_sender: mpsc::Sender<AgentMessage>,
    /// Receiver for LLM processing thread to receive messages
    message_receiver: Arc<Mutex<mpsc::Receiver<AgentMessage>>>,
}

impl MessageHandler {
    /// Create a new MessageHandler with the specified agent ID and configuration
    pub fn new(agent_id: String, buffer_size: usize) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);

        debug!(
            "Created message handler for agent '{}' with buffer size {}",
            agent_id, buffer_size
        );

        Self {
            agent_id,
            message_sender: sender,
            message_receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Get the agent ID
    pub fn agent_id(&self) -> &str {
        &self.agent_id
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
                    let error_msg = format!("Message channel closed for agent '{}'", self.agent_id);
                    error!("{}", error_msg);
                    return Err(MessageHandlerError::ChannelClosed);
                }
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_channel_buffer_overflow() {
        // Create a message handler with a small buffer size
        let handler = MessageHandler::new("overflow-agent".to_string(), 2);

        // Fill the buffer
        for i in 0..2 {
            let message = AgentMessage::new("sender".to_string(), format!("Message {}", i));
            let result = handler.try_send_message(message);
            assert!(result.is_ok());
        }

        // Try to send one more message - should fail due to buffer full
        let overflow_message =
            AgentMessage::new("sender".to_string(), "Overflow message".to_string());
        let overflow_result = handler.try_send_message(overflow_message);
        assert!(overflow_result.is_err());

        if let Err(MessageHandlerError::ChannelSendError(msg)) = overflow_result {
            assert!(msg.contains("Channel buffer full"));
        } else {
            panic!("Expected ChannelSendError for buffer overflow");
        }
    }

}
