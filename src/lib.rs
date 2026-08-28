//! Conclave agent library.
//!
//! Module surface for the conclave binary, examples, and future integration
//! tests. `main.rs` is a thin CLI shell over these modules.

pub mod cli;
pub mod llm;
pub mod message;
pub mod message_handler;
pub mod network;
pub mod processor;
pub mod tts;
pub mod validator;
