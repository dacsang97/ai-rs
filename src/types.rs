use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
    Developer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub input_text_tokens: u64,
    #[serde(default)]
    pub output_text_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<serde_json::Value>,
}

impl TokenUsage {
    pub fn new(input: u64, output: u64) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            input_text_tokens: input,
            output_text_tokens: output,
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            raw: None,
            provider_metadata: None,
        }
    }

    pub fn with_details(
        input: u64,
        output: u64,
        input_text: u64,
        output_text: u64,
        reasoning: u64,
        cache_read: u64,
        cache_write: u64,
    ) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            input_text_tokens: input_text,
            output_text_tokens: output_text,
            reasoning_tokens: reasoning,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            raw: None,
            provider_metadata: None,
        }
    }

    pub fn with_metadata(
        mut self,
        raw: Option<serde_json::Value>,
        provider_metadata: Option<serde_json::Value>,
    ) -> Self {
        self.raw = raw;
        self.provider_metadata = provider_metadata;
        self
    }
}
