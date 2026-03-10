use ai_rs::stream::google::parse_google_chunk;
use ai_rs::stream::handler::StreamChunk;
use ai_rs::types::StopReason;

#[test]
fn text_only_chunk() {
    let raw = r#"{ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Hello world" }] } }] }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert_eq!(chunks.len(), 1);
    assert!(
        matches!(&chunks[0], StreamChunk::TextDelta(t) if t == "Hello world"),
        "expected TextDelta, got {:?}",
        chunks[0]
    );
}

#[test]
fn reasoning_chunk_with_thought() {
    let raw = r#"{ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Let me think...", "thought": true }] } }] }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert_eq!(chunks.len(), 1);
    assert!(
        matches!(&chunks[0], StreamChunk::ReasoningDelta(t) if t == "Let me think..."),
        "expected ReasoningDelta, got {:?}",
        chunks[0]
    );
}

#[test]
fn tool_call_chunk() {
    let raw = r#"{ "candidates": [{ "content": { "role": "model", "parts": [{ "functionCall": { "name": "read_file", "args": { "path": "/tmp/test" } } }] } }] }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert_eq!(chunks.len(), 2);
    assert!(
        matches!(&chunks[0], StreamChunk::ToolCallStart { index: 0, name, .. } if name == "read_file"),
        "expected ToolCallStart, got {:?}",
        chunks[0]
    );
    assert!(
        matches!(&chunks[1], StreamChunk::ToolCallDelta { index: 0, arguments } if arguments.contains("/tmp/test")),
        "expected ToolCallDelta, got {:?}",
        chunks[1]
    );
}

#[test]
fn finish_reason_stop() {
    let raw = r#"{ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "done" }] }, "finishReason": "STOP" }] }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert_eq!(chunks.len(), 2);
    assert!(matches!(&chunks[0], StreamChunk::TextDelta(t) if t == "done"));
    assert!(
        matches!(&chunks[1], StreamChunk::Done { stop_reason: Some(StopReason::Stop), .. }),
        "expected Done with Stop, got {:?}",
        chunks[1]
    );
}

#[test]
fn usage_only_chunk() {
    let raw = r#"{ "candidates": [], "usageMetadata": { "promptTokenCount": 100, "candidatesTokenCount": 50, "thoughtsTokenCount": 20, "totalTokenCount": 170 } }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert_eq!(chunks.len(), 1);
    match &chunks[0] {
        StreamChunk::Done { stop_reason, usage } => {
            assert_eq!(*stop_reason, None);
            let u = usage.as_ref().expect("expected usage");
            assert_eq!(u.input_tokens, 100);
            assert_eq!(u.output_tokens, 70); // candidates(50) + thoughts(20)
            assert_eq!(u.total_tokens, 170);
        }
        other => panic!("expected Done, got {:?}", other),
    }
}

#[test]
fn mixed_text_and_tool_call() {
    let raw = r#"{
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    { "text": "I'll read that file." },
                    { "functionCall": { "name": "read_file", "args": { "path": "/tmp/x" } } }
                ]
            }
        }]
    }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert_eq!(chunks.len(), 3);
    assert!(matches!(&chunks[0], StreamChunk::TextDelta(t) if t == "I'll read that file."));
    assert!(matches!(&chunks[1], StreamChunk::ToolCallStart { name, .. } if name == "read_file"));
    assert!(matches!(&chunks[2], StreamChunk::ToolCallDelta { index: 0, .. }));
}

#[test]
fn empty_parts_array() {
    let raw = r#"{ "candidates": [{ "content": { "role": "model", "parts": [] } }] }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert!(chunks.is_empty(), "expected empty vec, got {:?}", chunks);
}

#[test]
fn finish_reason_max_tokens() {
    let raw = r#"{ "candidates": [{ "content": { "role": "model", "parts": [] }, "finishReason": "MAX_TOKENS" }] }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert_eq!(chunks.len(), 1);
    assert!(
        matches!(&chunks[0], StreamChunk::Done { stop_reason: Some(StopReason::Length), .. }),
        "expected Done with Length, got {:?}",
        chunks[0]
    );
}

#[test]
fn finish_reason_safety() {
    let raw = r#"{ "candidates": [{ "content": { "role": "model", "parts": [] }, "finishReason": "SAFETY" }] }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    assert_eq!(chunks.len(), 1);
    assert!(
        matches!(&chunks[0], StreamChunk::Done { stop_reason: Some(StopReason::ContentFilter), .. }),
    );
}

#[test]
fn finish_with_usage_metadata() {
    let raw = r#"{
        "candidates": [{
            "content": { "role": "model", "parts": [{ "text": "ok" }] },
            "finishReason": "STOP"
        }],
        "usageMetadata": { "promptTokenCount": 10, "candidatesTokenCount": 5, "thoughtsTokenCount": 0 }
    }"#;
    let chunks = parse_google_chunk(raw).unwrap();
    // TextDelta + Done (with usage from usageMetadata)
    assert_eq!(chunks.len(), 2);
    match &chunks[1] {
        StreamChunk::Done { stop_reason, usage } => {
            assert_eq!(*stop_reason, Some(StopReason::Stop));
            let u = usage.as_ref().expect("expected usage");
            assert_eq!(u.input_tokens, 10);
            assert_eq!(u.output_tokens, 5);
        }
        other => panic!("expected Done, got {:?}", other),
    }
}

// --- OpenRouter provider test ---

use ai_rs::provider::openai_compat::OpenAiCompatibleProvider;
use ai_rs::provider::Provider;

#[test]
fn openrouter_provider_created_with_headers() {
    let mut headers = std::collections::HashMap::new();
    headers.insert("HTTP-Referer".to_string(), "https://1system.app".to_string());
    headers.insert(
        "X-OpenRouter-Title".to_string(),
        "1System".to_string(),
    );
    let provider = OpenAiCompatibleProvider::with_headers(
        "https://openrouter.ai/api/v1",
        "test-key",
        "openai/gpt-4o",
        headers,
    );
    assert_eq!(provider.name(), "openai-compatible");
    assert_eq!(provider.model(), "openai/gpt-4o");
}
