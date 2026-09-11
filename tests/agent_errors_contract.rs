//! Contract: clear errors when the Cursor CLI is missing or not authenticated.

use std::path::PathBuf;

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app_with_agent(agent: PathBuf) -> axum::Router {
    let mut config = cursor_bridge::Config::test_default();
    config.agent_bin = agent.to_string_lossy().into_owned();
    let workspace = tempfile::tempdir().expect("workspace");
    config.workspace = workspace.path().to_string_lossy().into_owned();
    std::mem::forget(workspace);
    cursor_bridge::app(config)
}

#[tokio::test]
async fn missing_agent_binary_returns_502_with_clear_message() {
    let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/does-not-exist");
    let response = app_with_agent(missing)
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","stream":false,"messages":[{"role":"user","content":"hi"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let message = json["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.to_lowercase().contains("spawn") || message.to_lowercase().contains("agent"),
        "expected spawn/agent error, got {message}"
    );
    assert!(
        !message.contains("test-bridge-key"),
        "must not leak api key: {message}"
    );
}

#[tokio::test]
async fn auth_required_stderr_is_surfaced_without_secrets() {
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-agent-unauth");
    let response = app_with_agent(agent)
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"cursor-auto","stream":false,"messages":[{"role":"user","content":"hi"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let message = json["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.to_lowercase().contains("not logged in")
            || message.to_lowercase().contains("authentication"),
        "expected auth hint, got {message}"
    );
}
