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
      js_code: String,              // fetch()-based JS function body
      description: String,          // human-readable, shown to agent
      params_schema: Option<Value>, // JSON schema for params arg — informational only, not validated at runtime
      trusted: bool,                // true = compiled-in seed adapter; cannot be overwritten
      created_at: u64,
      last_used: u64,
      last_error: Option<String>,
    }
  },
  selectors: {
    selector_string → {
      successes: u32,               // cumulative lifetime successes
      failures: u32,                // cumulative lifetime failures
      last_seen: u64,               // unix micros
    }
  },
  auth_tokens: {
    kind → vault_ref: String,       // e.g. "site_vault:a3f9b2" — never raw
  },
  last_updated: u64,                // unix micros; updated on every write to this entry
}
```

**TTL — lazy expiry on read (same pattern as network log ring buffer):**
- Entire entries expire 90 days after `last_updated`; deadline derived at read time as `last_updated + 90_days`; checked and removed on `get_site_knowledge` and `call_site_api`
- `last_updated` is refreshed on every write: `register_adapter`, auth token detection, selector stability update
- Individual adapters with `last_used == 0` (never called) are pruned after 30 days; checked on `get_site_knowledge`
- No background task required

**`params_schema`** is stored and returned to the agent as documentation only. Runtime validation of `params` against the schema is out of scope for this iteration — the agent is responsible for passing correct params.

---

## Auth Token Vault (LNY-184)

Auth tokens are **site-level state** and must survive session close. They are **not** stored in the session-scoped anonymizer vault (`src/anonymizer/vault.rs`), which is purged on `close_session`.

Instead, auth tokens are encrypted with a dedicated key derived from the master DB key using a fixed salt: `b"site_knowledge_auth_tokens_v1"`. Encrypted values are stored directly in `site_knowledge[origin].auth_tokens[kind]` as base64 strings. The vault ref format is `"site_vault:<sha256_prefix_of_encrypted_value>"` — this is the reference stored in audit events and returned by `get_site_knowledge`.

Raw token values never leave the encrypted store. Even `get_site_knowledge` returns only vault refs.

---

## New MCP Tools + CLI Subcommands

All three tools follow the standard MCP + CLI parity pattern and are subject to the existing `check_tool_permitted` policy. In high-security sessions (e.g. `allowed_tools` explicitly set), `register_adapter` and `call_site_api` must be explicitly included to be callable — they are not implicitly permitted.

`build_args_summary` entries for audit logging:
- `get_site_knowledge`: log `origin`
- `register_adapter`: log `origin` and `name` only — **never log `js_code`**
- `call_site_api`: log `origin` and `name` only — **never log `params`**

### `get_site_knowledge(origin: String)`

Returns what pagerunner knows about an origin.

- **Auth tokens:** returned as vault refs only (e.g. `"site_vault:a3f9b2"`), never raw values
- **Adapter JS code:** each `js_code` field is individually wrapped:
  ```
  <<<ADAPTER_CODE>>>
  async (params, session) => { ... }
  <<<ADAPTER_CODE>>>
  ```
  Each adapter is a separate field in the JSON response; only the `js_code` string value is wrapped, not the whole response.
- **Selector stability:** returns selectors sorted by reliability score (`successes / (successes + failures)`), ties broken by `last_seen` descending
- Returns `null` (not an error) if no entry exists for the origin
- Triggers lazy TTL pruning: removes expired adapters before returning

### `register_adapter(origin, name, description, js_code, params_schema?)`

Stores an agent-written JS adapter for an origin.

- **Overwrite protection:** cannot overwrite an adapter with `trusted: true` — returns an error with a clear message
- **Size cap:** `js_code` max 64KB; `name` max 128 chars; `description` max 1KB
- Adapters registered via this tool always have `trusted: false`
- Audit event: `AdapterRegistered { origin, name, trusted: false }`

**Adapter contract — JS invocation:**

```js
// pagerunner wraps and executes the stored js_code as:
//
//   const AsyncFunction = Object.getPrototypeOf(async function(){}).constructor;
//   const fn = new AsyncFunction('params', 'session', js_code);
//   const result = await fn(params, { origin });
//
// 'params' is the JSON object passed by the agent call
// 'session' provides read-only context: { origin: string }
//   (no tokens, no cookies — browser context provides auth automatically)
//
// js_code must be a function body (not a function declaration).
// It must return a value (use return or top-level await).
```

Example adapter body (what goes in `js_code`):
```js
const res = await fetch('https://api.linear.app/graphql', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    query: `mutation CreateComment($issueId: String!, $body: String!) {
      commentCreate(input: { issueId: $issueId, body: $body }) {
        success comment { id createdAt }
      }
    }`,
    variables: { issueId: params.issue_id, body: params.body }
  })
});
return res.json();
```

### `call_site_api(session_id, target_id, origin, name, params)`

Executes a stored adapter via `evaluate()` in the specified tab.

**Security checks (in order, fail fast):**

1. **Tool permission check:** `check_tool_permitted("call_site_api")` — same as all other tools
2. **Origin-vs-tab check:** derive the tab's current origin from its URL in the `Session` struct's `tab_urls` map (populated by `navigate` and updated on CDP `Page.frameNavigated` events). If the tab's origin does not match the adapter's registered origin, return error: `"Adapter origin 'https://linear.app' does not match tab origin 'https://github.com'"`
3. **Allowed-domains check:** if the session has `allowed_domains`, the adapter's `origin` host must match the list
4. **Anonymization:** if the session has anonymization enabled, apply PII scrubbing to the result (same pipeline as `get_content`)
5. **Timeout:** 30s hard cap via existing `evaluate()` timeout parameter

**Network reach of adapters:** Adapters run JS `fetch()` inside the browser tab. The browser enforces CORS but does not block cross-origin requests initiated from a trusted origin. An adapter registered for `https://linear.app` can technically call any URL reachable from that tab. This is equivalent to the network reach of the existing `evaluate()` tool — `call_site_api` does not add new network capability, it adds convenience and caching. Treat `trusted: false` adapters as having the same trust level as arbitrary `evaluate()` calls.

**Result wrapping:** response content wrapped in `<<<UNTRUSTED_WEB_CONTENT>>>` markers (same as `get_content`).

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

Detected tokens are encrypted using the dedicated site vault key (see Auth Token Vault section above). The vault ref is written to `site_knowledge[origin].auth_tokens[kind]`.

If a token for the same `(origin, kind)` already exists, it is overwritten — this handles token rotation naturally.

**Audit event:** `AuthTokenDetected { origin, kind }` — no value, no vault ref.

---

## Selector Stability Tracking (LNY-191)

### What is tracked

`click`, `fill`, `select` tool calls update `site_knowledge[origin].selectors[selector]` on every execution. The **origin** is derived from the tab's current URL in `Session.tab_urls` at the time of the call — the same map used by `call_site_api`.

- On success: `successes += 1`, `last_seen = now`
- On failure (element not found, timeout): `failures += 1`, `last_seen = now`

Selector strings are capped at 2KB; entries beyond this limit are silently dropped (no error surfaced to agent).

### Fragility warnings in responses

Fragility is computed from **cumulative lifetime counters**: `failures / (successes + failures)`. When this rate exceeds 30% and the selector has been seen at least 5 times total (to avoid noise on new selectors), the response includes:

```json
{
  "_warning": "Selector '.submit-btn' has a 40% failure rate (8/20 uses) on https://linear.app — consider finding a more stable selector",
  "_hint": "Use get_site_knowledge('https://linear.app') to see alternative selectors with better reliability"
}
```

The "last 10 uses" wording is dropped — the data model stores only cumulative counters, and lifetime rate with a minimum sample size is simpler and equally useful.

### `get_site_knowledge` output

Selectors are returned sorted by reliability score descending (highest `successes / total` first). The agent can use this to pick the most reliable selector for a site.

---

## Seed Adapters (LNY-188)

### Delivery

Seed adapter JS bodies are stored as files under `src/adapters/` and compiled into the binary via `include_str!`. A `SeedAdapter` struct with `origin`, `name`, `description`, and `js_code` is defined as a `const` array. On first `call_site_api` or `get_site_knowledge` for an origin matching a seed adapter, that adapter is loaded into `site_knowledge` with `trusted: true`.

Seed adapters cannot be overwritten by `register_adapter`. If an agent tries, the error is: `"Cannot overwrite trusted seed adapter '<name>'. Use a different name to register a custom adapter for this origin."`

### Target sites (priority order)

1. GitHub (REST: create issue, list issues, search)
2. Linear (GraphQL: create issue, update status, create comment)
3. Jira (REST: create issue, transition issue)
4. Notion (REST: create page, append block)
5. Gmail (REST: list messages, get message)

### Seed adapter live tests

Each seed adapter has a corresponding integration test tagged `#[ignore]` unconditionally (same as the NER test). They require a real Chrome session authenticated to the target service. To run:

```bash
cargo test --test cli_tools_integration test_seed_adapter_github -- --ignored
```

These tests are never run in CI. They are manual validation only, run before any release that changes seed adapters.

---

## Error Handling

| Scenario | Behaviour |
|----------|-----------|
| `call_site_api` — origin mismatch | Error: descriptive message, no execution |
| `call_site_api` — adapter not found | Error with hint to use `register_adapter` or `get_site_knowledge` |
| `call_site_api` — JS throws | Error with JS error message; failure NOT recorded in selector stability |
| `call_site_api` — timeout | Error: `"Adapter timed out after 30s"` |
| `call_site_api` — tool not permitted | Error: `"Tool 'call_site_api' is not permitted in this session"` |
| `register_adapter` — overwrite trusted | Error: `"Cannot overwrite trusted seed adapter '<name>'. Use a different name to register a custom adapter for this origin."` |
| `register_adapter` — size cap exceeded | Error: `"js_code exceeds 64KB limit"` / `"name exceeds 128 char limit"` |
| Auth token detection — encryption error | Log warning, skip storage; ring buffer event still written (with token value redacted to `[REDACTED]`) |
| `get_site_knowledge` — unknown origin | Returns `null`, not an error |
| Selector update — selector > 2KB | Silently dropped, no error |
| TTL expiry on read | Expired entries removed before response returned; not surfaced as error |

---

## Testing

### Unit tests

- `site_knowledge` table CRUD (insert, update, lazy TTL expiry, adapter pruning)
- Auth token detection: each pattern matched correctly; vault ref stored; raw value absent from ring buffer; site vault key derivation is deterministic
- Selector stability: success/failure counting; fragility warning at >30% with minimum 5 samples; no warning below 5 samples
- `register_adapter`: trusted overwrite blocked; size caps enforced; `js_code` absent from audit log
- `call_site_api`: origin-vs-tab check; allowed-domains check; tool permission check; timeout enforcement
- `AsyncFunction` wrapping: adapter body with `await` executes correctly; syntax error in adapter returns clean error

### CLI integration tests (`tests/cli_tools_integration.rs`)

- `get-site-knowledge` — unknown origin returns null (non-Chrome)
- `register-adapter` — size cap errors (non-Chrome)
- `call-site-api` — tool-not-permitted error when blocked (non-Chrome)
- `register-adapter` + `call-site-api` round-trip (Chrome, macOS only)
- `call-site-api` origin mismatch returns error (Chrome, macOS only)
- Selector stability warning appears after simulated failures (Chrome, macOS only)
- Auth token detection: request with `Authorization: Bearer` header produces vault ref in `site_knowledge`, raw value absent from network log (Chrome, macOS only)

### Seed adapter live tests

Each seed adapter: `#[ignore]` unconditionally, requires authenticated Chrome session. Run manually before releases that touch seed adapters.

---

## Threat Model Summary

| Threat | Mitigation |
|--------|-----------|
| Raw auth tokens in logs/ring buffer | Encrypt at ingestion with dedicated site vault key; vault refs only in DB and responses |
| Stale auth tokens after session close | Site vault key is persistent (not session-scoped); tokens survive across sessions |
| Rogue adapter running on wrong origin | Origin-vs-tab check before execute |
| Adapter bypassing allowed_domains | Allowed-domains check on adapter origin before execute |
| Adapter making arbitrary outbound fetches | Explicitly documented as equivalent to `evaluate()` trust level; same trust as `trusted: false` adapter |
| `call_site_api` bypassing tool permission policy | Subject to `check_tool_permitted` like all other tools |
| Prompt injection via adapter results | `<<<UNTRUSTED_WEB_CONTENT>>>` wrapping |
| Prompt injection via adapter code | Each `js_code` field individually wrapped in `<<<ADAPTER_CODE>>>` in `get_site_knowledge` response |
| Seed adapter tampering via filesystem | Compiled into binary; `trusted: true` blocks overwrite via `register_adapter` |
| Hung adapter execution | 30s hard timeout via `evaluate()` timeout parameter |
| Anonymization bypass via `call_site_api` | Result passes anonymization pipeline if session has it enabled |
| Oversized selector/adapter entries | 2KB selector cap; 64KB adapter code cap; 128 char name cap |
| Sensitive params/code in audit log | `js_code` and `params` never passed to `build_args_summary` |
