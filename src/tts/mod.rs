//! Text-to-speech engines for spoken agent responses.
//!
//! Two engines are supported:
//!
//! - [`TtsEngine::Elevenlabs`] — ElevenLabs TTS.
//! - [`TtsEngine::Deepgram`] — Deepgram Flux TTS over the streaming
//!   `/v2/speak` WebSocket. Audio frames are played as they arrive, so a
//!   spoken turn starts within ~200 ms instead of waiting for the whole clip
//!   to synthesize.
//!
//! Each agent selects an engine and a voice on the command line, so multiple
//! agents debating on one machine are distinguishable by voice.

mod deepgram;
mod elevenlabs;

pub use deepgram::{DEFAULT_FLUX_VOICE, DeepgramFluxTts, FluxVoice};
pub use elevenlabs::ElevenLabsTts;

use anyhow::Result;

use crate::cli::TtsProvider as CliProvider;

/// A configured text-to-speech engine.
pub enum TtsEngine {
    /// ElevenLabs TTS with a voice id (premade name or UUID).
    Elevenlabs(ElevenLabsTts),
    /// Deepgram Flux TTS over the streaming WebSocket.
    Deepgram(DeepgramFluxTts),
}

impl TtsEngine {
    /// Build the engine selected on the command line.
    ///
    /// `voice` overrides the provider's default voice: an ElevenLabs voice id
    /// (e.g. `Brian`) or a Flux model string (e.g. `flux-haley-en`).
    ///
    /// # Errors
    ///
    /// Returns an error when provider credentials are missing or `voice` is
    /// invalid for the provider.
    pub fn from_args(provider: CliProvider, voice: Option<&str>) -> Result<Self> {
        match provider {
            CliProvider::Elevenlabs => Ok(Self::Elevenlabs(ElevenLabsTts::new(voice)?)),
            CliProvider::Deepgram => {
                let voice = FluxVoice::new(voice.unwrap_or(DEFAULT_FLUX_VOICE))?;
                Ok(Self::Deepgram(DeepgramFluxTts::new(voice)?))
            }
        }
    }

    /// Synthesize and play `text` aloud; returns when playback finishes.
    ///
    /// # Errors
    ///
    /// Returns an error when synthesis or audio playback fails.
    pub async fn say(&self, text: &str) -> Result<()> {
        match self {
            Self::Elevenlabs(tts) => tts.say(text).await,
            Self::Deepgram(tts) => tts.say(text).await,
        }
    }
}
