#!/usr/bin/env bash
# Ensure the Cursor CLI exists outside HOME, then exec the bridge binary.
# Designed to run as the unprivileged `bridge` user (uid 10001).
# HOME bind-mounts must never hide the agent binary.
set -euo pipefail

readonly CLI_HOME="${CURSOR_CLI_HOME:-/opt/cursor-cli}"

log() {
  printf '[cursor-bridge] %s\n' "$*"
}

die() {
  printf '[cursor-bridge] %s\n' "$*" >&2
  exit 1
}

default_agent_path() {
  printf '%s/.local/bin/agent' "${CLI_HOME}"
}

agent_is_executable() {
  local path="${1:-}"
  [[ -n "${path}" && -x "${path}" ]]
}

resolve_agent_bin() {
  if agent_is_executable "${CURSOR_BRIDGE_AGENT_BIN:-}"; then
    printf '%s' "${CURSOR_BRIDGE_AGENT_BIN}"
    return 0
  fi
  local default
  default="$(default_agent_path)"
  if agent_is_executable "${default}"; then
    printf '%s' "${default}"
    return 0
  fi
  return 1
}

install_cursor_cli() {
  [[ "${CURSOR_BRIDGE_INSTALL_CLI:-1}" == "1" ]] || {
    die "Cursor CLI missing at ${CLI_HOME} and CURSOR_BRIDGE_INSTALL_CLI=${CURSOR_BRIDGE_INSTALL_CLI:-}. Mount an agent or enable install."
  }

  command -v curl >/dev/null 2>&1 || die "curl required to install the Cursor CLI"
  mkdir -p "${CLI_HOME}" || die "Cannot create ${CLI_HOME}. Fix ownership: chown -R $(id -u):$(id -g) <host path mounted on ${CLI_HOME}>"
  [[ -w "${CLI_HOME}" ]] || die "${CLI_HOME} is not writable by uid $(id -u). Fix ownership on the host."

  log "installing Cursor CLI into ${CLI_HOME} (not under HOME)"
  HOME="${CLI_HOME}" bash -c 'curl -fsS https://cursor.com/install | bash' \
    || die "Cursor CLI install failed"
  [[ -x "$(default_agent_path)" ]] || die "install finished but $(default_agent_path) is not executable"
}

ensure_agent() {
  local bin
  if bin="$(resolve_agent_bin)"; then
    export CURSOR_BRIDGE_AGENT_BIN="${bin}"
    log "using agent at ${CURSOR_BRIDGE_AGENT_BIN}"
    return 0
  fi
  install_cursor_cli
  export CURSOR_BRIDGE_AGENT_BIN="$(default_agent_path)"
  log "using agent at ${CURSOR_BRIDGE_AGENT_BIN}"
}

main() {
  ensure_agent
  if [[ "$#" -eq 0 ]]; then
    set -- /usr/local/bin/cursor_bridge
  fi
  exec "$@"
}

main "$@"
