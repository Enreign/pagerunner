# pagerunner — Claude Code Notes

## Project
Rust MCP server that drives Chrome via CDP for AI agents. Serves Claude Code via stdio transport.

## Build & Test
```bash
cargo build --release          # build release binary
cargo build                    # debug build (use this to verify compilation)
cargo test <module_name>        # run tests for a specific module (e.g. cargo test site_knowledge)
cargo test --test cli_tools_integration   # run CLI integration tests only (also opens Chrome on macOS)
```

**Do NOT run bare `cargo test` on macOS** — it opens real Chrome browser windows. Use module-level filters instead.

Last known test counts: 407 unit + 82 CLI integration (macOS: 81 pass + 1 ignored).

## Key Files
- `src/mcp_server.rs` — main dispatch, session lifecycle, audit recording, `call_tool`
- `src/session.rs` — SessionManager, Chrome spawn (TCP-only), attach, close
- `src/session_registry.rs` — DB persistence for session reattach across daemon restarts
- `src/chrome.rs` — Chrome process spawn, port allocation
- `src/checkpoint.rs` — session checkpoint save/restore
- `src/cli_tools.rs` — CLI tool runner, screenshot output handling
- `src/config.rs` — PagerunnerConfig, CheckpointConfig, RetentionConfig
- `src/main.rs` — CLI entry (38 subcommands + mcp, daemon, audit)
- `~/.pagerunner/config.toml` — profile config, checkpoints, retention
- `~/.pagerunner/state.db` — encrypted ReDB (sessions, KV, snapshots, audit)

## Running Locally

### 1. Start the daemon
```bash
pagerunner daemon &
```
The daemon holds the DB lock, owns Chrome processes, and listens on `~/.pagerunner/daemon.sock`. Multiple Claude Code windows can share it.

### 2. Connect Claude Code
```bash
claude mcp add pagerunner /path/to/pagerunner mcp
```
Each `pagerunner mcp` instance detects the daemon and proxies tool calls through it.

### 3. After rebuilding
```bash
pkill -f "pagerunner mcp"    # kill MCP processes (Claude Code will reconnect)
# Use /mcp in Claude Code to reconnect
```

To restart the daemon itself:
```bash
pkill -f "pagerunner daemon"  # Chrome stays alive (TCP-only transport)
pagerunner daemon &            # reconciliation auto-reattaches surviving Chrome
```

## Session Persistence

Chrome uses TCP-only CDP transport (`--remote-debugging-port` on `127.0.0.1`). Chrome runs independently of the daemon — if the daemon restarts, Chrome stays alive and sessions auto-reattach on startup.

Auto-checkpoints provide a safety net:
- On every `close_session` (named "Autosave · close")
- Periodically (default every 5 min, configurable via `[checkpoints]` in `config.toml`)

### Config
```toml
[checkpoints]
enabled = true
interval_seconds = 300   # auto-checkpoint every 5 minutes

[retention]
max_snapshot_versions = 10  # per-origin snapshot history; 0 = unlimited
site_knowledge_ttl_days = 0  # 0 = never expire
```

## Profile Config
```toml
[[profiles]]
name = "personal"
display_name = "User (user@example.com)"
user_data_dir = "/Users/user/Library/Application Support/Google/Chrome/Default"
```
**Note:** Chrome locks profile directories — close any Chrome window using the profile before opening a pagerunner session on it.

## Anonymization

Pass `anonymize: true` to `open_session` to enable PII anonymization. Screenshots are blocked in anonymization mode.

Three forms:
- **Default** (`anonymize: true`): tokenize mode — EMAIL, PHONE, CREDIT_CARD, IBAN, SSN, IP
- **Named profile** (`anonymization_profile: "jira-work"`): uses profile from `config.toml`
- **Inline** (`anonymization_entities: ["EMAIL","PHONE"]`, `anonymization_mode: "tokenize"|"redact"`)

NER (PERSON/ORG) requires `cargo build --release --features ner` + `pagerunner download-model`.

## CLI Subcommands

All 38 MCP tools are exposed as CLI subcommands. All output JSON to stdout, errors to stderr with exit 1.

```bash
pagerunner list-profiles
pagerunner open-session <profile> [--stealth] [--anonymize] [--allowed-domains d1,d2]
pagerunner attach-session --debug-port 9222 [--profile <label>]
pagerunner close-session <session-id>
pagerunner list-sessions
pagerunner list-tabs <session-id>
pagerunner new-tab <session-id> [--url <url>]
pagerunner close-tab <session-id> <target-id>
pagerunner navigate <session-id> <target-id> <url>
pagerunner get-content <session-id> <target-id>
pagerunner screenshot <session-id> <target-id> [--base64]
pagerunner evaluate <session-id> <target-id> <expression>
pagerunner click <session-id> <target-id> <selector>
pagerunner fill <session-id> <target-id> <selector> <value>
pagerunner save-snapshot <session-id> <target-id> [--origin <url>]
pagerunner restore-snapshot <session-id> <target-id> <origin>
pagerunner kv-set <namespace> <key> <value>
pagerunner kv-get <namespace> <key>
pagerunner kv-list <namespace> [--prefix <pfx>] [--keys-only]
pagerunner get-network-log <session-id> [--target-id <tid>] [--limit <n>]
pagerunner get-site-knowledge <origin>
pagerunner save-session-checkpoint <session-id> [--name <name>]
pagerunner restore-session-checkpoint <session-id> <checkpoint-id>
pagerunner list-session-checkpoints --profile <name>
```

## macOS Menu Bar App

Native Swift companion app at `apps/menubar/`. Communicates with the Rust daemon over Unix socket — no code changes needed for backend updates.

### Build & Run

```bash
# 1. Start the daemon
pagerunner daemon &

# 2. Package the app (required — bare binary crashes without .app bundle)
cd apps/menubar/scripts
./package.sh                   # builds, ad-hoc signs → Pagerunner.app

# 3. Launch
open Pagerunner.app            # appears in menu bar
```

**Important:** Do NOT run `.build/release/PagerunnerBar` directly — it will crash. macOS requires the `.app` bundle for menu bar apps (notifications, proper lifecycle).

### Development cycle
```bash
cd apps/menubar
swift build -c release         # rebuild
cd scripts && ./package.sh     # re-package
open Pagerunner.app            # relaunch
swift test                     # run PagerunnerCoreTests (no Chrome needed)
```

### Architecture
- `Sources/PagerunnerCore/` — zero-UI, fully testable: Models, DaemonClient, PollingService
- `Sources/PagerunnerBar/` — app target: App, AppState, StatusItemController, Views
- `Tests/PagerunnerCoreTests/` — unit tests for Core

### Distribution
```bash
cd apps/menubar/scripts
./package.sh           # .app bundle + .zip
./notarize.sh          # Apple notarization (requires CODE_SIGN_IDENTITY, APPLE_TEAM_ID, APPLE_ID, NOTARIZE_PASSWORD)
```

## Testing

### Rules
- **Never add `#[ignore]`** to unit tests. Exceptions for CLI integration tests:
  - Chrome live tests: `#[cfg_attr(not(target_os = "macos"), ignore)]`
  - NER test: `#[ignore]` (requires `--features ner` + 431 MB model)
- **Tests first**: write or update tests before implementation code.
- **Never skip live tests**: live features must have tests in `tests/cli_tools_integration.rs`.

### Test harness
Tests use `PAGERUNNER_DB_PATH=/tmp/pagerunner_integration_test.db` to avoid conflicts. Chrome tests spin up a per-test daemon. `TestDaemon::drop()` kills orphaned Chrome processes via PID tracking.
