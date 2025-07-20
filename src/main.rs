use clap::Parser;

mod cli;
pub mod llm;
mod message;
mod message_handler;
mod network;
use crate::{
    cli::AgentArgs, message::AgentMessage, message_handler::MessageHandler, network::NetworkConfig,
};
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;
// We'll use the ChatMessage from the llm crate through our llm module

use tracing::{Level, debug, error, info, warn};

use tracing_subscriber;
/// Conclave Agent
/// Main entry point for the Conclave agent
/// This application initializes the agent, sets up logging, and starts the network listener.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    // Parse command-line arguments
    let args = AgentArgs::parse();

    // Validate arguments
    if let Err(e) = args.validate() {
        error!("Error: {}", e);
        std::process::exit(1);
    }

    info!(
        "Starting agent '{}' with {} backend and model '{}'",
        args.agent_id, args.llm_backend, args.model
    );

    // Initialize LLM module
    let llm_module = llm::LLMModule::new(&args)?;
    info!("LLM module initialized successfully");

    // Create network configuration
    let network_config = NetworkConfig {
        multicast_address: args.multicast_address.clone(),
        interface: args.interface.clone(),
        buffer_size: 65536, // 64KB buffer for better performance
    };

    // Initialize network manager
    let network_manager =
        Arc::new(network::NetworkManager::new(network_config, args.agent_id.clone()).await?);
    info!("Network manager initialized successfully");

    let buffer_size = 100; // Buffer up to 1000 messages

    let message_handler = Arc::new(MessageHandler::new(args.agent_id.clone(), buffer_size));
    debug!("Message handler initialized with MPSC channel");

    // Spawn UDP message intake task (Task 6.1)
    let udp_intake_handle =
        spawn_udp_intake_task(Arc::clone(&network_manager), Arc::clone(&message_handler)).await;
    info!("UDP message intake task spawned");

    // Spawn LLM processing task (Task 6.2)
    let llm_processing_handle = spawn_llm_processing_task(
        Arc::clone(&message_handler),
        llm_module,
        Arc::clone(&network_manager),
        args.agent_id.clone(),
    )
    .await;
    info!("LLM processing task spawned");

    info!(
        "Agent '{}' started successfully with concurrent processing",
        args.agent_id
    );

    // Wait for tasks to complete (they run indefinitely)
    let result = tokio::try_join!(udp_intake_handle, llm_processing_handle);

    match result {
        Ok((udp_result, llm_result)) => {
            info!(
                "Both tasks completed: UDP={:?}, LLM={:?}",
                udp_result, llm_result
            );
        }
        Err(e) => {
            error!("Task execution error: {}", e);
        }
    }

    Ok(())
}

/// Spawn UDP message intake task for continuous message reception
/// This task receives messages from UDP multicast and sends them to MPSC channel
async fn spawn_udp_intake_task(
    network_manager: Arc<network::NetworkManager>,
    message_handler: Arc<MessageHandler>,
) -> JoinHandle<Result<(), String>> {
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

/// Spawn LLM processing task for handling messages and generating responses
/// This task receives messages from MPSC channel, filters self-messages, and generates LLM responses
async fn spawn_llm_processing_task(
    message_handler: Arc<MessageHandler>,
    llm_module: llm::LLMModule,
    network_manager: Arc<network::NetworkManager>,
    agent_id: String,
) -> JoinHandle<Result<(), String>> {
    tokio::spawn(async move {
        info!("Starting LLM processing task for agent '{}'", agent_id);

        // Bootstrap the conversation with a greeting message, otherwise everyone is waiting for the first message
        let response_message = AgentMessage::new(agent_id.clone(), "Hi".to_string());

        // Broadcast response via network manager
        network_manager
            .send_message(&response_message)
            .await
            .expect("Failed to send start the conversation.");

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
                    let response_message = AgentMessage::new(agent_id.clone(), response_content);

                    // Broadcast response via network manager
                    network_manager
                        .send_message(&response_message)
                        .await
                        .expect("Failed to send msg");
                }
                Err(e) => {
                    error!("Message channel error: {}", e);
                    return Err(format!("LLM processing task failed: {}", e));
                }
            }
        }
    })
}
