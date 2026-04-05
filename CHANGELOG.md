# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1] — Secret Store Fix

### Fixed

- **store_as_secret stored JSON wrapper instead of plain value**: CDP `Runtime.evaluate` returns `{"type":"string","value":"..."}` for string results. `store_as_secret` was storing the entire JSON object, causing `use_secret` to pipe JSON instead of the raw token value to commands like `gh secret set`. Now correctly extracts the plain string via `result["value"]`.

## [0.7.0] — Daemon Hardening: Crash Recovery, Sleep/Wake, Session Health

### Added

**CDP Reconnection & Crash Recovery**
- Sessions auto-recover from Chrome crashes: daemon spawns a new Chrome process, restores the latest checkpoint, and preserves the session ID
- CDP reconnection with exponential backoff when the WebSocket connection drops
- `send_with_timeout` on CDP commands prevents indefinite hangs on frozen Chrome
- Swappable WebSocket transport in CdpConn enables reconnection without recreating sessions

**Sleep/Wake Resilience (macOS)**
- IOKit-based sleep/wake detection via `IORegisterForSystemPower`
- Pre-sleep: all sessions are checkpointed before macOS is allowed to sleep (`IOAllowPowerChange` deferred until checkpoints complete)
- Post-wake: session reconnection triggered automatically after wake

**Session Health State Machine**
- Sessions track health as `Alive`, `Reconnecting`, `Recovering`, or `Dead` (replaces boolean `alive` flag)
- `list_sessions` exposes `status` field for menu bar and Raycast integration
- Tool calls on non-alive sessions return specific errors (`SessionReconnecting`, `SessionRecovering`, `SessionDead`)

**Daemon Improvements**
- Graceful shutdown: all sessions checkpointed on SIGTERM/SIGINT before daemon exits
- Daemon tracing now tees to both stderr and `~/.pagerunner/daemon.log` for persistent logging
- Default log filter `pagerunner=info` when `RUST_LOG` is not set
- Session registry stores `ws_url` for faster post-sleep reconnection

### Fixed

- **Session not found in standalone CLI mode**: each CLI invocation now reconciles the session registry, reattaching to surviving Chrome processes from previous invocations
- **Chrome port timeout**: profile lock detection before spawn (checks `SingletonLock` symlink for live PID), early exit detection after spawn (`try_wait` within 100ms instead of 5s timeout)
- **Dangling symlink check**: `check_profile_lock` uses `symlink_metadata()` instead of `exists()` (which follows symlinks and returns false for Chrome's dangling `SingletonLock`)
- Pending CDP requests are drained on transport replacement, preventing leaked oneshot channels

## [0.6.1] — macOS Menu Bar Reliability & Onboarding Fixes

### Fixed

**macOS Menu Bar — Session/Tab Continuity**
- Sessions and tabs are now preserved on transient poll failures (empty daemon response during a hiccup no longer wipes visible state)
- `list_sessions` missing `data` key no longer increments the consecutive-failure counter — daemon is treated as alive
- `list_tabs` failure no longer clears existing tabs — stale tabs remain visible until a successful refresh
- `previousSessionStates` is no longer wiped when sessions are preserved, preventing spurious "session started" notifications after a transient empty response
- Idle tracker entries are now cleaned up when sessions crash, not just when they disappear entirely
- Concurrent poll guard: a second poll triggered by opening the panel no longer runs while the first is still in flight

**macOS Menu Bar — Optimistic UI**
- Closing a session removes it from the UI immediately (no 2s wait for next poll)
- Closing a tab removes it from the UI immediately
- Opening a new tab shows a placeholder immediately; replaced by real data on next poll
- Restoring a checkpoint clears stale tabs immediately so the restored state appears on the next poll
- `detachProfile` now removes the profile from `remoteSessions` — VM badge no longer lingers after detach
- Discovery cache is invalidated after attaching a Chrome instance, so re-scanning finds the right set of ports

**Onboarding**
- `install-launchd.sh` now detects the pagerunner binary via `which pagerunner` (works for Homebrew/Cargo/binary installs — no longer requires a source build at `target/release/`)
- Daemon status output no longer shows a false alarm when running in standalone mode — now reads "standalone mode — OK for single session" instead of implying the daemon is broken
- README: correct menu bar build instructions (`package.sh`, not `swift build`); document Homebrew Rust PATH shadowing for `cargo install` users; clarify two-restart requirement for launchd daemon setup
- Release pipeline: draft releases now wait for all platform binaries to upload before going live; Homebrew tap PR is opened automatically on release

### Internal
- `cargo fmt` and Clippy lint fixes applied across all `src/` files (pre-existing CI failures on `main`)
- 14 new regression tests in `apps/menubar/Tests/PagerunnerCoreTests/PollRegressionTests.swift` covering all fixed behaviours

## [0.6.0] — Session Persistence & TCP-Only Chrome

### Added

**Session Persistence**
- Chrome now uses TCP-only CDP transport (`--remote-debugging-port` on `127.0.0.1`) — Chrome runs independently of the daemon and survives restarts
- Session registry (`src/session_registry.rs`) — persists session metadata to DB for auto-reattach
- Startup reconciliation — on daemon/MCP startup, automatically reattaches to surviving Chrome instances
- Auto-checkpoint on every `close_session` (named "Autosave · close")
- Periodic auto-checkpoint background task (default every 5 minutes)

**Configurable Retention**
- `[checkpoints]` config section: `enabled` (default true), `interval_seconds` (default 300)
- `[retention]` config section: `max_snapshot_versions` (default 10, was hardcoded 3), `site_knowledge_ttl_days` (default 0 = indefinite, was hardcoded 90)

### Changed
- Chrome spawn no longer uses `--remote-debugging-pipe` — fully TCP-based
- Daemon graceful shutdown (SIGTERM/SIGINT) leaves Chrome alive for reattach on restart
- Test harness tracks Chrome PIDs and kills orphans on `TestDaemon` drop

### Removed
- Pipe-based CDP transport (fd3/fd4) from Chrome spawn — replaced by TCP WebSocket

## [0.5.0] — 2026-03-28

### Added

**macOS Menu Bar App**
- Native Swift companion app (`apps/menubar/`) — shows all Chrome profiles, active sessions, open tabs, and daemon status at a glance
- One-click open/close sessions, save session checkpoints, control the daemon — no terminal required
- Agent profiles section (distinct CPU icon, separate section) for profiles with `kind = "agent"` in config.toml
- Add Profile flow: auto-discovers unconfigured Chrome profiles + creates new agent profiles (creates `~/.chrome-<name>` dir)
- Active session indicators: green dot + "N windows" badge on profile rows; "Active" badge on live windows
- macOS native notifications: daemon stopped, session crashed, checkpoint saved, agent idle
- Global hotkey support via KeyboardShortcuts; auto-update via Sparkle 2.x
- Navigation strip scrolls horizontally when profiles overflow panel width

**Session Checkpoints**
- `save_session_checkpoint` — saves full tab state (URLs + scroll positions) for all alive tabs in a session
- `restore_session_checkpoint` — reopens all saved tabs and navigates to stored URLs
- `list_session_checkpoints` — lists saved checkpoints for a profile
- `delete_session_checkpoint` — removes a checkpoint by ID
- `attach_session` — attach to an already-running Chrome instance via `--remote-debugging-port`
- `close_tab` — close a specific tab within a session
- `get_console_log` MCP tool + CLI: capture JS console output from a page
- `ChromeProfile.kind` field in config.toml: `"personal"` (default) or `"agent"`

**Runtime Console Error Surfacing**
- `evaluate` responses now include runtime console errors and exceptions that occur during script execution
- Unhandled promise rejections and thrown exceptions surfaced alongside the result

### Changed
- CLI subcommand count: 27 → 38 (all new MCP tools have matching CLI subcommands)

## [0.4.0] — Site Intelligence Tier

### Added

- `site_knowledge` encrypted ReDB table: persistent per-origin store for adapters, selector stability, and auth token vault refs
- `get_site_knowledge` MCP tool + CLI: inspect what pagerunner has learned about a site
- `register_adapter` MCP tool + CLI: store a JS adapter (agent-written) for direct API calls
- `call_site_api` MCP tool + CLI: execute a stored adapter via AsyncFunction in the browser tab
- Auth token detection at network ingestion: Bearer, Basic, X-API-Key, session cookies → encrypted site vault (never in ring buffer or logs)
- Selector stability tracking: `click`/`fill`/`select` update per-origin per-selector success/failure counters; fragility warning appears in tool response when failure rate >30% over ≥5 uses
- Seed adapters (compiled-in, trusted): GitHub (list-issues, create-issue, search-issues), Linear (create-issue, create-comment, update-status), Jira (create-issue, transition-issue), Notion (create-page, append-block), Gmail (list-messages, get-message)
- `Page.frameNavigated` CDP event handler updates `tab_urls` in real-time via `frame_nav_processor` task
- New audit events: `AdapterRegistered`, `AuthTokenDetected`, `SiteApiCalled`

## [0.1.1] — 2026-03-22

### Added - Hallucination Prevention

**Semantic metadata for all tool responses** — prevents LLM hallucination when interpreting ambiguous data.

- **Array ambiguity detection**: `evaluate()` warns when returning unlabeled arrays like `[25, 2]` with explicit message: "Result is an array — field meanings cannot be inferred"
- **Condition clarity**: `wait_for()` clarifies `_condition_type` (selector/url/fixed_delay) and whether `_condition_met`
- **Schema information**: `list_tabs()`, `list_sessions()`, `list_profiles()` include field descriptions
- **Tool context**: All responses include `_tool`, `_result_type`, `_hint`, `_warning` fields
- **CLI/MCP parity**: Both interfaces receive identical metadata (no information loss)

**Problem solved** (incident 2026-03-21): Array `[25, 2]` was interpreted as "25 likes, 2 replies" instead of "25 views, 2 likes". With metadata warning, Claude now asks for clarification instead of guessing.

**Implementation**:
- New `ToolResponse` struct: `{result: String, metadata: Option<Value>}`
- `build_tool_metadata()` function analyzes results and generates context
- Single source of truth: metadata generated once in `dispatch_tool()`, used everywhere
- MCP envelope wraps metadata in second content block
- CLI wraps metadata in JSON output alongside result

**Test validation**:
- ✅ 232/232 unit tests passing (14 new metadata tests)
- ✅ Real-world testing: Compared against agent-browser and Playwright
- ✅ Zero breaking changes

**Examples**:
```bash
# Unlabeled array (triggers warning)
pagerunner evaluate $SESSION $TARGET "Array.from(...).map(el => parseInt(el.textContent))"
# Response includes: _warning: "Result is an array — field meanings cannot be inferred"

# Labeled object (no warning)
pagerunner evaluate $SESSION $TARGET "Array.from(...).map(el => ({value: ..., label: ...}))"
# Response includes: _hint: "Always return labeled objects..."
```

**Monitoring**:
- Track array warnings: `grep "_warning.*array" ~/.pagerunner/audit.log`
- Incident tracking: GitHub issues with `hallucination-prevention` label
- See [docs/MONITORING_HALLUCINATION_PREVENTION.md](docs/MONITORING_HALLUCINATION_PREVENTION.md) for detailed procedures

**Documentation**:
- [HALLUCINATION_PREVENTION.md](HALLUCINATION_PREVENTION.md) — User guide
- [docs/MONITORING_HALLUCINATION_PREVENTION.md](docs/MONITORING_HALLUCINATION_PREVENTION.md) — Monitoring guide

---

## [0.1.0] — 2026-03-21

### Added

**Core browser automation**
- Chrome browser automation via CDP (Chrome DevTools Protocol) — drives real Chrome with your existing user profiles (cookies, saved passwords, history)
- 27 MCP tools exposed over stdio transport, each with a matching direct CLI subcommand
- Session management: `open_session`, `close_session`, `list_sessions`, `list_profiles`
- Tab management: `new_tab`, `list_tabs`
- Navigation: `navigate`, `wait_for` (selector, URL pattern, or fixed delay)
- Content: `get_content` (sanitized page text), `screenshot` (PNG — file or base64), `evaluate` (JavaScript)
- Interactions: `click`, `type_text`, `fill` (input + textarea, with React/Vue/Angular synthetic events), `select`, `scroll`

**State persistence**
- Encrypted local database (ReDB, AES-256-GCM); key stored in macOS Keychain under `pagerunner / db_key`
- Snapshots: save/restore page cookies + localStorage per origin (`save_snapshot`, `restore_snapshot`, `list_snapshots`, `delete_snapshot`)
- Tab state: save and restore open tab URLs across sessions (`save_tab_state`, `restore_tab_state`)
- Persistent namespaced KV store: `kv_set`, `kv_get`, `kv_delete`, `kv_list`, `kv_clear`

**Security layer**
- SSRF protection: blocks navigation to private IPs, loopback, `file://`, `javascript:`, and embedded-credential URLs — at both the URL-validation layer and via CDP `Network.setBlockedURLs`
- Prompt injection sanitization: strips hidden elements, HTML tags, zero-width Unicode characters; wraps content in `<<<UNTRUSTED_WEB_CONTENT>>>` markers
- Domain allowlisting: restrict sessions to an explicit list of allowed domains (`allowed_domains`)
- Navigation budget: limit page loads per session (`max_navigations`)
- Snapshot integrity: stored origin must match requested origin before cookies are injected
- Audit log: append-only JSON-lines at `~/.pagerunner/audit.log`; DB copy at `~/.pagerunner/state.db`

**PII anonymization**
- Pass `anonymize: true` to `open_session` to strip PII from all `get_content` and `evaluate` results before they reach the AI
- Two modes: `tokenize` (replaces PII with reversible tokens like `[EMAIL:a3f9b2]`) and `redact` (one-way `[EMAIL]`)
- Default entities: EMAIL, PHONE, CREDIT_CARD, IBAN, SSN, IP address
- De-tokenization in `fill` and `type_text`: pass tokens back — Pagerunner writes the real values to the DOM
- Named anonymization profiles in `config.toml` (per-domain rules, custom regex patterns)
- NER (PERSON and ORG name detection) via local ONNX model — opt-in via `--features ner` build + `pagerunner download-model`

**Operations**
- Daemon mode: `pagerunner daemon` holds the single DB lock and proxies all tool calls over a Unix socket (`~/.pagerunner/daemon.sock`), enabling multiple Claude Code windows to share state
- Stealth mode: hides automation signals, masks `navigator.webdriver`, adds human-like input delays
- macOS launchd service installer (`scripts/install-launchd.sh`) — runs daemon at login, restarts on crash
- `pagerunner init`: auto-detects Chrome profiles and writes `~/.pagerunner/config.toml`
- `pagerunner status`: shows config and active sessions
- `pagerunner example-config`: prints an annotated config template
- `pagerunner audit`: query the audit log by tail count, session ID, or timestamp

[Unreleased]: https://github.com/Enreign/pagerunner/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/Enreign/pagerunner/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Enreign/pagerunner/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/Enreign/pagerunner/compare/v0.1.1...v0.4.0
[0.1.1]: https://github.com/Enreign/pagerunner/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Enreign/pagerunner/releases/tag/v0.1.0
