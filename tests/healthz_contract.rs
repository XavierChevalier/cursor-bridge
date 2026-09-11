//! Contract: GET /healthz is a public liveness probe.

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn healthz_returns_200_without_auth() {
    let app = cursor_bridge::app(cursor_bridge::Config::test_default());

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        !text.to_lowercase().contains("bearer"),
        "healthz must not leak credentials: {text}"
    );
    assert!(
        !text.contains("sk-"),
        "healthz must not look like an api key payload: {text}"
    );
}
