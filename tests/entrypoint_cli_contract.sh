#!/usr/bin/env bash
# Contract: CLI install path stays outside HOME; entrypoint prefers an existing agent.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENTRY="${ROOT}/bin/docker-entrypoint.sh"
DOCKERFILE="${ROOT}/Dockerfile"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

grep -q 'CURSOR_CLI_HOME=/opt/cursor-cli' "${DOCKERFILE}" || fail "Dockerfile must set CURSOR_CLI_HOME outside HOME"
grep -q 'docker-entrypoint.sh' "${DOCKERFILE}" || fail "Dockerfile must use docker-entrypoint.sh"
! grep -qE 'HOME=.*cursor\.com/install|install \| bash' "${DOCKERFILE}" \
  || fail "Dockerfile must not bake Cursor CLI install into image layers"

grep -q 'CURSOR_CLI_HOME' "${ENTRY}" || fail "entrypoint must honor CURSOR_CLI_HOME"
grep -q 'CURSOR_BRIDGE_INSTALL_CLI' "${ENTRY}" || fail "entrypoint must gate install"
grep -q 'gosu' "${ENTRY}" || fail "entrypoint must drop privileges with gosu"

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

mkdir -p "${tmp}/bin" "${tmp}/home" "${tmp}/cli"
cat >"${tmp}/bin/fake-agent" <<'EOF'
#!/bin/sh
echo fake-agent-ok
EOF
chmod +x "${tmp}/bin/fake-agent"

# Existing agent wins: no network install.
out="$(
  HOME="${tmp}/home" \
    CURSOR_CLI_HOME="${tmp}/cli" \
    CURSOR_BRIDGE_AGENT_BIN="${tmp}/bin/fake-agent" \
    CURSOR_BRIDGE_INSTALL_CLI=0 \
    "${ENTRY}" "${tmp}/bin/fake-agent" 2>&1
)"
printf '%s\n' "${out}" | grep -q 'using agent at' || fail "expected agent resolution log"
printf '%s\n' "${out}" | grep -q 'fake-agent-ok' || fail "expected fake agent to run"

# Missing agent + install disabled → hard fail (no silent success).
if HOME="${tmp}/home" \
  CURSOR_CLI_HOME="${tmp}/cli-empty" \
  CURSOR_BRIDGE_AGENT_BIN="${tmp}/cli-empty/missing" \
  CURSOR_BRIDGE_INSTALL_CLI=0 \
  "${ENTRY}" true 2>"${tmp}/err"; then
  fail "expected failure when agent missing and install disabled"
fi
grep -qi 'missing\|install' "${tmp}/err" || fail "expected install/missing hint on stderr"

echo "OK: entrypoint_cli_contract"
