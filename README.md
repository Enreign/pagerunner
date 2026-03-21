# Pagerunner

<p align="center">
  <a href="https://github.com/Enreign/pagerunner/actions/workflows/ci.yml"><img src="https://github.com/Enreign/pagerunner/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/Enreign/pagerunner/releases/latest"><img src="https://img.shields.io/github/v/release/Enreign/pagerunner" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.91%2B-orange" alt="Rust 1.91+">
</p>

A Chrome browser automation MCP server. Connect Claude (or any MCP client) to a real Chrome session — using your existing profiles with their cookies, saved passwords, and history.

> [!WARNING]
> Pagerunner is early-stage software (v0.x). APIs and configuration formats may change between releases.

## Contents

- [How it works](#how-it-works)
- [Setup](#setup)
- [Using with your project](#using-with-your-project)
- [Tools](#tools)
- [Anonymization (PII protection)](#anonymization-pii-protection)
- [Stealth mode](#stealth-mode)
- [Daemon (multiple Claude Code sessions / persistent state)](#daemon-multiple-claude-code-sessions--persistent-state)
- [Snapshots](#snapshots)
- [Example session](#example-session)
- [Legal & Responsible Use](#legal--responsible-use)

## How it works

Pagerunner launches Chrome with your existing user profile via CDP (Chrome DevTools Protocol) and exposes it as an MCP server over stdin/stdout. Claude can then navigate, click, type, screenshot, and run JavaScript in the live browser.

## Setup

**1. Install**

```bash
# Option A — pre-built binary (no Rust needed):

# macOS arm64 (Apple Silicon):
curl -L https://github.com/Enreign/pagerunner/releases/latest/download/pagerunner-macos-arm64 \
  -o pagerunner && chmod +x pagerunner

# macOS x86_64 (Intel):
curl -L https://github.com/Enreign/pagerunner/releases/latest/download/pagerunner-macos-x86_64 \
  -o pagerunner && chmod +x pagerunner

# Linux x86_64:
curl -L https://github.com/Enreign/pagerunner/releases/latest/download/pagerunner-linux-x86_64 \
  -o pagerunner && chmod +x pagerunner

# macOS: if Gatekeeper blocks the binary on first run:
xattr -d com.apple.quarantine pagerunner

# Option B — install via cargo (requires Rust):
cargo install --git https://github.com/Enreign/pagerunner --locked

# Option C — build locally:
cargo build --release
```

**2. Auto-detect Chrome profiles**

```bash
pagerunner init
```

This reads Chrome's profile list and writes `~/.pagerunner/config.toml` automatically.
Run `pagerunner status` to verify.

Alternatively, write `~/.pagerunner/config.toml` by hand — see `pagerunner example-config` for the format.

**3. Register as MCP server**

```bash
# If installed (Options A or B — binary is in PATH):
claude mcp add pagerunner "$(which pagerunner)" mcp

# If built locally (Option C):
claude mcp add pagerunner "$(pwd)/target/release/pagerunner" mcp
```

For Claude Desktop, add to `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "pagerunner": {
      "command": "/absolute/path/to/pagerunner",
      "args": ["mcp"]
    }
  }
}
```

> **Note:** The `mcp` subcommand is required — registering just the binary path without `mcp` will not work.
>
> **Note:** Chrome locks profile directories. Close any Chrome window using a profile before opening a Pagerunner session on it.

**4. (Optional, `--features ner` builds only) Download the NER model**

```bash
pagerunner download-model   # ~65MB, enables PERSON/ORG name detection
```

## Using with your project

Copy the example agent instructions into your project root so Claude (or any agent) immediately knows how to use Pagerunner:

```bash
# Claude Code projects:
curl -sL https://raw.githubusercontent.com/Enreign/pagerunner/main/docs/examples/CLAUDE.md -o CLAUDE.md

# Other agents (Gemini, Codex, etc.):
curl -sL https://raw.githubusercontent.com/Enreign/pagerunner/main/docs/examples/AGENTS.md -o AGENTS.md
```

Or paste the content manually and extend it with project-specific instructions.

## Tools

### Sessions & tabs

| Tool | Description |
|---|---|
| `list_profiles` | List configured Chrome profiles |
| `open_session` | Launch Chrome for a profile, returns `session_id` |
| `close_session` | Kill a Chrome session |
| `list_sessions` | List active sessions |
| `list_tabs` | List open tabs in a session |
| `new_tab` | Open a new tab, returns `target_id` |

### Navigation & content

| Tool | Description |
|---|---|
| `navigate` | Navigate a tab to a URL |
| `wait_for` | Wait for a CSS selector, URL pattern, or fixed delay |
| `get_content` | Get visible text content of a tab |
| `screenshot` | Capture a tab as PNG (saved to temp file or inline base64) |
| `evaluate` | Run JavaScript in a tab, returns the result |

### Interactions

| Tool | Description |
|---|---|
| `click` | Click an element by CSS selector |
| `type_text` | Type text at the focused element or a given selector |
| `fill` | Set input value with React/Vue/Angular synthetic events |
| `select` | Choose a dropdown option by value |
| `scroll` | Scroll the page by pixels or scroll an element into view |

### Snapshots & tab state

| Tool | Description |
|---|---|
| `save_snapshot` | Save current page cookies/localStorage to the encrypted DB |
| `restore_snapshot` | Restore a saved snapshot into the current tab |
| `list_snapshots` | List saved snapshots (optionally across all profiles) |
| `delete_snapshot` | Delete a saved snapshot |
| `save_tab_state` | Save all open tab URLs to the DB |
| `restore_tab_state` | Reopen the previously saved tabs |

### Key-value store

| Tool | Description |
|---|---|
| `kv_set` | Store a value in a persistent namespace |
| `kv_get` | Retrieve a value by key |
| `kv_delete` | Delete a key |
| `kv_list` | List keys in a namespace (with optional prefix filter) |
| `kv_clear` | Delete all keys in a namespace |

## Anonymization (PII protection)

Pass `anonymize: true` to `open_session` to strip PII from all `get_content` and `evaluate`
results before they reach Claude. Screenshots are blocked in anonymization mode.

```json
{ "tool": "open_session", "profile": "personal", "anonymize": true }
```

**Detected by default:** EMAIL, PHONE, CREDIT_CARD, IBAN, SSN, IP
**With `--features ner` build:** also detects PERSON and ORG names via a local ONNX model

### Modes

- **tokenize** (default): replaces PII with tokens like `[EMAIL:a3f9b2]`. Pass tokens back to
  `fill`/`type_text` — Pagerunner de-tokenizes before writing to the DOM.
- **redact**: one-way replacement with `[EMAIL]` — no vault, no de-tokenization.

### NER setup (PERSON and ORG name detection)

```bash
# 1. Build with NER support:
cargo build --release --features ner

# 2. Download the model (one-time, ~65MB):
pagerunner download-model
```

Disable globally in `~/.pagerunner/config.toml`:

```toml
[ner]
enabled = false
```

## Stealth mode

Pass `stealth: true` to `open_session` to enable anti-detection: hides automation signals, injects scripts to mask `navigator.webdriver`, and adds human-like delays between inputs.

```json
{ "tool": "open_session", "profile": "personal", "stealth": true }
```

## Example session

```
open_session(profile="personal")          → session_id: abc123
new_tab(session_id, url="https://...")    → target_id: TAB456
get_content(session_id, target_id)        → "page text..."
click(session_id, target_id, "#button")
type_text(session_id, target_id, "hello", selector="#input")
screenshot(session_id, target_id)         → "data:image/png;base64,..."
close_session(session_id)
```

## Daemon (multiple Claude Code sessions / persistent state)

By default each `pagerunner mcp` process opens the database directly — only one can run at a time. If you open Pagerunner in a second Claude Code window you'll hit a DB lock error.

Start the daemon once to share state across any number of sessions:

```bash
pagerunner daemon &
```

Each `pagerunner mcp` instance automatically detects the daemon, connects to it over a Unix socket (`~/.pagerunner/daemon.sock`), and proxies all tool calls through it. All Claude Code windows share the same open browsers, KV store, and snapshots.

To stop:

```bash
pkill -f "pagerunner daemon"
```

**Install as a background service** (starts at login, restarts on crash):

```bash
cargo build --release
./scripts/install-launchd.sh
```

Logs at `~/.pagerunner/daemon.log`.

## Snapshots

Save authenticated browser state to the encrypted local DB:

```
save_snapshot(session_id, target_id, origin="https://github.com")
```

Restore it in any future session:

```
restore_snapshot(session_id, target_id, origin="https://github.com")
```

Snapshots are encrypted with AES-256-GCM. The key is stored in macOS Keychain under `pagerunner / db_key` — never written to disk in plaintext.

## Legal & Responsible Use

Pagerunner automates a browser you own, using your own credentials. You are responsible for
complying with the Terms of Service of any website you automate, applicable data protection
laws (GDPR, CCPA), and authorized-access requirements (CFAA and equivalents).

Using `anonymize: true` is recommended for any workflow that involves processing personal
data belonging to other people.

See [DISCLAIMER.md](DISCLAIMER.md) for the full legal disclaimer, limitation of liability,
and responsible use obligations.
