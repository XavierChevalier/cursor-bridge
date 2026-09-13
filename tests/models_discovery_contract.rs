//! Contract: when discovery is enabled, GET /v1/models includes Cursor CLI models.

use std::path::PathBuf;

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn fixture_agent() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-agent")
}

fn app_with_discovery() -> axum::Router {
    let mut config = cursor_bridge::Config::test_default();
    config.agent_bin = fixture_agent().to_string_lossy().into_owned();
    config.discover_models = true;
    let workspace = tempfile::tempdir().expect("workspace");
    config.workspace = workspace.path().to_string_lossy().into_owned();
    std::mem::forget(workspace);
    cursor_bridge::app(config)
}

#[tokio::test]
async fn models_list_includes_discovered_cursor_models() {
    let response = app_with_discovery()
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/models")
                .header("Authorization", "Bearer test-bridge-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ids: Vec<&str> = json["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|m| m["id"].as_str().expect("id"))
        .collect();

    assert!(
        ids.contains(&"cursor-auto"),
        "default bridge id must remain: {ids:?}"
    );
    assert!(
        ids.contains(&"composer-2.5"),
        "expected discovered composer-2.5 in {ids:?}"
    );
    assert!(
        ids.contains(&"gpt-5.2"),
        "expected discovered gpt-5.2 in {ids:?}"
    );
    assert!(
        !ids.contains(&"auto"),
        "CLI 'auto' must map to cursor-auto, not a duplicate id: {ids:?}"
    );
}

#[tokio::test]
async fn chat_accepts_discovered_model_id() {
    let response = app_with_discovery()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Authorization", "Bearer test-bridge-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"model":"composer-2.5","stream":false,"messages":[{"role":"user","content":"hi"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "discovered model must be allowed for chat"
    );
}

#[test]
fn parse_models_output_reads_id_label_lines() {
    let raw = "\
Available models
auto - Auto (current, default)
composer-2.5 - Composer 2.5
gpt-5.2 - GPT-5.2
";
    let parsed = cursor_bridge::agent::parse_models_output(raw);
    let ids: Vec<&str> = parsed.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["auto", "composer-2.5", "gpt-5.2"]);
    assert_eq!(parsed[1].label, "Composer 2.5");
}
