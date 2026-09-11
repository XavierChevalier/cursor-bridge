//! HTTP routes for the OpenAI-compatible bridge and one-shot setup UI.

use axum::{
    body::Body,
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent;
use crate::setup::{mark_setup_complete, AppState};

pub fn router(state: AppState) -> Router {
    let protected_api = Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(from_fn_with_state(state.clone(), require_bearer));

    // First-time setup is intentionally open (Tailscale is the gate). After
    // login succeeds these handlers return 410 and /v1 stays Bearer-protected.
    let setup = Router::new()
        .route("/", get(setup_page))
        .route("/setup/login", post(setup_login))
        .route("/setup/status", get(setup_status));

    Router::new()
        .route("/healthz", get(healthz))
        .merge(protected_api)
        .merge(setup)
        .with_state(state)
}

async fn healthz() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn setup_page(State(state): State<AppState>) -> Response {
    if !state.setup.is_enabled() {
        return (
            StatusCode::GONE,
            Json(json!({
                "error": {
                    "message": "setup UI is disabled (Cursor already logged in, or setup completed)",
                    "type": "setup_complete"
                }
            })),
        )
            .into_response();
    }

    Html(SETUP_HTML).into_response()
}

async fn setup_login(State(state): State<AppState>) -> Response {
    if !state.setup.is_enabled() {
        return (
            StatusCode::GONE,
            Json(json!({
                "error": {
                    "message": "setup UI is disabled",
                    "type": "setup_complete"
                }
            })),
        )
            .into_response();
    }

    // Reuse an in-flight login so refreshing the page does not kill OAuth.
    {
        let guard = state.login.lock().await;
        if let Some(proc) = guard.as_ref() {
            return Json(json!({
                "login_url": proc.url,
                "setup_enabled": state.setup.is_enabled()
            }))
            .into_response();
        }
    }

    match agent::start_login(&state.config).await {
        Ok(proc) => {
            let login_url = proc.url.clone();
            *state.login.lock().await = Some(proc);
            Json(json!({
                "login_url": login_url,
                "setup_enabled": state.setup.is_enabled()
            }))
            .into_response()
        }
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": {
                    "message": sanitize_agent_error(&err.to_string()),
                    "type": "server_error"
                }
            })),
        )
            .into_response(),
    }
}

async fn setup_status(State(state): State<AppState>) -> Response {
    if !state.setup.is_enabled() {
        return Json(json!({
            "logged_in": true,
            "setup_enabled": false,
            "summary": "setup complete"
        }))
        .into_response();
    }

    match agent::status(&state.config).await {
        Ok(status) => {
            if status.logged_in {
                let _ = mark_setup_complete(&state).await;
            }
            Json(json!({
                "logged_in": status.logged_in,
                "setup_enabled": state.setup.is_enabled(),
                "summary": status.summary
            }))
            .into_response()
        }
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": {
                    "message": sanitize_agent_error(&err.to_string()),
                    "type": "server_error"
                }
            })),
        )
            .into_response(),
    }
}

const SETUP_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Cursor Bridge setup</title>
  <style>
    :root { color-scheme: light dark; font-family: ui-sans-serif, system-ui, sans-serif; }
    body { max-width: 40rem; margin: 2rem auto; padding: 0 1rem; line-height: 1.45; }
    button { font: inherit; padding: 0.5rem 1rem; cursor: pointer; }
    .row { margin: 1rem 0; }
    #url a { word-break: break-all; }
    .muted { opacity: 0.75; font-size: 0.95rem; }
    .err { color: #b00020; }
  </style>
</head>
<body>
  <h1>Cursor Bridge setup</h1>
  <p class="muted">One-time Cursor CLI login. No bridge API key needed here. This page disables itself after success; <code>/v1</code> stays Bearer-protected.</p>
  <div class="row">
    <button id="login" type="button">Start login</button>
  </div>
  <p id="status" class="muted">Not started.</p>
  <p id="url"></p>
  <p id="err" class="err"></p>
  <script>
    const statusEl = document.getElementById('status');
    const urlEl = document.getElementById('url');
    const errEl = document.getElementById('err');

    async function refreshStatus() {
      const res = await fetch('/setup/status');
      const body = await res.json();
      if (!res.ok) throw new Error(body.error?.message || res.statusText);
      statusEl.textContent = body.logged_in
        ? ('Logged in. Setup ' + (body.setup_enabled ? 'still open' : 'disabled.'))
        : (body.summary || 'Not logged in');
      if (body.logged_in && body.setup_enabled === false) {
        statusEl.textContent = 'Setup complete. This UI is gone; clients use /v1 with the bridge API key.';
        document.getElementById('login').disabled = true;
      }
      return body;
    }

    document.getElementById('login').onclick = async () => {
      errEl.textContent = '';
      urlEl.textContent = '';
      try {
        statusEl.textContent = 'Starting agent login… keep this page open until status flips.';
        const res = await fetch('/setup/login', { method: 'POST' });
        const body = await res.json();
        if (!res.ok) throw new Error(body.error?.message || res.statusText);
        if (body.login_url) {
          urlEl.innerHTML = 'Open this link to finish login: <a href="' + body.login_url + '" target="_blank" rel="noopener">' + body.login_url + '</a>';
        }
        await refreshStatus();
        const timer = setInterval(async () => {
          try {
            const st = await refreshStatus();
            if (st.logged_in && st.setup_enabled === false) clearInterval(timer);
          } catch (e) { /* keep polling */ }
        }, 2000);
      } catch (e) {
        errEl.textContent = String(e.message || e);
      }
    };

    refreshStatus().catch(() => {});
  </script>
</body>
</html>
"#;

async fn list_models(State(state): State<AppState>) -> Json<Value> {
    let data: Vec<Value> = state
        .config
        .models
        .keys()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "owned_by": "cursor-bridge"
            })
        })
        .collect();
    Json(json!({
        "object": "list",
        "data": data
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
    State(state): State<AppState>,
    Json(body): Json<ChatRequest>,
) -> Response {
    let config = &state.config;
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

    let Some(cursor_model) = config.resolve_model(&body.model) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": format!(
                        "model not allowed: {} (use GET /v1/models for the allowlist)",
                        if body.model.is_empty() { "(empty)" } else { &body.model }
                    ),
                    "type": "invalid_request_error"
                }
            })),
        )
            .into_response();
    };
    let cursor_model = cursor_model.to_string();
    let model_id = if body.model.is_empty() {
        config.default_model_id().to_string()
    } else {
        body.model.clone()
    };

    let content = match agent::print_turn(config, &cursor_model, &prompt).await {
        Ok(content) => content,
        Err(err) => {
            let message = sanitize_agent_error(&err.to_string());
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": {
                        "message": message,
                        "type": "server_error"
                    }
                })),
            )
                .into_response();
        }
    };

    if body.stream {
        return sse_completion(&model_id, &content);
    }

    Json(json!({
        "id": "chatcmpl-bridge",
        "object": "chat.completion",
        "model": model_id,
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

fn sanitize_agent_error(raw: &str) -> String {
    raw.replace("Bearer ", "Bearer [redacted] ")
}

async fn require_bearer(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.config.api_key);

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
