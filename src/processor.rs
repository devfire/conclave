use crate::{
    llm,
    message::AgentMessage,
    message_handler::{MessageHandler, MessageReceiver},
    network,
};
use ::llm::chat::ChatMessage;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use tokio_retry::Retry;
use tokio_retry::strategy::ExponentialBackoff;

/// Maximum messages (user + assistant combined) kept as LLM context —
/// roughly the last ten exchanges.
const MAX_HISTORY: usize = 20;

/// Explicit, bounded conversation history owned by the LLM processing task.
///
/// Replaces the `llm` crate's built-in sliding-window memory: llm 1.3.4's
/// `ChatWithMemory::chat_with_tools` appends each input message to memory and
/// then re-appends the same input slice to the request, so every peer
/// utterance reached the model twice (more on retry) — the root cause of the
/// "you repeated yourself twice" complaints. Owning the history here makes
/// the wire → context mapping exactly-once by construction.
struct ConversationHistory {
    messages: Vec<ChatMessage>,
    capacity: NonZeroUsize,
}

impl ConversationHistory {
    /// Create a history that retains at most `capacity` messages, dropping
    /// the oldest on overflow.
    fn new(capacity: NonZeroUsize) -> Self {
        Self {
            messages: Vec::new(),
            capacity,
        }
    }

    /// Append `message`, evicting oldest entries once capacity is reached.
    fn push(&mut self, message: ChatMessage) {
        while self.messages.len() >= self.capacity.get() {
            self.messages.remove(0);
        }
        self.messages.push(message);
    }

    /// Chronological view of the retained messages (oldest first).
    fn as_slice(&self) -> &[ChatMessage] {
        &self.messages
    }
}

pub struct Processor {
    message_handler: Arc<MessageHandler>,
    network_manager: Arc<network::NetworkManager>,
    agent_id: String,
}

impl Processor {
    pub fn new(
        message_handler: Arc<MessageHandler>,
        network_manager: Arc<network::NetworkManager>,
        agent_id: String,
    ) -> Self {
        Self {
            message_handler,
            network_manager,
            agent_id,
        }
    }

    /// Spawn LLM processing task for handling messages and generating responses
    /// This task receives messages from MPSC channel, filters self-messages, and generates LLM responses
    pub fn spawn_llm_processing_task(
        &self,
        llm_module: llm::LLMModule,
        mut message_receiver: MessageReceiver,
    ) -> JoinHandle<Result<(), String>> {
        let network_manager = Arc::clone(&self.network_manager);
        let agent_id = self.agent_id.clone();

        tokio::spawn(async move {
            info!("Starting LLM processing task for agent '{}'", agent_id);

            // Conversation continuity without the crate's memory (see
            // ConversationHistory docs): exactly-once context by construction.
            let mut history = ConversationHistory::new(
                NonZeroUsize::new(MAX_HISTORY).expect("MAX_HISTORY is non-zero"),
            );
            // Bootstrap the conversation with a greeting message, otherwise everyone is waiting for the first message
            let response_message =
                AgentMessage::new(agent_id.clone(), format!("Hi, I am {agent_id}."));

            info!(
                "Bootstrapping conversation with initial message: '{}'",
                response_message.content
            );

            // Broadcast response via network manager
            network_manager.send_message(&response_message).await?;

            loop {
                match message_receiver.receive_message().await {
                    Ok(message) => {
                        debug!(
                            "LLM processing received message from '{}' with content: '{}'",
                            message.sender_id,
                            message.content // message.content.chars().take(50).collect::<String>()
                        );

                        eprintln!("__________________________________");
                        eprintln!("{}: \n {}", message.sender_id, message.content);
                        eprintln!("__________________________________");
                        eprintln!();
                        // Record the peer's turn, then send the bounded
                        // history as this request's full context.
                        history.push(llm_module.create_user_message(&message.content));
                        let chat_messages: Vec<ChatMessage> = history.as_slice().to_vec();

                        // Retry an async operation
                        let llm_call_result = Retry::spawn(
                            ExponentialBackoff::from_millis(100)
                                .max_delay(Duration::from_secs(10))
                                .take(5),
                            || async {
                                debug!("Invoking LLM.");

                                llm_module.generate_llm_response(&chat_messages).await
                            },
                        )
                        .await;

                        let response_content = match llm_call_result {
                            Ok(response) => response,
                            Err(e) => {
                                error!("LLM call failed after retries, skipping response: {e}");
                                continue; // do NOT broadcast the error
                            }
                        };

                        // Record our reply so the next request carries both sides.
                        history.push(llm_module.create_assistant_message(&response_content));
                        // Say it
                        match llm_module.say(&response_content).await {
                            Ok(()) => info!("Speaking..."),
                            Err(e) => error!("TTS error: {e}"),
                        }

                        debug!(
                            "Sending response to message from '{}': '{}'",
                            message.sender_id, response_content
                        );

                        // Create response message
                        let response_message =
                            AgentMessage::new(agent_id.clone(), response_content);

                        // Broadcast response via network manager
                        network_manager.send_message(&response_message).await?;
                    }
                    Err(e) => {
                        error!("Message channel error: {}", e);
                        return Err(format!("LLM processing task failed: {e}"));
                    }
                }
            }
        })
    }

    /// Spawn UDP message intake task for continuous message reception
    /// This task receives messages from UDP multicast and sends them to MPSC channel
    pub fn spawn_udp_intake_task(&self) -> JoinHandle<Result<(), String>> {
        let network_manager = Arc::clone(&self.network_manager);
        let message_handler = Arc::clone(&self.message_handler);

        tokio::spawn(async move {
            info!(
                "Starting UDP message intake task for agent '{}'",
                message_handler.agent_id()
            );

            loop {
                match network_manager.receive_message().await {
                    Ok(message) => {
                        debug!(
                            "UDP intake received message from '{}' with content: '{}'",
                            message.sender_id,
                            message.content.chars().take(50).collect::<String>()
                        );

                        // Send message to MPSC channel (non-blocking)
                        if let Err(e) = message_handler.try_send_message(&message) {
                            warn!("Failed to send message to channel: {e}");
                            // Continue processing other messages even if channel is full
                        } else {
                            debug!(
                                "Successfully forwarded message from '{}' to processing channel",
                                message.sender_id
                            );
                        }
                    }
                    Err(network::NetworkError::Message(e)) => {
                        // Log malformed messages but continue processing
                        warn!("Received malformed message, skipping: {e}");
                    }
                    Err(e) => {
                        error!("UDP message reception error: {e}");
                        return Err(format!("UDP intake task failed: {e}"));
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::AgentArgs;
    use ::llm::chat::ChatRole;
    use clap::Parser;
    use parking_lot::Mutex;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;

    const SYSTEM_PROMPT: &str = "You are a concise debater.";

    fn contents(history: &ConversationHistory) -> Vec<&str> {
        history
            .as_slice()
            .iter()
            .map(|m| m.content.as_str())
            .collect()
    }

    #[test]
    fn history_keeps_newest_messages_in_order() {
        let mut history = ConversationHistory::new(NonZeroUsize::new(3).unwrap());
        for text in ["one", "two", "three", "four"] {
            history.push(ChatMessage::user().content(text).build());
        }
        assert_eq!(contents(&history), vec!["two", "three", "four"]);
    }

    #[test]
    fn history_mixes_roles_chronologically() {
        let mut history = ConversationHistory::new(NonZeroUsize::new(20).unwrap());
        history.push(ChatMessage::user().content("peer argument").build());
        history.push(ChatMessage::assistant().content("my rebuttal").build());
        history.push(ChatMessage::user().content("peer follow-up").build());
        assert_eq!(
            contents(&history),
            vec!["peer argument", "my rebuttal", "peer follow-up"]
        );
        assert_eq!(history.as_slice()[1].role, ChatRole::Assistant);
    }

    #[test]
    fn history_capacity_one_keeps_only_latest() {
        let mut history = ConversationHistory::new(NonZeroUsize::new(1).unwrap());
        history.push(ChatMessage::user().content("old").build());
        history.push(ChatMessage::assistant().content("new").build());
        assert_eq!(contents(&history), vec!["new"]);
    }

    /// Read one HTTP/1.1 request off `stream`, returning its body.
    /// Hand-rolled on purpose: the test must not drag in a server framework.
    fn read_request_body(stream: &mut TcpStream) -> Option<String> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            let n = stream.read(&mut chunk).ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
        };
        let headers = String::from_utf8_lossy(&buf[..header_end]).to_ascii_lowercase();
        let content_length: usize = headers
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        while buf.len() < header_end + 4 + content_length {
            let n = stream.read(&mut chunk).ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Some(String::from_utf8_lossy(&buf[header_end + 4..]).into_owned())
    }

    /// Minimal OpenAI-compatible mock: one response per entry in `replies`,
    /// capturing each request's JSON body for inspection.
    fn spawn_mock_openai(
        replies: Vec<&'static str>,
    ) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        let captured: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let Some(body) = read_request_body(&mut stream) else {
                    continue;
                };
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
                    sink.lock().push(value);
                }
                let payload = format!(
                    "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{reply}\"}}}}]}}"
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{addr}"), captured)
    }

    /// Extract text from an OpenAI-style message content field, which may be
    /// a plain string (user/assistant) or an array of typed parts (system).
    fn message_text(message: &serde_json::Value) -> String {
        match &message["content"] {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Array(parts) => parts
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join(""),
            other => other.to_string(),
        }
    }

    fn count_messages(request: &serde_json::Value, role: &str, content: &str) -> usize {
        request["messages"]
            .as_array()
            .map(|messages| {
                messages
                    .iter()
                    .filter(|m| m["role"] == role && message_text(m) == content)
                    .count()
            })
            .unwrap_or(0)
    }

    /// Regression for the "you repeated yourself twice" reports: every peer
    /// message and our own replies must reach the LLM exactly once per
    /// request, and the personality system prompt must ride along.
    #[tokio::test]
    async fn llm_request_contains_each_turn_exactly_once() {
        let (url, captured) = spawn_mock_openai(vec!["first rebuttal", "second rebuttal"]);

        let args = AgentArgs::parse_from([
            "conclave",
            "--agent-id",
            "affirmative",
            "--personality",
            SYSTEM_PROMPT,
            "--llm-backend",
            "openai",
            "--model",
            "mock-model",
            "--api-key",
            "test-key",
            "--endpoint",
            &url,
        ]);
        let module = crate::llm::LLMModule::new(&args).expect("LLM module");
        let mut history = ConversationHistory::new(NonZeroUsize::new(MAX_HISTORY).unwrap());

        // Turn 1: peer greeting arrives on the wire.
        history.push(module.create_user_message("Hi, I am negative."));
        let first = module
            .generate_llm_response(history.as_slice())
            .await
            .expect("first reply");
        history.push(module.create_assistant_message(&first));

        // Turn 2: peer argument arrives.
        history.push(module.create_user_message("State your main argument."));
        module
            .generate_llm_response(history.as_slice())
            .await
            .expect("second reply");

        let requests = captured.lock().clone();
        assert_eq!(requests.len(), 2, "one HTTP request per turn");

        let first_request = &requests[0];
        assert_eq!(count_messages(first_request, "system", SYSTEM_PROMPT), 1);
        assert_eq!(
            count_messages(first_request, "user", "Hi, I am negative."),
            1
        );
        let second_request = &requests[1];
        assert_eq!(count_messages(second_request, "system", SYSTEM_PROMPT), 1);
        assert_eq!(
            count_messages(second_request, "user", "Hi, I am negative."),
            1
        );
        assert_eq!(
            count_messages(second_request, "assistant", "first rebuttal"),
            1
        );
        assert_eq!(
            count_messages(second_request, "user", "State your main argument."),
            1
        );
    }
}
