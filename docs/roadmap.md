# Roadmap

Documentation-first repository. Implementation order below is intentional.

## Phase 0: Docs (current)

- [x] Product README and licence
- [x] Architecture, compatibility, clients, security notes
- [x] `.env.example` without real secrets

## Phase 1: Minimal gateway

- [ ] HTTP server: `/healthz`, `/v1/models`, `/v1/chat/completions`
- [ ] Bearer auth for the bridge API key
- [ ] One-shot Cursor turn via CLI (`stream-json` or ACP) for text-only replies
- [ ] SSE streaming compatible with common Open WebUI settings
- [ ] Automated tests with a fake `agent` binary (no network, no real credentials)

## Phase 2: Production hardening

- [ ] Explicit model map (Auto + allowlisted Cursor models)
- [ ] Conversation / session policy that avoids assistant-text replay
- [ ] Workspace path jail documentation + optional deny patterns
- [ ] Structured logging without secret leakage
- [ ] Container image build that installs nothing under a masked `HOME` mount

## Phase 3: Client polish

- [ ] Verified recipes for Open WebUI and Computer (OpenAI connection)
- [ ] Clear errors when Cursor is not logged in
- [ ] Optional request size / rate limits

## Phase 4: Stretch

- [ ] Better mapping of tool events into OpenAI-compatible streams (if clients benefit)
- [ ] Multi-workspace model ids
- [ ] Metrics (request count, latency, error class)

## Explicit non-goals until revisited

- Multi-tenant cloud hosting
- Bundling or rebranding Open WebUI / Computer
- Storing user Cursor passwords in Bridge
