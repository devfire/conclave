use tracing::{error, info, warn};

use crate::cli::{AgentArgs, LLMBackend};
use crate::llm::LLMModule;
use llm::chat::ChatMessage;

/// Errors that can occur during LLM access validation.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("API key is required for the '{backend}' backend but was not provided. Set it via --api-key or the appropriate environment variable.")]
    MissingApiKey { backend: String },

    #[error("API key for '{backend}' is empty or contains only whitespace.")]
    EmptyApiKey { backend: String },

    #[error("API key for '{backend}' contains invalid characters (whitespace or control characters).")]
    InvalidApiKeyFormat { backend: String },

    #[error("API key for '{backend}' is suspiciously short ({length} characters). Expected at least {min_length} characters.")]
    ApiKeyTooShort {
        backend: String,
        length: usize,
        min_length: usize,
    },

    #[error("Failed to connect to '{backend}' LLM provider: {reason}")]
    ConnectionFailed { backend: String, reason: String },
}

/// Minimum expected key lengths per provider (conservative lower bounds).
const MIN_KEY_LENGTH_OPENAI: usize = 20;
const MIN_KEY_LENGTH_ANTHROPIC: usize = 20;
const MIN_KEY_LENGTH_GOOGLE: usize = 10;
const MIN_KEY_LENGTH_OPENROUTER: usize = 20;

/// Validates that the LLM configuration in [`AgentArgs`] is usable before the
/// provider is built.
///
/// For **local** (Ollama) backends no API key is required, so validation is
/// skipped.  For cloud backends the function verifies that:
///
/// 1. An API key is present (via CLI flag or environment variable).
/// 2. The key is non-empty and free of whitespace / control characters.
/// 3. The key meets a minimum length threshold for the chosen provider.
///
/// # Errors
///
/// Returns a [`ValidationError`] describing the exact problem so the caller
/// can surface a friendly message and abort before attempting any network
/// requests.
pub fn validate_llm_access(args: &AgentArgs) -> Result<(), ValidationError> {
    // Local backends (Ollama) do not require an API key.
    if matches!(args.llm_backend, LLMBackend::Local) {
        info!("Local backend selected — skipping API key validation");
        return Ok(());
    }

    let backend_name = args.llm_backend.to_string();

    // 1. Ensure a key was provided at all.
    let api_key = args.get_api_key().ok_or_else(|| {
        error!("No API key found for backend '{}'", backend_name);
        ValidationError::MissingApiKey {
            backend: backend_name.clone(),
        }
    })?;

    // 2. Reject empty / whitespace-only keys.
    if api_key.trim().is_empty() {
        error!("API key for '{}' is empty or whitespace", backend_name);
        return Err(ValidationError::EmptyApiKey {
            backend: backend_name,
        });
    }

    // 3. Reject keys that contain spaces, newlines, or control characters.
    if api_key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        error!(
            "API key for '{}' contains invalid characters",
            backend_name
        );
        return Err(ValidationError::InvalidApiKeyFormat {
            backend: backend_name,
        });
    }

    // 4. Check minimum length per provider.
    let min_length = match args.llm_backend {
        LLMBackend::OpenAI => MIN_KEY_LENGTH_OPENAI,
        LLMBackend::Anthropic => MIN_KEY_LENGTH_ANTHROPIC,
        LLMBackend::Google => MIN_KEY_LENGTH_GOOGLE,
        LLMBackend::OpenRouter => MIN_KEY_LENGTH_OPENROUTER,
        LLMBackend::Local => unreachable!(), // handled above
    };

    if api_key.len() < min_length {
        error!(
            "API key for '{}' is only {} characters (minimum {})",
            backend_name,
            api_key.len(),
            min_length
        );
        return Err(ValidationError::ApiKeyTooShort {
            backend: backend_name,
            length: api_key.len(),
            min_length,
        });
    }

    info!(
        "API key for '{}' passed format validation ({} characters)",
        backend_name,
        api_key.len()
    );
    Ok(())
}

/// Sends a lightweight probe message ("hello") to the configured LLM provider
/// to verify the API key is actually accepted by the remote service.
///
/// This should be called **after** [`validate_llm_access`] (format checks) and
/// **before** the application starts its main processing loop.
///
/// For **local** (Ollama) backends, the probe is skipped since there is no
/// remote authentication to verify.
///
/// # Errors
///
/// Returns [`ValidationError::ConnectionFailed`] if the provider rejects the
/// request (e.g. invalid/expired token, network error, model not found).
pub async fn validate_llm_connection(args: &AgentArgs) -> Result<(), ValidationError> {
    if matches!(args.llm_backend, LLMBackend::Local) {
        info!("Local backend selected — skipping connection probe");
        return Ok(());
    }

    let backend_name = args.llm_backend.to_string();
    info!(
        "Probing '{}' backend to verify API key is accepted…",
        backend_name
    );

    // Build a temporary LLM provider using the same config the app will use.
    let llm = LLMModule::new(args).map_err(|e| {
        error!("Failed to build LLM provider for probe: {}", e);
        ValidationError::ConnectionFailed {
            backend: backend_name.clone(),
            reason: format!("provider build error: {e}"),
        }
    })?;

    // Send a minimal probe message.
    let probe = vec![ChatMessage::user().content("hello").build()];

    match llm.generate_llm_response(&probe).await {
        Ok(response) => {
            info!(
                "Connection probe succeeded for '{}' (response length: {} chars)",
                backend_name,
                response.len()
            );
            Ok(())
        }
        Err(e) => {
            let reason = format!("{e}");
            let lower = reason.to_lowercase();
            // Distinguish auth errors from transient failures for clearer messaging.
            // NOTE: We intentionally do NOT match on "invalid" alone — it is far
            // too broad and catches non-auth errors like "invalid model" from
            // OpenAI-compatible providers (e.g. OpenRouter).
            if lower.contains("unauthorized")
                || lower.contains("401")
                || lower.contains("invalid api key")
                || lower.contains("invalid_api_key")
                || lower.contains("authentication")
                || lower.contains("permission")
                || lower.contains("403")
            {
                error!(
                    "API key rejected by '{}' provider: {}",
                    backend_name, reason
                );
            } else {
                warn!(
                    "Connection probe to '{}' failed (may be transient): {}",
                    backend_name, reason
                );
            }
            Err(ValidationError::ConnectionFailed {
                backend: backend_name,
                reason,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    /// Helper to build a minimal [`AgentArgs`] for testing.
    fn make_args(backend: LLMBackend, api_key: Option<String>) -> AgentArgs {
        AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse::<SocketAddr>().unwrap(),
            interface: None,
            llm_backend: backend,
            model: "gpt-3.5-turbo".to_string(),
            api_key,
            endpoint: None,
            timeout_seconds: 30,
            max_retries: 3,
            log_level: "info".to_string(),
            personality: "You are a helpful AI agent.".to_string(),
            personality_file: None,
            processing_delay_ms: 5000,
            voice: false,
        }
    }

    #[test]
    fn local_backend_skips_validation() {
        let args = make_args(LLMBackend::Local, None);
        assert!(validate_llm_access(&args).is_ok());
    }

    #[test]
    fn missing_api_key_is_rejected() {
        let args = make_args(LLMBackend::OpenAI, None);
        // Clear env to avoid interference
        // SAFETY: This test is single-threaded and no other thread reads this var.
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        let err = validate_llm_access(&args).unwrap_err();
        assert!(matches!(err, ValidationError::MissingApiKey { .. }));
    }

    #[test]
    fn empty_api_key_is_rejected() {
        let args = make_args(LLMBackend::OpenAI, Some("   ".to_string()));
        let err = validate_llm_access(&args).unwrap_err();
        assert!(matches!(err, ValidationError::EmptyApiKey { .. }));
    }

    #[test]
    fn api_key_with_whitespace_is_rejected() {
        let args = make_args(
            LLMBackend::OpenAI,
            Some("sk-abc 123def456ghi789jkl".to_string()),
        );
        let err = validate_llm_access(&args).unwrap_err();
        assert!(matches!(err, ValidationError::InvalidApiKeyFormat { .. }));
    }

    #[test]
    fn short_api_key_is_rejected() {
        let args = make_args(LLMBackend::OpenAI, Some("sk-short".to_string()));
        let err = validate_llm_access(&args).unwrap_err();
        assert!(matches!(err, ValidationError::ApiKeyTooShort { .. }));
    }

    #[test]
    fn valid_api_key_passes() {
        let args = make_args(
            LLMBackend::OpenAI,
            Some("sk-validkey1234567890abcdefghijklmnop".to_string()),
        );
        assert!(validate_llm_access(&args).is_ok());
    }

    #[test]
    fn valid_anthropic_key_passes() {
        let args = make_args(
            LLMBackend::Anthropic,
            Some("sk-ant-validkey1234567890abcdefghijklmnop".to_string()),
        );
        assert!(validate_llm_access(&args).is_ok());
    }

    #[test]
    fn valid_google_key_passes() {
        let args = make_args(
            LLMBackend::Google,
            Some("AIzaSyA1234567890".to_string()),
        );
        assert!(validate_llm_access(&args).is_ok());
    }
}
