# Architecture

## Role

Cursor Bridge is a **sidecar HTTP service**. It translates OpenAI-style chat requests into Cursor CLI invocations and streams responses back. It does not own identity providers, file browsers, or terminals; clients do.

## High-level components

```
┌─────────────┐     Bearer API key      ┌────────────────┐
│ Chat client │ ──────────────────────► │ HTTP API       │
└─────────────┘                         │ /v1/models     │
                                        │ /v1/chat/...   │
                                        └───────┬────────┘
                                                │
                                        ┌───────▼────────┐
                                        │ Session / turn │
                                        │ mapper         │
                                        └───────┬────────┘
                                                │
                          ┌─────────────────────┼─────────────────────┐
                          ▼                     ▼                     ▼
                   Cursor ACP            agent -p               Future
                   (preferred)         stream-json              backends
                          │                     │
                          └──────────┬──────────┘
                                     ▼
                              workspace on disk
```

### HTTP API

- Speaks a **subset** of the OpenAI Chat Completions API (see [openai-compatibility.md](openai-compatibility.md)).
- Authenticates callers with a **bridge API key** (Bearer), independent of Cursor login.
- Exposes one or more logical model ids (default: Cursor Auto).

### Turn mapper

- Maps `messages[]` to a Cursor turn (last user message + optional truncated prior transcript).
- Chooses transport: **ACP** (Agent Client Protocol over stdio) when available and stable; otherwise **print / stream-json**.
- Maps Cursor model identifiers (e.g. Auto / `default`) to the ids advertised on `/v1/models`.

### Cursor runtime

- Runs on the **same machine** as the bridge (or the same container), because the CLI must see the workspace and the stored login.
- Authentication to Cursor is **not** the bridge API key: it is `agent login` (preferred) or an operator-supplied env var at runtime.

## Request flow (happy path)

1. Client `POST /v1/chat/completions` with `Authorization: Bearer <bridge-api-key>`.
2. Bridge validates the key, resolves `model` → Cursor model.
3. Bridge starts or reuses a Cursor session for this conversation id (strategy TBD in implementation; first version may be one-shot turns).
4. Bridge streams tokens (and optionally structured tool events) as OpenAI SSE chunks.
5. Bridge ends the stream when Cursor reports end of turn.

## Conversation state

OpenAI clients often send the **full message history** each request. Cursor may also keep native session state.

Bridge must pick a policy and document it:

| Policy                                              | Pros                                    | Cons                                                         |
| --------------------------------------------------- | --------------------------------------- | ------------------------------------------------------------ |
| Stateless CLI turn + client history only            | Predictable; no sticky sessions         | Larger prompts; weaker native Cursor memory                  |
| Sticky Cursor session per `conversation_id`         | Better agent continuity                 | Must avoid replaying prior assistant text into the HTTP body |
| Hybrid (sticky session, send only latest user turn) | Best of both when the client cooperates | Breaks clients that expect pure OpenAI semantics             |

The implementation should default to a policy that **does not concatenate previous assistant replies into the new completion** (a known footgun when resuming native Cursor ACP sessions naively).

## Deployment shapes

1. **Process on a workstation** bound to `127.0.0.1`, UI on the same host.
2. **Container** with the CLI installed, workspace mounted, loopback or private network publish.
3. **Private network only** (VPN / mesh): never a bare public port without an authenticating proxy.

Compose examples, if added later, must use env files and placeholders only. No hostnames, IPs, or account names from any personal lab belong in this repository.

## Relationship to other products

- **Open WebUI**: optional client via Connections → OpenAI-compatible base URL.
- **Open WebUI Computer**: optional client via the same OpenAI connection pattern, or Computer can keep using Cursor as a **native** agent profile without Bridge. Bridge is for the OpenAI-shaped path.
- **Cursor CLI**: required backend. Bridge does not replace it.
