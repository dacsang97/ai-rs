use futures::StreamExt;

use crate::message::{Message, ToolCallInfo};
use crate::provider::{ChatRequest, Provider};
use crate::stream::handler::{StreamChunk, ToolCallAccumulator};
use crate::stream::StreamEvent;
use crate::tool::ToolRegistry;
use crate::types::TokenUsage;

pub struct AgentConfig {
    pub max_steps: u32,
    pub cost_per_input: f64,
    pub cost_per_output: f64,
    pub headers: Option<std::collections::HashMap<String, String>>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: 25,
            cost_per_input: 0.0,
            cost_per_output: 0.0,
            headers: None,
        }
    }
}

pub async fn run_agent_loop(
    provider: &dyn Provider,
    messages: &mut Vec<Message>,
    tools: &ToolRegistry,
    config: &AgentConfig,
    abort_rx: &mut tokio::sync::watch::Receiver<bool>,
    event_tx: tokio::sync::mpsc::Sender<StreamEvent>,
) -> crate::Result<()> {
    let tool_defs = tools.definitions();

    let mut total_usage = TokenUsage::default();

    for _step in 0..config.max_steps {
        // Check abort
        if *abort_rx.borrow() {
            let _ = event_tx.send(StreamEvent::RunAborted).await;
            return Ok(());
        }

        // Build request
        let request = ChatRequest {
            messages: messages.clone(),
            tools: if tool_defs.is_empty() {
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
                    // Ensure accumulators vec is large enough
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
                    // Ensure accumulators vec is large enough
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

        // Continue loop for next step
    }

    // Max steps reached
    let _ = event_tx
        .send(StreamEvent::RunError {
            error: format!("Max steps ({}) reached", config.max_steps),
        })
        .await;
    Ok(())
}
