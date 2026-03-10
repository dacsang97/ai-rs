use futures::StreamExt;

use crate::message::{Message, ToolCallInfo};
use crate::provider::{ChatRequest, Provider};
use crate::session::approval::{ApprovalRequest, ApprovalResponse};
use crate::stream::handler::{StreamChunk, ToolCallAccumulator};
use crate::stream::StreamEvent;
use crate::tool::ToolRegistry;
use crate::types::TokenUsage;

const DEFAULT_MAX_STEPS: u32 = 100;
/// Chars threshold before pruning kicks in (~40K tokens).
const DEFAULT_PRUNE_AFTER: usize = 160_000;
/// Keep this many chars of recent tool outputs (~20K tokens).
const DEFAULT_PRUNE_KEEP: usize = 80_000;
/// Replacement text for pruned tool outputs.
const PRUNED_MARKER: &str = "[output pruned — use tools to re-read if needed]";

const DEFAULT_STOP_PROMPT: &str = "\
CRITICAL — MAXIMUM STEPS REACHED

The maximum number of steps allowed for this task has been reached. \
Tools are disabled. Respond with text only.

STRICT REQUIREMENTS:
1. Do NOT make any tool calls
2. Provide a text summary of work done so far
3. List any remaining tasks that were not completed
4. Recommend what should be done next";

pub struct AgentConfig {
    pub max_steps: u32,
    pub cost_per_input: f64,
    pub cost_per_output: f64,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub approval_timeout_secs: u64,
    /// Prompt injected on the last step so the model wraps up gracefully
    /// instead of hitting a hard error. Set `None` to use the built-in default.
    pub stop_prompt: Option<String>,
    /// Start pruning old tool outputs when total message chars exceed this.
    pub prune_after: usize,
    /// Keep this many chars of recent tool outputs unpruned.
    pub prune_keep: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: DEFAULT_MAX_STEPS,
            cost_per_input: 0.0,
            cost_per_output: 0.0,
            headers: None,
            approval_timeout_secs: 300,
            stop_prompt: None,
            prune_after: DEFAULT_PRUNE_AFTER,
            prune_keep: DEFAULT_PRUNE_KEEP,
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Rough char-count of a single message (used for pruning heuristic).
fn msg_chars(msg: &Message) -> usize {
    match msg {
        Message::System { content }
        | Message::Developer { content }
        | Message::Tool { content, .. } => content.len(),
        Message::User { content } => match content {
            crate::message::UserContent::Text(t) => t.len(),
            crate::message::UserContent::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    crate::message::ContentPart::Text { text } => text.len(),
                    crate::message::ContentPart::ImageUrl { .. } => 256,
                })
                .sum(),
        },
        Message::Assistant {
            content,
            reasoning,
            tool_calls,
        } => {
            let c = content.as_deref().map_or(0, str::len);
            let r = reasoning.as_deref().map_or(0, str::len);
            let t = tool_calls.as_ref().map_or(0, |calls| {
                calls.iter().map(|tc| tc.arguments.len() + tc.name.len()).sum()
            });
            c + r + t
        }
    }
}

/// Total chars across all messages.
fn total_chars(msgs: &[Message]) -> usize {
    msgs.iter().map(|m| msg_chars(m)).sum()
}

/// Prune old tool-result outputs when context grows too large.
/// Walks backwards, protecting the most recent `keep` chars of tool outputs,
/// then replaces older ones with a short marker.
/// Returns `(pruned_count, freed_chars)`.
fn prune_tool_outputs(msgs: &mut [Message], after: usize, keep: usize) -> (u32, u64) {
    let total = total_chars(msgs);
    if total <= after {
        return (0, 0);
    }

    // Walk backwards, accumulate tool-output chars, prune beyond `keep`.
    let mut recent = 0usize;
    let mut pruned = 0u32;
    let mut freed = 0u64;

    for msg in msgs.iter_mut().rev() {
        if let Message::Tool { content, .. } = msg {
            let len = content.len();
            if len <= PRUNED_MARKER.len() {
                continue;
            }
            if recent < keep {
                recent += len;
                continue;
            }
            freed += (len - PRUNED_MARKER.len()) as u64;
            *content = PRUNED_MARKER.to_string();
            pruned += 1;
        }
    }

    (pruned, freed)
}

// ── main loop ────────────────────────────────────────────────────────────────

pub async fn run_agent_loop(
    provider: &dyn Provider,
    messages: &mut Vec<Message>,
    tools: &ToolRegistry,
    config: &AgentConfig,
    abort_rx: &mut tokio::sync::watch::Receiver<bool>,
    event_tx: tokio::sync::mpsc::Sender<StreamEvent>,
    approval_tx: Option<tokio::sync::mpsc::Sender<ApprovalRequest>>,
) -> crate::Result<()> {
    let tool_defs = tools.definitions();

    let mut total_usage = TokenUsage::default();
    let mut empty_retries: u32 = 0;

    for step in 0..config.max_steps {
        // Check abort
        if *abort_rx.borrow() {
            let _ = event_tx.send(StreamEvent::RunAborted).await;
            return Ok(());
        }

        // ── Prune old tool outputs if context is too large ───────────
        let (pruned, freed) = prune_tool_outputs(messages, config.prune_after, config.prune_keep);
        if pruned > 0 {
            let _ = event_tx
                .send(StreamEvent::ContextPrune { pruned, freed })
                .await;
        }

        // ── Graceful last step: inject stop prompt, disable tools ────
        let last = step == config.max_steps - 1;
        if last {
            let _ = event_tx
                .send(StreamEvent::MaxStepsWarning {
                    step,
                    max_steps: config.max_steps,
                })
                .await;

            let prompt = config
                .stop_prompt
                .as_deref()
                .unwrap_or(DEFAULT_STOP_PROMPT);
            messages.push(Message::developer(prompt));
        }

        // Build request — strip tools on last step
        let request = ChatRequest {
            messages: messages.clone(),
            tools: if last || tool_defs.is_empty() {
                None
            } else {
                Some(tool_defs.clone())
            },
            temperature: None,
            max_tokens: None,
            stop: None,
            headers: config.headers.clone(),
            reasoning_effort: None,
        };

        // Call provider.chat_stream
        let mut stream = provider.chat_stream(request).await?;

        // Process stream chunks
        let mut text_content = String::new();
        let mut reasoning_content = String::new();
        let mut accumulators: Vec<ToolCallAccumulator> = Vec::new();
        let mut text_started = false;
        let mut reasoning_started = false;
        let text_part_id = uuid::Uuid::new_v4().to_string();
        let reasoning_part_id = uuid::Uuid::new_v4().to_string();
        let mut step_usage = TokenUsage::default();
        let mut stop_reason = String::from("stop");

        while let Some(chunk_result) = stream.next().await {
            // Check abort between chunks
            if *abort_rx.borrow() {
                let _ = event_tx.send(StreamEvent::RunAborted).await;
                return Ok(());
            }

            let chunk = chunk_result?;
            match chunk {
                StreamChunk::TextDelta(delta) => {
                    if !text_started {
                        text_started = true;
                        let _ = event_tx
                            .send(StreamEvent::TextStart {
                                part_id: text_part_id.clone(),
                            })
                            .await;
                    }
                    text_content.push_str(&delta);
                    let _ = event_tx
                        .send(StreamEvent::TextDelta {
                            part_id: text_part_id.clone(),
                            delta,
                        })
                        .await;
                }
                StreamChunk::ReasoningDelta(delta) => {
                    if !reasoning_started {
                        reasoning_started = true;
                        let _ = event_tx
                            .send(StreamEvent::ReasoningStart {
                                part_id: reasoning_part_id.clone(),
                            })
                            .await;
                    }
                    reasoning_content.push_str(&delta);
                    let _ = event_tx
                        .send(StreamEvent::ReasoningDelta {
                            part_id: reasoning_part_id.clone(),
                            delta,
                        })
                        .await;
                }
                StreamChunk::ToolCallStart { index, id, name } => {
                    while accumulators.len() <= index {
                        accumulators.push(ToolCallAccumulator::new(
                            accumulators.len(),
                            String::new(),
                            String::new(),
                        ));
                    }
                    accumulators[index] =
                        ToolCallAccumulator::new(index, id.clone(), name.clone());
                    let _ = event_tx
                        .send(StreamEvent::ToolPending {
                            call_id: id,
                            tool_name: name,
                        })
                        .await;
                }
                StreamChunk::ToolCallDelta { index, arguments } => {
                    while accumulators.len() <= index {
                        accumulators.push(ToolCallAccumulator::new(
                            accumulators.len(),
                            String::new(),
                            String::new(),
                        ));
                    }
                    accumulators[index].append_arguments(&arguments);
                    let call_id = accumulators[index].id.clone();
                    let _ = event_tx
                        .send(StreamEvent::ToolInputDelta {
                            call_id,
                            delta: arguments,
                        })
                        .await;
                }
                StreamChunk::Done {
                    stop_reason: sr,
                    usage,
                } => {
                    if let Some(sr) = sr {
                        stop_reason = format!("{sr:?}").to_lowercase();
                    }
                    if let Some(u) = usage {
                        step_usage = u;
                    }
                }
            }
        }

        // End text/reasoning streams
        if text_started {
            let _ = event_tx
                .send(StreamEvent::TextEnd {
                    part_id: text_part_id,
                })
                .await;
        }
        if reasoning_started {
            let _ = event_tx
                .send(StreamEvent::ReasoningEnd {
                    part_id: reasoning_part_id,
                })
                .await;
        }

        // Accumulate usage
        total_usage = TokenUsage::new(
            total_usage.input_tokens + step_usage.input_tokens,
            total_usage.output_tokens + step_usage.output_tokens,
        );

        // Filter out placeholder accumulators (those with empty id)
        let completed_calls: Vec<ToolCallAccumulator> = accumulators
            .into_iter()
            .filter(|tc| !tc.id.is_empty())
            .collect();

        if completed_calls.is_empty() {
            // No tool calls — step complete
            let cost = step_usage.input_tokens as f64 * config.cost_per_input
                + step_usage.output_tokens as f64 * config.cost_per_output;
            let _ = event_tx
                .send(StreamEvent::StepFinish {
                    tokens: step_usage,
                    cost,
                    reason: stop_reason,
                })
                .await;

            // If we got zero output after a tool call step (step > 0), retry once
            // Some models (e.g. Gemini) occasionally return empty after tool results
            if text_content.is_empty() && step > 0 && empty_retries < 1 {
                empty_retries += 1;
                continue;
            }

            let _ = event_tx.send(StreamEvent::RunComplete).await;

            // Add assistant message to history
            if !text_content.is_empty() {
                messages.push(Message::assistant(text_content));
            }
            return Ok(());
        }

        // Has tool calls — build assistant message with tool calls
        let tool_call_infos: Vec<ToolCallInfo> = completed_calls
            .iter()
            .map(|tc| ToolCallInfo {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            })
            .collect();
        messages.push(Message::assistant_with_tool_calls(
            if text_content.is_empty() {
                None
            } else {
                Some(text_content.clone())
            },
            tool_call_infos,
        ));

        // Execute tools sequentially
        for tc in &completed_calls {
            // If approval channel is configured, request approval before executing
            if let Some(ref atx) = approval_tx {
                let _ = event_tx
                    .send(StreamEvent::ToolApprovalRequired {
                        call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    })
                    .await;

                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<ApprovalResponse>();
                let req = ApprovalRequest {
                    call_id: tc.id.clone(),
                    tool_name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                    response_tx: resp_tx,
                };

                if atx.send(req).await.is_err() {
                    let denial = "Approval channel closed".to_string();
                    let _ = event_tx
                        .send(StreamEvent::ToolDenied {
                            call_id: tc.id.clone(),
                            error: denial.clone(),
                        })
                        .await;
                    messages.push(Message::tool_result(
                        tc.id.clone(),
                        format!("Tool denied: {denial}"),
                    ));
                    continue;
                }

                let timeout_dur = std::time::Duration::from_secs(config.approval_timeout_secs);
                let approval_result = tokio::select! {
                    res = tokio::time::timeout(timeout_dur, resp_rx) => {
                        match res {
                            Ok(Ok(response)) => Some(response),
                            Ok(Err(_)) => {
                                Some(ApprovalResponse::Denied {
                                    message: Some("Approval channel closed".to_string()),
                                })
                            }
                            Err(_) => {
                                Some(ApprovalResponse::Denied {
                                    message: Some("Approval timed out".to_string()),
                                })
                            }
                        }
                    }
                    _ = abort_rx.changed() => {
                        if *abort_rx.borrow() {
                            let _ = event_tx.send(StreamEvent::RunAborted).await;
                            return Ok(());
                        }
                        Some(ApprovalResponse::Denied {
                            message: Some("Aborted during approval".to_string()),
                        })
                    }
                };

                match approval_result {
                    Some(ApprovalResponse::Approved) => {}
                    Some(ApprovalResponse::Denied { message }) => {
                        let denial = message.unwrap_or_else(|| "Tool call denied by user".to_string());
                        let _ = event_tx
                            .send(StreamEvent::ToolDenied {
                                call_id: tc.id.clone(),
                                error: denial.clone(),
                            })
                            .await;
                        messages.push(Message::tool_result(
                            tc.id.clone(),
                            format!("Tool denied: {denial}"),
                        ));
                        continue;
                    }
                    None => {
                        continue;
                    }
                }
            }

            let _ = event_tx
                .send(StreamEvent::ToolRunning {
                    call_id: tc.id.clone(),
                    tool_name: Some(tc.name.clone()),
                })
                .await;

            let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

            let result = tools.execute(&tc.name, args).await;

            match result {
                Ok(tool_result) => {
                    let _ = event_tx
                        .send(StreamEvent::ToolCompleted {
                            call_id: tc.id.clone(),
                            output: tool_result.output.clone(),
                            title: tool_result.title.clone(),
                        })
                        .await;
                    messages.push(Message::tool_result(tc.id.clone(), tool_result.output));
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    let _ = event_tx
                        .send(StreamEvent::ToolError {
                            call_id: tc.id.clone(),
                            error: error_msg.clone(),
                        })
                        .await;
                    messages.push(Message::tool_result(
                        tc.id.clone(),
                        format!("Error: {error_msg}"),
                    ));
                }
            }
        }

        // Emit step finish
        let cost = step_usage.input_tokens as f64 * config.cost_per_input
            + step_usage.output_tokens as f64 * config.cost_per_output;
        let _ = event_tx
            .send(StreamEvent::StepFinish {
                tokens: step_usage,
                cost,
                reason: stop_reason,
            })
            .await;
    }

    // Exhausted all steps (should not reach here if graceful stop worked)
    let _ = event_tx
        .send(StreamEvent::RunError {
            error: format!("Max steps ({}) reached", config.max_steps),
        })
        .await;
    Ok(())
}
