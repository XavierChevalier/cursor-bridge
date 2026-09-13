# Client recipes

All examples use **placeholders**. Replace host, port, and API key with values from your own `.env`. Never paste production secrets into tickets or screenshots.

Assume the bridge listens on `http://127.0.0.1:8787` during local development.

## Generic HTTP

List models:

```bash
curl -sS http://127.0.0.1:8787/v1/models \
  -H "Authorization: Bearer ${CURSOR_BRIDGE_API_KEY}"
```

Non-streaming chat:

```bash
curl -sS http://127.0.0.1:8787/v1/chat/completions \
  -H "Authorization: Bearer ${CURSOR_BRIDGE_API_KEY}" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "cursor-auto",
    "messages": [
      {"role": "user", "content": "Reply with exactly: OK"}
    ],
    "stream": false
  }'
```

Streaming chat: set `"stream": true` and read SSE lines.

## Open WebUI

1. Run Cursor Bridge on a host Open WebUI can reach (same machine via loopback, or a private network URL).
2. In Open WebUI: **Admin → Connections → OpenAI**.
3. Set:
   - **API Base URL** → `http://127.0.0.1:8787/v1` (or your private URL + `/v1`)
   - **API Key** → the bridge key from your `.env` (`CURSOR_BRIDGE_API_KEY`)
4. Enable the connection and select a model from `/v1/models` (default `cursor-auto`, plus account models when discovery is on).

Notes:

- Open WebUI will not show Cursor-native tool approval UI. Assume elevated tool autonomy on the bridge host.
- Knowledge bases, Open WebUI tools, and filters are **not** forwarded to Cursor unless you explicitly design that later.
- With `CURSOR_BRIDGE_DISCOVER_MODELS` enabled (default), Bridge merges `agent models` into the list so Computer can pick Composer, Claude, GPT, etc., not only Auto.

## Open WebUI Computer

Computer already supports Cursor as a **native agent profile** (`agent acp`). That path does not need Bridge.

Use Bridge when you want Computer (or another app) to talk to Cursor **through the OpenAI-compatible gateway**:

1. In Computer, create or use an OpenAI-compatible connection pointing at the bridge `/v1` base URL.
2. Use the same Bearer API key as above.
3. Prefer a dedicated workspace directory mounted only for agent work.

If both native Cursor and Bridge are configured, keep the mental model clear: native ACP ≠ OpenAI gateway.

## LibreChat and other OpenAI clients

Same pattern as Open WebUI:

- Custom endpoint / OpenAI provider
- Base URL ending in `/v1`
- API key = bridge key
- Model id from `/v1/models`

Verify streaming and empty-assistant edge cases; client libraries differ.

## What not to hardcode

Do not commit:

- Personal VPN or MagicDNS names
- LAN IPs
- Real API keys or Cursor cookies
- Paths to unrelated home directories or infrastructure repos

Document only placeholders and generic private-network guidance.
