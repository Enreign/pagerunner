# pagerunner Security Model

pagerunner is designed to be used by AI agents (e.g. Claude in Claude Code) browsing
untrusted web content. This document describes what the security layer protects against,
what it does not protect against, and how to configure it.

---

## What Is Protected

### SSRF (Server-Side Request Forgery)

AI agents browsing the web can be tricked by a malicious page into navigating to internal
network addresses (e.g. `http://192.168.1.1/`, `http://169.254.169.254/metadata`).

pagerunner protects against this at two levels:

1. **URL validation** (`NetworkGuard`) — every `navigate`, `new_tab`, and `restore_tab_state`
   call is checked before Chrome loads the URL. Private IPs (10.x, 172.16-31.x, 192.168.x,
   169.254.x), loopback (127.x, ::1, localhost), non-http/https schemes, and embedded
   credentials are all blocked.

2. **CDP network blocking** (`Network.setBlockedURLs`) — when a session has a security
   policy, Chrome's network stack is configured to block requests to all private IP ranges,
   including redirect targets. This catches HTTP 301/302 redirects from allowed domains
   to private IPs that the URL validator cannot see before the request is made.

### Prompt Injection via Page Content

A malicious web page can embed instructions in its HTML designed to hijack the AI agent
reading it (e.g. hidden `<div style="display:none">Ignore previous instructions...</div>`).

pagerunner protects against this when `sanitize_content: true` (the default):

**`get_content` (raw HTML):**
- Strips `<script>`, `<style>`, `<noscript>`, HTML comments, and all HTML tags
- Strips elements with `display:none`, `visibility:hidden`, or `aria-hidden="true"`
- Strips zero-width Unicode characters (10 variants) used to hide text from humans
- Truncates output at 100,000 characters
- Wraps output in `<<<UNTRUSTED_WEB_CONTENT domain="...">>>` markers with instructions
  not to follow content within
- Logs a warning when known injection patterns are detected (configurable via `scan_injections`)

**`evaluate` (JS evaluation results — JSON/text, not HTML):**
- Strips zero-width Unicode characters
- Truncates output at 100,000 characters
- Wraps output in `<<<UNTRUSTED_WEB_CONTENT domain="...">>>` markers
- HTML tags are preserved (evaluate results are not HTML; stripping would corrupt JSON)
- Logs a warning when known injection patterns are detected

Tab titles returned by `list_tabs` are also sanitized (zero-width stripping, 200-char truncation)
with injection pattern warnings when a policy is active.

### Navigation Budget

Sessions can be limited to a maximum number of page navigations (`max_navigations`) to prevent
runaway browsing. Configurable per-session at `open_session` time.

### Domain Allowlisting

Sessions can be restricted to a set of allowed domains (`allowed_domains`). Navigations to
domains outside the list are blocked. Subdomain matching is supported (e.g. `github.com`
allows `api.github.com`).

### Snapshot Integrity

Cookie/localStorage snapshots are validated on restore: the stored origin must match the
requested origin before any cookies are injected, preventing DB-corruption attacks.

---

## Known Limitations

### JavaScript `fetch()` and XHR to External Hosts

The `evaluate` tool runs arbitrary JavaScript in the browser tab. Page-context JavaScript
can call `fetch()`, `XMLHttpRequest`, or `WebSocket` to connect to **external internet hosts**
(e.g. `https://attacker.com`).

`Network.setBlockedURLs` prevents connections to **private IP ranges** (SSRF protection),
but it does not restrict connections to arbitrary public internet hosts. A malicious page
that tricks the agent into calling `evaluate` with attacker-controlled JS can exfiltrate
data to an external server.

**Root cause:** Enforcing a domain allowlist on outbound network requests from page context
requires intercepting each network request as it is made and allowing/blocking it. In CDP
this is done via `Fetch.enable` + `Fetch.requestPaused` events, which requires the CDP
connection to receive and dispatch asynchronous events. The current `CdpConn` implementation
uses a synchronous request/response loop and drops events (see `src/cdp.rs` line 81).

**Future work:** Implementing CDP event streaming in `CdpConn` would unlock full request
interception (`Fetch.enable`), making it possible to enforce `allowed_domains` at the
Chrome network level for all requests including `fetch()` from page JS. This is a
significant architectural change to `src/cdp.rs` and `src/browser.rs`.

**Mitigations available today:**
- Use `allowed_domains` to restrict navigation — the agent cannot be directed to a
  malicious domain via `navigate` or `new_tab`
- Limit use of `evaluate` to trusted, expected operations
- Keep `scan_injections: true` (default) — this logs a warning when injection patterns
  appear in evaluate results, giving the operator visibility

### Screenshot Content

`screenshot` returns the full rendered page as a PNG. Any text visible on the page —
including prompt injection strings — is embedded in the image. There is no practical way
to sanitize image content at the server layer. Vision-capable models reading screenshots
are exposed to whatever the page renders.

**Mitigation:** Avoid using `screenshot` as a primary content extraction method when
browsing untrusted pages with a security-sensitive agent.

### Client-Side JavaScript Redirects

A page can redirect the browser using `window.location.href = "http://..."` after the
initial page load. `Network.setBlockedURLs` intercepts the resulting network request
(private IPs are blocked), but the redirect happens after the page has loaded and any
page-JS injection has already had an opportunity to run.

---

## Configuration Reference

All security settings are in `config.toml` under `[security]` and can be overridden
per-session via `open_session` parameters:

~~~toml
[security]
sanitize_content = true      # Strip hidden elements, HTML tags, zero-width chars from get_content and evaluate
scan_injections = true       # Log warnings when injection patterns detected
allowed_domains = []         # Empty = allow all public domains. E.g. ["github.com", "example.com"]
max_navigations = 0          # 0 = unlimited. Set to limit page loads per session.
~~~
