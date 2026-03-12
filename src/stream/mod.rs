pub mod google;
pub mod handler;
pub mod sse;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::TokenUsage;

/// Stream events emitted during an AI chat session.
/// Legacy variants are kept stable for existing consumers. New orchestration-aware
/// variants are additive so downstream adapters can opt in incrementally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePart {
    pub id: String,
    pub source_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPart {
    pub id: String,
    pub data_type: String,
    pub data: Value,
    #[serde(default)]
    pub transient: bool,
}

/// Stream events emitted during an AI chat session.
/// This is the public contract consumed by the frontend — do not change tag names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StreamEvent {
    RunStart {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    StepStart {
        run_id: String,
        step_id: String,
        step: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    TextStart { part_id: String },
    TextDelta { part_id: String, delta: String },
    TextEnd { part_id: String },
    ReasoningStart { part_id: String },
    ReasoningDelta { part_id: String, delta: String },
    ReasoningEnd { part_id: String },
    PartMetadata {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_id: Option<String>,
        part_id: String,
        part_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    ToolPending { call_id: String, tool_name: String },
    ToolInputDelta { call_id: String, delta: String },
    ToolRunning { call_id: String, tool_name: Option<String> },
    ToolCompleted { call_id: String, output: String, title: Option<String> },
    ToolError { call_id: String, error: String },
    ToolCallMetadata {
        run_id: String,
        step_id: String,
        call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    ToolApprovalRequired { call_id: String, tool_name: String, arguments: String },
    ToolDenied { call_id: String, error: String },
    Source {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_id: Option<String>,
        source: SourcePart,
    },
    Data {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_id: Option<String>,
        part: DataPart,
    },
    StepFinish { tokens: TokenUsage, cost: f64, reason: String },
    ContextPrune { pruned: u32, freed: u64 },
    MaxStepsWarning { step: u32, max_steps: u32 },
    RunComplete,
    RunError { error: String },
    RunAborted,
}
