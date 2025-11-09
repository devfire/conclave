# Conclave - AI Agent Swarm

Conclave is a distributed system of autonomous AI agents that communicate with each other using UDP multicast. Each agent operates independently with a pluggable LLM backend (OpenAI, Anthropic, Google, OpenRouter, or local models via Ollama) and a configurable personality system that supports both inline prompts and file-based personalities.

## Overview

This project allows you to create a swarm of AI agents that can collaborate on tasks. The agents communicate in a decentralized manner, with each agent broadcasting messages to the group and responding to messages from others. This enables complex, emergent behaviors and decentralized problem-solving. Agents can also be configured for voice responses using ElevenLabs integration and support structured debate scenarios.

## Features

- **Decentralized Communication:** Agents communicate via UDP multicast, eliminating the need for a central server.
- **Pluggable LLM Backends:** Easily switch between different LLM providers, including OpenAI, Anthropic, Google, OpenRouter, and local models.
- **Voice Integration:** Optional ElevenLabs text-to-speech for voice responses.
- **Configurable Agents:** Customize each agent's ID, personality (inline or file-based), and LLM model.
- **Debate System:** Built-in support for structured Public Forum debates with predefined personality files for affirmative, negative, and judge roles.
- **Resilient Networking:** The system is designed to be resilient to network errors and agent failures with retry logic.
- **Concurrent Processing:** Agents can process messages and generate responses concurrently, enabling real-time interaction.
- **Memory Management:** Sliding window strategy for conversation context management.

## Getting Started

### Prerequisites

- Rust (latest stable version)
- An API key for your chosen LLM provider (e.g., OpenAI, Anthropic, Google, OpenRouter)
- Optional: ElevenLabs API key for voice responses
- Optional: ALSA development libraries for audio playback (Linux)

### Installation

1.  Clone the repository:
    ```sh
    git clone https://github.com/your-username/conclave.git
    cd conclave
    ```

2.  Build the project:
    ```sh
    cargo build --release
    ```

## Usage

To run an agent, you need to provide a unique agent ID and specify the LLM backend and model to use.

### Basic Example

Run a single agent using the OpenAI backend:

```sh
cargo run --release -- \
    --agent-id agent-1 \
    --llm-backend openai \
    --model gpt-4 \
    --api-key YOUR_OPENAI_API_KEY
```

### Voice-Enabled Agent

Run an agent with ElevenLabs voice responses:

```sh
ELEVENLABS_API_KEY=your_elevenlabs_key cargo run --release -- \
    --agent-id agent-1 \
    --llm-backend openai \
    --model gpt-4 \
    --api-key YOUR_OPENAI_API_KEY \
    --voice
```

### Debate Agent

Run an agent configured for Public Forum debate (affirmative side):

```sh
cargo run --release -- \
    --agent-id debater-1 \
    --llm-backend openai \
    --model gpt-4 \
    --api-key YOUR_OPENAI_API_KEY \
    --personality-file src/affirmative.md
```

### Running Multiple Agents

To create a swarm, run multiple agents in separate terminal windows. Each agent must have a unique ID.

**Terminal 1:**

```sh
cargo run --release -- \
    --agent-id agent-1 \
    --llm-backend openai \
    --model gpt-4 \
    --api-key YOUR_OPENAI_API_KEY
```

**Terminal 2:**

```sh
cargo run --release -- \
    --agent-id agent-2 \
    --llm-backend anthropic \
    --model claude-3-sonnet-20240229 \
    --api-key YOUR_ANTHROPIC_API_KEY
```

## Configuration

You can configure the agents using the following command-line arguments:

| Argument | Short | Long | Description | Default |
| --- | --- | --- | --- | --- |
| Agent ID | `-i` | `--agent-id` | Unique identifier for this agent | |
| Multicast Address | `-a` | `--multicast-address` | UDP multicast address for communication | `239.255.255.250:8080` |
| Network Interface | | `--interface` | Network interface to bind to | |
| LLM Backend | `-b` | `--llm-backend` | LLM backend to use | `openai` |
| Model | `-m` | `--model` | Specific LLM model to use | `gpt-3.5-turbo` |
| API Key | `-k` | `--api-key` | API key for the LLM backend | |
| Endpoint | | `--endpoint` | Custom API endpoint URL | |
| Timeout | | `--timeout` | Request timeout in seconds | `30` |
| Max Retries | | `--max-retries` | Maximum retry attempts for failed requests | `3` |
| Log Level | | `--log-level` | Set the log level | `info` |
| Personality | `-p` | `--personality` | Agent personality for the system prompt | `You are a helpful AI agent...` |
| Personality File | | `--personality-file` | Read personality from file (mutually exclusive with --personality) | |
| Processing Delay | | `--processing-delay` | Processing delay in milliseconds for simulation | `0` |
| Voice | | `--voice` | Enable ElevenLabs voice responses | `false` |

### Environment Variables

You can also provide API keys via environment variables:

-   `OPENAI_API_KEY`
-   `ANTHROPIC_API_KEY`
-   `GEMINI_API_KEY`
-   `OPENROUTER_API_KEY`
-   `ELEVENLABS_API_KEY`

## Supported LLM Backends

-   **OpenAI:** `openai`
-   **Anthropic:** `anthropic`
-   **Google:** `google`
-   **OpenRouter:** `openrouter`
-   **Local (Ollama):** `local`

## Debate System

Conclave includes built-in support for structured Public Forum debates. Use the provided personality files:

- `src/affirmative.md` - For affirmative debaters
- `src/neg.md` - For negative debaters
- `src/debate_judge_prompt.md` - For debate judges

Example debate setup:

```sh
# Affirmative debater
cargo run --release -- \
    --agent-id affirmative \
    --llm-backend openai \
    --model gpt-4 \
    --api-key YOUR_OPENAI_API_KEY \
    --personality-file src/affirmative.md

# Negative debater
cargo run --release -- \
    --agent-id negative \
    --llm-backend openai \
    --model gpt-4 \
    --api-key YOUR_OPENAI_API_KEY \
    --personality-file src/neg.md

# Judge
cargo run --release -- \
    --agent-id judge \
    --llm-backend openai \
    --model gpt-4 \
    --api-key YOUR_OPENAI_API_KEY \
    --personality-file src/debate_judge_prompt.md
```

## Development

### Running Tests

To run the test suite, use the following command:

```sh
cargo test
```

### Building the Protocol Buffers

The project uses Protocol Buffers for message serialization. If you modify the `.proto` files, you'll need to rebuild the generated Rust code:

```sh
cargo build
```

## License

This project is licensed under the MIT License. See the `LICENSE` file for details.