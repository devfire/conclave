use clap::Parser;

mod cli;
pub mod llm;
mod message;
mod message_handler;
mod network;
use crate::{cli::AgentArgs, message::AgentMessage, network::NetworkConfig};
use std::time::Duration;

use tracing::{Level, error, info};

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
    let config = NetworkConfig {
        multicast_address: args.multicast_address.clone(),
        interface: args.interface.clone(),
        buffer_size: 1024,
        timeout: Duration::from_secs(args.timeout_seconds),
    };

    // Start the network listener
    let manager = network::NetworkManager::new(config, args.agent_id).await?;
    // If we reach here, the listener started successfully
    info!("Network listener started successfully");

    // Start the manager loop
    tokio::spawn(async move {
        if let Err(e) = manager.start_message_loop(handle_message).await {
            error!("Network manager error: {}", e);
        }
    });

    Ok(())
}

async fn handle_message(message: AgentMessage) {
    // Process the message here
    info!("Sending {:?} to LLM.", message);
}
