use clap::{Parser, ValueEnum};
use std::fs;
use std::net::SocketAddr;
use std::path::Path;

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

    /// OpenRouter
    #[value(name = "openrouter")]
    OpenRouter,

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
            LLMBackend::OpenRouter => write!(f, "openrouter"),
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
        help = "API key for LLM backend (or set ANTHROPIC_API_KEY/GEMINI_API_KEY env var)",
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

    /// Log level filter
    #[arg(
        long = "log-level",
        help = "Set the log level",
        default_value = "info",
        value_parser = ["error", "warn", "info", "debug", "trace"]
    )]
    pub log_level: String,

    /// Agent personality for LLM system prompt
    #[arg(
        long = "personality",
        help = "Agent personality that defines behavior and response style",
        default_value = "You are a helpful AI agent. Keep responses concise and professional.",
        value_name = "PERSONALITY",
        conflicts_with = "personality_file"
    )]
    pub personality: String,

    /// Read agent personality from file (mutually exclusive with -p/--personality)
    #[arg(
        long = "personality-file",
        help = "Read agent personality from file (mutually exclusive with --personality)",
        value_name = "FILE_PATH",
        conflicts_with = "personality"
    )]
    pub personality_file: Option<String>,

    /// Processing delay in milliseconds for simulated processing time
    #[arg(
        long = "processing-delay",
        help = "Processing delay in milliseconds for simulating processing time",
        default_value = "5000",
        value_name = "MILLISECONDS"
    )]
    pub processing_delay_ms: u64,
}

impl AgentArgs {
    /// Get the effective personality prompt, reading from file if specified
    pub fn get_personality(&self) -> Result<String, String> {
        if let Some(file_path) = &self.personality_file {
            let path = Path::new(file_path);
            if !path.exists() {
                return Err(format!("Personality file '{}' does not exist", file_path));
            }
            if !path.is_file() {
                return Err(format!("'{}' is not a file", file_path));
            }

            match fs::read_to_string(path) {
                Ok(content) => {
                    if content.trim().is_empty() {
                        Err("Personality file is empty".to_string())
                    } else {
                        Ok(content)
                    }
                }
                Err(e) => Err(format!(
                    "Failed to read personality file '{}': {}",
                    file_path, e
                )),
            }
        } else {
            Ok(self.personality.clone())
        }
    }

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

        // Validate processing delay is reasonable
        if self.processing_delay_ms > 60000 {
            return Err("Processing delay cannot exceed 60 seconds".to_string());
        }

        // Validate personality file can be read if specified
        if let Some(ref file_path) = self.personality_file {
            if let Err(e) = self.get_personality() {
                return Err(format!("Invalid personality file '{}': {}", file_path, e));
            }
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
            LLMBackend::Google => std::env::var("GEMINI_API_KEY").ok(),
            LLMBackend::OpenRouter => std::env::var("OPENROUTER_API_KEY").ok(),
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
            log_level: "info".to_string(),
            personality: "You are a helpful AI agent.".to_string(),
            personality_file: None,
            processing_delay_ms: 5000,
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
            log_level: "info".to_string(),
            personality: "You are a helpful AI agent.".to_string(),
            personality_file: None,
            processing_delay_ms: 5000,
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
            log_level: "info".to_string(),
            personality: "You are a helpful AI agent.".to_string(),
            personality_file: None,
            processing_delay_ms: 5000,
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
            log_level: "info".to_string(),
            personality: "You are a helpful AI agent.".to_string(),
            personality_file: None,
            processing_delay_ms: 5000,
        };

        assert!(args.validate().is_err());
        assert!(
            args.validate()
                .unwrap_err()
                .contains("is not a valid multicast address")
        );
    }

    #[test]
    fn test_get_personality_from_inline() {
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
            log_level: "info".to_string(),
            personality: "You are a helpful AI agent.".to_string(),
            personality_file: None,
            processing_delay_ms: 5000,
        };

        assert_eq!(
            args.get_personality().unwrap(),
            "You are a helpful AI agent."
        );
    }

    #[test]
    fn test_personality_file_mutual_exclusivity() {
        // This test verifies that clap's conflicts_with attribute works
        // When both flags are provided, clap will return an error
        let result = AgentArgs::try_parse_from(&[
            "conclave",
            "--agent-id", "test-agent",
            "--personality", "inline personality",
            "--personality-file", "/path/to/file"
        ]);
        
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("cannot be used with"));
    }

    #[test]
    fn test_personality_file_flag_alone() {
        // Test that only --personality-file flag works
        let result = AgentArgs::try_parse_from(&[
            "conclave",
            "--agent-id", "test-agent",
            "--personality-file", "/path/to/personality.txt"
        ]);
        
        assert!(result.is_ok());
        let args = result.unwrap();
        assert_eq!(args.personality_file, Some("/path/to/personality.txt".to_string()));
        // personality should still have default value since only file was specified
        assert!(!args.personality.is_empty());
    }

    #[test]
    fn test_personality_inline_flag_alone() {
        // Test that only --personality flag works (default behavior)
        let result = AgentArgs::try_parse_from(&[
            "conclave",
            "--agent-id", "test-agent",
            "--personality", "custom personality"
        ]);
        
        assert!(result.is_ok());
        let args = result.unwrap();
        assert_eq!(args.personality, "custom personality");
        assert_eq!(args.personality_file, None);
    }

    #[test]
    fn test_no_personality_flags() {
        // Test default behavior when neither flag is provided
        let result = AgentArgs::try_parse_from(&[
            "conclave",
            "--agent-id", "test-agent"
        ]);
        
        assert!(result.is_ok());
        let args = result.unwrap();
        assert!(!args.personality.is_empty());
        assert_eq!(args.personality_file, None);
    }
}
