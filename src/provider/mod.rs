pub mod google;
pub mod openai;
pub mod openai_compat;
pub mod transform;

use std::collections::HashMap;
use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::message::Message;
use crate::stream::handler::StreamChunk;
use crate::tool::ToolDef;
use crate::types::{StopReason, TokenUsage};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Tool(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportMode {
    Auto,
    Http,
    Sse,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDef>>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub reasoning_effort: Option<String>,
    pub session_id: Option<String>,
    pub provider_options: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    pub tool_choice: Option<ToolChoice>,
    pub active_tools: Option<Vec<String>>,
    pub transport: Option<TransportMode>,
    pub max_retries: Option<u32>,
}

impl ChatRequest {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            messages,
            tools: None,
            temperature: None,
            max_tokens: None,
            stop: None,
            headers: None,
            reasoning_effort: None,
            session_id: None,
            provider_options: None,
            metadata: None,
            tool_choice: None,
            active_tools: None,
            transport: None,
            max_retries: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<crate::message::ToolCallInfo>,
    pub usage: TokenUsage,
    pub stop_reason: Option<StopReason>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;
    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>>;
}
