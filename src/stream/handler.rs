use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::types::{StopReason, TokenUsage};

#[derive(Debug, Clone)]
pub enum StreamChunk {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
        /// Gemini 3 thought signature — must be echoed back.
        thought_signature: Option<String>,
    },
    ToolCallDelta {
        index: usize,
        arguments: String,
    },
    Done {
        stop_reason: Option<StopReason>,
        usage: Option<TokenUsage>,
    },
}

#[derive(Debug, Clone)]
pub struct ToolCallAccumulator {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub thought_signature: Option<String>,
}

impl ToolCallAccumulator {
    pub fn new(index: usize, id: String, name: String) -> Self {
        Self {
            index,
            id,
            name,
            arguments: String::new(),
            thought_signature: None,
        }
    }

    pub fn append_arguments(&mut self, delta: &str) {
        self.arguments.push_str(delta);
    }
}

// --- OpenAI streaming response structures ---

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionChunk {
    #[serde(default)]
    pub choices: Vec<ChunkChoice>,
    #[serde(default)]
    pub usage: Option<ChunkUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChunkChoice {
    #[serde(default)]
    pub delta: ChunkDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ChunkDelta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ChunkToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChunkToolCall {
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<ChunkFunction>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChunkFunction {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ChunkUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<ChunkPromptTokenDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<ChunkCompletionTokenDetails>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct ChunkPromptTokenDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct ChunkCompletionTokenDetails {
    #[serde(default)]
    pub reasoning_tokens: u64,
}

fn parse_stop_reason(reason: &str) -> Option<StopReason> {
    match reason {
        "stop" => Some(StopReason::Stop),
        "length" => Some(StopReason::Length),
        "tool_calls" => Some(StopReason::ToolCalls),
        "content_filter" => Some(StopReason::ContentFilter),
        _ => None,
    }
}

pub fn parse_chunk(raw: &str) -> crate::Result<Vec<StreamChunk>> {
    let chunk: ChatCompletionChunk = serde_json::from_str(raw)?;
    let mut results = Vec::new();

    for choice in &chunk.choices {
        if let Some(ref text) = choice.delta.content {
            if !text.is_empty() {
                results.push(StreamChunk::TextDelta(text.clone()));
            }
        }

        if let Some(ref reasoning) = choice.delta.reasoning_content {
            if !reasoning.is_empty() {
                results.push(StreamChunk::ReasoningDelta(reasoning.clone()));
            }
        }

        if let Some(ref tool_calls) = choice.delta.tool_calls {
            for tc in tool_calls {
                let has_id = tc.id.is_some();
                let has_name = tc
                    .function
                    .as_ref()
                    .is_some_and(|f| f.name.is_some());

                if has_id || has_name {
                    results.push(StreamChunk::ToolCallStart {
                        index: tc.index,
                        id: tc.id.clone().unwrap_or_default(),
                        name: tc
                            .function
                            .as_ref()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_default(),
                        thought_signature: None,
                    });
                }

                if let Some(ref func) = tc.function {
                    if let Some(ref args) = func.arguments {
                        if !args.is_empty() {
                            results.push(StreamChunk::ToolCallDelta {
                                index: tc.index,
                                arguments: args.clone(),
                            });
                        }
                    }
                }
            }
        }

        if let Some(ref reason) = choice.finish_reason {
            results.push(StreamChunk::Done {
                stop_reason: parse_stop_reason(reason),
                usage: chunk.usage.as_ref().map(|u| {
                    let cache_read = u
                        .prompt_tokens_details
                        .as_ref()
                        .map_or(0, |d| d.cached_tokens);
                    let reasoning = u
                        .completion_tokens_details
                        .as_ref()
                        .map_or(0, |d| d.reasoning_tokens);

                    TokenUsage::with_details(
                        u.prompt_tokens,
                        u.completion_tokens,
                        u.prompt_tokens.saturating_sub(cache_read),
                        u.completion_tokens.saturating_sub(reasoning),
                        reasoning,
                        cache_read,
                        0,
                    )
                    .with_metadata(
                        serde_json::to_value(u).ok(),
                        Some(json!({
                            "provider": "openai",
                            "cached_input_tokens": cache_read,
                            "reasoning_tokens": reasoning,
                        })),
                    )
                }),
            });
        }
    }

    // Handle usage-only chunks (no choices but has usage)
    if chunk.choices.is_empty() {
        if let Some(ref u) = chunk.usage {
            results.push(StreamChunk::Done {
                stop_reason: None,
                usage: Some(
                    TokenUsage::with_details(
                        u.prompt_tokens,
                        u.completion_tokens,
                        u.prompt_tokens.saturating_sub(
                            u.prompt_tokens_details
                                .as_ref()
                                .map_or(0, |d| d.cached_tokens),
                        ),
                        u.completion_tokens.saturating_sub(
                            u.completion_tokens_details
                                .as_ref()
                                .map_or(0, |d| d.reasoning_tokens),
                        ),
                        u.completion_tokens_details
                            .as_ref()
                            .map_or(0, |d| d.reasoning_tokens),
                        u.prompt_tokens_details
                            .as_ref()
                            .map_or(0, |d| d.cached_tokens),
                        0,
                    )
                    .with_metadata(
                        serde_json::to_value(u).ok(),
                        Some(json!({
                            "provider": "openai",
                            "cached_input_tokens": u.prompt_tokens_details
                                .as_ref()
                                .map_or(0, |d| d.cached_tokens),
                            "reasoning_tokens": u.completion_tokens_details
                                .as_ref()
                                .map_or(0, |d| d.reasoning_tokens),
                        })),
                    ),
                ),
            });
        }
    }

    Ok(results)
}
