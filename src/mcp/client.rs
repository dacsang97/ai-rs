use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::transport::{McpTransport, StdioTransport};
use super::types::{
    JsonRpcRequest, McpInitializeResult, McpListToolsResult, McpServerCapabilities,
    McpToolCallResult, McpToolInfo,
};

pub struct McpClient {
    transport: Box<dyn McpTransport>,
    next_id: AtomicU64,
}

impl McpClient {
    pub async fn new_stdio(
        command: &str,
        args: &[&str],
        env: Option<HashMap<String, String>>,
    ) -> crate::Result<Self> {
        let transport = StdioTransport::spawn(command, args, env).await?;
        Ok(Self {
            transport: Box::new(transport),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn from_transport(transport: Box<dyn McpTransport>) -> Self {
        Self {
            transport,
            next_id: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> crate::Result<serde_json::Value> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id(),
            method: method.to_string(),
            params,
        };

        let response = self.transport.send(request).await?;

        if let Some(err) = response.error {
            return Err(crate::AiError::Mcp(format!(
                "JSON-RPC error {}: {}",
                err.code, err.message
            )));
        }

        response
            .result
            .ok_or_else(|| crate::AiError::Mcp("Empty result in JSON-RPC response".to_string()))
    }

    pub async fn initialize(&self) -> crate::Result<McpServerCapabilities> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "ai-rs",
                "version": "0.1.0"
            }
        });

        let result = self
            .request("initialize", Some(params))
            .await?;

        let init_result: McpInitializeResult = serde_json::from_value(result)?;

        // Send initialized notification (fire and forget via a request with no expected response)
        let _notification = self
            .transport
            .send(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: self.next_id(),
                method: "notifications/initialized".to_string(),
                params: None,
            })
            .await;

        Ok(init_result.capabilities)
    }

    pub async fn list_tools(&self) -> crate::Result<Vec<McpToolInfo>> {
        let result = self.request("tools/list", None).await?;
        let list_result: McpListToolsResult = serde_json::from_value(result)?;
        Ok(list_result.tools)
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> crate::Result<McpToolCallResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });

        let result = self.request("tools/call", Some(params)).await?;
        let call_result: McpToolCallResult = serde_json::from_value(result)?;
        Ok(call_result)
    }

    pub async fn close(&self) -> crate::Result<()> {
        self.transport.close().await
    }
}
