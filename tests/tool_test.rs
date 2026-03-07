use ai_rs::{ToolRegistry, ToolResult};

#[test]
fn tool_registry_new_is_empty() {
    let registry = ToolRegistry::new();
    assert!(registry.definitions().is_empty());
}

#[test]
fn tool_registry_default_is_empty() {
    let registry = ToolRegistry::default();
    assert!(registry.definitions().is_empty());
}

#[test]
fn tool_result_construction() {
    let result = ToolResult {
        output: "file contents here".into(),
        title: Some("Read foo.rs".into()),
        is_error: false,
    };
    assert_eq!(result.output, "file contents here");
    assert_eq!(result.title.as_deref(), Some("Read foo.rs"));
    assert!(!result.is_error);
}

#[test]
fn tool_result_error() {
    let result = ToolResult {
        output: "command not found".into(),
        title: None,
        is_error: true,
    };
    assert!(result.is_error);
    assert!(result.title.is_none());
}

#[test]
fn tool_result_serialization() {
    let result = ToolResult {
        output: "ok".into(),
        title: None,
        is_error: false,
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["output"], "ok");
    assert_eq!(json["is_error"], false);
}
