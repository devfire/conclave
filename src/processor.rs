use crate::{llm, message::AgentMessage, message_handler::MessageHandler, network};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

pub struct Processor {
    message_handler: Arc<MessageHandler>,
    network_manager: Arc<network::NetworkManager>,
    agent_id: String,
}

impl Processor {
    pub fn new(
        message_handler: Arc<MessageHandler>,
        network_manager: Arc<network::NetworkManager>,
        agent_id: String,
    ) -> Self {
        Self {
            message_handler,
            network_manager,
            agent_id,
        }
    }

    /// Spawn LLM processing task for handling messages and generating responses
    /// This task receives messages from MPSC channel, filters self-messages, and generates LLM responses
    pub async fn spawn_llm_processing_task(
        &self,
        llm_module: llm::LLMModule,
    ) -> JoinHandle<Result<(), String>> {
        let message_handler = Arc::clone(&self.message_handler);
        let network_manager = Arc::clone(&self.network_manager);
        let agent_id = self.agent_id.clone();

        tokio::spawn(async move {
            info!("Starting LLM processing task for agent '{}'", agent_id);

            // Bootstrap the conversation with a greeting message, otherwise everyone is waiting for the first message
            let response_message = AgentMessage::new(agent_id.clone(), "Hi".to_string());

            // Broadcast response via network manager
            network_manager.send_message(&response_message).await?;

            loop {
                match message_handler.receive_message().await {
                    Ok(message) => {
                        debug!(
                            "LLM processing received message from '{}' with content: '{}'",
                            message.sender_id,
                            message.content // message.content.chars().take(50).collect::<String>()
                        );

                        // Create chat messages for LLM context
                        let chat_messages = vec![llm_module.create_user_message(&message.content)];

                        // Generate LLM response
                        let response_content = match llm_module
                            .generate_llm_response(&chat_messages)
                            .await
                        {
                            Ok(llm_response) => {
                                debug!(
                                    "LLM generated response for message from '{}': '{}'",
                                    message.sender_id, llm_response
                                );
                                llm_response
                            }
                            Err(e) => {
                                error!(
                                    "LLM failed to generate response for message from '{}': {}",
                                    message.sender_id, e
                                );
                                // Fallback to a simple acknowledgment if LLM fails
                                format!(
                                    "Agent {} received your message but couldn't generate a proper response: {}",
                                    agent_id, e
                                )
                            }
                        };
                        println!("{}: {}", message.sender_id, message.content);
                        println!("{}: {}", agent_id, response_content);

                        debug!(
                            "Sending response to message from '{}': '{}'",
                            message.sender_id, response_content
                        );

                        // Create response message
                        let response_message =
                            AgentMessage::new(agent_id.clone(), response_content);

                        // Broadcast response via network manager
                        network_manager
                            .send_message(&response_message)
                            .await?;
                    }
                    Err(e) => {
                        error!("Message channel error: {}", e);
                        return Err(format!("LLM processing task failed: {}", e));
                    }
                }
            }
        })
    }

    /// Spawn UDP message intake task for continuous message reception
    /// This task receives messages from UDP multicast and sends them to MPSC channel
    pub async fn spawn_udp_intake_task(&self) -> JoinHandle<Result<(), String>> {
        let network_manager = Arc::clone(&self.network_manager);
        let message_handler = Arc::clone(&self.message_handler);

        tokio::spawn(async move {
            info!(
                "Starting UDP message intake task for agent '{}'",
                message_handler.agent_id()
            );

            loop {
                match network_manager.receive_message().await {
                    Ok(message) => {
                        debug!(
                            "UDP intake received message from '{}' with content: '{}'",
                            message.sender_id,
                            message.content.chars().take(50).collect::<String>()
                        );

                        // Introduce an artificial delay to simulate processing time
                        tokio::time::sleep(Duration::from_millis(5000)).await;

                        // Send message to MPSC channel (non-blocking)
                        if let Err(e) = message_handler.try_send_message(message.clone()) {
                            warn!("Failed to send message to channel: {}", e);
                            // Continue processing other messages even if channel is full
                        } else {
                            debug!(
                                "Successfully forwarded message from '{}' to processing channel",
                                message.sender_id
                            );
                        }
                    }
                    Err(network::NetworkError::DeserializationError(e)) => {
                        // Log malformed messages but continue processing
                        warn!("Received malformed message, skipping: {}", e);
                        continue;
                    }
                    Err(e) => {
                        error!("UDP message reception error: {}", e);
                        return Err(format!("UDP intake task failed: {}", e));
                    }
                }
            }
        })
    }
}
