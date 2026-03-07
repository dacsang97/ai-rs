use serde::Deserialize;

use crate::types::{StopReason, TokenUsage};

#[derive(Debug, Clone)]
pub enum StreamChunk {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
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
}

impl ToolCallAccumulator {
    pub fn new(index: usize, id: String, name: String) -> Self {
        Self {
            index,
            id,
            name,
            arguments: String::new(),
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

#[derive(Debug, Deserialize)]
pub(crate) struct ChunkUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
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
                usage: chunk.usage.as_ref().map(|u| TokenUsage::new(
                    u.prompt_tokens,
                    u.completion_tokens,
                )),
            });
        }
    }

    // Handle usage-only chunks (no choices but has usage)
    if chunk.choices.is_empty() {
        if let Some(ref u) = chunk.usage {
            results.push(StreamChunk::Done {
                stop_reason: None,
                usage: Some(TokenUsage::new(u.prompt_tokens, u.completion_tokens)),
            });
        }
    }

    Ok(results)
}
