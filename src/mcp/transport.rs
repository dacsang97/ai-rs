use async_trait::async_trait;
use futures::StreamExt;
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
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
    sse_endpoint: String,
    client: reqwest::Client,
    event_source: Arc<Mutex<Option<EventSource>>>,
    message_endpoint: Arc<Mutex<Option<String>>>,
}

impl SseTransport {
    pub fn new(endpoint: String) -> Self {
        Self {
            sse_endpoint: endpoint,
            client: reqwest::Client::new(),
            event_source: Arc::new(Mutex::new(None)),
            message_endpoint: Arc::new(Mutex::new(None)),
        }
    }

    async fn ensure_event_source(&self) -> crate::Result<()> {
        let mut guard = self.event_source.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let request = self
            .client
            .get(&self.sse_endpoint)
            .header("Accept", "text/event-stream");
        let event_source = request
            .eventsource()
            .map_err(|e| crate::AiError::Mcp(format!("SSE transport init error: {e}")))?;
        *guard = Some(event_source);
        Ok(())
    }

    async fn resolve_message_endpoint(&self) -> crate::Result<String> {
        if let Some(endpoint) = self.message_endpoint.lock().await.clone() {
            return Ok(endpoint);
        }

        self.ensure_event_source().await?;
        let mut guard = self.event_source.lock().await;
        let es = guard
            .as_mut()
            .ok_or_else(|| crate::AiError::Mcp("SSE transport stream unavailable".to_string()))?;

        loop {
            match es.next().await {
                Some(Ok(Event::Open)) => continue,
                Some(Ok(Event::Message(msg))) => {
                    if msg.event != "endpoint" {
                        continue;
                    }
                    let endpoint = resolve_message_endpoint(&self.sse_endpoint, &msg.data)?;
                    *self.message_endpoint.lock().await = Some(endpoint.clone());
                    return Ok(endpoint);
                }
                Some(Err(e)) => {
                    *guard = None;
                    return Err(crate::AiError::Mcp(format!(
                        "SSE transport endpoint discovery error: {e}"
                    )));
                }
                None => {
                    *guard = None;
                    return Err(crate::AiError::Mcp(
                        "SSE transport stream ended before endpoint event".to_string(),
                    ));
                }
            }
        }
    }
}

#[async_trait]
impl McpTransport for SseTransport {
    async fn send(&self, request: JsonRpcRequest) -> crate::Result<JsonRpcResponse> {
        let message_endpoint = self.resolve_message_endpoint().await?;

        let resp = self
            .client
            .post(&message_endpoint)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&request)
            .send()
            .await
            .map_err(|e| crate::AiError::Mcp(format!("SSE transport HTTP error: {e}")))?;

        if resp.status().is_success() {
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();

            if content_type.starts_with("application/json") {
                let body = resp
                    .text()
                    .await
                    .map_err(|e| crate::AiError::Mcp(format!("SSE transport read error: {e}")))?;
                let response: JsonRpcResponse = serde_json::from_str(&body).map_err(|e| {
                    crate::AiError::Mcp(format!("SSE transport parse error: {e}\nRaw: {body}"))
                })?;
                return Ok(response);
            }
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::AiError::Mcp(format!(
                "SSE transport HTTP error {}: {body}",
                status
            )));
        }

        self.ensure_event_source().await?;
        let mut guard = self.event_source.lock().await;
        let es = guard
            .as_mut()
            .ok_or_else(|| crate::AiError::Mcp("SSE transport stream unavailable".to_string()))?;

        loop {
            match es.next().await {
                Some(Ok(Event::Open)) => continue,
                Some(Ok(Event::Message(msg))) => {
                    if msg.event == "endpoint" {
                        let endpoint = resolve_message_endpoint(&self.sse_endpoint, &msg.data)?;
                        *self.message_endpoint.lock().await = Some(endpoint);
                        continue;
                    }

                    let data = msg.data.trim();
                    if data.is_empty() {
                        continue;
                    }

                    let response: JsonRpcResponse = serde_json::from_str(data).map_err(|e| {
                        crate::AiError::Mcp(format!(
                            "SSE transport parse error: {e}\nRaw: {data}"
                        ))
                    })?;

                    if response.id == Some(request.id) {
                        return Ok(response);
                    }
                }
                Some(Err(e)) => {
                    *guard = None;
                    return Err(crate::AiError::Mcp(format!("SSE transport stream error: {e}")));
                }
                None => {
                    *guard = None;
                    return Err(crate::AiError::Mcp(
                        "SSE transport stream ended unexpectedly".to_string(),
                    ));
                }
            }
        }
    }

    async fn close(&self) -> crate::Result<()> {
        let mut guard = self.event_source.lock().await;
        if let Some(mut es) = guard.take() {
            es.close();
        }
        Ok(())
    }
}

fn resolve_message_endpoint(base: &str, endpoint: &str) -> crate::Result<String> {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        return Ok(endpoint.to_string());
    }

    let base = reqwest::Url::parse(base)
        .map_err(|e| crate::AiError::Mcp(format!("Invalid SSE base URL: {e}")))?;
    let resolved = base
        .join(endpoint)
        .map_err(|e| crate::AiError::Mcp(format!("Invalid SSE message endpoint: {e}")))?;
    Ok(resolved.to_string())
}

#[cfg(test)]
mod tests {
    use super::resolve_message_endpoint;

    #[test]
    fn resolves_query_only_endpoint() {
        let actual =
            resolve_message_endpoint("https://localhost/sse", "?sessionId=x").unwrap();
        assert_eq!(actual, "https://localhost/sse?sessionId=x");
    }

    #[test]
    fn resolves_relative_path_endpoint() {
        let actual =
            resolve_message_endpoint("https://localhost/some_path/sse", "message?sessionId=x")
                .unwrap();
        assert_eq!(actual, "https://localhost/some_path/message?sessionId=x");
    }

    #[test]
    fn resolves_absolute_path_endpoint() {
        let actual =
            resolve_message_endpoint("https://localhost/sse", "/message?sessionId=x").unwrap();
        assert_eq!(actual, "https://localhost/message?sessionId=x");
    }
}
