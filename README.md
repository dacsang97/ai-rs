# ai-rs

A focused Rust AI SDK for building AI-powered applications with OpenAI-compatible providers.

## Features

- **Streaming**: Real-time text and reasoning streaming via SSE
- **Tool System**: Async tool execution with built-in tools (bash, read_file, write_file, edit_file, glob, grep)
- **Agentic Loop**: Multi-step tool calling with automatic continuation
- **MCP Client**: Model Context Protocol support (stdio + SSE transports)
- **Skills**: SKILL.md discovery and loading system
- **Session Management**: Abort signaling via watch channels

## Quick Start

```rust
use ai_rs::{Message, ChatRequest, ToolRegistry, AgentConfig, run_agent_loop};
use ai_rs::provider::openai_compat::OpenAiCompatibleProvider;
use ai_rs::tool::builtin::BuiltinTools;

// Create provider
let provider = OpenAiCompatibleProvider::new(
    "https://api.openai.com/v1".to_string(),
    "sk-...".to_string(),
    "gpt-4".to_string(),
);

// Set up tools
let mut tools = ToolRegistry::new();
tools.register(Box::new(BuiltinTools::new("/workspace".to_string())));

// Run agentic loop
let (tx, mut rx) = tokio::sync::mpsc::channel(256);
let (abort_tx, mut abort_rx) = tokio::sync::watch::channel(false);
let mut messages = vec![Message::user("Hello!")];
let config = AgentConfig::default();

run_agent_loop(&provider, &mut messages, &tools, &config, &mut abort_rx, tx).await?;
```

## Architecture

```
ai-rs/src/
├── lib.rs           # Public API re-exports
├── error.rs         # AiError enum
├── types.rs         # TokenUsage, Role, StopReason
├── message.rs       # Message enum (system, user, assistant, tool)
├── client.rs        # HTTP client wrapper
├── provider/        # Provider trait + OpenAI-compatible implementation
├── stream/          # StreamEvent enum + SSE parsing + chunk handler
├── tool/            # Tool system (ToolRegistry, ToolExecutor, built-ins, MCP)
├── mcp/             # MCP client (stdio + SSE transports)
├── session/         # Session management + agentic loop
└── skill/           # SKILL.md discovery and frontmatter parsing
```

## StreamEvent Contract

The `StreamEvent` enum uses **kebab-case** tags (`#[serde(tag = "type", rename_all = "kebab-case")]`) and is the public contract consumed by the frontend. Available events:

| Event | Key Fields |
|-------|-----------|
| `text-start` | `part_id` |
| `text-delta` | `part_id`, `delta` |
| `text-end` | `part_id` |
| `reasoning-start` | `part_id` |
| `reasoning-delta` | `part_id`, `delta` |
| `reasoning-end` | `part_id` |
| `tool-pending` | `call_id`, `tool_name` |
| `tool-input-delta` | `call_id`, `delta` |
| `tool-running` | `call_id`, `tool_name` |
| `tool-completed` | `call_id`, `output`, `title` |
| `tool-error` | `call_id`, `error` |
| `step-finish` | `tokens`, `cost`, `reason` |
| `run-complete` | _(none)_ |
| `run-error` | `error` |
| `run-aborted` | _(none)_ |

## Testing

```bash
cargo test -p ai-rs
```
