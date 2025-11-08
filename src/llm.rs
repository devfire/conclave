use anyhow::{Result, anyhow};
use llm::{
    LLMProvider,
    builder::{LLMBackend, LLMBuilder},
    chat::ChatMessage,
};
use tracing::debug;

use elevenlabs_rs::endpoints::genai::tts::{TextToSpeech, TextToSpeechBody};
use elevenlabs_rs::utils::play;
use elevenlabs_rs::{DefaultVoice, ElevenLabsClient, Model};

// Import project-specific types
use crate::cli::{AgentArgs, LLMBackend as CliBackend};

/// Common LLM module for handling different backends
pub struct LLMModule {
    provider: Box<dyn LLMProvider>,
    tts: Option<Box<dyn LLMProvider>>,
    elevenlabs_client: Option<ElevenLabsClient>,
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

        let tts = if args.voice {
            let elevenlabs_api_key = std::env::var("ELEVENLABS_API_KEY")?;

            debug!("Elevenlabs API key is set.");

            Some(
                LLMBuilder::new()
                    .backend(LLMBackend::ElevenLabs)
                    .api_key(elevenlabs_api_key)
                    .model("eleven_turbo_v2_5")
                    .voice("JBFqnCsd6RMkjVDRZzb")
                    .build()?,
            )
        } else {
            None
        };

        let elevenlabs_client = if args.voice {
            Some(ElevenLabsClient::from_env().map_err(|e| anyhow!("ElevenLabsClient: {e}"))?)
        } else {
            None
        };

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
            .sliding_window_with_strategy(20, llm::memory::TrimStrategy::Summarize)
            // set the system message for the LLM to the personality prompt
            .system(&personality);

        // Set custom endpoint if provided
        if let Some(url) = &args.endpoint {
            builder = builder.base_url(url);
        }

        let provider = builder.build()?;

        Ok(Self {
            provider,
            tts,
            elevenlabs_client,
        })
    }

    /// Generates a response based on the provided message history
    pub async fn generate_llm_response(&self, messages: &[ChatMessage]) -> Result<String> {
        debug!("Sending {:?} messages.", messages);
        let response = self.provider.chat(messages).await?;
        Ok(response.to_string())
    }

    /// Create a user ChatMessage from content
    pub fn create_user_message(&self, content: &str) -> ChatMessage {
        ChatMessage::user().content(content).build()
    }

    // /// Save the response to mp3
    // pub async fn save_to_mp3(&self, response: &str) -> Result<()> {
    //     // Generate speech
    //     let audio_data = match &self.tts {
    //         Some(tts) => tts.speech(response).await?,
    //         None => return Err(anyhow!("Tried to generate an mp3 but failed.")),
    //     };

    //     // Save the audio to a file
    //     std::fs::write("output-speech-elevenlabs.mp3", audio_data)?;
    //     Ok(())
    // }

    pub async fn say(&self, response: &str) -> Result<()> {
        let body = TextToSpeechBody::new(response).with_model_id(Model::ElevenTurboV2_5);

        let endpoint = TextToSpeech::new(DefaultVoice::Brian, body);

        let speech = if let Some(elevenlabs) = &self.elevenlabs_client {
            elevenlabs
                .hit(endpoint)
                .await
                .map_err(|e| anyhow!("Error: {}", e))?
        } else {
            return Err(anyhow!("Failed to hit the elevenlabs endpoint"));
        };

        play(speech).map_err(|e| anyhow!("Error: {}", e))?;

        Ok(())
    }
}
