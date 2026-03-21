# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/Enreign/pagerunner/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Enreign/pagerunner/releases/tag/v0.1.0
