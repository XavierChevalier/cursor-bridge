//! One-shot setup UI: open during first login, then gone; /v1 stays Bearer-gated.

use std::path::PathBuf;
use std::time::Duration;

use axum::body::Body;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn fixture_agent() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-agent")
}

fn app_with_state_dir(state_dir: &std::path::Path, force_setup: bool) -> axum::Router {
    let mut config = cursor_bridge::Config::test_default();
    config.agent_bin = fixture_agent().to_string_lossy().into_owned();
    config.state_dir = state_dir.to_path_buf();
    let workspace = tempfile::tempdir().expect("workspace");
    config.workspace = workspace.path().to_string_lossy().into_owned();
    std::mem::forget(workspace);
    cursor_bridge::app_with_options(config, cursor_bridge::AppOptions { force_setup })
}

#[tokio::test]
async fn setup_page_is_open_without_bearer_during_setup() {
    let state = tempfile::tempdir().unwrap();
    let app = app_with_state_dir(state.path(), true);
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body =
        String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes()).into_owned();
    assert!(body.contains("Start login"), "body={body}");
    assert!(body.contains("No bridge API key needed here"), "body={body}");
}

#[tokio::test]
async fn setup_login_keeps_process_alive_until_status_flips() {
    let state = tempfile::tempdir().unwrap();
    let app = app_with_state_dir(state.path(), true);

    let login = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/setup/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let login_status = login.status();
    let login_body = login.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        login_status,
        axum::http::StatusCode::OK,
        "body={}",
        String::from_utf8_lossy(&login_body)
    );
    let json: serde_json::Value = serde_json::from_slice(&login_body).unwrap();
    let url = json["login_url"].as_str().expect("login_url");
    assert!(url.starts_with("https://"), "url={url}");

    // Still logged out until OAuth completes (login child must still be running).
    let pending = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/setup/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let pending_json: serde_json::Value =
        serde_json::from_slice(&pending.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(pending_json["logged_in"], false, "body={pending_json}");

    // Simulate browser OAuth completing while agent login is still alive.
    std::fs::write(state.path().join("logged_in"), b"1").unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let status = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/setup/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status_body = status.into_body().collect().await.unwrap().to_bytes();
    let status_json: serde_json::Value = serde_json::from_slice(&status_body).unwrap();
    assert_eq!(status_json["logged_in"], true, "body={status_json}");
    assert_eq!(status_json["setup_enabled"], false);

    let gone = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gone.status(), axum::http::StatusCode::GONE);
    assert!(state.path().join(".cursor-bridge-setup-complete").is_file());

    let unauthorized = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn setup_stays_gone_when_flag_present() {
    let state = tempfile::tempdir().unwrap();
    std::fs::write(state.path().join(".cursor-bridge-setup-complete"), b"1").unwrap();
    let app = app_with_state_dir(state.path(), false);
    let gone = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gone.status(), axum::http::StatusCode::GONE);
}
