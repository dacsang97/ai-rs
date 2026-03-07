use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::{ToolDef, ToolExecutor, ToolResult};
use crate::mcp::McpClient;

pub struct McpToolExecutor {
    client: Arc<McpClient>,
    tools: Vec<ToolDef>,
}

impl McpToolExecutor {
    pub async fn new(client: Arc<McpClient>) -> crate::Result<Self> {
        let mcp_tools = client.list_tools().await?;
        let tools = mcp_tools
            .into_iter()
            .map(|t| ToolDef {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
            })
            .collect();
        Ok(Self { client, tools })
    }
}

#[async_trait]
impl ToolExecutor for McpToolExecutor {
    async fn execute(&self, name: &str, input: Value) -> crate::Result<ToolResult> {
        let result = self.client.call_tool(name, input).await?;
        let output = result
            .content
            .iter()
            .filter_map(|c| c.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ToolResult {
            output,
            title: None,
            is_error: result.is_error,
        })
    }

    fn definitions(&self) -> Vec<ToolDef> {
        self.tools.clone()
    }
}
