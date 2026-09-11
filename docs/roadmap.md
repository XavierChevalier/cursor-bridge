# Roadmap

Documentation-first repository. Implementation order below is intentional.

## Phase 0: Docs

- [x] Product README and licence
- [x] Architecture, compatibility, clients, security, testing notes
- [x] `.env.example` without real secrets

## Phase 1: Minimal gateway

- [x] HTTP server: `/healthz`, `/v1/models`, `/v1/chat/completions`
- [x] Bearer auth for the bridge API key
- [x] One-shot Cursor turn via CLI print mode (`agent -p`) for text-only replies
- [x] SSE streaming compatible with common Open WebUI settings
- [x] Automated tests with a fake `agent` binary (no network, no real credentials); see [testing.md](testing.md)

## Phase 2: Production hardening

- [ ] Explicit model map (Auto + allowlisted Cursor models)
- [ ] Conversation / session policy that avoids assistant-text replay
- [ ] Workspace path jail documentation + optional deny patterns
- [ ] Structured logging without secret leakage
- [ ] Container image build that installs nothing under a masked `HOME` mount

## Phase 3: Client polish

- [ ] Verified recipes for Open WebUI and Computer (OpenAI connection)
- [ ] Consumer E2E (Docker): Open WebUI + Computer against bridge + fake agent ([testing.md](testing.md) layer 3)
- [ ] Clear errors when Cursor is not logged in
- [ ] Optional request size / rate limits

## Phase 4: Stretch

- [ ] Better mapping of tool events into OpenAI-compatible streams (if clients benefit)
- [ ] Multi-workspace model ids
- [ ] Metrics (request count, latency, error class)
- [ ] Optional live Cursor CI job (secrets at runtime only; never merge-blocking)

## Explicit non-goals until revisited

- Multi-tenant cloud hosting
- Bundling or rebranding Open WebUI / Computer
- Storing user Cursor passwords in Bridge
