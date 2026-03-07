use ai_rs::{StreamEvent, TokenUsage};

#[test]
fn text_start_serialization() {
    let event = StreamEvent::TextStart {
        part_id: "p1".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "text-start");
    assert_eq!(json["part_id"], "p1");
}

#[test]
fn text_delta_serialization() {
    let event = StreamEvent::TextDelta {
        part_id: "p1".into(),
        delta: "hello ".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "text-delta");
    assert_eq!(json["part_id"], "p1");
    assert_eq!(json["delta"], "hello ");
}

#[test]
fn text_end_serialization() {
    let event = StreamEvent::TextEnd {
        part_id: "p1".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "text-end");
}

#[test]
fn reasoning_start_serialization() {
    let event = StreamEvent::ReasoningStart {
        part_id: "r1".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "reasoning-start");
    assert_eq!(json["part_id"], "r1");
}

#[test]
fn reasoning_delta_serialization() {
    let event = StreamEvent::ReasoningDelta {
        part_id: "r1".into(),
        delta: "hmm".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "reasoning-delta");
    assert_eq!(json["delta"], "hmm");
}

#[test]
fn tool_pending_serialization() {
    let event = StreamEvent::ToolPending {
        call_id: "call_42".into(),
        tool_name: "bash".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool-pending");
    assert_eq!(json["call_id"], "call_42");
    assert_eq!(json["tool_name"], "bash");
}

#[test]
fn tool_input_delta_serialization() {
    let event = StreamEvent::ToolInputDelta {
        call_id: "call_42".into(),
        delta: r#"{"cmd":"#.into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool-input-delta");
    assert_eq!(json["call_id"], "call_42");
}

#[test]
fn tool_running_serialization() {
    let event = StreamEvent::ToolRunning {
        call_id: "call_42".into(),
        tool_name: Some("bash".into()),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool-running");
}

#[test]
fn tool_completed_serialization() {
    let event = StreamEvent::ToolCompleted {
        call_id: "call_42".into(),
        output: "done".into(),
        title: Some("Run bash".into()),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool-completed");
    assert_eq!(json["output"], "done");
    assert_eq!(json["title"], "Run bash");
}

#[test]
fn tool_error_serialization() {
    let event = StreamEvent::ToolError {
        call_id: "call_42".into(),
        error: "command failed".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "tool-error");
    assert_eq!(json["error"], "command failed");
}

#[test]
fn step_finish_with_token_usage() {
    let event = StreamEvent::StepFinish {
        tokens: TokenUsage::new(100, 200),
        cost: 0.003,
        reason: "stop".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "step-finish");
    assert_eq!(json["tokens"]["input_tokens"], 100);
    assert_eq!(json["tokens"]["output_tokens"], 200);
    assert_eq!(json["tokens"]["total_tokens"], 300);
    assert_eq!(json["cost"], 0.003);
    assert_eq!(json["reason"], "stop");
}

#[test]
fn run_complete_serialization() {
    let event = StreamEvent::RunComplete;
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "run-complete");
    // RunComplete has no other fields
    assert_eq!(json.as_object().unwrap().len(), 1);
}

#[test]
fn run_error_serialization() {
    let event = StreamEvent::RunError {
        error: "timeout".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "run-error");
    assert_eq!(json["error"], "timeout");
}

#[test]
fn run_aborted_serialization() {
    let event = StreamEvent::RunAborted;
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "run-aborted");
}

#[test]
fn all_tags_are_kebab_case() {
    // Exhaustive check that every variant uses kebab-case tags
    let events: Vec<StreamEvent> = vec![
        StreamEvent::TextStart { part_id: "x".into() },
        StreamEvent::TextDelta { part_id: "x".into(), delta: "d".into() },
        StreamEvent::TextEnd { part_id: "x".into() },
        StreamEvent::ReasoningStart { part_id: "x".into() },
        StreamEvent::ReasoningDelta { part_id: "x".into(), delta: "d".into() },
        StreamEvent::ReasoningEnd { part_id: "x".into() },
        StreamEvent::ToolPending { call_id: "x".into(), tool_name: "t".into() },
        StreamEvent::ToolInputDelta { call_id: "x".into(), delta: "d".into() },
        StreamEvent::ToolRunning { call_id: "x".into(), tool_name: None },
        StreamEvent::ToolCompleted { call_id: "x".into(), output: "o".into(), title: None },
        StreamEvent::ToolError { call_id: "x".into(), error: "e".into() },
        StreamEvent::StepFinish { tokens: TokenUsage::new(0, 0), cost: 0.0, reason: "stop".into() },
        StreamEvent::RunComplete,
        StreamEvent::RunError { error: "e".into() },
        StreamEvent::RunAborted,
    ];

    for event in &events {
        let json = serde_json::to_value(event).unwrap();
        let tag = json["type"].as_str().unwrap();
        assert!(
            tag.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "Tag '{}' is not kebab-case",
            tag
        );
    }
}
