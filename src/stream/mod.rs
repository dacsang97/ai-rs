pub mod handler;
pub mod sse;

use serde::{Deserialize, Serialize};

use crate::types::TokenUsage;

/// Stream events emitted during an AI chat session.
/// This is the public contract consumed by the frontend — do not change tag names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StreamEvent {
    TextStart { part_id: String },
    TextDelta { part_id: String, delta: String },
    TextEnd { part_id: String },
    ReasoningStart { part_id: String },
    ReasoningDelta { part_id: String, delta: String },
    ReasoningEnd { part_id: String },
    ToolPending { call_id: String, tool_name: String },
    ToolInputDelta { call_id: String, delta: String },
    ToolRunning { call_id: String, tool_name: Option<String> },
    ToolCompleted { call_id: String, output: String, title: Option<String> },
    ToolError { call_id: String, error: String },
    ToolApprovalRequired { call_id: String, tool_name: String, arguments: String },
    ToolDenied { call_id: String, error: String },
    StepFinish { tokens: TokenUsage, cost: f64, reason: String },
    RunComplete,
    RunError { error: String },
    RunAborted,
}
