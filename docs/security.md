# Security

## Threat model (short)

Cursor Bridge turns an HTTP API into a coding agent with shell and filesystem access inside the configured workspace. A valid bridge API key is therefore close to **remote code execution** on that host.

Treat exposure the same way you would treat an SSH port limited to one directory, not like a public chatbot.

## Trust boundaries

| Boundary          | Control                                                                                  |
| ----------------- | ---------------------------------------------------------------------------------------- |
| Internet → bridge | Do not publish publicly without an authenticating proxy and TLS. Default bind: loopback. |
| Client → bridge   | Bearer `CURSOR_BRIDGE_API_KEY`                                                           |
| Bridge → Cursor   | `agent login` or operator-managed env at runtime (never committed)                       |
| Agent → files     | Mount / configure a dedicated workspace only                                             |
| Agent → secrets   | Deny lists, no unrelated secrets in the process environment                              |

## Secrets handling

- `.env` is gitignored. Commit only `.env.example` with empty or fake placeholders.
- Never embed API keys, tokens, or account emails in README, compose, CI logs, or issues.
- Prefer interactive `agent login` over long-lived `CURSOR_API_KEY` in the environment.
- If a key must be used, inject it at runtime (orchestrator secrets, not files in git).
- Rotate the bridge API key if it leaks; revoke Cursor sessions from the Cursor account if the CLI credential leaks.

## Hardening checklist

- [ ] Bind `127.0.0.1` unless a private network path is intentional
- [ ] Non-empty `CURSOR_BRIDGE_API_KEY` outside pure local experiments
- [ ] Workspace directory contains only projects you accept the agent editing
- [ ] No Docker socket mounted into a bridge container
- [ ] No host networking unless you understand the exposure
- [ ] Resource limits (CPU, memory, pids) if running under an orchestrator
- [ ] Audit or access logs without writing Authorization headers or cookies

## Tool approval reality

OpenAI Chat Completions clients generally cannot render Cursor permission prompts. Bridge print turns pass `--trust` and `--force` for the configured workspace so tools run without interactive approval (still blocked by explicit `deny` entries in `cli-config.json`). That is a product trade-off, not a free lunch. Constrain the workspace and network egress accordingly.

## Reporting issues

Use private security reporting on the repository for vulnerabilities in Bridge itself. Issues in the Cursor CLI or in third-party UIs belong upstream.
