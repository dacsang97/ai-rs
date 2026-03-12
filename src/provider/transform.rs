use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::message::{ContentPart, Message, UserContent};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallIdMode {
    SafeAscii,
    MistralNineChar,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCompatOptions {
    #[serde(default)]
    pub filter_empty_messages: bool,
    #[serde(default)]
    pub ensure_assistant_after_tool: bool,
    #[serde(default)]
    pub fallback_image_to_text: bool,
    #[serde(default)]
    pub tool_call_id_mode: Option<ToolCallIdMode>,
}

impl ProviderCompatOptions {
    pub fn from_provider_options(value: Option<&serde_json::Value>) -> Option<Self> {
        let compat = value?.get("compat")?;
        serde_json::from_value(compat.clone()).ok()
    }
}

pub fn normalize_messages(
    messages: &[Message],
    options: &ProviderCompatOptions,
) -> Vec<Message> {
    let mut normalized = messages.to_vec();

    if options.fallback_image_to_text {
        normalized = normalized
            .into_iter()
            .map(fallback_unsupported_images_to_text)
            .collect();
    }

    if options.filter_empty_messages {
        normalized = normalized
            .into_iter()
            .filter_map(filter_empty_message)
            .collect();
    }

    if let Some(mode) = &options.tool_call_id_mode {
        normalized = normalize_tool_call_ids(normalized, mode);
    }

    if options.ensure_assistant_after_tool {
        normalized = ensure_assistant_after_tool(normalized);
    }

    normalized
}

fn fallback_unsupported_images_to_text(message: Message) -> Message {
    match message {
        Message::User {
            content: UserContent::Parts(parts),
        } => {
            let parts = parts
                .into_iter()
                .map(|part| match part {
                    ContentPart::ImageUrl { image_url } => ContentPart::Text {
                        text: format!("[image: {}]", image_url.url),
                    },
                    other => other,
                })
                .collect();
            Message::User {
                content: UserContent::Parts(parts),
            }
        }
        other => other,
    }
}

fn filter_empty_message(message: Message) -> Option<Message> {
    match message {
        Message::System { content } if content.trim().is_empty() => None,
        Message::Developer { content } if content.trim().is_empty() => None,
        Message::User {
            content: UserContent::Text(content),
        } if content.trim().is_empty() => None,
        Message::User {
            content: UserContent::Parts(parts),
        } => {
            let parts = parts
                .into_iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } if text.trim().is_empty() => None,
                    other => Some(other),
                })
                .collect::<Vec<_>>();

            if parts.is_empty() {
                None
            } else {
                Some(Message::User {
                    content: UserContent::Parts(parts),
                })
            }
        }
        Message::Assistant {
            content,
            reasoning,
            tool_calls,
        } => {
            let content = content.and_then(|text| {
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text)
                }
            });

            let reasoning = reasoning.and_then(|text| {
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text)
                }
            });

            if content.is_none() && reasoning.is_none() && tool_calls.as_ref().is_none_or(Vec::is_empty) {
                None
            } else {
                Some(Message::Assistant {
                    content,
                    reasoning,
                    tool_calls,
                })
            }
        }
        other => Some(other),
    }
}

fn normalize_tool_call_ids(messages: Vec<Message>, mode: &ToolCallIdMode) -> Vec<Message> {
    let mut id_map = HashMap::new();

    let messages = messages
        .into_iter()
        .map(|message| match message {
            Message::Assistant {
                content,
                reasoning,
                tool_calls,
            } => {
                let tool_calls = tool_calls.map(|calls| {
                    calls.into_iter()
                        .map(|mut call| {
                            let normalized = normalize_tool_call_id(&call.id, mode);
                            id_map.insert(call.id.clone(), normalized.clone());
                            call.id = normalized;
                            call
                        })
                        .collect()
                });

                Message::Assistant {
                    content,
                    reasoning,
                    tool_calls,
                }
            }
            other => other,
        })
        .collect::<Vec<_>>();

    messages
        .into_iter()
        .map(|message| match message {
            Message::Tool {
                tool_call_id,
                content,
            } => Message::Tool {
                tool_call_id: id_map.get(&tool_call_id).cloned().unwrap_or(tool_call_id),
                content,
            },
            other => other,
        })
        .collect()
}

fn ensure_assistant_after_tool(messages: Vec<Message>) -> Vec<Message> {
    let mut result = Vec::with_capacity(messages.len());

    for index in 0..messages.len() {
        let current = messages[index].clone();
        let next = messages.get(index + 1);
        result.push(current.clone());

        if matches!(current, Message::Tool { .. }) && matches!(next, Some(Message::User { .. })) {
            result.push(Message::assistant("Done."));
        }
    }

    result
}

fn normalize_tool_call_id(id: &str, mode: &ToolCallIdMode) -> String {
    match mode {
        ToolCallIdMode::SafeAscii => id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                    ch
                } else {
                    '_'
                }
            })
            .collect(),
        ToolCallIdMode::MistralNineChar => {
            let mut normalized = id
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .take(9)
                .collect::<String>();
            while normalized.len() < 9 {
                normalized.push('0');
            }
            normalized
        }
    }
}

