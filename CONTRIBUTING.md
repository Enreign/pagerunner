# Contributing to Pagerunner

Thank you for your interest in contributing to Pagerunner! This document provides guidelines for contributing code, reporting issues, and improving documentation.

## Code of Conduct

Be respectful, inclusive, and professional. We're building a tool for everyone.

## How to Contribute

### 1. Report a Bug

Found a bug? Open a [GitHub Issue](https://github.com/Enreign/pagerunner/issues) with:
- **Title**: Clear, concise description of the bug
- **Description**: 
  - What you expected to happen
  - What actually happened
  - Steps to reproduce
  - Your environment (OS, Chrome version, Pagerunner version)
- **Logs**: Relevant output from `~/.pagerunner/audit.log` or CLI stderr

Example:
```
Title: screenshot hangs when page has infinite animation

Steps to reproduce:
1. pagerunner open-session default
2. pagerunner navigate $SID $TID https://example.com/infinite-spinner
3. pagerunner screenshot $SID $TID (hangs indefinitely)

Environment: macOS 14.2, Chrome 123.0.6312.59, Pagerunner 0.1.1
```

### 2. Report a Security Issue

Do **not** open a public GitHub issue for security vulnerabilities. Instead, email **security@enreign.io**. See [SECURITY.md](SECURITY.md) for details.

### 3. Suggest a Feature

Open a [GitHub Issue](https://github.com/Enreign/pagerunner/issues) with:
- **Title**: Brief feature description
- **Use case**: Why is this feature needed? What problem does it solve?
- **Example**: How would the tool be used?
- **Alternatives**: Other ways to solve this problem

Example:
```
Title: Add support for multiple selector matching

Use case: Need to click one of several possible selectors (for pages with A/B tests)

Example:
pagerunner click-any $SID $TID ".submit-btn" ".action-button" ".proceed"

Would try selectors in order and click the first one that exists.
```

### 4. Submit Code

#### Prerequisites
- Rust 1.91+
- Chrome or Chromium installed locally
- macOS Keychain (macOS) or env vars (Linux) for encrypted database

#### Setup
```bash
git clone https://github.com/Enreign/pagerunner.git
cd pagerunner
cargo build --release
cargo test                           # Run all tests
cargo test --test cli_tools_integration  # Run CLI integration tests only
```

#### Branch Naming
Use descriptive names:
- `feat/add-feature-name` — New feature
- `fix/issue-description` — Bug fix
- `docs/what-changed` — Documentation
- `refactor/what-changed` — Code cleanup (no behavior change)

#### Coding Standards

**Rust Style**
- Follow standard Rust conventions (enforced by `cargo fmt`)
- Run `cargo fmt` before committing
- Address `cargo clippy` warnings
- Aim for zero unsafe code (justified comments if necessary)

**Code Organization**
- `src/main.rs` — CLI entry point
- `src/mcp_server.rs` — MCP protocol dispatch and session lifecycle
- `src/cli_tools.rs` — Tool implementations
- `src/audit.rs` — Audit logging
- `src/security.rs` — Security policies and blocking rules
- Tests inline in each module, integration tests in `tests/`

**Naming**
- Functions: `snake_case`
- Types: `PascalCase`
- Constants: `SCREAMING_SNAKE_CASE`
- Private items: prefix with underscore if unused

**Comments**
- Add comments for non-obvious logic
- Use `// ` for single lines, `/* */` for blocks
- Avoid redundant comments ("x = x + 1 // increment x")

**Error Handling**
- Return `Result<T>` for fallible operations
- Use `anyhow::Result<T>` for non-critical errors
- Use `thiserror::Error` for domain-specific errors
- Always provide context: `err.context("what was happening")?`

#### Testing

**Write Tests First**
1. Write test case with expected behavior
2. Run `cargo test` (test will fail)
3. Implement feature/fix
4. Test passes

**Test Location**
- Unit tests: inline in module with `#[cfg(test)] mod tests { ... }`
- Integration tests: `tests/cli_tools_integration.rs` for CLI tests
- Live tests: use `#[cfg_attr(not(target_os = "macos"), ignore)]` for Chrome-dependent tests

**Test Examples**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_happy_path() {
        // Arrange
        let input = ...;
        
        // Act
        let result = ...;
        
        // Assert
        assert_eq!(result, expected);
    }

    #[test]
    fn test_error_case() {
        let input = ...;
        assert!(matches!(some_function(input), Err(_)));
    }

    // Chrome live test (runs on macOS, skipped on Linux CI)
    #[cfg_attr(not(target_os = "macos"), ignore)]
    #[tokio::test]
    async fn test_cli_navigate() {
        let session_id = open_temp_session().await;
        let tabs = list_tabs(session_id).await;
        assert!(!tabs.is_empty());
    }
}
```

**Running Tests**
```bash
cargo test                                      # All tests
cargo test --lib                               # Unit tests only
cargo test --test cli_tools_integration        # CLI integration tests
cargo test --test cli_tools_integration test_cli_navigate    # Specific test
cargo test -- --ignored                        # Only ignored tests
cargo test -- --nocapture                      # Show println! output
```

**Coverage**
- Aim for >80% line coverage on new code
- Critical paths (security, state management) should have near 100% coverage
- Use `cargo tarpaulin --out Html` to generate coverage reports (optional)

#### Documentation

**Code Comments**
```rust
/// Navigates a tab to a URL with SSRF protection.
///
/// # Arguments
/// * `session_id` - Session identifier
/// * `target_id` - Tab target ID (from list_tabs)
/// * `url` - URL to navigate to
///
/// # Errors
/// Returns an error if:
/// - Session or tab does not exist
/// - URL is invalid or blocked (private IP, file://, javascript:)
/// - Navigation times out
///
/// # Example
/// ```
/// let result = navigate(session_id, target_id, "https://example.com").await;
/// ```
pub async fn navigate(...) -> Result<String> { ... }
```

**README.md Changes**
If adding a new feature, update [README.md](README.md):
- Add to feature list with brief description
- Add CLI example in subcommands section
- Update MCP tools list if adding new tools

**CHANGELOG.md**
Add entry under `[Unreleased]` section:
```markdown
### Added
- New feature description

### Fixed
- Bug description

### Changed
- Behavior change description
```

#### Committing Code

**Commit Messages**
Follow conventional commits:
```
type(scope): subject

body

footer
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`
Scope: module name or feature area (optional)
Subject: < 50 characters, imperative mood
Body: < 72 characters per line, explain *why* not *what*

Examples:
```
feat(mcp): add tool response metadata for hallucination prevention

Tools now return semantic metadata (_warning, _hint, etc.) in a second
content block to prevent LLM hallucinations on ambiguous data.

Fixes #42

---

fix(audit): use correct timestamp format in JSON logs

The ISO8601 format was missing the 'T' separator. Changed from
"2026-03-22 12:34:56" to "2026-03-22T12:34:56Z" to match the JSON schema.

---

docs: clarify anonymization token format

Added examples showing how [EMAIL:a3f9b2] tokens are generated
and how to use them in fill() and type_text() calls.
```

**Before Pushing**
```bash
cargo fmt          # Format code
cargo clippy       # Check warnings
cargo test         # Run all tests
git status         # Review changes
git diff           # Check diffs
```

#### Pull Request Process

1. **Fork** the repository
2. **Create a branch** from `main`: `git checkout -b feat/my-feature`
3. **Make changes** and commit with clear messages
4. **Push** your branch: `git push origin feat/my-feature`
5. **Open a PR** with:
   - Title: Clear description of changes
   - Description: Why this change? What problem does it solve?
   - Testing: What was tested? How can reviewers verify?
   - Related Issues: Closes #123, Relates to #456

PR Template:
```markdown
## Description
Brief explanation of the change.

## Problem
What issue or use case does this address?

## Solution
How does this PR solve the problem?

## Testing
- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Tested locally on macOS / Linux
- [ ] Cargo test, fmt, clippy all pass
- [ ] Audit log reviewed for expected events

## Screenshots / Examples
If applicable, include before/after or examples.

## Checklist
- [ ] Tests pass
- [ ] Documentation updated
- [ ] No breaking changes (or clearly documented)
- [ ] Commits follow conventional commit format
```

#### Review Process

**What reviewers look for:**
- Correctness and test coverage
- Security implications (input validation, SSRF, prompt injection)
- Performance impact (database queries, network calls)
- Error handling and user feedback
- Code style and readability
- Documentation clarity

**How to respond to feedback:**
- Ask clarifying questions if feedback is unclear
- Discuss tradeoffs and alternatives
- Update code and push new commits (don't force-push once reviewed)
- Mark conversations as resolved after addressing

#### Merging

Maintainers will merge PRs after:
- ✅ All CI checks pass (fmt, clippy, tests)
- ✅ At least one approval from maintainer
- ✅ Zero conflicts with `main`
- ✅ Commit history is clean and follows conventions

Use **squash and merge** for feature branches (keeps history clean), **rebase and merge** for single commits.

## Development Setup

### Local Build
```bash
cargo build --release
./target/release/pagerunner --version
```

### Running MCP Server Locally
```bash
cargo build --release
./target/release/pagerunner mcp
# In another terminal, connect via Claude Code: /mcp /path/to/target/release/pagerunner mcp
```

### Testing with Real Browser
```bash
# Set up test profile
./target/release/pagerunner init

# List available profiles
./target/release/pagerunner list-profiles

# Open a session
SESSION_ID=$(...open-session default... | jq -r .session_id)
TARGET_ID=$(...list-tabs $SESSION_ID... | jq -r '.[0].target_id')

# Run commands
./target/release/pagerunner navigate $SESSION_ID $TARGET_ID https://example.com
./target/release/pagerunner get-content $SESSION_ID $TARGET_ID
./target/release/pagerunner close-session $SESSION_ID
```

### Debugging

**Enable tracing**
```bash
RUST_LOG=pagerunner=debug cargo test test_name -- --nocapture
```

**View audit log**
```bash
tail -f ~/.pagerunner/audit.log | jq .
```

**Attach to running MCP server**
```bash
# If MCP is already running via /mcp, see all tool calls in real-time
tail -f ~/.pagerunner/audit.log | jq 'select(.event_type == "ToolCalled")'
```

## Project Structure

```
pagerunner/
├── src/
│   ├── main.rs               # CLI entry, 27 subcommands
│   ├── mcp_server.rs         # MCP protocol, dispatch_tool, sessions
│   ├── cli_tools.rs          # Tool implementations
│   ├── audit.rs              # AuditLog, AuditEvent
│   ├── security.rs           # SecurityPolicy, blocking rules
│   ├── anonymization.rs      # PII detection and anonymization
│   ├── ipc.rs                # Daemon socket communication
│   └── lib.rs                # Module exports
├── tests/
│   └── cli_tools_integration.rs    # CLI and Chrome live tests
├── docs/
│   ├── test-plans/           # Test case documentation
│   └── test-runs/            # Test execution records
├── Cargo.toml
├── Cargo.lock                # Lock file (committed)
├── README.md
├── CHANGELOG.md
├── SECURITY.md
├── CLAUDE.md                 # Project instructions
└── .gitignore
```

## Maintainance Notes

**Release Process**
1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md` with changes
3. Tag release: `git tag v0.1.x`
4. Push: `git push origin main --tags`
5. GitHub Actions builds and publishes binaries automatically

**Support Timeline**
- Current version: Full support
- Previous major version: Bug fixes only
- Older versions: No support

## Getting Help

- **How do I...?** → Check [README.md](README.md) or [CLAUDE.md](CLAUDE.md)
- **Found a bug?** → [GitHub Issues](https://github.com/Enreign/pagerunner/issues)
- **Security issue?** → [SECURITY.md](SECURITY.md)
- **Code review?** → Open a PR, maintainers will review
- **Feature request?** → [GitHub Discussions](https://github.com/Enreign/pagerunner/discussions)

## License

By contributing to Pagerunner, you agree that your contributions will be licensed under the MIT License.

---

Happy coding! 🚀
