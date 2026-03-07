pub mod openai;
pub mod openai_compat;

use std::collections::HashMap;
use std::pin::Pin;

use async_trait::async_trait;
use futures::stream::Stream;

use crate::message::Message;
use crate::stream::handler::StreamChunk;
use crate::tool::ToolDef;
use crate::types::{StopReason, TokenUsage};
use crate::Result;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDef>>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop: Option<Vec<String>>,
    pub headers: Option<HashMap<String, String>>,
    pub reasoning_effort: Option<String>,
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
