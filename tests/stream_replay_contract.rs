//! Contract: streaming SSE and no prior-assistant replay into the agent prompt.

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
async fn chat_sends_only_latest_user_turn_to_agent() {
    // Fake agent echoes the prompt. Prior assistant text must not be included.
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
                        {"role":"user","content":"ONE"},
                        {"role":"assistant","content":"ONE"},
                        {"role":"user","content":"TWO"}
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
    assert_eq!(content, "TWO");
    assert!(
        !content.contains("ONE"),
        "prior assistant/user text leaked into agent prompt: {content}"
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
    assert_eq!(assembled, "OK");
}
