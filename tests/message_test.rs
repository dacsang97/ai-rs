use ai_rs::{ImageUrl, Message, MessageEnvelope, MessageMetadata, ToolCallInfo};

#[test]
fn system_constructor() {
    let msg = Message::system("You are helpful.");
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "system");
    assert_eq!(json["content"], "You are helpful.");
}

#[test]
fn user_constructor() {
    let msg = Message::user("Hello!");
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["content"], "Hello!");
}

#[test]
fn assistant_constructor() {
    let msg = Message::assistant("Hi there.");
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "assistant");
    assert_eq!(json["content"], "Hi there.");
    assert!(json.get("tool_calls").is_none());
    assert!(json.get("reasoning").is_none());
}

#[test]
fn tool_result_constructor() {
    let msg = Message::tool_result("call_123", "file contents");
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "tool");
    assert_eq!(json["tool_call_id"], "call_123");
    assert_eq!(json["content"], "file contents");
}

#[test]
fn assistant_with_tool_calls_serialization() {
    let calls = vec![ToolCallInfo {
        id: "call_1".into(),
        name: "read_file".into(),
        arguments: r#"{"path":"foo.rs"}"#.into(),
        thought_signature: None,
    }];
    let msg = Message::assistant_with_tool_calls(Some("thinking...".into()), calls);
    let json = serde_json::to_value(&msg).unwrap();

    assert_eq!(json["role"], "assistant");
    assert_eq!(json["content"], "thinking...");
    let tc = json["tool_calls"].as_array().unwrap();
    assert_eq!(tc.len(), 1);
    assert_eq!(tc[0]["id"], "call_1");
    assert_eq!(tc[0]["name"], "read_file");
}

#[test]
fn tool_call_info_serialization() {
    let info = ToolCallInfo {
        id: "c1".into(),
        name: "bash".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
        thought_signature: None,
    };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["id"], "c1");
    assert_eq!(json["name"], "bash");
    assert_eq!(json["arguments"], r#"{"cmd":"ls"}"#);
}

#[test]
fn message_round_trip() {
    let msg = Message::user("round trip test");
    let serialized = serde_json::to_string(&msg).unwrap();
    let deserialized: Message = serde_json::from_str(&serialized).unwrap();
    let json = serde_json::to_value(&deserialized).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["content"], "round trip test");
}

#[test]
fn user_with_images_constructor() {
    let msg = Message::user_with_images(
        "hello",
        vec![ImageUrl {
            url: "data:image/png;base64,abc".into(),
            detail: None,
        }],
    );
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "user");
    let content = json["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "hello");
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(
        content[1]["image_url"]["url"],
        "data:image/png;base64,abc"
    );
}

#[test]
fn user_content_backward_compat() {
    let msg = Message::user("text");
    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["content"], "text");
}

#[test]
fn user_content_round_trip() {
    let msg = Message::user_with_images(
        "describe this",
        vec![ImageUrl {
            url: "data:image/jpeg;base64,xyz".into(),
            detail: Some("high".into()),
        }],
    );
    let serialized = serde_json::to_string(&msg).unwrap();
    let deserialized: Message = serde_json::from_str(&serialized).unwrap();
    let json = serde_json::to_value(&deserialized).unwrap();
    assert_eq!(json["role"], "user");
    let content = json["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(
        content[1]["image_url"]["url"],
        "data:image/jpeg;base64,xyz"
    );
    assert_eq!(content[1]["image_url"]["detail"], "high");
}

#[test]
fn message_envelope_with_metadata_serialization() {
    let envelope = MessageEnvelope::with_metadata(
        Message::assistant("done"),
        MessageMetadata {
            message_id: Some("msg-1".into()),
            run_id: Some("run-1".into()),
            step_id: Some("step-1".into()),
            data: Some(serde_json::json!({ "source": "agent-loop" })),
        },
    );

    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["message"]["role"], "assistant");
    assert_eq!(json["metadata"]["message_id"], "msg-1");
    assert_eq!(json["metadata"]["run_id"], "run-1");
    assert_eq!(json["metadata"]["step_id"], "step-1");
    assert_eq!(json["metadata"]["data"]["source"], "agent-loop");
}
