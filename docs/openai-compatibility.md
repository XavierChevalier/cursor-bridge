# OpenAI compatibility

Cursor Bridge aims for **practical compatibility** with self-hosted chat UIs, not a perfect clone of the OpenAI platform.

## Planned endpoints

| Method | Path                   | Purpose                                |
| ------ | ---------------------- | -------------------------------------- |
| `GET`  | `/v1/models`           | List logical models the bridge exposes |
| `GET`  | `/v1/models/{id}`      | Model metadata                         |
| `POST` | `/v1/chat/completions` | Chat turn (sync or SSE stream)         |
| `GET`  | `/healthz`             | Liveness (no secrets in response)      |

Out of scope for the first versions unless explicitly scheduled: Assistants API, Responses API, embeddings, images, audio, fine-tuning, organization APIs.

## Authentication

```http
Authorization: Bearer <CURSOR_BRIDGE_API_KEY>
```

- This key protects **the bridge**, not your Cursor account.
- Generate a long random secret per deployment.
- Do not put real keys in docs, issues, or commits. Use `.env` locally.

## Models

| Bridge `model` id | Cursor side                         | Notes                                      |
| ----------------- | ----------------------------------- | ------------------------------------------ |
| `cursor-auto`     | Auto (`default` / `auto`)           | Recommended default (`CURSOR_BRIDGE_*`)    |
| Cursor CLI ids    | Same id passed to `agent --model`   | From `agent models` when discovery is on   |
| Extra map entries | Operator-defined                    | `CURSOR_BRIDGE_EXTRA_MODELS`               |

`GET /v1/models` lists the static allowlist plus, by default, every model
returned by `agent models` for the logged-in Cursor account
(`CURSOR_BRIDGE_DISCOVER_MODELS`, default on). Set that env to `0` to advertise
only `CURSOR_BRIDGE_MODEL_ID` and `CURSOR_BRIDGE_EXTRA_MODELS`.

## Chat Completions request

Supported fields (planned):

- `model` (required)
- `messages` (required): `system` / `user` / `assistant` roles; text content first
- `stream` (boolean)
- `temperature` and related sampling params: **best-effort** or ignored with a warning header if Cursor does not honor them

Ignored or rejected with a clear error until designed:

- `tools` / `tool_choice` (OpenAI function calling): Cursor tools are agent-native; mapping is lossy
- `response_format` JSON schema mode
- vision / audio parts (until a deliberate multimodal path exists)

## Streaming

When `stream: true`, the bridge emits SSE chunks shaped like OpenAI Chat Completions streams (`data: {...}` / `data: [DONE]`).

Clients that require WebSocket-only transports are out of scope; use HTTP SSE.

## What you lose vs a native Cursor UI

Plain Chat Completions have no first-class UX for:

- Per-tool approval prompts (`ask` / `auto` / `full` style flows)
- Inline diffs and git panels
- Interactive terminal sessions

Those features stay in ACP-native front-ends. Through Bridge, unattended agent turns typically run with **auto-approved tools** inside a locked-down workspace. Treat that as remote code execution you requested, and read [security.md](security.md).

## Error shape

Errors should use JSON bodies compatible with common OpenAI client expectations (`error.message`, `error.type`, HTTP 4xx/5xx), without leaking host paths, env vars, or credential material.
