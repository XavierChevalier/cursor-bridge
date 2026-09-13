//! HTTP routes for the OpenAI-compatible bridge and one-shot setup UI.

use axum::{
    body::Body,
    extract::State,
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;

use crate::agent::{self, BridgeDelta};
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
    let models = state.models.effective_models(&state.config).await;
    let data: Vec<Value> = models
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
    #[serde(deserialize_with = "deserialize_message_content")]
    content: String,
}

fn deserialize_message_content<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(piece) = part.get("text").and_then(|t| t.as_str()) {
                        text.push_str(piece);
                    }
                } else if let Some(piece) = part.as_str() {
                    text.push_str(piece);
                }
            }
            Ok(text)
        }
        other => Err(serde::de::Error::custom(format!(
            "message content must be string or content parts array, got {other}"
        ))),
    }
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
    let prompt = match conversation_prompt(&body.messages) {
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

    let Some(cursor_model) = state.models.resolve_model(config, &body.model).await else {
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
    let model_id = if body.model.is_empty() {
        config.default_model_id().to_string()
    } else {
        body.model.clone()
    };

    if body.stream {
        return match live_sse_completion(config, &cursor_model, &model_id, &prompt) {
            Ok(response) => response,
            Err(err) => {
                let message = sanitize_agent_error(&err.to_string());
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({
                        "error": {
                            "message": message,
                            "type": "server_error"
                        }
                    })),
                )
                    .into_response()
            }
        };
    }

    let turn = match agent::print_turn(config, &cursor_model, &prompt).await {
        Ok(turn) => turn,
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

    let mut message = json!({
        "role": "assistant",
        "content": turn.content
    });
    if !turn.reasoning_content.is_empty() {
        message["reasoning_content"] = json!(turn.reasoning_content);
    }

    Json(json!({
        "id": "chatcmpl-bridge",
        "object": "chat.completion",
        "model": model_id,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": "stop"
        }]
    }))
    .into_response()
}

fn live_sse_completion(
    config: &crate::Config,
    cursor_model: &str,
    model_id: &str,
    prompt: &str,
) -> Result<Response, agent::AgentError> {
    let mut deltas = agent::stream_print_turn(config, cursor_model, prompt)?;
    let model = model_id.to_string();

    let stream = async_stream::stream! {
        let mut failed = false;
        let mut sent_role = false;
        while let Some(item) = deltas.next().await {
            match item {
                Ok(piece) => {
                    let mut delta = match piece {
                        BridgeDelta::Reasoning(text) => json!({ "reasoning_content": text }),
                        BridgeDelta::Content(text) => json!({ "content": text }),
                    };
                    if !sent_role {
                        delta["role"] = json!("assistant");
                        sent_role = true;
                    }
                    let chunk = json!({
                        "id": "chatcmpl-bridge",
                        "object": "chat.completion.chunk",
                        "model": model,
                        "choices": [{
                            "index": 0,
                            "delta": delta,
                            "finish_reason": null
                        }]
                    });
                    yield Ok::<Event, Infallible>(Event::default().data(chunk.to_string()));
                }
                Err(err) => {
                    failed = true;
                    let message = sanitize_agent_error(&err.to_string());
                    let chunk = json!({
                        "id": "chatcmpl-bridge",
                        "object": "chat.completion.chunk",
                        "model": model,
                        "choices": [{
                            "index": 0,
                            "delta": { "content": format!("\n\n[bridge error: {message}]") },
                            "finish_reason": "stop"
                        }]
                    });
                    yield Ok(Event::default().data(chunk.to_string()));
                    break;
                }
            }
        }

        if !failed {
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
            yield Ok(Event::default().data(done.to_string()));
        }
        yield Ok(Event::default().data("[DONE]"));
    };

    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

/// Build a print-mode prompt from the OpenAI `messages` array.
/// Requires at least one user turn; includes prior user/assistant/system text
/// because CLI print turns have no sticky Cursor session of their own.
fn conversation_prompt(messages: &[ChatMessage]) -> Option<String> {
    let has_user = messages.iter().any(|message| message.role == "user");
    if !has_user {
        return None;
    }

    let mut lines = Vec::new();
    for message in messages {
        let label = match message.role.as_str() {
            "system" => "System",
            "user" => "User",
            "assistant" => "Assistant",
            other => other,
        };
        lines.push(format!("{label}: {}", message.content));
    }
    Some(lines.join("\n"))
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
