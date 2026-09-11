//! Contract: POST /v1/chat/completions talks to a real fake agent process.

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
    // Keep tempdir alive for the process lifetime of this test via leak (ok in tests).
    std::mem::forget(workspace);
    cursor_bridge::app(config)
}

#[tokio::test]
async fn chat_without_bearer_returns_401() {
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","messages":[{"role":"user","content":"hi"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn chat_non_stream_content_is_exactly_ok() {
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","messages":[{"role":"user","content":"Say something"}],"stream":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "body={}",
        String::from_utf8_lossy(&body)
    );
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .expect("content string");
    assert_eq!(content, "User: Say something");
}
