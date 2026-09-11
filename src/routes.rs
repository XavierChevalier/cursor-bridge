//! HTTP routes for the OpenAI-compatible bridge.

use axum::{
    body::Body,
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent;
use crate::Config;

pub fn router(config: Config) -> Router {
    let protected = Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(from_fn_with_state(config.clone(), require_bearer));

    Router::new()
        .route("/healthz", get(healthz))
        .merge(protected)
        .with_state(config)
}

async fn healthz() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn list_models(State(config): State<Config>) -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [{
            "id": config.model_id,
            "object": "model",
            "owned_by": "cursor-bridge"
        }]
    }))
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    #[serde(default)]
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(default)]
    stream: bool,
}

async fn chat_completions(
    State(config): State<Config>,
    Json(body): Json<ChatRequest>,
) -> Response {
    let prompt = match latest_user_prompt(&body.messages) {
        Some(text) => text,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": "messages must include a user turn",
                        "type": "invalid_request_error"
                    }
                })),
            )
                .into_response();
        }
    };

    let model = if body.model.is_empty() {
        config.model_id.clone()
    } else {
        body.model.clone()
    };

    let content = match agent::print_turn(&config, &prompt).await {
        Ok(content) => content,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": {
                        "message": err.to_string(),
                        "type": "server_error"
                    }
                })),
            )
                .into_response();
        }
    };

    if body.stream {
        return sse_completion(&model, &content);
    }

    Json(json!({
        "id": "chatcmpl-bridge",
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": "stop"
        }]
    }))
    .into_response()
}

fn sse_completion(model: &str, content: &str) -> Response {
    let mut body = String::new();
    for ch in content.chars() {
        let piece = ch.to_string();
        let chunk = json!({
            "id": "chatcmpl-bridge",
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": { "content": piece },
                "finish_reason": null
            }]
        });
        body.push_str("data: ");
        body.push_str(&chunk.to_string());
        body.push_str("\n\n");
    }
    let done = json!({
        "id": "chatcmpl-bridge",
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop"
        }]
    });
    body.push_str("data: ");
    body.push_str(&done.to_string());
    body.push_str("\n\n");
    body.push_str("data: [DONE]\n\n");

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );

    (StatusCode::OK, headers, body).into_response()
}

fn latest_user_prompt(messages: &[ChatMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.clone())
}

async fn require_bearer(
    State(config): State<Config>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == config.api_key);

    if authorized {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": {
                    "message": "invalid api key",
                    "type": "invalid_request_error"
                }
            })),
        )
            .into_response()
    }
}
