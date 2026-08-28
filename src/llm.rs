use anyhow::{Result, anyhow};
use llm::{
    LLMProvider,
    builder::{LLMBackend, LLMBuilder},
    chat::ChatMessage,
};
use tracing::debug;

// Import project-specific types
use crate::cli::AgentArgs;
use crate::cli::LLMBackend as CliBackend;
use crate::tts::TtsEngine;

/// Common LLM module for handling different backends
pub struct LLMModule {
    provider: Box<dyn LLMProvider>,
    tts: Option<TtsEngine>,
}

impl LLMModule {
    /// Creates a new LLM module instance based on command-line arguments.
    ///
    /// # Errors
    ///
    /// Returns an error if the TTS engine cannot be created, the personality
    /// cannot be loaded, or the underlying LLM provider fails to build.
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

        let tts = match args.tts {
            Some(provider) => Some(TtsEngine::from_args(provider, args.tts_voice.as_deref())?),
            None => None,
        };

        // Get personality prompt (either from inline flag or file)
        let personality = args
            .get_personality()
            .map_err(|e| anyhow!("Failed to load personality: {e}"))?;

        debug!("Personality: {}", personality);

        // Configure common parameters
        builder = builder
            .model(&args.model)
            .timeout_seconds(args.timeout_seconds)
            .max_tokens(8192)
            .temperature(0.7)
            .sliding_window_with_strategy(20, llm::memory::TrimStrategy::Summarize)
            // set the system message for the LLM to the personality prompt
            .system(&personality);

        // Set custom endpoint if provided.
        // The underlying `llm` crate uses `Url::join()` to append paths like
        // "chat/completions" to the base URL.  `Url::join()` follows RFC 3986
        // semantics: if the base path does NOT end with '/', the last segment
        // is replaced instead of appended.  For example:
        //   "https://openrouter.ai/api/v1" + "chat/completions"
        //     → "https://openrouter.ai/api/chat/completions"   (WRONG)
        //   "https://openrouter.ai/api/v1/" + "chat/completions"
        //     → "https://openrouter.ai/api/v1/chat/completions" (CORRECT)
        // We normalise here so users don't have to worry about trailing slashes.
        if let Some(url) = &args.endpoint {
            let normalised = if url.ends_with('/') {
                url.clone()
            } else {
                format!("{url}/")
            };
            builder = builder.base_url(&normalised);
        }

        let provider = builder.build()?;

        Ok(Self { provider, tts })
    }

    /// Generates a response based on the provided message history.
    ///
    /// # Errors
    ///
    /// Returns an error if the LLM provider fails to generate a chat response.
    pub async fn generate_llm_response(&self, messages: &[ChatMessage]) -> Result<String> {
        debug!("Sending {:?} messages.", messages);
        let response = self.provider.chat(messages).await?;
        Ok(response.to_string())
    }

    /// Create a user `ChatMessage` from content
    #[must_use]
    pub fn create_user_message(&self, content: &str) -> ChatMessage {
        ChatMessage::user().content(content).build()
    }

    /// Speak `response` through the configured text-to-speech engine.
    ///
    /// # Errors
    ///
    /// Returns an error if voice is disabled or synthesis/playback fails.
    pub async fn say(&self, response: &str) -> Result<()> {
        let tts = self
            .tts
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("voice is disabled: start with --tts <provider>"))?;
        tts.say(response).await
    }
}
