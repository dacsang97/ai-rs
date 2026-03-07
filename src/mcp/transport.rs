use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::types::{JsonRpcRequest, JsonRpcResponse};

#[async_trait]
pub trait McpTransport: Send + Sync {
    async fn send(&self, request: JsonRpcRequest) -> crate::Result<JsonRpcResponse>;
    async fn close(&self) -> crate::Result<()>;
}

// ---------------------------------------------------------------------------
// Stdio transport
// ---------------------------------------------------------------------------

pub struct StdioTransport {
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    stdout: Arc<Mutex<BufReader<tokio::process::ChildStdout>>>,
    child: Arc<Mutex<Child>>,
}

impl StdioTransport {
    pub fn new(mut child: Child) -> crate::Result<Self> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| crate::AiError::Mcp("Failed to capture child stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| crate::AiError::Mcp("Failed to capture child stdout".to_string()))?;

        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            child: Arc::new(Mutex::new(child)),
        })
    }

    pub async fn spawn(
        command: &str,
        args: &[&str],
        env: Option<std::collections::HashMap<String, String>>,
    ) -> crate::Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());

        if let Some(env_vars) = env {
            for (k, v) in env_vars {
                cmd.env(k, v);
            }
        }

        let child = cmd
            .spawn()
            .map_err(|e| crate::AiError::Mcp(format!("Failed to spawn MCP server: {e}")))?;

        Self::new(child)
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn send(&self, request: JsonRpcRequest) -> crate::Result<JsonRpcResponse> {
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');

        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| crate::AiError::Mcp(format!("Failed to write to stdin: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| crate::AiError::Mcp(format!("Failed to flush stdin: {e}")))?;
        }

        let mut response_line = String::new();
        {
            let mut stdout = self.stdout.lock().await;
            loop {
                response_line.clear();
                let bytes_read = stdout
                    .read_line(&mut response_line)
                    .await
                    .map_err(|e| {
                        crate::AiError::Mcp(format!("Failed to read from stdout: {e}"))
                    })?;
                if bytes_read == 0 {
                    return Err(crate::AiError::Mcp(
                        "MCP server closed stdout unexpectedly".to_string(),
                    ));
                }
                let trimmed = response_line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Skip JSON-RPC notifications (no id field)
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    if val.get("id").is_some() {
                        break;
                    }
                    // It's a notification, keep reading
                    continue;
                }
                break;
            }
        }

        let response: JsonRpcResponse = serde_json::from_str(response_line.trim())
            .map_err(|e| {
                crate::AiError::Mcp(format!(
                    "Failed to parse JSON-RPC response: {e}\nRaw: {response_line}"
                ))
            })?;

        Ok(response)
    }

    async fn close(&self) -> crate::Result<()> {
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SSE transport (stub for MVP)
// ---------------------------------------------------------------------------

pub struct SseTransport {
    endpoint: String,
    client: reqwest::Client,
}

impl SseTransport {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl McpTransport for SseTransport {
    async fn send(&self, request: JsonRpcRequest) -> crate::Result<JsonRpcResponse> {
        let resp = self
            .client
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| crate::AiError::Mcp(format!("SSE transport HTTP error: {e}")))?;

        let body = resp
            .text()
            .await
            .map_err(|e| crate::AiError::Mcp(format!("SSE transport read error: {e}")))?;

        let response: JsonRpcResponse = serde_json::from_str(&body).map_err(|e| {
            crate::AiError::Mcp(format!("SSE transport parse error: {e}\nRaw: {body}"))
        })?;

        Ok(response)
    }

    async fn close(&self) -> crate::Result<()> {
        Ok(())
    }
}
