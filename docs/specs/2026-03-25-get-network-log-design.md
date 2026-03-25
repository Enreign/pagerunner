# get_network_log — Interface Design Spec

**Date:** 2026-03-25
**Issue:** LNY-165
**Status:** Approved
**Blocks:** LNY-167 (network subscriptions), LNY-168 (ring buffer + tool impl)

---

## Context

The current `CdpConn` drops all CDP events (line 79 of `src/cdp.rs`). Once LNY-166 rebuilds the connection to multiplex commands and events, the network domain can be subscribed to (LNY-167) and events stored in a ring buffer (LNY-168). This spec defines the agent-facing query interface that the ring buffer must support — storage shape follows from query shape.

---

## Use Cases

Three equally-weighted scenarios drive the design:

1. **Debugging failed interactions** — agent needs status + response body to understand why a click/form action failed.
2. **API discovery** — agent inspects what endpoints a SPA calls to understand the data model or call them directly later.
3. **Validation** — agent confirms that navigating to a page or triggering an action fired the expected request.

---

## Tool Signature

```json
{
  "name": "get_network_log",
  "description": "Return captured network requests for a tab. Requires network events enabled (LNY-166/167). Filter by URL pattern, HTTP method, status code range, or time window. Response bodies are truncated to 2KB by default — use full_response: true for the complete body.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "session_id": {
        "type": "string",
        "description": "Session ID from open_session"
      },
      "target_id": {
        "type": "string",
        "description": "Tab target ID from list_tabs or new_tab"
      },
      "url_pattern": {
        "type": "string",
        "description": "Substring or glob match against request URL. E.g. \"/api/*\" or \"graphql\""
      },
      "method": {
        "type": "string",
        "description": "HTTP method filter: GET, POST, PUT, DELETE, PATCH, etc."
      },
      "status_min": {
        "type": "integer",
        "description": "Minimum HTTP status code (inclusive). E.g. 400 to see all errors."
      },
      "status_max": {
        "type": "integer",
        "description": "Maximum HTTP status code (inclusive). E.g. 499 for 4xx only."
      },
      "since_ms": {
        "type": "integer",
        "description": "Only return events from the last N milliseconds."
      },
      "limit": {
        "type": "integer",
        "description": "Max entries to return. Default 50, max 500."
      },
      "include_request_body": {
        "type": "boolean",
        "description": "Include outgoing request body in entries (default false). Useful for debugging POST payloads."
      },
      "full_response": {
        "type": "boolean",
        "description": "Return full response body without truncation (default false). Use when body preview is insufficient for API discovery."
      },
      "all_tabs": {
        "type": "boolean",
        "description": "Return events across all tabs in this session rather than just target_id (default false)."
      }
    },
    "required": ["session_id", "target_id"]
  }
}
```

---

## Response Shape

### Success

```json
{
  "ok": true,
  "entries": [
    {
      "request_id":         "3A4B2C",
      "url":                "https://api.example.com/v1/users",
      "method":             "POST",
      "status":             201,
      "duration_ms":        143,
      "timestamp_ms":       1743000123456,
      "request_headers":    { "Content-Type": "application/json" },
      "request_body":       null,
      "response_body":      "{\"id\":\"usr_9f2\",\"email\":\"[EMAIL:a3f9b]\",...}",
      "response_truncated": true,
      "tab_id":             "ABC123"
    }
  ],
  "total_matched":  12,
  "total_captured": 87,
  "result_truncated": true
}
```

### Error

```json
{
  "ok": false,
  "error_type": "NETWORK_LOG_UNAVAILABLE",
  "recovery_hint": "Network events require the rebuilt CdpConn (LNY-166) and Network domain subscriptions (LNY-167). Ensure pagerunner is built from a version that includes those changes."
}
```

### Field notes

| Field | Notes |
|---|---|
| `request_body` | `null` unless `include_request_body: true` |
| `response_body` | Truncated to 2KB unless `full_response: true`; `response_truncated: true` signals truncation |
| `response_truncated` | `false` when body fits in 2KB or `full_response: true` |
| `total_matched` | Entries matching the applied filters (before `limit`) |
| `total_captured` | Total entries in the ring buffer for this tab/session |
| `result_truncated` | `true` when `total_matched > limit` |
| `tab_id` | Always present — useful when `all_tabs: true` returns mixed-tab results |
| Sensitive headers | `Authorization`, `Cookie`, `Set-Cookie`, `X-Auth-Token` stripped at write time, never stored |
| Anonymization | If session has anonymization enabled, `response_body` and `request_body` pass through the same PII pipeline as `get_content` |

---

## Ring Buffer Spec

| Property | Value |
|---|---|
| Capacity | 500 entries per tab (configurable in `config.toml` under `[network]`) |
| Scope | Per `(session_id, target_id)` |
| Eviction | Oldest-first when full |
| TTL | Cleared on `close_session`; entries expire after 24h regardless |
| Storage | ReDB (same store as KV and snapshots) |
| Sensitive headers | Stripped at write time: `Authorization`, `Cookie`, `Set-Cookie`, `X-Auth-Token` |

---

## URL Pattern Matching

`url_pattern` supports two forms:
- **Substring**: `"graphql"` matches any URL containing that string
- **Glob**: `"/api/v1/*"` matches the path segment; `"*.example.com/*"` matches across host + path

No regex support in v1 — globs cover the practical cases without the safety/complexity overhead.

---

## Dependency Chain

```
LNY-166  Rebuild CdpConn (events no longer dropped)
  └── LNY-167  Network.enable + subscribe requestWillBeSent / responseReceived / loadingFinished
        └── LNY-168  Ring buffer + get_network_log tool (this spec)
```

This spec must be written and approved before LNY-168 begins, to ensure storage schema matches the query interface.

---

## Out of Scope (v1)

- `wait_for_request` reactive tool (separate concern, future issue)
- Response body capture for binary/non-text content types
- Per-request response body fetch via `Network.getResponseBody` (v1 captures at event time only)
- Filtering by request initiator or resource type
