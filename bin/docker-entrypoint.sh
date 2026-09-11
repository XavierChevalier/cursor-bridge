#!/usr/bin/env bash
# Ensure the Cursor CLI exists outside HOME, then run as the bridge user.
# HOME bind-mounts must never hide the agent binary.
set -euo pipefail

readonly CLI_HOME="${CURSOR_CLI_HOME:-/opt/cursor-cli}"
readonly BRIDGE_UID="${CURSOR_BRIDGE_UID:-10001}"
readonly BRIDGE_GID="${CURSOR_BRIDGE_GID:-10001}"

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
  mkdir -p "${CLI_HOME}"
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

drop_privileges_if_root() {
  if [[ "$(id -u)" -ne 0 ]]; then
    exec "$@"
  fi

  # Volumes on Unraid are often root-owned on first create; fix for the bridge user.
  mkdir -p "${CLI_HOME}" "${HOME:-/home/bridge}" "${CURSOR_BRIDGE_WORKSPACE:-/workspace}"
  chown -R "${BRIDGE_UID}:${BRIDGE_GID}" "${CLI_HOME}" "${HOME:-/home/bridge}" \
    "${CURSOR_BRIDGE_WORKSPACE:-/workspace}" 2>/dev/null || true

  if command -v gosu >/dev/null 2>&1; then
    exec gosu "${BRIDGE_UID}:${BRIDGE_GID}" "$@"
  fi
  if command -v runuser >/dev/null 2>&1; then
    exec runuser -u bridge -- "$@"
  fi
  die "gosu or runuser required to drop from root to bridge"
}

main() {
  ensure_agent
  if [[ "$#" -eq 0 ]]; then
    set -- /usr/local/bin/cursor_bridge
  fi
  drop_privileges_if_root "$@"
}

main "$@"
