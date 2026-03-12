pub mod client;
pub mod error;
pub mod mcp;
pub mod message;
pub mod provider;
pub mod session;
pub mod skill;
pub mod stream;
pub mod tool;
pub mod types;

pub use client::HttpClient;
pub use error::{AiError, Result};
pub use message::{
    ContentPart, ImageUrl, Message, MessageEnvelope, MessageMetadata, ToolCallInfo, UserContent,
};
pub use provider::{ChatRequest, ChatResponse, Provider, ToolChoice, TransportMode};
pub use session::agent::{
    AgentConfig, LoopState, PrepareStepHook, StepContext, StepPreparation, StopWhenHook,
    run_agent_loop,
};
pub use session::approval::{ApprovalRequest, ApprovalResponse};
pub use session::SessionManager;
pub use stream::handler::StreamChunk;
pub use stream::{DataPart, SourcePart, StreamEvent};
pub use tool::{ToolDef, ToolExecutor, ToolRegistry, ToolResult};
pub use types::{Role, StopReason, TokenUsage};
