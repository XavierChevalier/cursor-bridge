# Cursor Bridge

**Status:** Phase 1 gateway implemented in Rust (OpenAI-compatible HTTP + fake-agent contracts). Open WebUI / Computer E2E and live Cursor still ahead (see [docs/roadmap.md](docs/roadmap.md)).

Cursor Bridge is an **independent, OpenAI-compatible HTTP gateway** in front of the [Cursor CLI](https://cursor.com/docs/cli/using) (`agent`), aimed at **Cursor Auto** (and other selectable Cursor models).

Any client that speaks the OpenAI Chat Completions API can drive a real coding agent on a machine you control: Open WebUI, Open WebUI Computer (via its OpenAI connection), LibreChat, custom bots, scripts, and more.

```
Open WebUI / Computer / LibreChat / curl / …
        │  HTTPS (your reverse proxy or private network)
        ▼
┌──────────────────────────┐
│  Cursor Bridge           │
│  POST /v1/chat/completions
│  GET  /v1/models         │
└────────────┬─────────────┘
             │  Cursor CLI (ACP or print/stream-json)
             ▼
        agent  (Cursor Auto / chosen model)
             │
             ▼
        your workspace on disk
```

## Why a bridge (not a workbench UI)

| Approach                         | Role                                                                     |
| -------------------------------- | ------------------------------------------------------------------------ |
| Cursor Bridge                    | Thin API brick. No browser UI of its own. Swap the front-end freely.     |
| Full machine UIs (e.g. Computer) | Chat + files + terminal + git in one product. Great UX, different scope. |
| Direct CLI in a chat pipe        | Possible, but every UI reimplements streaming, tools, and auth.          |

Bridge keeps **one** integration surface (`/v1/...`) and lets you choose the UI.

## Goals

1. **OpenAI-compatible enough** for common self-hosted chat UIs (chat completions + models list; streaming).
2. **Cursor Auto by default**, with an explicit model map you control.
3. **Client-agnostic**: documented recipes for Open WebUI, Computer, and generic HTTP clients.
4. **Hardened defaults**: loopback bind, optional API key, no secrets in the repo, workspace-scoped tool access.
5. **Honest limits**: tool approval, diffs, and interactive confirms do not map cleanly through plain Chat Completions; document what is lost vs ACP-native UIs.

## Non-goals (for now)

- Shipping a chat UI, IDE, or PWA.
- Redistributing the Cursor CLI or Cursor credentials.
- Guaranteeing bit-identical OpenAI tool-calling semantics for every client.
- Multi-tenant SaaS.

## Docs

| Doc                                                          | Contents                                                                 |
| ------------------------------------------------------------ | ------------------------------------------------------------------------ |
| [docs/architecture.md](docs/architecture.md)                 | Components, request flow, ACP vs print mode                              |
| [docs/openai-compatibility.md](docs/openai-compatibility.md) | Endpoints, models, streaming, tool limits                                |
| [docs/clients.md](docs/clients.md)                           | Wiring Open WebUI, Computer, and generic clients                         |
| [docs/security.md](docs/security.md)                         | Threat model, auth, secrets handling                                     |
| [docs/testing.md](docs/testing.md)                           | FIRST pyramid: fake agent, Open WebUI / Computer E2E, live Cursor opt-in |
| [docs/deploy.md](docs/deploy.md)                             | Workspace jail, container image, logging                                 |
| [docs/roadmap.md](docs/roadmap.md)                           | Planned implementation phases                                            |

## Development

```bash
cp .env.example .env   # set CURSOR_BRIDGE_API_KEY locally
cargo test --tests
cargo run
# optional Docker consumer E2E (Open WebUI + fake agent):
./tests/e2e/openwebui.e2e.sh
```

Contract tests spawn `tests/fixtures/fake-agent` (no Cursor cloud, no secrets). See [docs/testing.md](docs/testing.md).

## Configuration

Copy [`.env.example`](.env.example) to `.env` and edit locally.

- Never commit `.env`, API keys, or Cursor login material.
- Prefer `agent login` on the host that runs the bridge over long-lived keys in the environment.
- Bind to loopback (`127.0.0.1`) unless a private network or authenticating proxy sits in front.

## Requirements (planned runtime)

- Cursor CLI (`agent`) installed and authenticated on the same host as the bridge.
- An active Cursor subscription (or a key you manage yourself; see security docs).
- Docker optional; a plain process behind your reverse proxy is enough.

## Development status

This repository currently holds **product and design documentation** only. No server binary is published yet. Contributions should start from the roadmap and keep secrets out of git.

## License

MIT for this repository's files. See [LICENSE](LICENSE). Cursor and third-party clients remain under their own terms.
