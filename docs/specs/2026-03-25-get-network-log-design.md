# get_network_log — Interface Design Spec

**Date:** 2026-03-25
**Issue:** LNY-165
**Status:** Approved
**Blocks:** LNY-168 (ring buffer + tool impl)

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
  "description": "Return captured network requests for a tab (or all tabs in a session). Requires network events enabled (available after the CdpConn rebuild). Filter by URL pattern, HTTP method, status code range, or lookback window. Response bodies are truncated to 2KB by default — use full_response: true for the complete body.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "session_id": {
        "type": "string",
        "description": "Session ID from open_session"
      },
      "target_id": {
        "type": "string",
        "description": "Tab target ID from list_tabs or new_tab. Required unless all_tabs is true."
      },
      "url_pattern": {
        "type": "string",
        "description": "Substring or glob match against the full URL string (scheme + host + path + query). E.g. \"/api/*\", \"graphql\", \"*.example.com/*\""
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
      "lookback_ms": {
        "type": "integer",
        "description": "Only return events captured within the last N milliseconds, relative to the time this query executes."
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
        "description": "Return full response body without truncation (default false). Use when the 2KB preview is insufficient for API discovery."
      },
      "all_tabs": {
        "type": "boolean",
        "description": "Return events across all tabs in this session. When true, target_id is not required and is ignored if provided."
      }
    },
    "required": ["session_id"]
  }
}
```

### Validation rule

Exactly one of the following must be true at call time, or the tool returns a `VALIDATION_ERROR`:
- `target_id` is provided (and `all_tabs` is absent or false)
- `all_tabs: true` (in which case `target_id` is ignored even if present)

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
      "response_truncated": false,
      "tab_id":             "ABC123"
    }
  ],
  "total_matched":    12,
  "total_captured":   87,
  "result_truncated": true
}
```

### Error examples

```json
{
  "ok": false,
  "error_type": "NETWORK_LOG_UNAVAILABLE",
  "recovery_hint": "Network event capture is not enabled in this build of pagerunner. Upgrade to a version that includes network subscriptions."
}
```

> `NETWORK_LOG_UNAVAILABLE` is returned at runtime when `Network.enable` was not successfully called on session open (i.e. the session predates network subscription support). It is a runtime check, not a compile-time feature flag. Sessions opened after LNY-167 ships will always have network subscriptions active.

```json
{
  "ok": false,
  "error_type": "VALIDATION_ERROR",
  "recovery_hint": "Provide either target_id (for a specific tab) or all_tabs: true (for all tabs in the session)."
}
```

### Field notes

| Field | Notes |
|---|---|
| `request_headers` | Always present; may be an empty object `{}` if the CDP event carried no headers. Sensitive headers stripped at write time (see Security section). No `response_headers` field in v1. |
| `request_body` | `null` unless `include_request_body: true` |
| `response_body` | Truncated to 2KB unless `full_response: true`. `null` when there is no response body (e.g. 204 No Content) — in-flight requests are excluded from results entirely (see Entry Lifecycle). |
| `response_truncated` | `true` when body exceeded 2KB and was truncated. `false` when body fits in 2KB, `full_response: true`, or `response_body` is `null`. |
| `status` | Always a valid HTTP status code — in-flight requests are not returned (see Entry Lifecycle). |
| `duration_ms` | Always populated — in-flight requests are not returned. |
| `total_matched` | Count of entries matching applied filters, before `limit` is applied. |
| `total_captured` | Single-tab query: total entries in that tab's ring buffer. `all_tabs: true`: sum across all tab ring buffers in the session. |
| `result_truncated` | `true` when `total_matched > limit` |
| `tab_id` | Always present — essential for identifying origin when `all_tabs: true` returns mixed-tab results. |
| Anonymization | If session has anonymization enabled, `response_body` and `request_body` pass through the same PII pipeline as `get_content`. Header values are not anonymized beyond the fixed sensitive-header strip (see Security section). |

---

## Entry Lifecycle

The ring buffer writes entries **on response complete**, triggered by `loadingFinished`. At that point, `Network.getResponseBody` is called eagerly to capture the body before writing the entry to ReDB. This means:

- In-flight requests (fired but response not yet received) are **not included** in results.
- All entry fields are fully populated at write time — no two-phase write/update pattern.
- If the session closes before a response completes, the partial request entry is discarded.

> Rationale: write-on-complete avoids a two-phase storage pattern (write stub → update on response). The tradeoff is that very slow requests may not appear in `get_network_log` until they finish.

---

## Security

Sensitive headers are stripped at **write time** and never stored:

- `Authorization`
- `Cookie`
- `Set-Cookie`
- `X-Auth-Token`

This applies to `request_headers`. No response headers are stored in v1, but the same list applies if added later.

---

## Ring Buffer Spec

| Property | Value |
|---|---|
| Capacity | 500 entries per tab (configurable — see config below) |
| Scope | Per `(session_id, target_id)` |
| Eviction | Oldest-first when at capacity |
| TTL | Entries expire lazily at query time if older than 24h. No background sweep. |
| Storage | ReDB, table name: `network_log` |
| Key structure | `(session_id: &str, target_id: &str, sequence_u64)` — monotonic per-tab counter, enables ordered scan and oldest-first eviction |
| Cleanup | All entries for a session are deleted on `close_session` |
| Result ordering | Newest-first by `timestamp_ms` (descending). Results are sorted in memory by `timestamp_ms` after retrieval from the sequence-keyed index — sequence order is used only for eviction and storage scan, not for result ordering. For `all_tabs: true`, entries from all tab buffers are merged and sorted by `timestamp_ms` before applying `limit`. |

### config.toml

```toml
[network]
buffer_capacity = 500   # entries per tab; default 500, must be >= 1
```

---

## URL Pattern Matching

`url_pattern` supports two forms, matched against the **full URL string** (scheme + host + path + query string):

- **Substring**: `"graphql"` matches any URL containing that string anywhere (including query params).
- **Glob**: standard glob syntax applied to the full URL string. The segment-boundary rule applies to path components only — in the hostname portion, `*` matches any subdomain freely (no `/` separators to respect). `*` matches within a single path segment; `**` crosses `/` boundaries. Examples: `"/api/v1/*"` matches `/api/v1/users` but not `/api/v1/users/123`; use `"/api/v1/**"` for recursive match. `"*.example.com/**"` matches any subdomain on any path.

No regex support in v1.

---

## Dependency Chain

```
LNY-166  Rebuild CdpConn (events no longer dropped)
  └── LNY-167  Network.enable + subscribe requestWillBeSent / responseReceived / loadingFinished
        └── LNY-168  Ring buffer + get_network_log tool (this spec)
```

This spec must be written and approved before LNY-168 begins.

---

## Out of Scope (v1)

- `wait_for_request` reactive tool (separate concern, future issue)
- Response headers capture
- Response body capture for binary/non-text content types
- Deferred/on-demand response body fetch (v1 calls `Network.getResponseBody` eagerly at `loadingFinished` time; lazy fetching by `request_id` is not supported)
- Filtering by request initiator or resource type
- Background TTL sweep (expiry is lazy at query time)
