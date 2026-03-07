use ai_rs::types::{Role, StopReason, TokenUsage};

#[test]
fn token_usage_new_computes_total() {
    let usage = TokenUsage::new(150, 350);
    assert_eq!(usage.input_tokens, 150);
    assert_eq!(usage.output_tokens, 350);
    assert_eq!(usage.total_tokens, 500);
}

#[test]
fn token_usage_new_zero() {
    let usage = TokenUsage::new(0, 0);
    assert_eq!(usage.total_tokens, 0);
}

#[test]
fn token_usage_default() {
    let usage = TokenUsage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
}

#[test]
fn role_serialization_lowercase() {
    assert_eq!(serde_json::to_string(&Role::System).unwrap(), r#""system""#);
    assert_eq!(serde_json::to_string(&Role::User).unwrap(), r#""user""#);
    assert_eq!(serde_json::to_string(&Role::Assistant).unwrap(), r#""assistant""#);
    assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), r#""tool""#);
    assert_eq!(serde_json::to_string(&Role::Developer).unwrap(), r#""developer""#);
}

#[test]
fn role_deserialization() {
    let role: Role = serde_json::from_str(r#""user""#).unwrap();
    assert_eq!(role, Role::User);

    let role: Role = serde_json::from_str(r#""assistant""#).unwrap();
    assert_eq!(role, Role::Assistant);
}

#[test]
fn stop_reason_serialization_snake_case() {
    assert_eq!(serde_json::to_string(&StopReason::Stop).unwrap(), r#""stop""#);
    assert_eq!(serde_json::to_string(&StopReason::Length).unwrap(), r#""length""#);
    assert_eq!(serde_json::to_string(&StopReason::ToolCalls).unwrap(), r#""tool_calls""#);
    assert_eq!(serde_json::to_string(&StopReason::ContentFilter).unwrap(), r#""content_filter""#);
}

#[test]
fn stop_reason_deserialization() {
    let reason: StopReason = serde_json::from_str(r#""tool_calls""#).unwrap();
    assert_eq!(reason, StopReason::ToolCalls);
}

#[test]
fn token_usage_serialization_round_trip() {
    let usage = TokenUsage::new(42, 58);
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: TokenUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.input_tokens, 42);
    assert_eq!(parsed.output_tokens, 58);
    assert_eq!(parsed.total_tokens, 100);
}
