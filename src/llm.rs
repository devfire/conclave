use anyhow::{Result, anyhow};
use llm::{
    LLMProvider,
    builder::{LLMBackend, LLMBuilder},
    chat::ChatMessage,
};
use tracing::debug;

// Import project-specific types
use crate::cli::{AgentArgs, LLMBackend as CliBackend};

/// Common LLM module for handling different backends
pub struct LLMModule {
    provider: Box<dyn LLMProvider>,
}

impl LLMModule {
    /// Creates a new LLM module instance based on command-line arguments
    pub fn new(args: &AgentArgs) -> Result<Self> {
        let mut builder = LLMBuilder::new();

        // Map project backend to provider backend
        let backend = match args.llm_backend {
            CliBackend::OpenAI => LLMBackend::OpenAI,
            CliBackend::Anthropic => LLMBackend::Anthropic,
            CliBackend::Google => LLMBackend::Google,
            CliBackend::Local => LLMBackend::Ollama,
            CliBackend::OpenRouter => LLMBackend::OpenRouter,
        };

        builder = builder.backend(backend);

        // Set API key if available
        if let Some(key) = args.get_api_key() {
            builder = builder.api_key(key);
        }

        // Get personality prompt (either from inline flag or file)
        let personality = args
            .get_personality()
            .map_err(|e| anyhow!("Failed to load personality: {}", e))?;

        debug!("Personality: {}", personality);

        // Configure common parameters
        builder = builder
            .model(&args.model)
            .timeout_seconds(args.timeout_seconds)
            .max_tokens(8192)
            .temperature(0.7)
            .sliding_window_with_strategy(10, llm::memory::TrimStrategy::Summarize)
            // set the system message for the LLM to the personality prompt
            .system(&personality);

        // Set custom endpoint if provided
        if let Some(url) = &args.endpoint {
            builder = builder.base_url(url);
        }

        let provider = builder
            .build()
            .map_err(|e| anyhow!("Failed to build LLM provider: {:?}", e))?;

        Ok(Self { provider })
    }

    /// Generates a response based on the provided message history
    pub async fn generate_llm_response(&self, messages: &[ChatMessage]) -> Result<String> {
        debug!("Sending {:?} messages.", messages);
        let response = self
            .provider
            .chat(messages)
            .await
            .map_err(|e| anyhow!("Failed to generate LLM response: {:?}", e))?;
        Ok(response.to_string())
    }

    /// Create a user ChatMessage from content
    pub fn create_user_message(&self, content: &str) -> ChatMessage {
        ChatMessage::user().content(content).build()
    }
}
