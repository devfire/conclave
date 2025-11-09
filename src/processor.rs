use crate::{llm, message::AgentMessage, message_handler::MessageHandler, network};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use tokio_retry::Retry;
use tokio_retry::strategy::ExponentialBackoff;

pub struct Processor {
    message_handler: Arc<MessageHandler>,
    network_manager: Arc<network::NetworkManager>,
    agent_id: String,
    processing_delay_ms: u64,
}

impl Processor {
    pub fn new(
        message_handler: Arc<MessageHandler>,
        network_manager: Arc<network::NetworkManager>,
        agent_id: String,
        processing_delay_ms: u64,
    ) -> Self {
        Self {
            message_handler,
            network_manager,
            agent_id,
            processing_delay_ms,
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
            let response_message =
                AgentMessage::new(agent_id.clone(), format!("Hi, I am {agent_id}."));

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

                        eprintln!("__________________________________");
                        eprintln!("{}: \n {}", message.sender_id, message.content);
                        eprintln!("__________________________________");
                        eprintln!();
                        // Create chat messages for LLM context
                        let chat_messages = vec![llm_module.create_user_message(&message.content)];

                        // Retry an async operation
                        let llm_call_result = Retry::spawn(
                            ExponentialBackoff::from_millis(100)
                                .max_delay(Duration::from_secs(10))
                                .take(5),
                            || async {
                                debug!("Invoking LLM.");

                                llm_module.generate_llm_response(&chat_messages).await
                            },
                        )
                        .await;

                        let response_content = match llm_call_result {
                            Ok(response) => response,
                            Err(e) => e.to_string(),
                        };

                        // Say it
                        match llm_module.say(&response_content).await {
                            Ok(_) => info!("Speaking..."),
                            Err(e) => error!("ElevenLabs error: {e}"),
                        }

                        debug!(
                            "Sending response to message from '{}': '{}'",
                            message.sender_id, response_content
                        );

                        // Create response message
                        let response_message =
                            AgentMessage::new(agent_id.clone(), response_content);

                        // Broadcast response via network manager
                        network_manager.send_message(&response_message).await?;
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
        let processing_delay_ms = self.processing_delay_ms;

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
                        tokio::time::sleep(Duration::from_millis(processing_delay_ms)).await;

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
