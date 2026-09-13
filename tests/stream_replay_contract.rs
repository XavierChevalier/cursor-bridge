//! Contract: streaming SSE and multi-turn history forwarded to the agent prompt.

use std::path::PathBuf;
use std::time::{Duration, Instant};

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
async fn chat_forwards_prior_turns_in_agent_prompt() {
    // Fake agent echoes the prompt. OpenAI clients send full history each request;
    // print-mode turns have no sticky Cursor session, so prior turns must be included.
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{
                      "model":"cursor-auto",
                      "stream":false,
                      "messages":[
                        {"role":"user","content":"Create a file"},
                        {"role":"assistant","content":"What should it be named?"},
                        {"role":"user","content":"What did we say in this conversation?"}
                      ]
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .expect("content");
    assert!(
        content.contains("Create a file"),
        "missing prior user turn in agent prompt: {content}"
    );
    assert!(
        content.contains("What should it be named?"),
        "missing prior assistant turn in agent prompt: {content}"
    );
    assert!(
        content.contains("What did we say in this conversation?"),
        "missing latest user turn in agent prompt: {content}"
    );
}

#[tokio::test]
async fn chat_stream_emits_sse_with_reassembled_content() {
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","stream":true,"messages":[{"role":"user","content":"OK"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "content-type={content_type}"
    );

    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();

    assert!(
        body.contains("data: [DONE]"),
        "missing [DONE] in stream:\n{body}"
    );

    let mut assembled = String::new();
    for line in body.lines() {
        let Some(payload) = line.strip_prefix("data: ") else {
            continue;
        };
        if payload == "[DONE]" {
            continue;
        }
        let chunk: serde_json::Value = serde_json::from_str(payload).expect("sse json");
        if let Some(piece) = chunk["choices"][0]["delta"]["content"].as_str() {
            assembled.push_str(piece);
        }
    }
    assert_eq!(assembled, "User: OK");
}

/// Root-cause guard: Open WebUI Computer expects tokens while the agent is still
/// generating. Buffering the full CLI turn before the first SSE byte looks like
/// "no streaming" in the UI.
#[tokio::test]
async fn chat_stream_emits_first_sse_before_agent_finishes() {
    let started = Instant::now();
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","stream":true,"messages":[{"role":"user","content":"__SLOW_STREAM__ abcdefghijklmnop"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let mut body = response.into_body();
    let mut buf = Vec::new();
    let mut first_data_at = None;

    loop {
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .expect("timed out waiting for SSE frame")
            .expect("body frame error")
            .expect("unexpected end of SSE body before [DONE]");
        if let Ok(data) = frame.into_data() {
            if first_data_at.is_none() && !data.is_empty() {
                first_data_at = Some(started.elapsed());
            }
            buf.extend_from_slice(&data);
            if std::str::from_utf8(&buf).unwrap_or("").contains("data: [DONE]") {
                break;
            }
        }
    }

    let first = first_data_at.expect("expected at least one SSE data frame");
    assert!(
        first < Duration::from_millis(250),
        "first SSE byte arrived at {first:?}; expected <250ms. \
         The bridge likely waited for the full agent turn (fake sleeps 400ms mid-stream) \
         before writing any SSE; Open WebUI shows no live generation."
    );

    let total = started.elapsed();
    assert!(
        total >= Duration::from_millis(350),
        "stream finished too fast ({total:?}); slow fake agent did not pause mid-turn"
    );

    let text = String::from_utf8(buf).unwrap();
    let mut assembled = String::new();
    for line in text.lines() {
        let Some(payload) = line.strip_prefix("data: ") else {
            continue;
        };
        if payload == "[DONE]" {
            continue;
        }
        let chunk: serde_json::Value = serde_json::from_str(payload).expect("sse json");
        if let Some(piece) = chunk["choices"][0]["delta"]["content"].as_str() {
            assembled.push_str(piece);
        }
    }
    assert!(
        assembled.contains("__SLOW_STREAM__"),
        "reassembled stream missing prompt echo: {assembled}"
    );
}
