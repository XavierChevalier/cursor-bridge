# Workspace and image notes

## Workspace jail

Point `CURSOR_BRIDGE_WORKSPACE` at a dedicated directory that contains only
projects you accept the agent editing. The bridge sets that path as the
Cursor CLI working directory for print-mode turns.

Do not mount your entire home directory, secrets stores, or unrelated
infrastructure trees.

Optional deny patterns for the Cursor CLI itself belong in the CLI
permission config on the host that runs `agent` (outside this repository's
runtime). Bridge does not re-implement a full sandbox.

## Container image

`Dockerfile` builds a release binary as a non-root user. It intentionally
does **not** install the Cursor CLI into the image:

- Avoids baking credentials or CLI updates into layers you redistribute.
- Avoids the failure mode where a `HOME` volume masks binaries installed
  under that home path.

At deploy time, provide `CURSOR_BRIDGE_AGENT_BIN` pointing at an `agent`
binary available in the container (bind-mount or sidecar install).

## Logging

The binary uses `tracing` with `RUST_LOG` / default `info`. Do not log
`Authorization` headers or raw API keys. Agent stderr may be surfaced in
HTTP 502 bodies for operator diagnosis; keep bridge API keys out of those
messages (see contract tests).
