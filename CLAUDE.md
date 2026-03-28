# pagerunner — Claude Code Notes

## Project
Rust MCP server that drives Chrome via CDP for AI agents. Serves Claude Code via stdio transport.

## Build & Test
```bash
cargo build --release          # build release binary
cargo build                    # debug build (use this to verify compilation)
cargo test <module_name>        # run tests for a specific module (e.g. cargo test site_knowledge)
cargo test                     # run ALL tests — opens Chrome windows on macOS, use sparingly
cargo test --test cli_tools_integration   # run CLI integration tests only (also opens Chrome on macOS)
```

### ⚠️ When to run `cargo test` (bare, no filter)
**Do NOT run bare `cargo test` as a routine build/verify step on macOS.**
On macOS, `#[cfg_attr(not(target_os = "macos"), ignore)]` tests are NOT skipped — they run and open real Chrome browser windows.

Use these instead:
- **Compilation check:** `cargo build`
- **Unit tests for a module:** `cargo test <module_name>` (e.g. `cargo test audit`, `cargo test site_knowledge`)
- **All unit tests without Chrome:** `cargo test --bin pagerunner` won't work (binary target); use module-level filters
- **Full test suite including Chrome:** only run bare `cargo test` when explicitly testing Chrome integration, and warn the user first

Last known test counts: 256 unit + 50 CLI integration (macOS: 49 pass + 1 ignored; Linux CI: 24 pass + 25 cfg_attr-ignored + 1 ignored)

## Key Files
- `src/mcp_server.rs` — main dispatch, session lifecycle, audit recording, `call_tool`
- `src/cli_tools.rs` — CLI tool runner, screenshot output handling
- `src/audit.rs` — AuditLog, AuditEvent types, build_args_summary
- `src/security.rs` — SecurityPolicy, PolicySummary
- `src/main.rs` — CLI entry (27 subcommands + mcp, daemon, audit)
- `~/.pagerunner/config.toml` — profile config (Chrome user data dirs)
- `~/.pagerunner/state.db` — encrypted ReDB (sessions, KV, snapshots, audit)
- `~/.pagerunner/audit.log` — append-only JSON-lines audit log (0600)

## MCP Server
The binary requires the `mcp` subcommand to run as MCP server:
```
/path/to/pagerunner mcp
```
The `claude mcp add` command must include this subcommand — just the binary path is not enough.

## Running MCP Locally
After `cargo build --release`, kill any stale pagerunner process before reconnecting:
```bash
pkill -f "pagerunner mcp"
```
Then use `/mcp` in Claude Code to reconnect.

## Multi-Session (5 Claude Code windows)
By default each `pagerunner mcp` process opens the DB directly — only one can run at a time.
To share pagerunner across multiple Claude Code sessions, start the daemon first:
```bash
pagerunner daemon &     # holds the single DB lock, listens on ~/.pagerunner/daemon.sock
```
Each `pagerunner mcp` instance will then detect the daemon, connect to it, and proxy all tool calls through it. All sessions share the same state (open browsers, KV store, snapshots).

To stop the daemon:
```bash
pkill -f "pagerunner daemon"
```

## Profile Config
Real Chrome profiles are configured in `~/.pagerunner/config.toml`. Example:
```toml
[[profiles]]
name = "personal"
display_name = "User (user@example.com)"
user_data_dir = "/Users/user/Library/Application Support/Google/Chrome/Default"
```
**Note:** Chrome locks profile directories — close any Chrome window using the profile before opening a pagerunner session on it.

## Audit CLI
```bash
pagerunner audit --tail 50
pagerunner audit --session <id>
pagerunner audit --since 2026-03-20T14:00:00Z
```
The audit DB is locked while the MCP server is running — use `audit.log` for live inspection:
```bash
tail -f ~/.pagerunner/audit.log | jq .
```

## Anonymization

Pass `anonymize: true` to `open_session` to enable PII anonymization. All `get_content` and `evaluate` results will have PII stripped before reaching Claude. Screenshots are blocked in anonymization mode.

### open_session params

```json
{ "profile": "personal", "anonymize": true }
```

Three forms:
- **Default** (`anonymize: true` only): tokenize mode, detects EMAIL, PHONE, CREDIT_CARD, IBAN, SSN, IP
- **Named profile** (`anonymization_profile: "jira-work"`): uses profile from `config.toml`
- **Inline** (`anonymization_entities: ["EMAIL","PHONE"]`, `anonymization_mode: "tokenize"|"redact"`): overrides for this session

Named profile and inline params are mutually exclusive.

### Modes

- **tokenize**: Replaces PII with tokens like `[EMAIL:a3f9b2]`. Tokens are stored in an encrypted session vault. Pass tokens back to `fill`/`type_text` — pagerunner de-tokenizes before writing to DOM.
- **redact**: Replaces PII with `[EMAIL]` (no vault, one-way). `fill`/`type_text` with token-shaped values will error.

### Domain profiles (config.toml)

```toml
[[anonymization.profiles]]
name = "jira-work"
domains = ["jira.acme.com", "*.atlassian.net"]
mode = "tokenize"
entities = ["EMAIL", "PHONE", "CREDIT_CARD"]
custom_patterns = [
  { name = "JIRA_CODE", pattern = "(?:PROJ|INFRA)-\\d+" },
]
```

### NER (PERSON/ORG detection)

Requires `--features ner` build + model download:

```bash
cargo build --release --features ner
pagerunner download-model
```

When compiled with `--features ner`, `anonymize: true` defaults also include `PERSON` and `ORG`.
Disable globally with `[ner] enabled = false` in `config.toml`.

### Audit log

`ContentAnonymized` events record entity type counts only — no values, no tokens.

## CLI Subcommands

All 27 MCP tools are exposed as direct CLI subcommands — no MCP registration required:

```bash
pagerunner list-profiles
pagerunner open-session <profile> [--stealth] [--anonymize] [--allowed-domains d1,d2]
pagerunner close-session <session-id>
pagerunner list-sessions
pagerunner list-tabs <session-id>
pagerunner new-tab <session-id> [--url <url>]
pagerunner navigate <session-id> <target-id> <url>
pagerunner wait-for <session-id> <target-id> [--selector <sel>] [--url <pat>] [--ms <n>]
pagerunner get-content <session-id> <target-id>
pagerunner screenshot <session-id> <target-id> [--base64]
pagerunner evaluate <session-id> <target-id> <expression>
pagerunner click <session-id> <target-id> <selector>
pagerunner type-text <session-id> <target-id> <text> [--selector <sel>]
pagerunner fill <session-id> <target-id> <selector> <value>
pagerunner select <session-id> <target-id> <selector> <value>
pagerunner scroll <session-id> <target-id> [--selector <sel>] [--x <n>] [--y <n>]
pagerunner save-snapshot <session-id> <target-id> [--origin <url>]
pagerunner restore-snapshot <session-id> <target-id> <origin> [--from-profile <name>]
pagerunner list-snapshots [--profile <name>] [--all]
pagerunner delete-snapshot <profile> <origin> [--saved-at <unix-us>]
pagerunner save-tab-state <session-id>
pagerunner restore-tab-state <session-id>
pagerunner kv-set <namespace> <key> <value>
pagerunner kv-get <namespace> <key>
pagerunner kv-delete <namespace> <key>
pagerunner kv-list <namespace> [--prefix <pfx>] [--keys-only]
pagerunner kv-clear <namespace>
```

All commands output JSON to stdout. Errors go to stderr with exit 1.
`screenshot` saves a PNG to a temp file by default; `--base64` returns inline JSON.

CLI calls try the daemon socket first (`~/.pagerunner/daemon.sock`), then fall back to opening the DB directly. If a live MCP server is running standalone, start the daemon mode first to avoid DB lock conflicts.

## Known Issues
None currently. On CI (Linux): 262 tests pass (239 unit + 23 non-Chrome CLI), 23 skipped. On macOS locally: 283 pass (239 unit + 44 Chrome CLI), 1 NER test skipped (requires `--features ner` build + model). NER live tests pass with model at `~/.pagerunner/models/ner.onnx`.
None currently. On CI (Linux): 255 tests pass (232 unit + 23 non-Chrome CLI), 23 skipped. On macOS locally: 276 pass (232 unit + 44 Chrome CLI), 1 NER test skipped (requires `--features ner` build + model). NER live tests pass with model at `~/.pagerunner/models/ner.onnx`.
None currently. On CI (Linux): 255 tests pass (232 unit + 23 non-Chrome CLI), 25 skipped. On macOS locally: 278 pass (232 unit + 46 Chrome CLI), 1 NER test skipped (requires `--features ner` build + model). NER live tests pass with model at `~/.pagerunner/models/ner.onnx`.
None currently. On CI (Linux): 280 tests pass (256 unit + 24 non-Chrome CLI), 26 skipped. On macOS locally: 306 pass (256 unit + 50 CLI integration), 1 NER test skipped (requires `--features ner` build + model). NER live tests pass with model at `~/.pagerunner/models/ner.onnx`.

## Testing

### Rules
- **Never add `#[ignore]`** to unit tests. Two exceptions for CLI integration tests:
  - **Chrome live tests** use `#[cfg_attr(not(target_os = "macos"), ignore)]` — they run automatically on macOS (local) but are skipped on Linux (CI), because CI has no Chrome profile.
  - **NER test** (`test_cli_ner_anonymize_person_masked`) is `#[ignore]` unconditionally — it requires `cargo build --release --features ner` + the 431 MB model. Run with `cargo test --test cli_tools_integration test_cli_ner_anonymize_person_masked -- --ignored`.
- **Tests first**: write or update tests before writing implementation code.
- **Never skip live tests**: when a feature has a live (Chrome/network) path, that path must have a test in `tests/cli_tools_integration.rs`.

### Test Plan
The master test plan lives at [`docs/test-plans/master-test-plan.md`](docs/test-plans/master-test-plan.md). It covers all major surfaces: session management, navigation, content, interactions, KV, snapshots, security, prompt injection, anonymization Phase 1 (regex), and anonymization Phase 2 (NER).

**Maintenance rule:** After any medium or large change — new feature, refactor touching >1 module, or security-relevant fix — verify the test plan is still accurate and that automated coverage (`✅`) reflects reality. Record a dated execution in `docs/test-runs/` if manual (`🖐`) tests were run.

**Tests-first rule:** Before implementing any new feature or fixing a bug, write the tests first. All planned test cases (including `#[ignore]`-tagged live tests) must exist before writing implementation code. Never mark a test as skipped/deferred as a substitute for actually running it — if a test requires external resources (model file, live browser), tag it `#[ignore]` and document the requirement.

**Never skip live tests:** All `🖐` (manual) tests in the test plan must be run after implementation and results recorded in a test run doc. Do not leave live test cases as `🖐` after implementation is complete — run them, mark them `✅`, and update the test run.

Test runs are saved in `docs/test-runs/`. Run naming: `YYYY-MM-DD-run-N.md`.

### CLI Integration Tests (`tests/cli_tools_integration.rs`)

26 non-Chrome tests cover subcommands without a live browser:
24 non-Chrome tests cover subcommands without a live browser:
- `list-profiles`, `list-sessions`, `list-snapshots` — happy-path output shape
- KV store — full lifecycle (set, get, list, prefix filter, keys-only, delete, clear)
- `init --json` — flag acceptance, CLAUDE.md snippet return, AGENTS.md snippet return
- Error cases — all session-requiring subcommands return non-zero for invalid sessions
- Help text — `screenshot --base64`, `open-session --anonymize`, `wait-for` modes
- Network log — `get_network_log` error case (invalid session)

Tests use `PAGERUNNER_DB_PATH=/tmp/pagerunner_integration_test.db` automatically, so they never conflict with a running `pagerunner mcp` process. Session-based Chrome tests spin up a per-test daemon (also using the test DB) so session state persists across separate CLI invocations.

### Last CLI Test Run: 2026-03-26 (`cargo test --test cli_tools_integration`)
| Category | Pass | Notes |
|----------|------|-------|
| Non-Chrome (profiles, sessions, KV, errors, help) | 23/23 | |
| Non-Chrome: init --json | 3/3 | |
| Non-Chrome (profiles, sessions, KV, errors, help) | 24/24 | includes `test_list_sessions_has_status_field` |
| Non-Chrome (profiles, sessions, KV, errors, help) | 24/24 | |
| Chrome: sessions + tabs | 4/4 | |
| Chrome: screenshot, evaluate | 3/3 | |
| Chrome: interactions (click, fill, type, select, scroll) | 8/8 | |
| Chrome: wait-for | 4/4 | +2 new: stability_ms (selector + url) |
| Chrome: anonymization | 2/2 | |
| Chrome: security (allowed-domains) | 1/1 | |
| Chrome: NER CLI | 1/1 | `#[ignore]` — requires `--features ner` + model |
| Chrome: kv-roundtrip, snapshots, tab-state | 3/3 | |
| **Total** | **48/48** | macOS: 47 pass + 1 ignored; Linux CI: 26 pass + 22 cfg_attr-ignored + 1 ignored |
| **Total** | **46/46** | macOS: 45 pass + 1 ignored; Linux CI: 24 pass + 21 cfg_attr-ignored + 1 ignored |
| **Total** | **47/47** | macOS: 46 pass + 1 ignored; Linux CI: 23 pass + 24 cfg_attr-ignored + 1 ignored |
| Chrome: network log (get_network_log) | 3/3 | macOS only |
| **Total** | **50/50** | macOS: 49 pass + 1 ignored; Linux CI: 24 pass + 25 cfg_attr-ignored + 1 ignored |

### Last Full Live Test Run: [2026-03-21-run-6](docs/test-runs/2026-03-21-run-6.md)
| Category | Pass | Notes |
|----------|------|-------|
| Sessions | 5/5 | |
| Tab Management | 3/3 | |
| Navigation | 6/6 | |
| Content | 5/5 | |
| Interactions | 7/7 | `fill` on textarea bug fixed this run |
| KV Store | 7/7 | |
| Snapshots | 4/4 | |
| Tab State | 2/2 | |
| Security | 6/6 | |
| Prompt Injection | 4/4 | |
| Anonymization (live) | 5/5 | |
| NER (unit, no model) | 16/16 | |
| NER (unit+live, with model) | 13/13 | requires `--features ner` + model |
| NER live CLI (NP8, NP9) | 2/2 | get_content + fill with PERSON token |
| **Total** | **65/65** | |
