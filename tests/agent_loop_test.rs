use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use ai_rs::provider::{ChatRequest, ChatResponse, Provider, ToolChoice};
use ai_rs::session::agent::{AgentConfig, LoopState, StepPreparation};
use ai_rs::stream::handler::StreamChunk;
use ai_rs::tool::{ToolDef, ToolExecutor, ToolRegistry, ToolResult};
use ai_rs::{Message, StopReason, StreamEvent, TokenUsage};
use async_trait::async_trait;
use futures::stream;
use futures::Stream;
use tokio::sync::Mutex;

#[derive(Clone)]
struct MockProvider {
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    responses: Arc<Mutex<Vec<Vec<StreamChunk>>>>,
}

impl MockProvider {
    fn new(responses: Vec<Vec<StreamChunk>>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(vec![])),
            responses: Arc::new(Mutex::new(responses)),
        }
    }

    async fn captured_requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn model(&self) -> &str {
        "mock-model"
    }

    async fn chat(&self, _request: ChatRequest) -> ai_rs::Result<ChatResponse> {
        Err(ai_rs::AiError::Other("unused in tests".into()))
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> ai_rs::Result<Pin<Box<dyn Stream<Item = ai_rs::Result<StreamChunk>> + Send>>> {
        self.requests.lock().await.push(request);
        let next = self
            .responses
            .lock()
            .await
            .remove(0)
            .into_iter()
            .map(Ok);
        Ok(Box::pin(stream::iter(next)))
    }
}

struct DummyToolExecutor;

#[async_trait]
impl ToolExecutor for DummyToolExecutor {
    async fn execute(&self, name: &str, _input: serde_json::Value) -> ai_rs::Result<ToolResult> {
        Ok(ToolResult {
            output: format!("{name} ok"),
            title: Some(format!("Executed {name}")),
            is_error: false,
        })
    }

    fn definitions(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "allowed_tool".into(),
                description: "Allowed tool".into(),
                input_schema: serde_json::json!({ "type": "object" }),
            },
            ToolDef {
                name: "blocked_tool".into(),
                description: "Blocked tool".into(),
                input_schema: serde_json::json!({ "type": "object" }),
            },
        ]
    }
}

#[tokio::test]
async fn prepare_step_shapes_chat_request() {
    let provider = MockProvider::new(vec![vec![
        StreamChunk::TextDelta("done".into()),
        StreamChunk::Done {
            stop_reason: Some(StopReason::Stop),
            usage: Some(TokenUsage::new(10, 5)),
        },
    ]]);

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(DummyToolExecutor));

    let mut messages = vec![Message::user("hello")];
    let (abort_tx, mut abort_rx) = tokio::sync::watch::channel(false);
    let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
    drop(abort_tx);

    let mut config = AgentConfig::default();
    config.prepare_step = Some(Arc::new(|ctx| StepPreparation {
        tool_choice: Some(ToolChoice::Tool("allowed_tool".into())),
        active_tools: Some(vec!["allowed_tool".into()]),
        metadata: Some(serde_json::json!({ "step": ctx.step, "run_id": ctx.run_id })),
        headers: Some(HashMap::from([("X-Step".into(), ctx.step.to_string())])),
        stop: Some(vec!["STOP".into()]),
        extra_messages: vec![Message::developer("extra-step-context")],
    }));

    ai_rs::run_agent_loop(
        &provider,
        &mut messages,
        &tools,
        &config,
        None,
        &mut abort_rx,
        event_tx,
        None,
    )
    .await
    .unwrap();

    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.tool_choice,
        Some(ToolChoice::Tool("allowed_tool".into()))
    );
    assert_eq!(request.active_tools, Some(vec!["allowed_tool".into()]));
    assert_eq!(request.stop, Some(vec!["STOP".into()]));
    assert_eq!(
        request.headers.as_ref().and_then(|h| h.get("X-Step")),
        Some(&"1".to_string())
    );
    assert_eq!(request.metadata.as_ref().unwrap()["step"], 1);
    assert!(matches!(
        request.messages.last(),
        Some(Message::Developer { content }) if content == "extra-step-context"
    ));
}

#[tokio::test]
async fn stop_when_halts_before_second_provider_call() {
    let provider = MockProvider::new(vec![vec![
        StreamChunk::ToolCallStart {
            index: 0,
            id: "call-1".into(),
            name: "allowed_tool".into(),
        },
        StreamChunk::ToolCallDelta {
            index: 0,
            arguments: "{}".into(),
        },
        StreamChunk::Done {
            stop_reason: Some(StopReason::ToolCalls),
            usage: Some(TokenUsage::new(3, 2)),
        },
    ]]);

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(DummyToolExecutor));

    let mut messages = vec![Message::user("use a tool")];
    let (_abort_tx, mut abort_rx) = tokio::sync::watch::channel(false);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

    let mut config = AgentConfig::default();
    config.stop_when = Some(Arc::new(|state: &LoopState| {
        state.completed_steps >= 1 && !state.last_tool_calls.is_empty()
    }));

    ai_rs::run_agent_loop(
        &provider,
        &mut messages,
        &tools,
        &config,
        None,
        &mut abort_rx,
        event_tx,
        None,
    )
    .await
    .unwrap();

    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 1);

    let mut saw_run_complete = false;
    while let Ok(event) = event_rx.try_recv() {
        if matches!(event, StreamEvent::RunComplete) {
            saw_run_complete = true;
        }
    }
    assert!(saw_run_complete);
}
