use clap::Parser;

mod cli;
mod message;
mod network;

use crate::cli::AgentArgs;

fn main() {
    let args = AgentArgs::parse();

    // Validate arguments
    if let Err(e) = args.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    println!(
        "Starting agent '{}' with {} backend",
        args.agent_id, args.llm_backend
    );
    println!("Multicast address: {}", args.multicast_address);
    println!("Model: {}", args.model);
}
