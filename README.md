# Conclave - AI Agent Swarm

Conclave is a distributed system of autonomous AI agents that communicate with each other using UDP multicast. Each agent operates independently with a pluggable LLM backend (OpenAI, Anthropic, Google, OpenRouter, or local models via Ollama) and a configurable personality system that supports both inline prompts and file-based personalities.

## Features

- **Decentralized Communication:** Agents communicate via UDP multicast, eliminating the need for a central server.
- **Pluggable LLM Backends:** Easily switch between different LLM providers, including OpenAI, Anthropic, Google, OpenRouter, and local models.
- **Voice Integration:** Optional text-to-speech (TTS) for voice responses via ElevenLabs or Deepgram Flux TTS (streaming WebSocket), each agent speaking with its own configurable voice.
- **Docker Support:** Run agents in containers without local Rust installation.
- **Configurable Agents:** Customize each agent's ID, personality (inline or file-based), and LLM model.
- **Debate System:** Built-in support for structured Public Forum debates with predefined personality files for affirmative, negative, and judge roles.
- **Resilient Networking:** The system is designed to be resilient to network errors and agent failures with retry logic.
- **Concurrent Processing:** Agents can process messages and generate responses concurrently, enabling real-time interaction.
- **Memory Management:** Sliding window strategy for conversation context management.

## Usage

To run an agent, you need to provide a unique agent ID and specify the LLM backend and model to use.

### Simultaneous Voice Debate
Give each debating agent its own `--tts-voice` so listeners can tell speakers apart. Flux voices are Deepgram model strings like `flux-hannah-en` (see the [Flux voice catalog](https://developers.deepgram.com/docs/flux-tts/voices)):

```sh
# Terminal 1 — affirmative, American female
target/release/conclave --personality-file src/personalities/negative.md \
                        --agent-id negative \
                        --tts deepgram \
                        --tts-voice flux-hannah-en \
                        --llm-backend openrouter \
                        --model google/gemini-3.7-flash
```

```sh
# Terminal 2 — negative, British male
target/release/conclave --personality-file src/personalities/affirmative.md \
                        --agent-id affirmative \
                        --tts deepgram \
                        --tts-voice flux-colin-en \
                        --llm-backend openrouter \
                        --model google/gemini-3.7-flash
```

## Configuration

### Environment Variables

You can provide API keys via environment variables:

-   `OPENAI_API_KEY`
-   `ANTHROPIC_API_KEY`
-   `GEMINI_API_KEY`
-   `OPENROUTER_API_KEY`
-   `ELEVENLABS_API_KEY`
-   `DEEPGRAM_API_KEY`

#### `.env` File

Instead of exporting variables in every shell, put them in a `.env` file in the project root (or a parent directory):

```sh
OPENAI_API_KEY=your_openai_key
DEEPGRAM_API_KEY=your_deepgram_key
```

The file is loaded at startup via [dotenvy](https://crates.io/crates/dotenvy). Real environment variables take precedence over `.env` values, and a missing file is not an error. `.env` is gitignored, so keys stay out of version control.

## Supported LLM Backends

-   **OpenAI:** `openai`
-   **Anthropic:** `anthropic`
-   **Google:** `google`
-   **OpenRouter:** `openrouter`
-   **Local (Ollama):** `local`
-   **Openrouter** `openai`
    -   NOTE: use this command for openrouter: `cargo run --release -- --agent-id agent_1 --llm-backend openai --api-key $OPENROUTER_API_KEY --model model_id --endpoint https://openrouter.ai/api/v1`

## Docker Support

Conclave includes Docker support for easy deployment and containerized execution. You can run agents using Docker without needing to install Rust or other dependencies locally.

### Building the Docker Image

Build the Docker image from the project root:

```sh
docker build -t conclave .
```

### Running with Docker

Run multiple agents in separate containers:

```sh
# Terminal 1
docker run --rm \
    -e OPENAI_API_KEY=your_openai_key \
    conclave \
    --agent-id agent-1 \
    --llm-backend openai \
    --model {INSERT_MODEL_NAME}

# Terminal 2
docker run --rm \
    -e ANTHROPIC_API_KEY=your_anthropic_key \
    conclave \
    --agent-id agent-2 \
    --llm-backend anthropic \
    --model {INSERT_MODEL_NAME}
```

For voice-enabled agents with Docker:

```sh
docker run --rm \
    -e OPENAI_API_KEY=your_openai_key \
    -e DEEPGRAM_API_KEY=your_deepgram_key \
    --device /dev/snd \
    conclave \
    --agent-id agent-1 \
    --llm-backend openai \
    --model google/gemini-3.7-flash\
    --tts deepgram
```

## Development

### Building the Protocol Buffers

The project uses Protocol Buffers for message serialization. If you modify the `.proto` files, you'll need to rebuild the generated Rust code:

```sh
cargo build
```

## License

This project is licensed under the MIT License. See the `LICENSE` file for details.
