use clap::Parser;

mod cli;
pub mod llm;
mod message;
mod message_handler;
mod network;
mod processor;
use crate::{
    cli::AgentArgs, message_handler::MessageHandler, network::NetworkConfig, processor::Processor,
};
use std::sync::Arc;
// We'll use the ChatMessage from the llm crate through our llm module

use tracing::{debug, error, info, Level};

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

    let processor = Processor::new(
        Arc::clone(&message_handler),
        Arc::clone(&network_manager),
        args.agent_id.clone(),
    );

    // Spawn UDP message intake task
    let udp_intake_handle = processor.spawn_udp_intake_task().await;
    info!("UDP message intake task spawned");

    // Spawn LLM processing task
    let llm_processing_handle = processor.spawn_llm_processing_task(llm_module).await;
    info!("LLM processing task spawned");

    // Wait for tasks to complete (they run indefinitely)
    let _result = tokio::try_join!(udp_intake_handle, llm_processing_handle)?;

    Ok(())
}
