# Conclave - AI Agent Swarm

Conclave is a distributed system of autonomous AI agents that communicate with each other using UDP multicast. Each agent operates independently with a pluggable LLM backend (OpenAI, Anthropic, Google, or local models via Ollama) and a configurable personality system.

## Overview

This project allows you to create a swarm of AI agents that can collaborate on tasks. The agents communicate in a decentralized manner, with each agent broadcasting messages to the group and responding to messages from others. This enables complex, emergent behaviors and decentralized problem-solving.

## Features

- **Decentralized Communication:** Agents communicate via UDP multicast, eliminating the need for a central server.
- **Pluggable LLM Backends:** Easily switch between different LLM providers, including OpenAI, Anthropic, Google, and local models.
- **Configurable Agents:** Customize each agent's ID, personality, and LLM model.
- **Resilient Networking:** The system is designed to be resilient to network errors and agent failures.
- **Concurrent Processing:** Agents can process messages and generate responses concurrently, enabling real-time interaction.

## Getting Started

### Prerequisites

- Rust (latest stable version)
- An API key for your chosen LLM provider (e.g., OpenAI, Anthropic, Google)

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
| Verbose | `-v` | `--verbose` | Enable verbose logging | |
| Log Level | | `--log-level` | Set the log level | `info` |
| Personality | `-p` | `--personality` | Agent personality for the system prompt | `You are a helpful AI agent...` |

### Environment Variables

You can also provide API keys via environment variables:

-   `OPENAI_API_KEY`
-   `ANTHROPIC_API_KEY`
-   `GEMINI_API_KEY`

## Supported LLM Backends

-   **OpenAI:** `openai`
-   **Anthropic:** `anthropic`
-   **Google:** `google`
-   **Local (Ollama):** `local`

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