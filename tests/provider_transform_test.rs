use ai_rs::message::{ContentPart, ImageUrl, UserContent};
use ai_rs::provider::transform::{
    ProviderCompatOptions, ToolCallIdMode, normalize_messages,
};
use ai_rs::Message;
use serde_json::json;

#[test]
fn compat_options_parse_from_provider_options() {
    let value = json!({
        "compat": {
            "filter_empty_messages": true,
            "ensure_assistant_after_tool": true,
            "fallback_image_to_text": true,
            "tool_call_id_mode": "safe_ascii"
        }
    });

    let options = ProviderCompatOptions::from_provider_options(Some(&value)).unwrap();
    assert!(options.filter_empty_messages);
    assert!(options.ensure_assistant_after_tool);
    assert!(options.fallback_image_to_text);
    assert_eq!(options.tool_call_id_mode, Some(ToolCallIdMode::SafeAscii));
}

#[test]
fn filters_empty_messages_and_parts() {
    let messages = vec![
        Message::system(""),
        Message::user(""),
        Message::User {
            content: UserContent::Parts(vec![
                ContentPart::Text { text: "".into() },
                ContentPart::Text { text: "keep".into() },
            ]),
        },
        Message::Assistant {
            content: Some("".into()),
            reasoning: None,
            tool_calls: None,
        },
    ];

    let normalized = normalize_messages(
        &messages,
        &ProviderCompatOptions {
            filter_empty_messages: true,
            ..Default::default()
        },
    );

    assert_eq!(normalized.len(), 1);
    match &normalized[0] {
        Message::User {
            content: UserContent::Parts(parts),
        } => {
            assert_eq!(parts.len(), 1);
            assert!(matches!(&parts[0], ContentPart::Text { text } if text == "keep"));
        }
        other => panic!("unexpected message: {:?}", other),
    }
}

#[test]
fn inserts_assistant_after_tool_before_user() {
    let messages = vec![
        Message::tool_result("call-1", "done"),
        Message::user("next question"),
    ];

    let normalized = normalize_messages(
        &messages,
        &ProviderCompatOptions {
            ensure_assistant_after_tool: true,
            ..Default::default()
        },
    );

    assert_eq!(normalized.len(), 3);
    assert!(matches!(normalized[1], Message::Assistant { .. }));
}

#[test]
fn normalizes_tool_call_ids_and_tool_results() {
    let messages = vec![
        Message::assistant_with_tool_calls(
            None,
            vec![ai_rs::ToolCallInfo {
                id: "call:bad/id".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            }],
        ),
        Message::tool_result("call:bad/id", "ok"),
    ];

    let normalized = normalize_messages(
        &messages,
        &ProviderCompatOptions {
            tool_call_id_mode: Some(ToolCallIdMode::SafeAscii),
            ..Default::default()
        },
    );

    match &normalized[0] {
        Message::Assistant {
            tool_calls: Some(tool_calls),
            ..
        } => assert_eq!(tool_calls[0].id, "call_bad_id"),
        other => panic!("unexpected assistant message: {:?}", other),
    }

    match &normalized[1] {
        Message::Tool { tool_call_id, .. } => assert_eq!(tool_call_id, "call_bad_id"),
        other => panic!("unexpected tool message: {:?}", other),
    }
}

#[test]
fn falls_back_images_to_text() {
    let messages = vec![Message::user_with_images(
        "look",
        vec![ImageUrl {
            url: "data:image/png;base64,abc".into(),
            detail: None,
        }],
    )];

    let normalized = normalize_messages(
        &messages,
        &ProviderCompatOptions {
            fallback_image_to_text: true,
            ..Default::default()
        },
    );

    match &normalized[0] {
        Message::User {
            content: UserContent::Parts(parts),
        } => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[1], ContentPart::Text { text } if text.starts_with("[image: data:image/png")));
        }
        other => panic!("unexpected message: {:?}", other),
    }
}

