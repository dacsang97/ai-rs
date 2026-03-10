pub mod builtin;
pub mod mcp;

use std::collections::HashSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
    pub title: Option<String>,
    pub is_error: bool,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, input: serde_json::Value) -> crate::Result<ToolResult>;
    fn definitions(&self) -> Vec<ToolDef>;
}

pub struct ToolRegistry {
    executors: Vec<Box<dyn ToolExecutor>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            executors: Vec::new(),
        }
    }

    pub fn register(&mut self, executor: Box<dyn ToolExecutor>) {
        self.executors.push(executor);
    }

    pub fn definitions(&self) -> Vec<ToolDef> {
        self.executors.iter().flat_map(|e| e.definitions()).collect()
    }

    pub fn definitions_excluding(&self, exclude: &HashSet<String>) -> Vec<ToolDef> {
        self.executors
            .iter()
            .flat_map(|e| e.definitions())
            .filter(|d| !exclude.contains(&d.name))
            .collect()
    }

    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> crate::Result<ToolResult> {
        for executor in &self.executors {
            let defs = executor.definitions();
            if defs.iter().any(|d| d.name == name) {
                return executor.execute(name, input).await;
            }
        }
        Err(crate::AiError::Tool {
            tool: name.to_string(),
            message: format!("No executor found for tool '{name}'"),
        })
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
