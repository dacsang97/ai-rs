use std::pin::Pin;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;

use crate::client::HttpClient;
use crate::message::{ContentPart, Message, ToolCallInfo, UserContent};
use crate::stream::handler::{self, StreamChunk};
use crate::types::{StopReason, TokenUsage};
use crate::{AiError, Result};

use super::{ChatRequest, ChatResponse, Provider, ToolDef};

const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAiProvider {
    client: HttpClient,
    model: String,
}

impl OpenAiProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: HttpClient::new(OPENAI_BASE_URL, api_key),
            model: model.into(),
        }
    }

    pub(crate) fn with_base_url(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: HttpClient::new(base_url, api_key),
            model: model.into(),
        }
    }

    pub(crate) fn with_base_url_and_headers(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        headers: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            client: HttpClient::new(base_url, api_key).with_default_headers(headers),
            model: model.into(),
        }
    }

    fn build_request_body(&self, request: &ChatRequest, stream: bool) -> serde_json::Value {
        let mut body = json!({
            "model": self.model,
            "messages": request.messages.iter().map(message_to_openai).collect::<Vec<_>>(),
        });

        if stream {
            body["stream"] = json!(true);
            body["stream_options"] = json!({"include_usage": true});
        }

        if let Some(ref tools) = request.tools {
            if !tools.is_empty() {
                body["tools"] = json!(tools.iter().map(tool_to_openai).collect::<Vec<_>>());
            }
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }

        if let Some(max) = request.max_tokens {
            body["max_tokens"] = json!(max);
        }

        if let Some(ref stop) = request.stop {
            body["stop"] = json!(stop);
        }

        if let Some(ref effort) = request.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }

        body
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let body = self.build_request_body(&request, false);
        let resp: OpenAiChatResponse = self
            .client
            .post_json("/chat/completions", &body, request.headers.as_ref())
            .await?;
        parse_chat_response(resp)
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        let body = self.build_request_body(&request, true);

        let url = format!("{}/chat/completions", self.client.base_url());
        log::info!("[chat_stream] POST {} model={}", url, self.model);
        log::debug!("[chat_stream] body: {}", serde_json::to_string_pretty(&body).unwrap_or_default());

        // Pre-flight: send request manually to capture error body on non-2xx
        let mut req_builder = reqwest::Client::new()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.client.api_key()))
            .header("Content-Type", "application/json");

        for (k, v) in self.client.default_headers() {
            req_builder = req_builder.header(k.as_str(), v.as_str());
        }

        if let Some(ref extra) = request.headers {
            for (k, v) in extra {
                req_builder = req_builder.header(k.as_str(), v.as_str());
            }
        }

        req_builder = req_builder.json(&body);

        let response = req_builder.send().await?;
        let status = response.status();
        log::info!("[chat_stream] response status: {}", status);

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            log::error!("[chat_stream] error response ({}): {}", status, error_body);
            return Err(AiError::Api {
                status: status.as_u16(),
                message: error_body,
            });
        }

        // Build SSE stream from the successful response bytes using eventsource_stream
        let raw_stream = response.bytes_stream().eventsource().filter_map(|result| async move {
            match result {
                Ok(event) => {
                    let data = event.data.trim().to_string();
                    if data.is_empty() || data.starts_with(':') || data == "[DONE]" {
                        None
                    } else {
                        Some(Ok(data))
                    }
                }
                Err(e) => Some(Err(AiError::Stream(e.to_string()))),
            }
        });

        let chunk_stream = raw_stream.flat_map(|result| {
            let chunks: Vec<Result<StreamChunk>> = match result {
                Ok(data) => match handler::parse_chunk(&data) {
                    Ok(parsed) => parsed.into_iter().map(Ok).collect(),
                    Err(e) => vec![Err(e)],
                },
                Err(e) => vec![Err(e)],
            };
            futures::stream::iter(chunks)
        });

        Ok(Box::pin(chunk_stream))
    }
}

// --- Message conversion ---

fn message_to_openai(msg: &Message) -> serde_json::Value {
    match msg {
        Message::System { content } => json!({
            "role": "system",
            "content": content,
        }),
        Message::Developer { content } => json!({
            "role": "developer",
            "content": content,
        }),
        Message::User { content } => match content {
            UserContent::Text(text) => json!({
                "role": "user",
                "content": text,
            }),
            UserContent::Parts(parts) => json!({
                "role": "user",
                "content": parts.iter().map(|p| match p {
                    ContentPart::Text { text } => json!({ "type": "text", "text": text }),
                    ContentPart::ImageUrl { image_url } => json!({
                        "type": "image_url",
                        "image_url": { "url": &image_url.url }
                    }),
                }).collect::<Vec<_>>()
            }),
        },
        Message::Assistant {
            content,
            reasoning: _,
            tool_calls,
        } => {
            let mut msg = json!({"role": "assistant"});
            if let Some(c) = content {
                msg["content"] = json!(c);
            }
            if let Some(tcs) = tool_calls {
                msg["tool_calls"] = json!(tcs
                    .iter()
                    .map(|tc| json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments,
                        }
                    }))
                    .collect::<Vec<_>>());
            }
            msg
        }
        Message::Tool {
            tool_call_id,
            content,
        } => json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": content,
        }),
    }
}

fn tool_to_openai(tool: &ToolDef) -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}

// --- Non-streaming response types ---

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    id: String,
    function: OpenAiFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    #[allow(dead_code)]
    total_tokens: u64,
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

fn parse_chat_response(resp: OpenAiChatResponse) -> Result<ChatResponse> {
    let choice = resp
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| AiError::Api {
            status: 0,
            message: "no choices in response".to_string(),
        })?;

    let tool_calls = choice
        .message
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|tc| ToolCallInfo {
            id: tc.id,
            name: tc.function.name,
            arguments: tc.function.arguments,
        })
        .collect();

    let usage = resp
        .usage
        .map(|u| TokenUsage::new(u.prompt_tokens, u.completion_tokens))
        .unwrap_or_default();

    let stop_reason = choice
        .finish_reason
        .as_deref()
        .and_then(parse_stop_reason);

    Ok(ChatResponse {
        content: choice.message.content,
        reasoning: choice.message.reasoning_content,
        tool_calls,
        usage,
        stop_reason,
    })
}
