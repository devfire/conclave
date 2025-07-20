// Import required modules from the LLM library
use llm::{
    LLMProvider,
    builder::{LLMBackend as ProviderBackend, LLMBuilder},
    chat::ChatMessage,
    error::LLMError,
};

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
            CliBackend::OpenAI => ProviderBackend::OpenAI,
            CliBackend::Anthropic => ProviderBackend::Anthropic,
            CliBackend::Google => ProviderBackend::Google,
            CliBackend::Local => ProviderBackend::Ollama,
        };

        builder = builder.backend(backend);

        // Set API key if available
        if let Some(key) = args.get_api_key() {
            builder = builder.api_key(key);
        }

        // Configure common parameters
        builder = builder
            .model(&args.model)
            .timeout_seconds(args.timeout_seconds)
            .stream(false)
            .temperature(0.7)
            .system("Keep responses concise.");

        // Set custom endpoint if provided
        if let Some(url) = &args.endpoint {
            builder = builder.base_url(url);
        }

        let provider = builder.build()?;

        Ok(Self { provider })
    }

    /// Generates a response based on the provided message history with retry logic
    pub async fn generate_llm_response(
        &self,
        messages: &[ChatMessage],
    ) -> Result<String, LLMError> {
        self.generate_llm_response_with_retries(messages, 3).await
    }

    /// Generates a response with configurable retry attempts
    pub async fn generate_llm_response_with_retries(
        &self,
        messages: &[ChatMessage],
        max_retries: u32,
    ) -> Result<String, LLMError> {
        let mut last_error = None;

        for attempt in 0..=max_retries {
            match self.provider.chat(messages).await {
                Ok(response) => {
                    if attempt > 0 {
                        tracing::info!("LLM request succeeded on attempt {}", attempt + 1);
                    }
                    return Ok(response.to_string());
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < max_retries {
                        let delay = std::time::Duration::from_millis(1000 * (attempt + 1) as u64);
                        tracing::warn!(
                            "LLM request failed on attempt {}, retrying in {:?}: {}",
                            attempt + 1,
                            delay,
                            last_error.as_ref().unwrap()
                        );
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        // If we get here, all retries failed
        let final_error = last_error.unwrap();
        tracing::error!(
            "LLM request failed after {} attempts: {}",
            max_retries + 1,
            final_error
        );
        Err(final_error)
    }

    /// Get the configured model name
    pub fn model_name(&self) -> &str {
        // This is a placeholder - the actual implementation would depend on the LLM provider
        "configured-model"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{AgentArgs, LLMBackend};
    use std::net::SocketAddr;

    fn create_test_args() -> AgentArgs {
        AgentArgs {
            agent_id: "test-agent".to_string(),
            multicast_address: "239.255.255.250:8080".parse::<SocketAddr>().unwrap(),
            interface: None,
            llm_backend: LLMBackend::Local,
            model: "test-model".to_string(),
            api_key: None,
            endpoint: Some("http://localhost:11434".to_string()),
            timeout_seconds: 30,
            max_retries: 3,
            verbose: 0,
            log_level: "info".to_string(),
        }
    }

    #[test]
    fn test_llm_module_creation_local_backend() {
        let args = create_test_args();
        let result = LLMModule::new(&args);

        // For local backend, creation should succeed even without API key
        assert!(result.is_ok());
    }

    #[test]
    fn test_llm_module_creation_openai_backend() {
        let mut args = create_test_args();
        args.llm_backend = LLMBackend::OpenAI;
        args.api_key = Some("test-api-key".to_string());

        let result = LLMModule::new(&args);

        // Should succeed with API key provided
        assert!(result.is_ok());
    }

    #[test]
    fn test_llm_module_creation_anthropic_backend() {
        let mut args = create_test_args();
        args.llm_backend = LLMBackend::Anthropic;
        args.api_key = Some("test-api-key".to_string());

        let result = LLMModule::new(&args);

        // Should succeed with API key provided
        assert!(result.is_ok());
    }

    #[test]
    fn test_llm_module_creation_google_backend() {
        let mut args = create_test_args();
        args.llm_backend = LLMBackend::Google;
        args.api_key = Some("test-api-key".to_string());

        let result = LLMModule::new(&args);

        // Should succeed with API key provided
        assert!(result.is_ok());
    }

    #[test]
    fn test_backend_mapping() {
        let test_cases = vec![
            LLMBackend::OpenAI,
            LLMBackend::Anthropic,
            LLMBackend::Google,
            LLMBackend::Local,
        ];

        for cli_backend in test_cases {
            let mut args = create_test_args();
            args.llm_backend = cli_backend.clone();
            args.api_key = Some("test-key".to_string());

            // We can't directly test the mapping without exposing internal state,
            // but we can verify that creation succeeds for all backends
            let result = LLMModule::new(&args);
            assert!(
                result.is_ok(),
                "Failed to create LLM module for backend: {:?}",
                cli_backend
            );
        }
    }

    #[test]
    fn test_model_name_placeholder() {
        let args = create_test_args();
        let llm_module = LLMModule::new(&args).expect("Failed to create LLM module");

        // Test the placeholder model name method
        assert_eq!(llm_module.model_name(), "configured-model");
    }

    // Note: We can't easily test generate_llm_response without a real LLM backend
    // or mocking framework, as it requires actual network calls to LLM providers.
    // In a production environment, you would want to add integration tests
    // that test against real or mock LLM endpoints.
}
