use std::pin::Pin;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::message::{ContentPart, Message, ToolCallInfo, UserContent};
use crate::stream::google as google_stream;
use crate::stream::handler::StreamChunk;
use crate::types::{StopReason, TokenUsage};
use crate::{AiError, Result};

use super::{ChatRequest, ChatResponse, Provider, ToolDef};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GoogleProvider {
    api_key: String,
    model: String,
    base_url: String,
    thinking_config: Option<ThinkingConfig>,
}

#[derive(Debug, Clone)]
pub struct ThinkingConfig {
    pub thinking_budget: Option<u32>,
    pub include_thoughts: bool,
}

impl GoogleProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            thinking_config: None,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_thinking(mut self, config: ThinkingConfig) -> Self {
        self.thinking_config = Some(config);
        self
    }

    fn build_request_body(&self, request: &ChatRequest) -> serde_json::Value {
        let (contents, system_instruction) = messages_to_google(&request.messages);
        let filtered_tools = request.tools.as_ref().map(|tools| {
            if let Some(active) = &request.active_tools {
                tools.iter()
                    .filter(|tool| active.iter().any(|name| name == &tool.name))
                    .collect::<Vec<_>>()
            } else {
                tools.iter().collect::<Vec<_>>()
            }
        });

        let mut body = json!({ "contents": contents });

        if let Some(si) = system_instruction {
            body["systemInstruction"] = si;
        }

        // generationConfig
        let mut gen_config = serde_json::Map::new();
        if let Some(temp) = request.temperature {
            gen_config.insert("temperature".into(), json!(temp));
        }
        if let Some(max) = request.max_tokens {
            gen_config.insert("maxOutputTokens".into(), json!(max));
        }
        if let Some(ref tc) = self.thinking_config {
            let mut thinking = serde_json::Map::new();
            if let Some(budget) = tc.thinking_budget {
                thinking.insert("thinkingBudget".into(), json!(budget));
            }
            thinking.insert("includeThoughts".into(), json!(tc.include_thoughts));
            gen_config.insert("thinkingConfig".into(), json!(thinking));
        }
        if !gen_config.is_empty() {
            body["generationConfig"] = json!(gen_config);
        }

        // tools
        if let Some(tools) = filtered_tools {
            if !tools.is_empty() {
                let declarations: Vec<serde_json::Value> =
                    tools.iter().map(|tool| tool_to_google(tool)).collect();
                body["tools"] = json!([{ "functionDeclarations": declarations }]);
                body["toolConfig"] = json!({ "functionCallingConfig": { "mode": "AUTO" } });
            }
        }

        body
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    fn name(&self) -> &str {
        "google"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let body = self.build_request_body(&request);
        let url = format!("{}/models/{}:generateContent", self.base_url, self.model);

        log::info!("[google chat] POST {} model={}", url, self.model);

        let response = reqwest::Client::new()
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::Api {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let resp: GoogleChatResponse = response.json().await?;
        parse_chat_response(resp)
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>> {
        let body = self.build_request_body(&request);
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            self.base_url, self.model
        );

        log::info!("[google chat_stream] POST {} model={}", url, self.model);
        log::debug!(
            "[google chat_stream] body: {}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );

        let response = reqwest::Client::new()
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        log::info!("[google chat_stream] response status: {}", status);

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            log::error!(
                "[google chat_stream] error response ({}): {}",
                status,
                error_body
            );
            return Err(AiError::Api {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let raw_stream =
            response
                .bytes_stream()
                .eventsource()
                .filter_map(|result| async move {
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
                Ok(data) => match google_stream::parse_google_chunk(&data) {
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

fn find_tool_name(messages: &[Message], tool_call_id: &str) -> String {
    for msg in messages.iter().rev() {
        if let Message::Assistant {
            tool_calls: Some(tcs),
            ..
        } = msg
        {
            for tc in tcs {
                if tc.id == tool_call_id {
                    return tc.name.clone();
                }
            }
        }
    }
    tool_call_id.to_string()
}

/// Google role for a message (for merging consecutive same-role messages).
fn google_role(msg: &Message) -> Option<&'static str> {
    match msg {
        Message::System { .. } | Message::Developer { .. } => None,
        Message::User { .. } | Message::Tool { .. } => Some("user"),
        Message::Assistant { .. } => Some("model"),
    }
}

fn message_parts(msg: &Message, all_messages: &[Message]) -> Vec<serde_json::Value> {
    match msg {
        Message::User { content } => match content {
            UserContent::Text(t) => vec![json!({ "text": t })],
            UserContent::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => json!({ "text": text }),
                    ContentPart::ImageUrl { image_url } => {
                        if let Some(rest) = image_url.url.strip_prefix("data:") {
                            if let Some((meta, data)) = rest.split_once(',') {
                                let mime = meta.trim_end_matches(";base64");
                                return json!({
                                    "inlineData": {
                                        "mimeType": mime,
                                        "data": data,
                                    }
                                });
                            }
                        }
                        json!({ "text": format!("[image: {}]", image_url.url) })
                    }
                })
                .collect(),
        },
        Message::Assistant {
            content,
            tool_calls,
            ..
        } => {
            let mut parts = Vec::new();
            if let Some(c) = content {
                parts.push(json!({ "text": c }));
            }
            if let Some(tcs) = tool_calls {
                for tc in tcs {
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.arguments).unwrap_or(json!({}));
                    parts.push(json!({
                        "functionCall": {
                            "name": tc.name,
                            "args": args,
                        }
                    }));
                }
            }
            if parts.is_empty() {
                parts.push(json!({ "text": "" }));
            }
            parts
        }
        Message::Tool {
            tool_call_id,
            content,
        } => {
            let name = find_tool_name(all_messages, tool_call_id);
            vec![json!({
                "functionResponse": {
                    "name": name,
                    "response": { "content": content },
                }
            })]
        }
        Message::System { .. } | Message::Developer { .. } => vec![],
    }
}

fn messages_to_google(
    messages: &[Message],
) -> (Vec<serde_json::Value>, Option<serde_json::Value>) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut contents: Vec<serde_json::Value> = Vec::new();

    // Track current merged message
    let mut current_role: Option<&str> = None;
    let mut current_parts: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        match msg {
            Message::System { content } | Message::Developer { content } => {
                system_parts.push(content.clone());
            }
            _ => {
                let role = google_role(msg).unwrap();
                let parts = message_parts(msg, messages);

                if Some(role) == current_role {
                    // Same role — merge parts
                    current_parts.extend(parts);
                } else {
                    // Different role — flush previous
                    if let Some(r) = current_role {
                        if !current_parts.is_empty() {
                            contents.push(json!({
                                "role": r,
                                "parts": current_parts,
                            }));
                        }
                    }
                    current_role = Some(role);
                    current_parts = parts;
                }
            }
        }
    }

    // Flush final
    if let Some(r) = current_role {
        if !current_parts.is_empty() {
            contents.push(json!({
                "role": r,
                "parts": current_parts,
            }));
        }
    }

    let system_instruction = if system_parts.is_empty() {
        None
    } else {
        let combined = system_parts.join("\n");
        Some(json!({ "parts": [{ "text": combined }] }))
    };

    (contents, system_instruction)
}

// --- Tool schema conversion ---

fn tool_to_google(tool: &ToolDef) -> serde_json::Value {
    let params = convert_schema(&tool.input_schema);
    json!({
        "name": tool.name,
        "description": tool.description,
        "parameters": params,
    })
}

fn convert_schema(schema: &serde_json::Value) -> serde_json::Value {
    match schema {
        serde_json::Value::Object(obj) => {
            let mut result = serde_json::Map::new();

            // Handle array type (e.g., ["string", "null"] → nullable)
            let (resolved_type, nullable) = resolve_type(obj);

            if let Some(t) = resolved_type {
                result.insert("type".into(), json!(t.to_uppercase()));
            }
            if nullable {
                result.insert("nullable".into(), json!(true));
            }

            // Handle const → enum
            if let Some(const_val) = obj.get("const") {
                result.insert("enum".into(), json!([value_to_string(const_val)]));
            }

            // Handle enum — convert integer values to strings
            if let Some(serde_json::Value::Array(variants)) = obj.get("enum") {
                let str_variants: Vec<serde_json::Value> = variants
                    .iter()
                    .map(|v| json!(value_to_string(v)))
                    .collect();
                result.insert("enum".into(), json!(str_variants));
            }

            // Copy description
            if let Some(desc) = obj.get("description") {
                result.insert("description".into(), desc.clone());
            }

            // Determine if this is an object type
            let is_object = resolved_type.map_or(false, |t| t == "object");

            // Handle properties (only for object type)
            if is_object {
                if let Some(serde_json::Value::Object(props)) = obj.get("properties") {
                    let mut converted = serde_json::Map::new();
                    for (k, v) in props {
                        converted.insert(k.clone(), convert_schema(v));
                    }
                    result.insert("properties".into(), json!(converted));
                }
                if let Some(req) = obj.get("required") {
                    result.insert("required".into(), req.clone());
                }
            }

            // Handle items (for arrays)
            if let Some(items) = obj.get("items") {
                let converted_items = convert_schema(items);
                // Ensure items has a type
                let items_val = if converted_items.get("type").is_none() {
                    json!({ "type": "STRING" })
                } else {
                    converted_items
                };
                result.insert("items".into(), items_val);
            }

            // Handle anyOf / oneOf — pass through with conversion
            for key in &["anyOf", "oneOf"] {
                if let Some(serde_json::Value::Array(variants)) = obj.get(*key) {
                    let converted: Vec<serde_json::Value> =
                        variants.iter().map(convert_schema).collect();
                    result.insert((*key).to_string(), json!(converted));
                }
            }

            json!(result)
        }
        other => other.clone(),
    }
}

fn resolve_type(obj: &serde_json::Map<String, serde_json::Value>) -> (Option<&str>, bool) {
    match obj.get("type") {
        Some(serde_json::Value::Array(types)) => {
            let mut nullable = false;
            let mut primary_type: Option<&str> = None;
            for t in types {
                if let serde_json::Value::String(s) = t {
                    if s == "null" {
                        nullable = true;
                    } else if primary_type.is_none() {
                        primary_type = Some(s.as_str());
                    }
                }
            }
            (primary_type, nullable)
        }
        Some(serde_json::Value::String(s)) => (Some(s.as_str()), false),
        _ => (None, false),
    }
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// --- Non-streaming response types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleChatResponse {
    #[serde(default)]
    candidates: Vec<GoogleResponseCandidate>,
    #[serde(default)]
    usage_metadata: Option<GoogleResponseUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleResponseCandidate {
    #[serde(default)]
    content: Option<GoogleResponseContent>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleResponseContent {
    #[serde(default)]
    parts: Vec<GoogleResponsePart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleResponsePart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thought: Option<bool>,
    #[serde(default)]
    function_call: Option<GoogleResponseFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct GoogleResponseFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleResponseUsage {
    #[serde(default)]
    prompt_token_count: u64,
    #[serde(default)]
    candidates_token_count: u64,
    #[serde(default)]
    thoughts_token_count: u64,
}

fn parse_google_finish_reason(reason: &str) -> Option<StopReason> {
    match reason {
        "STOP" => Some(StopReason::Stop),
        "MAX_TOKENS" => Some(StopReason::Length),
        "SAFETY" | "RECITATION" => Some(StopReason::ContentFilter),
        _ => None,
    }
}

fn parse_chat_response(resp: GoogleChatResponse) -> Result<ChatResponse> {
    let candidate = resp
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| AiError::Api {
            status: 0,
            message: "no candidates in Google response".to_string(),
        })?;

    let mut content_text: Option<String> = None;
    let mut reasoning_text: Option<String> = None;
    let mut tool_calls: Vec<ToolCallInfo> = Vec::new();

    if let Some(content) = candidate.content {
        for part in content.parts {
            if let Some(fc) = part.function_call {
                tool_calls.push(ToolCallInfo {
                    id: Uuid::new_v4().to_string(),
                    name: fc.name,
                    arguments: serde_json::to_string(&fc.args).unwrap_or_default(),
                });
            } else if let Some(text) = part.text {
                if part.thought == Some(true) {
                    reasoning_text
                        .get_or_insert_with(String::new)
                        .push_str(&text);
                } else {
                    content_text
                        .get_or_insert_with(String::new)
                        .push_str(&text);
                }
            }
        }
    }

    let usage = resp
        .usage_metadata
        .map(|u| {
            let output = u.candidates_token_count + u.thoughts_token_count;
            TokenUsage::with_details(
                u.prompt_token_count,
                output,
                u.prompt_token_count,
                u.candidates_token_count,
                u.thoughts_token_count,
                0,
                0,
            )
            .with_metadata(
                serde_json::to_value(&u).ok(),
                Some(json!({
                    "provider": "google",
                    "candidates_tokens": u.candidates_token_count,
                    "thought_tokens": u.thoughts_token_count,
                })),
            )
        })
        .unwrap_or_default();

    let stop_reason = candidate
        .finish_reason
        .as_deref()
        .and_then(parse_google_finish_reason);

    Ok(ChatResponse {
        content: content_text,
        reasoning: reasoning_text,
        tool_calls,
        usage,
        stop_reason,
    })
}
