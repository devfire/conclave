//! ElevenLabs TTS over the REST API; the full clip is synthesized before
//! playback starts.

use anyhow::{Result, anyhow};
use elevenlabs_rs::endpoints::genai::tts::{TextToSpeech, TextToSpeechBody};
use elevenlabs_rs::utils::play;
use elevenlabs_rs::{DefaultVoice, ElevenLabsClient, Model};

/// ElevenLabs TTS with a voice id (premade name or UUID).
pub struct ElevenLabsTts {
    client: ElevenLabsClient,
    voice_id: String,
}

impl ElevenLabsTts {
    /// Create a client, reading credentials from the environment.
    ///
    /// `voice` overrides the default `Brian` voice.
    ///
    /// # Errors
    ///
    /// Returns an error when ElevenLabs credentials are missing.
    pub fn new(voice: Option<&str>) -> Result<Self> {
        let client = ElevenLabsClient::from_env().map_err(|e| anyhow!("ElevenLabsClient: {e}"))?;
        let voice_id = match voice {
            Some(voice) => voice.to_string(),
            None => String::from(DefaultVoice::Brian),
        };
        Ok(Self { client, voice_id })
    }

    /// Synthesize `text` and play it; returns when playback finishes.
    ///
    /// # Errors
    ///
    /// Returns an error when synthesis or audio playback fails.
    pub async fn say(&self, text: &str) -> Result<()> {
        let endpoint = TextToSpeech::new(
            self.voice_id.clone(),
            TextToSpeechBody::new(text).with_model_id(Model::ElevenTurboV2_5),
        );
        let speech = self
            .client
            .hit(endpoint)
            .await
            .map_err(|e| anyhow!("ElevenLabs error: {e}"))?;
        play(speech).map_err(|e| anyhow!("audio playback error: {e}"))
    }
}
