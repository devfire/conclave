// Import required modules from the LLM library
use llm::{
    LLMProvider,
    builder::{LLMBackend, LLMBuilder},
    chat::ChatMessage,
    error::LLMError,
};
use tracing::info;

// Import project-specific types
use crate::cli::{AgentArgs, LLMBackend as CliBackend};

/// Common LLM module for handling different backends
pub struct LLMModule {
    provider: Box<dyn LLMProvider>,
}

impl LLMModule {
    /// Creates a new LLM module instance based on command-line arguments
    pub fn new(args: &AgentArgs) -> Result<Self, LLMError> {
        let mut builder = LLMBuilder::new();

        // Map project backend to provider backend
        let backend = match args.llm_backend {
            CliBackend::OpenAI => LLMBackend::OpenAI,
            CliBackend::Anthropic => LLMBackend::Anthropic,
            CliBackend::Google => LLMBackend::Google,
            CliBackend::Local => LLMBackend::Ollama,
        };

        builder = builder.backend(backend);

        // Set API key if available
        if let Some(key) = args.get_api_key() {
            builder = builder.api_key(key);
        }

        info!("Personality: {}", args.personality);

        // Configure common parameters
        builder = builder
            .model(&args.model)
            .timeout_seconds(args.timeout_seconds)
            .stream(false)
            .max_tokens(8192)
            .temperature(0.7)
            // set the system message for the LLM to args.personality and default to "Keep responses concise." if not provided
            .system(if args.personality.is_empty() {
                "Keep responses concise."
            } else {
                &args.personality
            });

        // Set custom endpoint if provided
        if let Some(url) = &args.endpoint {
            builder = builder.base_url(url);
        }

        let provider = builder.build()?;

        Ok(Self { provider })
    }

    /// Generates a response based on the provided message history
    pub async fn generate_llm_response(
        &self,
        messages: &[ChatMessage],
    ) -> Result<String, LLMError> {
        let response = self.provider.chat(messages).await?;
        Ok(response.to_string())
    }

    /// Create a user ChatMessage from content
    pub fn create_user_message(&self, content: &str) -> ChatMessage {
        ChatMessage::user().content(content).build()
    }
}
