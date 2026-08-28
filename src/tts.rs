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

use anyhow::{Result, anyhow};
use elevenlabs_rs::endpoints::genai::tts::{TextToSpeech, TextToSpeechBody};
use elevenlabs_rs::utils::play;
use elevenlabs_rs::{DefaultVoice, ElevenLabsClient, Model};
use futures_util::{SinkExt, StreamExt};
use rodio::OutputStream;
use rodio::Sink;
use rodio::buffer::SamplesBuffer;
use serde_json::json;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tracing::{debug, warn};

use crate::cli::TtsProvider as CliProvider;

/// Deepgram Flux streaming endpoint; Flux voices are served only on `/v2/speak`.
const DEEPGRAM_SPEAK_URL: &str = "wss://api.deepgram.com/v2/speak";

/// Default Flux voice, matching Deepgram's own default for v2 speak.
pub const DEFAULT_FLUX_VOICE: &str = "flux-kit-en";

/// Flux streams raw mono 16-bit PCM; this is the model-native sample rate.
const FLUX_SAMPLE_RATE: u32 = 24_000;

/// A validated Flux TTS model string, `flux-{voice}-{language}` (e.g.
/// `flux-haley-en`).
///
/// Constructing this type is the only way to obtain a Flux voice, so an
/// invalid model string is rejected at the CLI boundary instead of mid-debate
/// by the Deepgram API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FluxVoice(String);

impl FluxVoice {
    /// Validate `value` as a Flux model string and wrap it.
    ///
    /// # Errors
    ///
    /// Returns an error unless `value` is lowercase
    /// `flux-<voice>-<language>` with a non-empty voice and a two-letter
    /// language code.
    pub fn new(value: &str) -> Result<Self> {
        fn is_ascii_segment(segment: &str) -> bool {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }

        fn is_language_code(lang: &str) -> bool {
            lang.len() == 2 && lang.chars().all(|c| c.is_ascii_lowercase())
        }

        let mut parts = value.split('-');
        let valid = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some("flux"), Some(voice), Some(lang), None) => {
                is_ascii_segment(voice) && is_language_code(lang)
            }
            _ => false,
        };

        if valid {
            Ok(Self(value.to_string()))
        } else {
            Err(anyhow!(
                "invalid Flux voice '{value}': expected format flux-<voice>-<language>, e.g. {DEFAULT_FLUX_VOICE}"
            ))
        }
    }

    /// The Flux model string, e.g. `flux-haley-en`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Deepgram Flux TTS over the streaming `/v2/speak` WebSocket.
///
/// One connection is opened per spoken response: the full text is sent as a
/// single `Speak`, the turn is closed with `Flush`, binary audio frames are
/// appended to a rodio sink as they arrive, and the server's `SpeechMetadata`
/// marks the end of the turn's audio.
pub struct DeepgramFluxTts {
    api_key: String,
    voice: FluxVoice,
}

impl DeepgramFluxTts {
    /// Create a client, reading the API key from `DEEPGRAM_API_KEY`.
    ///
    /// # Errors
    ///
    /// Returns an error when `DEEPGRAM_API_KEY` is not set.
    pub fn new(voice: FluxVoice) -> Result<Self> {
        let api_key = std::env::var("DEEPGRAM_API_KEY")
            .map_err(|_| anyhow!("DEEPGRAM_API_KEY must be set for --tts deepgram"))?;
        Ok(Self { api_key, voice })
    }

    /// The WebSocket URL with connection parameters for `voice`.
    fn ws_url(voice: &FluxVoice) -> String {
        format!(
            "{DEEPGRAM_SPEAK_URL}?model={}&encoding=linear16&sample_rate={FLUX_SAMPLE_RATE}",
            voice.as_str()
        )
    }

    /// Synthesize `text` and play it; returns when playback finishes.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection, authentication, or synthesis
    /// fails, or the server closes before the turn's audio completes.
    pub async fn say(&self, text: &str) -> Result<()> {
        let mut request = Self::ws_url(&self.voice)
            .into_client_request()
            .map_err(|e| anyhow!("invalid Deepgram TTS request: {e}"))?;
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Token {}", self.api_key))
                .map_err(|_| anyhow!("DEEPGRAM_API_KEY contains invalid characters"))?,
        );

        let (socket, response) = connect_async(request)
            .await
            .map_err(|e| anyhow!("Deepgram TTS connection failed: {e}"))?;
        debug!(
            status = %response.status(),
            voice = self.voice.as_str(),
            "connected to Deepgram Flux TTS"
        );

        let (mut write, mut read) = socket.split();

        write
            .send(Message::text(
                json!({"type": "Speak", "text": text}).to_string(),
            ))
            .await
            .map_err(|e| anyhow!("failed to send Speak: {e}"))?;
        write
            .send(Message::text(json!({"type": "Flush"}).to_string()))
            .await
            .map_err(|e| anyhow!("failed to send Flush: {e}"))?;

        // rodio's output stream and sink are not `Send`, so playback runs on a
        // dedicated thread fed with decoded samples; this keeps the returned
        // future `Send` and stops playback from blocking the async worker.
        let (samples_tx, samples_rx) = std::sync::mpsc::channel::<Vec<i16>>();
        let playback = std::thread::spawn(move || -> Result<()> {
            let (stream, handle) =
                OutputStream::try_default().map_err(|e| anyhow!("audio output: {e}"))?;
            let sink = Sink::try_new(&handle).map_err(|e| anyhow!("audio sink: {e}"))?;
            for samples in samples_rx {
                if !samples.is_empty() {
                    sink.append(SamplesBuffer::new(1, FLUX_SAMPLE_RATE, samples));
                }
            }
            // Channel closed: the turn's audio has been queued in full.
            sink.sleep_until_end();
            drop(sink);
            drop(stream);
            Ok(())
        });

        let mut audio_frames = 0_usize;
        let mut turn_complete = false;
        while let Some(message) = read.next().await {
            match message.map_err(|e| anyhow!("Deepgram TTS socket error: {e}"))? {
                Message::Binary(pcm) => {
                    audio_frames += 1;
                    let samples = decode_linear16(pcm);
                    if samples_tx.send(samples).is_err() {
                        return Err(anyhow!("audio playback thread exited unexpectedly"));
                    }
                }
                Message::Text(frame) => {
                    let value: serde_json::Value = serde_json::from_str(&frame)
                        .map_err(|e| anyhow!("unparseable Deepgram message '{frame}': {e}"))?;
                    match value.get("type").and_then(|t| t.as_str()) {
                        // The turn's audio is complete; playback drains the sink.
                        Some("SpeechMetadata") => {
                            turn_complete = true;
                            break;
                        }
                        Some("Error") => {
                            let code = value.get("code").and_then(|c| c.as_str()).unwrap_or("?");
                            let description = value
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("unknown error");
                            return Err(anyhow!("Deepgram TTS error {code}: {description}"));
                        }
                        Some("Warning") => {
                            warn!("Deepgram TTS warning: {frame}");
                        }
                        // Connected, SpeechStarted, Flushed, ...
                        Some(other) => debug!("Deepgram TTS message '{other}'"),
                        None => debug!("Deepgram TTS message without type: {frame}"),
                    }
                }
                Message::Close(frame) => {
                    return Err(anyhow!(
                        "Deepgram TTS closed before the turn finished: {frame:?}"
                    ));
                }
                // Ping/Pong are answered by tungstenite automatically.
                _ => {}
            }
        }

        // Graceful shutdown; the server drains and replies with Close.
        let _ = write.send(Message::Close(None)).await;
        drop(write);
        drop(read);
        drop(samples_tx); // signal the playback thread that no more audio is coming

        if !turn_complete {
            warn!(
                "Deepgram TTS stream ended early; playing the {audio_frames} audio frames received"
            );
        }

        debug!("Flux turn complete: {audio_frames} audio frames");
        match playback.join() {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

/// Decode little-endian mono 16-bit PCM bytes into `i16` samples; a trailing
/// odd byte is discarded.
fn decode_linear16(bytes: Vec<u8>) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

/// A configured text-to-speech engine.
pub enum TtsEngine {
    /// ElevenLabs TTS with a voice id (premade name or UUID).
    Elevenlabs {
        client: ElevenLabsClient,
        voice_id: String,
    },
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
            CliProvider::Elevenlabs => {
                let client =
                    ElevenLabsClient::from_env().map_err(|e| anyhow!("ElevenLabsClient: {e}"))?;
                let voice_id = match voice {
                    Some(voice) => voice.to_string(),
                    None => String::from(DefaultVoice::Brian),
                };
                Ok(Self::Elevenlabs { client, voice_id })
            }
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
            Self::Elevenlabs { client, voice_id } => {
                let endpoint = TextToSpeech::new(
                    voice_id.clone(),
                    TextToSpeechBody::new(text).with_model_id(Model::ElevenTurboV2_5),
                );
                let speech = client
                    .hit(endpoint)
                    .await
                    .map_err(|e| anyhow!("ElevenLabs error: {e}"))?;
                play(speech).map_err(|e| anyhow!("audio playback error: {e}"))
            }
            Self::Deepgram(tts) => tts.say(text).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flux_voice_accepts_valid_model_strings() {
        assert_eq!(
            FluxVoice::new("flux-haley-en").unwrap().as_str(),
            "flux-haley-en"
        );
        assert_eq!(
            FluxVoice::new(DEFAULT_FLUX_VOICE).unwrap().as_str(),
            DEFAULT_FLUX_VOICE
        );
    }

    #[test]
    fn flux_voice_rejects_invalid_model_strings() {
        for invalid in [
            "",                 // empty
            "flux--en",         // empty voice segment
            "flux-haley",       // missing language
            "flux-haley-en-us", // extra segment
            "Flux-haley-en",    // uppercase prefix
            "flux-Haley-en",    // uppercase voice
            "flux-haley-EN",    // uppercase language
            "flux-haley-e1",    // digit in language code
            "aura-2-thalia-en", // Aura model on /v2/speak
            "flux-haley_en",    // wrong separator
        ] {
            assert!(
                FluxVoice::new(invalid).is_err(),
                "should reject {invalid:?}"
            );
        }
    }

    #[test]
    fn flux_ws_url_carries_model_and_audio_parameters() {
        let voice = FluxVoice::new("flux-hannah-en").unwrap();
        let url = DeepgramFluxTts::ws_url(&voice);
        assert!(url.starts_with("wss://api.deepgram.com/v2/speak?"), "{url}");
        assert!(url.contains("model=flux-hannah-en"), "{url}");
        assert!(url.contains("encoding=linear16"), "{url}");
        assert!(url.contains("sample_rate=24000"), "{url}");
    }

    #[test]
    fn linear16_decode_converts_le_bytes() {
        assert_eq!(decode_linear16(vec![]), Vec::<i16>::new());
        assert_eq!(decode_linear16(vec![0x00, 0x01, 0x00, 0x00]), vec![256, 0]);
        assert_eq!(decode_linear16(vec![0xFF, 0xFF]), vec![-1]);
        // Trailing odd byte is discarded.
        assert_eq!(decode_linear16(vec![0x34, 0x12, 0x56]), vec![0x1234]);
    }

    /// Proves the full handshake path (URL, TLS, auth header) against the
    /// real endpoint; run explicitly with:
    /// `DEEPGRAM_API_KEY=bogus cargo test --ignored deepgram_say`
    #[tokio::test]
    #[ignore = "hits the live Deepgram endpoint"]
    async fn deepgram_say_reaches_endpoint_and_rejects_bad_credentials() {
        let tts = DeepgramFluxTts::new(FluxVoice::new("flux-haley-en").unwrap())
            .expect("DEEPGRAM_API_KEY must be set for this test");

        let err = tts
            .say("Hello from conclave.")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("connection failed"), "{err}");
        assert!(err.contains("401"), "{err}");
    }
}
