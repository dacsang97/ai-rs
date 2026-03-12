use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// JSON-RPC 2.0
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// MCP protocol types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCallResult {
    pub content: Vec<McpContent>,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub type_: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerCapabilities {
    pub tools: Option<serde_json::Value>,
    #[serde(default)]
    pub resources: Option<serde_json::Value>,
    #[serde(default)]
    pub prompts: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpInitializeResult {
    pub capabilities: McpServerCapabilities,
    #[serde(default)]
    pub server_info: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpListToolsResult {
    pub tools: Vec<McpToolInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpListPromptsResult {
    pub prompts: Vec<McpPromptInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceInfo {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpListResourcesResult {
    pub resources: Vec<McpResourceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceContents {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub blob: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpReadResourceResult {
    pub contents: Vec<McpResourceContents>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptMessage {
    pub role: String,
    pub content: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpGetPromptResult {
    #[serde(default)]
    pub description: Option<String>,
    pub messages: Vec<McpPromptMessage>,
}
