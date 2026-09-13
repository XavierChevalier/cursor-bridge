//! Contract: thinking + tool_call stream-json events reach Open WebUI-shaped SSE.
//!
//! Computer must see reasoning and tool traces. Never emit OpenAI delta.tool_calls
//! (Open WebUI would re-execute tools in a loop).

use std::path::PathBuf;

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn fixture_agent() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-agent")
}

fn app() -> axum::Router {
    let mut config = cursor_bridge::Config::test_default();
    config.agent_bin = fixture_agent().to_string_lossy().into_owned();
    let workspace = tempfile::tempdir().expect("workspace");
    config.workspace = workspace.path().to_string_lossy().into_owned();
    std::mem::forget(workspace);
    cursor_bridge::app(config)
}

#[tokio::test]
async fn stream_forwards_thinking_as_reasoning_content() {
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","stream":true,"messages":[{"role":"user","content":"__AGENTIC__ hello"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    };

    let mut reasoning = String::new();
    let mut content = String::new();
    for line in body.lines() {
        let Some(payload) = line.strip_prefix("data: ") else {
            continue;
        };
        if payload == "[DONE]" {
            continue;
        }
        let chunk: serde_json::Value = serde_json::from_str(payload).expect("sse json");
        let delta = &chunk["choices"][0]["delta"];
        assert!(
            delta.get("tool_calls").is_none(),
            "must never emit delta.tool_calls (Open WebUI loop): {payload}"
        );
        if let Some(piece) = delta["reasoning_content"].as_str() {
            reasoning.push_str(piece);
        }
        if let Some(piece) = delta["content"].as_str() {
            content.push_str(piece);
        }
    }

    assert!(
        reasoning.contains("planning"),
        "expected thinking mapped to reasoning_content, got {reasoning:?}"
    );
    assert!(
        content.contains("type=\"tool_calls\""),
        "expected tool details in content, got {content:?}"
    );
    assert!(
        content.contains("hello"),
        "expected assistant text in content, got {content:?}"
    );
}

#[tokio::test]
async fn non_stream_includes_reasoning_and_tool_details() {
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","stream":false,"messages":[{"role":"user","content":"__AGENTIC__ hello"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let message = &json["choices"][0]["message"];
    let reasoning = message["reasoning_content"].as_str().unwrap_or("");
    let content = message["content"].as_str().unwrap_or("");
    assert!(
        reasoning.contains("planning"),
        "non-stream missing reasoning_content: {message}"
    );
    assert!(
        content.contains("type=\"tool_calls\""),
        "non-stream missing tool details: {content}"
    );
    assert!(content.contains("hello"), "non-stream missing text: {content}");
}

#[tokio::test]
async fn accepts_multipart_message_content_array() {
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","stream":false,"messages":[{"role":"user","content":[{"type":"text","text":"ping"}]}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");
    assert!(
        content.contains("ping"),
        "multipart content must flatten into prompt echo: {content}"
    );
}

#[test]
fn map_stream_json_maps_thinking_and_tools() {
    let thinking = r#"{"type":"thinking","subtype":"delta","text":"hmm","timestamp_ms":1}"#;
    let tool_started = r#"{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"readToolCall":{"args":{"path":"README.md"}}}}"#;
    let tool_done = r#"{"type":"tool_call","subtype":"completed","call_id":"c1","tool_call":{"readToolCall":{"args":{"path":"README.md"},"result":{"success":{"totalLines":3}}}}}"#;
    let assistant = r#"{"type":"assistant","timestamp_ms":2,"message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;

    match cursor_bridge::agent::map_stream_json_line(thinking) {
        Some(cursor_bridge::agent::BridgeDelta::Reasoning(t)) => assert_eq!(t, "hmm"),
        other => panic!("expected Reasoning, got {other:?}"),
    }
    match cursor_bridge::agent::map_stream_json_line(tool_started) {
        Some(cursor_bridge::agent::BridgeDelta::Content(c)) => {
            assert!(c.contains("type=\"tool_calls\""));
            assert!(c.contains("done=\"false\""));
            assert!(c.contains("README.md"));
        }
        other => panic!("expected tool start Content, got {other:?}"),
    }
    match cursor_bridge::agent::map_stream_json_line(tool_done) {
        Some(cursor_bridge::agent::BridgeDelta::Content(c)) => {
            assert!(c.contains("done=\"true\""));
            assert!(c.contains("totalLines"));
        }
        other => panic!("expected tool done Content, got {other:?}"),
    }
    match cursor_bridge::agent::map_stream_json_line(assistant) {
        Some(cursor_bridge::agent::BridgeDelta::Content(c)) => assert_eq!(c, "hi"),
        other => panic!("expected assistant Content, got {other:?}"),
    }
}
