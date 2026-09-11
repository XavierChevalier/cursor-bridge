//! Contract: GET /v1/models requires a bridge API key and lists cursor-auto.

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app() -> axum::Router {
    cursor_bridge::app(cursor_bridge::Config::test_default())
}

#[tokio::test]
async fn models_without_bearer_returns_401() {
    let response = app()
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn models_with_bearer_lists_cursor_auto() {
    let response = app()
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
        .map(|m| m["id"].as_str().expect("id string"))
        .collect();
    assert!(
        ids.contains(&"cursor-auto"),
        "expected cursor-auto in {ids:?}"
    );
}
