#!/usr/bin/env bash
# @requires-docker
# Open WebUI consumer E2E against Bridge + fake agent (no Cursor cloud).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMPOSE_FILE="${ROOT}/tests/e2e/docker-compose.openwebui.yml"
PROJECT="cursor-bridge-e2e-owui"

if ! command -v docker >/dev/null 2>&1; then
  echo "SKIP: docker not available"
  exit 0
fi
if ! docker info >/dev/null 2>&1; then
  echo "SKIP: docker daemon not reachable"
  exit 0
fi

cleanup() {
  docker compose -p "${PROJECT}" -f "${COMPOSE_FILE}" down -v --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

cleanup
docker compose -p "${PROJECT}" -f "${COMPOSE_FILE}" up -d --build

echo "waiting for bridge models…"
for _ in $(seq 1 60); do
  if docker compose -p "${PROJECT}" -f "${COMPOSE_FILE}" exec -T bridge \
    curl -fsS -H "Authorization: Bearer e2e-bridge-key" \
    http://127.0.0.1:8787/v1/models | grep -q cursor-auto; then
    break
  fi
  sleep 2
done

models="$(docker compose -p "${PROJECT}" -f "${COMPOSE_FILE}" exec -T bridge \
  curl -fsS -H "Authorization: Bearer e2e-bridge-key" http://127.0.0.1:8787/v1/models)"
echo "${models}" | grep -q cursor-auto

chat="$(docker compose -p "${PROJECT}" -f "${COMPOSE_FILE}" exec -T bridge \
  curl -fsS -H "Authorization: Bearer e2e-bridge-key" \
  -H "Content-Type: application/json" \
  -d '{"model":"cursor-auto","stream":false,"messages":[{"role":"user","content":"PING"}]}' \
  http://127.0.0.1:8787/v1/chat/completions)"
echo "${chat}" | grep -q '"content":"PING"'

echo "waiting for openwebui…"
for _ in $(seq 1 90); do
  if docker compose -p "${PROJECT}" -f "${COMPOSE_FILE}" exec -T openwebui \
    curl -fsS http://127.0.0.1:8080/health >/dev/null 2>&1; then
    break
  fi
  sleep 2
done

# Open WebUI must be able to reach the bridge OpenAI endpoint on the compose network.
from_ui="$(docker compose -p "${PROJECT}" -f "${COMPOSE_FILE}" exec -T openwebui \
  curl -fsS -H "Authorization: Bearer e2e-bridge-key" http://bridge:8787/v1/models)"
echo "${from_ui}" | grep -q cursor-auto

from_ui_chat="$(docker compose -p "${PROJECT}" -f "${COMPOSE_FILE}" exec -T openwebui \
  curl -fsS -H "Authorization: Bearer e2e-bridge-key" \
  -H "Content-Type: application/json" \
  -d '{"model":"cursor-auto","stream":false,"messages":[{"role":"user","content":"OWUI"}]}' \
  http://bridge:8787/v1/chat/completions)"
echo "${from_ui_chat}" | grep -q '"content":"OWUI"'

echo "OK: openwebui e2e (bridge reachable + chat via OpenAI path)"
