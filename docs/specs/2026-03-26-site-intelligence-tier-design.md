# Site Intelligence Tier — Design

**Date:** 2026-03-26
**Issues:** LNY-184, LNY-185, LNY-187, LNY-188, LNY-191
**Status:** Approved

---

## Problem

Pagerunner currently drives every interaction through the browser DOM — clicking, filling, evaluating JS. This works but is fragile: selectors break when UIs change, DOM interactions are slow, and there's no memory across sessions about what works on a given site.

The site intelligence tier adds a persistent learning layer: pagerunner remembers selectors that work, detects and vaults auth tokens, and lets agents store JS adapters that call site APIs directly — bypassing the DOM entirely for stable, fast interactions.

---

## Architecture

### `site_knowledge` ReDB table (LNY-185)

New encrypted table in `~/.pagerunner/state.db`, keyed by origin string (e.g. `https://linear.app`).

```
site_knowledge[origin] = {
  adapters: {
    name → {
      js_code: String,          // fetch()-based JS function body
      description: String,      // human-readable, shown to agent
      params_schema: Option<Value>, // JSON schema for params arg
      trusted: bool,            // true = compiled-in seed adapter
      created_at: u64,
      last_used: u64,
      last_error: Option<String>,
    }
  },
  selectors: {
    selector_string → {
      successes: u32,
      failures: u32,
      last_seen: u64,           // unix micros
    }
  },
  auth_tokens: {
    kind → vault_ref: String,   // e.g. "vault:a3f9b2" — never raw
  },
  ttl: u64,                     // expiry unix micros for entire entry
}
```

**TTL:** Entries expire after 90 days of no `last_used` update. Adapters and selectors within an entry are pruned independently — a never-used adapter is removed after 30 days.

---

## New MCP Tools + CLI Subcommands

All three tools follow the standard MCP + CLI parity pattern.

### `get_site_knowledge(origin: String)`

Returns what pagerunner knows about an origin.

- **Auth tokens:** returned as vault refs only (e.g. `"vault:a3f9b2"`), never raw values
- **Adapter JS code:** wrapped in `<<<ADAPTER_CODE>>>` markers in the response to signal to the agent it is data, not instructions
- **Selector stability:** returns selectors sorted by reliability score (`successes / (successes + failures)`)
- Returns `null` (not an error) if no entry exists for the origin

### `register_adapter(origin, name, description, js_code, params_schema?)`

Stores an agent-written JS adapter for an origin.

- **Overwrite protection:** cannot overwrite an adapter with `trusted: true` — returns an error with a clear message
- **Size cap:** `js_code` max 64KB; `name` max 128 chars; `description` max 1KB
- Adapters registered via this tool always have `trusted: false`
- Audit event: `AdapterRegistered { origin, name, trusted: false }`

**Adapter contract — what the JS function receives:**

```js
// pagerunner calls the stored js_code as:
//   const fn = new Function('params', 'session', js_code);
//   fn(params, { origin })
//
// 'params' is the object passed by the agent
// 'session' is read-only context (origin only — no tokens exposed)
//
// The function must return a Promise (use async/await or return fetch(...))
```

### `call_site_api(session_id, target_id, origin, name, params)`

Executes a stored adapter via `evaluate()` in the specified tab.

**Security checks (in order, fail fast):**

1. Origin-vs-tab check: tab's current URL must share origin with the adapter's registered origin. If mismatch, return error: `"Adapter origin 'https://linear.app' does not match tab origin 'https://github.com'"`
2. Allowed-domains check: if the session has `allowed_domains`, the adapter origin must be in the list
3. Anonymization: if the session has anonymization enabled, apply PII scrubbing to the result (same pipeline as `get_content`)
4. Timeout: 30s hard cap via existing `evaluate()` timeout parameter

**Result wrapping:** response content wrapped in `<<<UNTRUSTED_WEB_CONTENT>>>` markers (same as `get_content`) — the API result is data from the web.

**Audit event:** `SiteApiCalled { origin, adapter_name }` — no params or result values logged.

---

## Auth Token Detection (LNY-184)

### Detection point

Tokens are detected **at network event ingestion**, before the event is written to the ring buffer. Raw token values never reach the ring buffer or audit log.

### Detected patterns

| Kind | Pattern |
|------|---------|
| `bearer` | `Authorization: Bearer <token>` |
| `basic` | `Authorization: Basic <base64>` |
| `api_key` | `X-API-Key: <value>`, `X-Auth-Token: <value>` |
| `session_cookie` | `Cookie:` header containing `session=`, `token=`, `auth=` |

### Storage

Detected tokens are encrypted using the existing session vault (AES-256-GCM, same as PII anonymization vault). The vault ref is written to `site_knowledge[origin].auth_tokens[kind]`.

If a token for the same `(origin, kind)` already exists, it is overwritten — this handles token rotation naturally.

**Audit event:** `AuthTokenDetected { origin, kind }` — no value, no vault ref.

---

## Selector Stability Tracking (LNY-191)

### What is tracked

`click`, `fill`, `select` tool calls update `site_knowledge[origin].selectors[selector]` on every execution:

- On success: `successes += 1`, `last_seen = now`
- On failure (element not found, timeout): `failures += 1`, `last_seen = now`

Selector strings are capped at 2KB; entries beyond this limit are silently dropped.

### Fragility warnings in responses

When a tool call uses a selector with a fragility rate > 30% over the last 10 uses, the response includes:

```json
{
  "_warning": "Selector '.submit-btn' has failed 4/10 recent uses on https://linear.app — consider finding a more stable selector",
  "_hint": "Use get_site_knowledge('https://linear.app') to see alternative selectors with better reliability"
}
```

### `get_site_knowledge` output

Selectors are returned sorted by reliability score descending. The agent can use this to pick the most reliable selector for a site.

---

## Seed Adapters (LNY-188)

### Delivery

Seed adapters are compiled into the binary as Rust `const` strings (via `include_str!`). They cannot be overwritten by `register_adapter` (enforced by `trusted: true` flag). They are loaded into `site_knowledge` on first use of `call_site_api` for a given origin if no entry exists.

### Target sites (priority order)

1. GitHub (REST: issues, PRs, search)
2. Linear (GraphQL: issues, comments, status updates)
3. Jira (REST: issues, transitions)
4. Notion (REST: pages, blocks)
5. Gmail (REST: messages, labels)

### Adapter structure (example)

```js
// Linear — create comment
// params: { issue_id: string, body: string }
async function(params, session) {
  const res = await fetch('https://api.linear.app/graphql', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      query: `mutation CreateComment($issueId: String!, $body: String!) {
        commentCreate(input: { issueId: $issueId, body: $body }) {
          success
          comment { id createdAt }
        }
      }`,
      variables: { issueId: params.issue_id, body: params.body }
    })
  });
  return res.json();
}
```

Seed adapters use `fetch()` with no explicit auth headers — the browser session's cookies and any stored credentials handle authentication automatically.

---

## Error Handling

| Scenario | Behaviour |
|----------|-----------|
| `call_site_api` — origin mismatch | Error: descriptive message, no execution |
| `call_site_api` — adapter not found | Error with hint to use `register_adapter` or `get_site_knowledge` |
| `call_site_api` — JS throws | Error with JS error message wrapped; failure NOT recorded in selector stability (different surface) |
| `call_site_api` — timeout | Error: `"Adapter timed out after 30s"` |
| `register_adapter` — overwrite trusted | Error: `"Cannot overwrite trusted seed adapter 'linear-create-comment'"` |
| Auth token detection — vault full | Log warning, skip storage; ring buffer entry still written (without raw token) |
| `get_site_knowledge` — unknown origin | Returns `null`, not an error |

---

## Testing

### Unit tests

- `site_knowledge` table CRUD (insert, update, TTL expiry, pruning)
- Auth token detection: each pattern matched correctly; vault ref stored; raw value absent from ring buffer
- Selector stability: success/failure counting; fragility warning threshold
- `register_adapter`: trusted overwrite blocked; size caps enforced
- `call_site_api`: origin check; allowed-domains check; timeout enforcement

### CLI integration tests (`tests/cli_tools_integration.rs`)

- `get-site-knowledge` — unknown origin returns null
- `register-adapter` + `call-site-api` round-trip (Chrome, macOS only)
- `call-site-api` origin mismatch returns error
- Selector stability warning appears after simulated failures (Chrome, macOS only)
- Auth token detection: request with `Authorization: Bearer` header produces vault ref in site_knowledge, not raw value in network log (Chrome, macOS only)

### Seed adapter tests

Each seed adapter has a corresponding live test (macOS only, `#[cfg_attr(not(target_os = "macos"), ignore)]`) that calls a real API endpoint and validates the response shape.

---

## Threat Model Summary

| Threat | Mitigation |
|--------|-----------|
| Raw auth tokens in logs/ring buffer | Encrypt at ingestion; vault refs only |
| Rogue adapter running on wrong origin | Origin-vs-tab check before execute |
| Adapter bypassing allowed_domains | Allowed-domains check before execute |
| Prompt injection via adapter results | `<<<UNTRUSTED_WEB_CONTENT>>>` wrapping |
| Prompt injection via adapter code | `<<<ADAPTER_CODE>>>` wrapping in `get_site_knowledge` |
| Seed adapter tampering | Compiled into binary; `trusted: true` blocks overwrite |
| Hung adapter execution | 30s hard timeout |
| Anonymization bypass via call_site_api | Result passes anonymization pipeline if session has it enabled |
| Oversized selector/adapter entries | 2KB selector cap; 64KB adapter cap |
