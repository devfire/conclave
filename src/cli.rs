use clap::{Parser, ValueEnum};
use std::net::SocketAddr;

/// Supported LLM backend types
#[derive(Debug, Clone, ValueEnum)]
pub enum LLMBackend {
    /// OpenAI GPT models
    #[value(name = "openai")]
    OpenAI,
    /// Anthropic Claude models
    #[value(name = "anthropic")]
    Anthropic,
    /// Google Gemini models
    #[value(name = "google")]
    Google,
    /// Local models via Ollama
    #[value(name = "local")]
    Local,
}

impl std::fmt::Display for LLMBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LLMBackend::OpenAI => write!(f, "openai"),
            LLMBackend::Anthropic => write!(f, "anthropic"),
            LLMBackend::Google => write!(f, "google"),
            LLMBackend::Local => write!(f, "local"),
        }
    }
}

/// Command-line arguments for the AI Agent Swarm
#[derive(Parser, Debug)]
#[command(
    name = "conclave",
    about = "AI Agent Swarm - Autonomous agents communicating via UDP multicast",
    long_about = "A distributed system of autonomous AI agents that communicate with each other via protobuf UDP multicast. Each agent operates independently with a pluggable LLM backend and configurable personality system.",
    version
)]
pub struct AgentArgs {
    /// Unique identifier for this agent
    #[arg(
        short = 'i',
        long = "agent-id",
        help = "Unique identifier for this agent (e.g., 'agent-1', 'researcher', 'coordinator')",
        value_name = "ID"
    )]
    pub agent_id: String,

    /// UDP multicast address for agent communication
    #[arg(
        short = 'a',
        long = "multicast-address",
        help = "UDP multicast address for agent communication",
        default_value = "239.255.255.250:8080",
        value_name = "ADDRESS:PORT"
    )]
    pub multicast_address: SocketAddr,

    /// Network interface to bind to (optional)
    #[arg(
        long = "interface",
        help = "Network interface to bind to (e.g., 'eth0', '192.168.1.100')",
        value_name = "INTERFACE"
    )]
    pub interface: Option<String>,

    /// LLM backend type to use
    #[arg(
        short = 'b',
        long = "llm-backend",
        help = "LLM backend type to use for generating responses",
        default_value = "openai",
        value_enum
    )]
    pub llm_backend: LLMBackend,

    /// LLM model name
    #[arg(
        short = 'm',
        long = "model",
        help = "Specific model to use (e.g., 'gpt-4', 'claude-3-sonnet', 'llama2')",
        default_value = "gpt-3.5-turbo",
        value_name = "MODEL"
    )]
    pub model: String,

    /// API key for LLM backend (can also be set via environment variable)
    #[arg(
        short = 'k',
        long = "api-key",
        help = "API key for LLM backend (or set OPENAI_API_KEY/ANTHROPIC_API_KEY env var)",
        value_name = "KEY"
    )]
    pub api_key: Option<String>,

    /// Custom API endpoint URL
    #[arg(
        long = "endpoint",
        help = "Custom API endpoint URL for LLM backend",
        value_name = "URL"
    )]
    pub endpoint: Option<String>,

    /// Request timeout in seconds
    #[arg(
        long = "timeout",
        help = "Request timeout for LLM backend in seconds",
        default_value = "30",
        value_name = "SECONDS"
    )]
    pub timeout_seconds: u64,

    /// Maximum retry attempts for failed requests
    #[arg(
        long = "max-retries",
        help = "Maximum number of retry attempts for failed LLM requests",
        default_value = "3",
        value_name = "COUNT"
    )]
    pub max_retries: u32,

    /// Enable verbose logging
    #[arg(
        short = 'v',
        long = "verbose",
        help = "Enable verbose logging output",
        action = clap::ArgAction::Count
    )]
    pub verbose: u8,

    /// Log level filter
    #[arg(
        long = "log-level",
        help = "Set the log level",
        default_value = "info",
        value_parser = ["error", "warn", "info", "debug", "trace"]
    )]
    pub log_level: String,
}

impl AgentArgs {
    /// Validate the provided arguments
    pub fn validate(&self) -> Result<(), String> {
        // Validate agent ID is not empty
        if self.agent_id.trim().is_empty() {
            return Err("Agent ID cannot be empty".to_string());
        }

        // Validate agent ID contains only valid characters
        if !self
            .agent_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "Agent ID can only contain alphanumeric characters, hyphens, and underscores"
                    .to_string(),
            );
        }

        // Validate multicast address is in the multicast range
        if !self.multicast_address.ip().is_multicast() {
            return Err(format!(
                "Address {} is not a valid multicast address",
                self.multicast_address.ip()
            ));
        }

        // Validate timeout is reasonable
        if self.timeout_seconds == 0 || self.timeout_seconds > 300 {
            return Err("Timeout must be between 1 and 300 seconds".to_string());
        }

        // Validate max retries is reasonable
        if self.max_retries > 10 {
            return Err("Max retries cannot exceed 10".to_string());
        }

        // Validate model name is not empty
        if self.model.trim().is_empty() {
            return Err("Model name cannot be empty".to_string());
        }

        Ok(())
    }

    /// Get the effective API key, checking environment variables if not provided
    pub fn get_api_key(&self) -> Option<String> {
        if let Some(key) = &self.api_key {
            return Some(key.clone());
        }

        // Check environment variables based on backend type
        match self.llm_backend {
            LLMBackend::OpenAI => std::env::var("OPENAI_API_KEY").ok(),
            LLMBackend::Anthropic => std::env::var("ANTHROPIC_API_KEY").ok(),
            LLMBackend::Google => std::env::var("GOOGLE_API_KEY").ok(),
            LLMBackend::Local => None, // Local models typically don't need API keys
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_args_validation_valid() {
        let args = AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::OpenAI,
            model: "gpt-3.5-turbo".to_string(),
            api_key: None,
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert!(args.validate().is_ok());
    }

    #[test]
    fn test_agent_args_validation_empty_agent_id() {
        let args = AgentArgs {
            agent_id: "".to_string(),
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::OpenAI,
            model: "gpt-3.5-turbo".to_string(),
            api_key: None,
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert!(args.validate().is_err());
        assert_eq!(args.validate().unwrap_err(), "Agent ID cannot be empty");
    }

    #[test]
    fn test_agent_args_validation_invalid_agent_id_characters() {
        let args = AgentArgs {
            agent_id: "invalid@agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::OpenAI,
            model: "gpt-3.5-turbo".to_string(),
            api_key: None,
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert!(args.validate().is_err());
        assert_eq!(
            args.validate().unwrap_err(),
            "Agent ID can only contain alphanumeric characters, hyphens, and underscores"
        );
    }

    #[test]
    fn test_agent_args_validation_non_multicast_address() {
        let args = AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "192.168.1.1:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::OpenAI,
            model: "gpt-3.5-turbo".to_string(),
            api_key: None,
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert!(args.validate().is_err());
        assert!(
            args.validate()
                .unwrap_err()
                .contains("is not a valid multicast address")
        );
    }

    #[test]
    fn test_agent_args_validation_invalid_timeout() {
        let args = AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::OpenAI,
            model: "gpt-3.5-turbo".to_string(),
            api_key: None,
            endpoint: None,
            timeout_seconds: 0,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert!(args.validate().is_err());
        assert_eq!(
            args.validate().unwrap_err(),
            "Timeout must be between 1 and 300 seconds"
        );
    }

    #[test]
    fn test_agent_args_validation_excessive_retries() {
        let args = AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::OpenAI,
            model: "gpt-3.5-turbo".to_string(),
            api_key: None,
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 15,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert!(args.validate().is_err());
        assert_eq!(args.validate().unwrap_err(), "Max retries cannot exceed 10");
    }

    #[test]
    fn test_agent_args_validation_empty_model() {
        let args = AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::OpenAI,
            model: "".to_string(),
            api_key: None,
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert!(args.validate().is_err());
        assert_eq!(args.validate().unwrap_err(), "Model name cannot be empty");
    }

    #[test]
    fn test_agent_args_get_api_key_from_arg() {
        let args = AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::OpenAI,
            model: "gpt-3.5-turbo".to_string(),
            api_key: Some("test-key".to_string()),
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert_eq!(args.get_api_key(), Some("test-key".to_string()));
    }

    #[test]
    fn test_agent_args_get_api_key_local_backend() {
        let args = AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse().unwrap(),
            interface: None,
            llm_backend: LLMBackend::Local,
            model: "llama2".to_string(),
            api_key: None,
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        };

        assert_eq!(args.get_api_key(), None);
    }

    #[test]
    fn test_llm_backend_display() {
        assert_eq!(LLMBackend::OpenAI.to_string(), "openai");
        assert_eq!(LLMBackend::Anthropic.to_string(), "anthropic");
        assert_eq!(LLMBackend::Local.to_string(), "local");
    }
}