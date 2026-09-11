# Testing strategy

Cursor Bridge must prove two things:

1. The OpenAI-compatible API behaves correctly in front of Cursor.
2. Real clients (Open WebUI and Open WebUI Computer) can complete a chat through that API.

Tests follow **FIRST** and stay honest: no tautological asserts, no mocking the system under test into a no-op. Deterministic **fakes** (a real subprocess that speaks ACP or stream-json) and **real client containers** are encouraged. Interaction-style mocks of the bridge itself are not.

## FIRST

| Letter              | Rule for this repo                                                                                                                |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| **F**ast            | Default CI suite runs without Cursor cloud or heavy UI images when possible. Docker consumer suites may be slower and opt-in.     |
| **I**ndependent     | Each case gets its own temp dir, free ports, and process tree. No shared global bridge instance across cases.                     |
| **R**epeatable      | No dependency on a personal lab hostname, LAN IP, or checked-in credential. Same result on a clean machine.                       |
| **S**elf-validating | Exit code + assertions on HTTP status, JSON fields, reassembled SSE text, or client API responses. No manual inspection required. |
| **T**imely          | Contract tests land with the HTTP server (Phase 1). Consumer E2E lands with client recipes (Phase 3).                             |

## What we refuse

- `expect(true).toBe(true)` and empty or meaningless snapshots
- Mocking the bridge HTTP handlers or turn mapper so the test only checks that a stub was called
- Screenshot-only E2E without asserting assistant content
- Tests that require a specific home-lab identity (private MagicDNS names, LAN IPs, personal accounts)
- One mega-suite that mixes Computer's **native** Cursor ACP profile with the **Bridge** OpenAI path

## Collaborators: fakes vs live Cursor

| Collaborator        | Role                                                                                                                                                 | When                                           |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| **Fake `agent`**    | Real executable on `PATH` / configured bin path. Speaks a minimal Cursor-like ACP or `stream-json` protocol. Returns deterministic text (e.g. `OK`). | Default CI and merge gate                      |
| **Live Cursor CLI** | Real `agent` after `agent login` (or runtime-injected key never committed to git).                                                                   | Optional job / manual; never required to merge |

The fake is a **test double at the process boundary**, not a mock inside the bridge. The bridge under test is always the real server.

## Pyramid

```
        ┌─────────────────────────┐
        │ 4. Live Cursor (opt-in) │  real agent + optional real UIs
        └───────────┬─────────────┘
        ┌───────────▼─────────────┐
        │ 3. Consumer E2E         │  Open WebUI + Computer containers
        │    + bridge + fake agent│
        └───────────┬─────────────┘
        ┌───────────▼─────────────┐
        │ 2. Client-shaped HTTP   │  OpenAI SDK / same wire as UIs
        └───────────┬─────────────┘
        ┌───────────▼─────────────┐
        │ 1. Bridge OpenAI        │  bridge + fake agent
        │    contract             │
        └─────────────────────────┘
```

### Layer 1: Bridge OpenAI contract (merge-blocking)

Run the real bridge against the fake agent.

Required assertions (non-exhaustive):

- `GET /healthz` → 200, no secrets in body
- `GET /v1/models` → includes the documented default model id (e.g. `cursor-auto`)
- `POST /v1/chat/completions` with valid Bearer → assistant content equals the fake's payload (e.g. exact `OK`)
- Same with `stream: true` → valid SSE, reassembled content equals `OK`, stream ends with `[DONE]`
- Missing or wrong Bearer → 401
- Multi-turn: second completion must include prior user/assistant turns in the agent prompt (client history is the only memory in print mode)

### Layer 2: Client-shaped HTTP (merge-blocking)

Same stack as layer 1, but drive the API the way UIs do:

- Official or minimal OpenAI-compatible client pointed at `http://127.0.0.1:<port>/v1`
- Model id from `/v1/models`
- Assert the returned message content

This catches base-URL and streaming mismatches before pulling UI images.

### Layer 3: Consumer E2E (Docker; merge or nightly)

Real images, real configuration APIs, **fake agent** behind the bridge (no Cursor cloud).

| Client         | Proof                                                                                                                       |
| -------------- | --------------------------------------------------------------------------------------------------------------------------- |
| **Open WebUI** | Create an OpenAI-compatible connection to the bridge `/v1`, send a chat, assert the assistant message body matches the fake |
| **Computer**   | Use Computer's **OpenAI-compatible connection** (not the native Cursor agent profile). Same content assertion               |

Harness rules:

- Compose or equivalent with dynamic host ports
- Ephemeral volumes under a per-case temp directory
- Tear down containers and networks even on failure
- Mark clearly (e.g. `@requires-docker`) and skip cleanly when Docker is unavailable
- Document that native `agent acp` inside Computer is **out of scope** for Bridge consumer tests

### Layer 4: Live Cursor (optional)

Same harness as layers 1–3 with `CURSOR_BRIDGE_AGENT_BIN` pointing at a real CLI and an authenticated environment supplied at runtime.

- Gate on explicit env (e.g. secrets present); skip otherwise
- Never store Cursor cookies, API keys, or bridge keys in the repository
- Useful for Auto model behaviour and latency; **must not** block merges

## Isolation and artifacts

- Prefer one bridge process per test case (or a carefully reset instance with proven isolation)
- Allocate ports with OS ephemeral bind (`:0`) or a collision-safe allocator
- Workspace directory: empty or fixture-owned under the case temp dir
- Logs on failure: bridge stderr + fake agent stderr; redact `Authorization` headers

## Naming and layout (when code lands)

Suggested taxonomy (align names with the eventual test runner):

| Kind                   | Intent                                                         |
| ---------------------- | -------------------------------------------------------------- |
| `*.unit.test.*`        | Pure helpers (parsing SSE, model map) with real inputs/outputs |
| `*.contract.test.*`    | OpenAI wire + fake agent (layers 1–2)                          |
| `*.integration.test.*` | Bridge process wired with fakes, no UI images                  |
| `*.e2e.test.*`         | Open WebUI / Computer containers (layer 3)                     |
| `*.live.test.*`        | Real Cursor (layer 4), skipped by default                      |

Assert **observable behaviour** (status, body, stream assembly). Do not assert internal private function call counts.

## CI policy

| Suite      | Merge gate                                               | Needs                                     |
| ---------- | -------------------------------------------------------- | ----------------------------------------- |
| Layers 1–2 | Yes                                                      | Fake agent only                           |
| Layer 3    | Yes if Docker available in CI; otherwise nightly + local | Docker images for Open WebUI and Computer |
| Layer 4    | No                                                       | Operator-provided Cursor auth at runtime  |

## Implementation order

1. Ship the fake `agent` and FIRST harness with Phase 1 (HTTP server).
2. Add layer 1–2 contracts in the same change as `/v1/chat/completions`.
3. Add Open WebUI then Computer compose E2E when client docs are exercised (Phase 3).
4. Add optional live job last; keep secrets out of git forever.

## Relation to the roadmap

Phase 1 already calls for automated tests with a fake `agent`. This document is the authority for **what** those tests must prove, including Open WebUI and Computer consumer coverage and the live Cursor opt-in tier.
