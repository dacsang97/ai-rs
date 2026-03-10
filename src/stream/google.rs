use serde::Deserialize;
use uuid::Uuid;

use crate::stream::handler::StreamChunk;
use crate::types::{StopReason, TokenUsage};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleStreamResponse {
    #[serde(default)]
    candidates: Vec<GoogleCandidate>,
    #[serde(default)]
    usage_metadata: Option<GoogleUsageMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleCandidate {
    #[serde(default)]
    content: Option<GoogleContent>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleContent {
    #[serde(default)]
    parts: Vec<GooglePart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GooglePart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thought: Option<bool>,
    #[serde(default)]
    function_call: Option<GoogleFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct GoogleFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleUsageMetadata {
    #[serde(default)]
    prompt_token_count: u64,
    #[serde(default)]
    candidates_token_count: u64,
    #[serde(default)]
    thoughts_token_count: u64,
}

fn parse_finish_reason(reason: &str) -> Option<StopReason> {
    match reason {
        "STOP" => Some(StopReason::Stop),
        "MAX_TOKENS" => Some(StopReason::Length),
        "SAFETY" | "RECITATION" => Some(StopReason::ContentFilter),
        _ => None,
    }
}

pub fn parse_google_chunk(raw: &str) -> crate::Result<Vec<StreamChunk>> {
    let resp: GoogleStreamResponse = serde_json::from_str(raw)?;
    let mut results = Vec::new();

    for candidate in &resp.candidates {
        if let Some(ref content) = candidate.content {
            let mut tool_index = 0usize;
            for part in &content.parts {
                if let Some(ref fc) = part.function_call {
                    let id = Uuid::new_v4().to_string();
                    let args_str = serde_json::to_string(&fc.args).unwrap_or_default();
                    results.push(StreamChunk::ToolCallStart {
                        index: tool_index,
                        id,
                        name: fc.name.clone(),
                    });
                    results.push(StreamChunk::ToolCallDelta {
                        index: tool_index,
                        arguments: args_str,
                    });
                    tool_index += 1;
                } else if let Some(ref text) = part.text {
                    if !text.is_empty() {
                        if part.thought == Some(true) {
                            results.push(StreamChunk::ReasoningDelta(text.clone()));
                        } else {
                            results.push(StreamChunk::TextDelta(text.clone()));
                        }
                    }
                }
            }
        }

        if let Some(ref reason) = candidate.finish_reason {
            let stop_reason = parse_finish_reason(reason);
            let usage = resp.usage_metadata.as_ref().map(|u| {
                TokenUsage::new(
                    u.prompt_token_count,
                    u.candidates_token_count + u.thoughts_token_count,
                )
            });
            results.push(StreamChunk::Done { stop_reason, usage });
        }
    }

    // Handle usage-only chunks (no candidates)
    if resp.candidates.is_empty() {
        if let Some(ref u) = resp.usage_metadata {
            results.push(StreamChunk::Done {
                stop_reason: None,
                usage: Some(TokenUsage::new(
                    u.prompt_token_count,
                    u.candidates_token_count + u.thoughts_token_count,
                )),
            });
        }
    }

    Ok(results)
}
