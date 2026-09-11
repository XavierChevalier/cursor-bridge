# Workspace and image notes

## Workspace jail

Point `CURSOR_BRIDGE_WORKSPACE` at a dedicated directory that contains only
projects you accept the agent editing. The bridge sets that path as the
Cursor CLI working directory for print-mode turns and passes `--trust` so
non-interactive HTTP turns do not hang on the workspace-trust prompt.

Do not mount your entire home directory, secrets stores, or unrelated
infrastructure trees.

Optional deny patterns for the Cursor CLI itself belong in the CLI
permission config on the host that runs `agent` (outside this repository's
runtime). Bridge does not re-implement a full sandbox.

## Container image

Published image (Docker Hub): `xavierchevalier/cursor-bridge` (tags `latest`,
`sha-*`, and semver on `v*` git tags). CI builds `linux/amd64` after tests pass.

The image intentionally does **not** bake the Cursor CLI into layers:

- Avoids embedding CLI updates and credentials in redistributed layers.
- Avoids the failure mode where a `HOME` volume masks binaries installed
  under that home path.

At start, `bin/docker-entrypoint.sh`:

1. Uses `CURSOR_BRIDGE_AGENT_BIN` when it points at an executable.
2. Otherwise uses `${CURSOR_CLI_HOME:-/opt/cursor-cli}/.local/bin/agent` if present.
3. Otherwise installs the CLI into `CURSOR_CLI_HOME` when
   `CURSOR_BRIDGE_INSTALL_CLI=1` (default).
4. Drops from root to uid 10001 (`bridge`) via `gosu` after fixing volume ownership.

Mount a persistent volume on `/opt/cursor-cli` and on `$HOME` (login state).
Set `CURSOR_BRIDGE_INSTALL_CLI=0` in tests that supply a fake agent.

## Logging

The binary uses `tracing` with `RUST_LOG` / default `info`. Do not log
`Authorization` headers or raw API keys. Agent stderr may be surfaced in
HTTP 502 bodies for operator diagnosis; keep bridge API keys out of those
messages (see contract tests).

## One-shot setup UI

On first boot (Cursor not logged in, no `.cursor-bridge-setup-complete` flag in
`CURSOR_BRIDGE_STATE_DIR` / `$HOME`), the bridge serves a minimal HTML page at
`/` (open during first setup (no API key); `/v1` stays Bearer-protected).

1. Open the Tailscale hostname (e.g. `https://cursor-bridge`).
2. Enter the bridge API key, click **Start login**, open the printed Cursor URL.
3. When `agent status` reports logged in, the bridge writes the setup-complete
   flag and setup routes return **410 Gone**. Only `/healthz` and `/v1/*` remain.

To re-run setup: `agent logout`, delete `.cursor-bridge-setup-complete`, restart.
