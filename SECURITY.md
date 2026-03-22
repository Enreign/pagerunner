# Security Policy

## Overview

Pagerunner is a browser automation MCP server that controls real Chrome instances on your system. This document outlines security features, best practices, and how to report security issues.

## Supported Versions

| Version | Status | Security Updates |
|---------|--------|-----------------|
| 0.1.x | Current | Yes |
| <0.1.0 | EOL | No |

## Security Features

### Built-in Protections

**SSRF Protection**
- Blocks navigation to private IP ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 127.0.0.0/8)
- Blocks `file://`, `javascript:`, and URLs with embedded credentials
- Enforced at both validation layer and via CDP `Network.setBlockedURLs`

**Prompt Injection Mitigation**
- Content marked with `<<<UNTRUSTED_WEB_CONTENT>>>` markers
- Hidden elements stripped from page text
- HTML tags removed
- Zero-width Unicode characters filtered
- NOT a replacement for input validation in your LLM application

**Domain Allowlisting**
- Optional: restrict sessions to explicit allowed domains
- Pass `allowed_domains` to `open_session`
- Example: `["example.com", "*.sub.example.com"]`

**Navigation Budget**
- Optional: limit page loads per session via `max_navigations`
- Prevents runaway automation consuming resources
- Default: unlimited (not recommended for untrusted workloads)

**Audit Logging**
- Append-only JSON-lines log at `~/.pagerunner/audit.log`
- Logs all tool calls, arguments, results, errors, and PII anonymization events
- File mode: 0600 (read/write for owner only on Unix)
- Database copy stored at `~/.pagerunner/state.db` (encrypted)

**PII Anonymization**
- Two modes: `tokenize` (reversible) or `redact` (one-way)
- Detects: EMAIL, PHONE, CREDIT_CARD, IBAN, SSN, IP address
- Named profiles in `config.toml` for per-domain rules
- Optional: NER (Named Entity Recognition) for PERSON/ORG detection (requires `--features ner` build)
- De-tokenization in `fill()` and `type_text()` automatically reverses tokens before writing to DOM

**Encrypted State Database**
- ReDB with AES-256-GCM encryption
- Encryption key stored in macOS Keychain (or environment on Linux)
- Database includes all snapshots, KV store, and audit records

**Snapshot Integrity**
- Stored origin must match requested origin before cookies are injected
- Prevents cross-origin snapshot misuse

## Security Limitations & Constraints

### What Pagerunner Does NOT Protect Against

**1. Malicious Web Content**
Pagerunner does NOT sandbox web content. The page can:
- Execute arbitrary JavaScript in your browser context
- Access your browser's cookies, localStorage, and history
- Access your machine's local files (if Chrome has permission)
- Call any Pagerunner tool via the JS API

**Mitigation**: Only automate pages you trust. Consider:
- Running Pagerunner in a separate Chrome profile with minimal data
- Using `allowed_domains` to restrict navigation
- Monitoring audit logs for suspicious patterns

**2. Malicious LLM Instructions**
If an LLM can call Pagerunner tools, it can:
- Navigate to any URL (within allowed_domains)
- Extract page content and send it to external services
- Fill forms and submit sensitive data
- Click links and buttons

**Mitigation**:
- Use `allowed_domains` to constrain navigation
- Review what an untrusted LLM can do with returned page content
- Consider using `anonymize: true` to strip PII before the LLM sees it
- Monitor audit logs for anomalous tool sequences

**3. Content Inference Attacks**
Even with anonymization enabled, an LLM could infer sensitive information from:
- Page structure and field names
- DOM element counts and hierarchy
- URL patterns and redirects
- Timing of operations

**Mitigation**: Treat returned content as UNTRUSTED. Do not assume anonymization alone is sufficient.

**4. Keylogger-Level Access**
If a malicious script is injected into the page, it can:
- Monitor all keystrokes via `fill()` and `type_text()`
- Intercept data before submission
- Modify form data before Pagerunner processes it

**Mitigation**: Only automate trusted pages. Prefer `fill()` over `type_text()` (less keystroke exposure).

**5. Supply Chain Attacks**
Pagerunner depends on:
- Rust crates (managed via Cargo.lock and `cargo-deny`)
- Chrome/Chromium binary (must be installed separately)
- System libraries

**Mitigation**:
- Use `--locked` flag during builds to ensure reproducible dependencies
- Keep Chrome/Chromium updated
- Monitor GitHub Security Advisories for dependencies
- Use `cargo audit` locally: `cargo audit`

## Security Best Practices for Users

### 1. Profile Isolation
```bash
# Use a dedicated Chrome profile for Pagerunner
pagerunner list-profiles
# or create a new profile in Chrome's user data dir
```

Don't use your personal profile. The browser session will have access to all cookies and stored credentials.

### 2. Domain Allowlisting
```bash
pagerunner open-session default \
  --allowed-domains github.com,gitlab.com,example.com
```

Restricts navigation to whitelisted domains. Reduces risk of unintended external requests.

### 3. PII Anonymization
```bash
pagerunner open-session default --anonymize
```

Automatically strips PII from `get_content()` and `evaluate()` results. Useful when LLMs will process the content.

Named profiles allow per-domain rules:
```toml
[[anonymization.profiles]]
name = "financial"
domains = ["banking.example.com"]
entities = ["EMAIL", "CREDIT_CARD", "PERSON"]
mode = "tokenize"
```

### 4. Audit Log Monitoring
```bash
# Real-time monitoring
tail -f ~/.pagerunner/audit.log | jq .

# Search for specific patterns
grep '"tool":"evaluate"' ~/.pagerunner/audit.log | jq '.result'

# Check for anonymization events
grep 'ContentAnonymized' ~/.pagerunner/audit.log
```

### 5. Navigation Budget
```bash
pagerunner open-session default --max-navigations 10
```

Prevents runaway automation (e.g., infinite redirect loops).

### 6. Stealth Mode (Detection Evasion)
```bash
pagerunner open-session default --stealth
```

Masks `navigator.webdriver`, adds human-like input delays, hides other automation signals.

**Use only for legitimate testing** (e.g., testing your own site's anti-bot measures). Avoid using to bypass security controls on third-party services.

## Reporting Security Issues

### Private Disclosure

If you discover a security vulnerability in Pagerunner, please **do not** open a public GitHub issue.

Instead, email: **security@enreign.io** with:
- Description of the vulnerability
- Steps to reproduce
- Potential impact (CVSS score if known)
- Suggested fix (if available)

Include:
- Pagerunner version (`pagerunner --version`)
- OS and Chrome version
- Full error messages and logs

**Response Timeline**:
- Acknowledgment within 24 hours
- Status update within 7 days
- Public disclosure after fix is released

### Public Disclosure

Once a fix is released, the vulnerability will be:
1. Described in the GitHub release notes
2. Added to `CHANGELOG.md` with `[SECURITY]` label
3. Listed on this page under "Resolved Vulnerabilities"

## Known Vulnerabilities

None currently. All reported issues are tracked in [GitHub Security Advisories](https://github.com/Enreign/pagerunner/security/advisories).

## Resolved Vulnerabilities

See [CHANGELOG.md](CHANGELOG.md) for past security fixes.

## Dependencies & Supply Chain

### Cargo Dependencies
Managed via `Cargo.lock` (committed to repo) and verified with `cargo-deny`:
```bash
cargo deny check advisories
```

### Chrome/Chromium
Pagerunner does NOT bundle Chrome. You must install separately:
- macOS: `brew install chromium` or use your existing Chrome profile
- Linux: `apt install chromium-browser` or equivalent
- Windows: Download from chromium.org (Windows support TBD)

Keep Chrome updated independently via your system's package manager.

### Reproducible Builds
```bash
# Ensures all dependencies match locked versions
cargo build --release --locked
```

## Security Checklist for Deployment

- [ ] Using a dedicated Chrome profile (not personal)
- [ ] Have `~/.pagerunner/config.toml` reviewed for correct profile paths
- [ ] Database encryption key stored securely (Keychain on macOS, env var on Linux)
- [ ] Audit logging enabled (default)
- [ ] If using LLM integration: anonymization enabled with appropriate rules
- [ ] If untrusted LLM: domain allowlisting configured
- [ ] If sensitive workflows: navigation budget set
- [ ] Regular audit log review: `tail -f ~/.pagerunner/audit.log | jq .`
- [ ] Cargo.lock committed and verified with `cargo-deny check advisories`
- [ ] Chrome/Chromium kept up-to-date

## Security Incident Response

If you suspect Pagerunner has been compromised:

1. **Immediate**: Stop all active sessions
   ```bash
   pkill -f "pagerunner mcp"
   pkill -f "pagerunner daemon"
   ```

2. **Investigate**: Review audit logs for anomalies
   ```bash
   pagerunner audit --tail 1000 | jq '.[] | select(.result | contains("error") | not)'
   ```

3. **Report**: Email security@enreign.io with logs and timeline

4. **Recover**:
   - Rotate any credentials entered via `fill()` or `type_text()` during incident window
   - Delete compromised snapshots: `pagerunner delete-snapshot <profile> <origin>`
   - Clear KV store if used for secrets: `pagerunner kv-clear <namespace>`

## Security Research & Testing

Pagerunner is designed for legitimate browser automation. Security researchers interested in:
- Penetration testing (with authorization)
- Vulnerability discovery
- Performance/stress testing

Should **coordinate with the maintainers before testing**. This prevents false positives and ensures findings are addressed responsibly.

## Contact

- **Security Issue**: security@enreign.io
- **General Questions**: See [README.md](README.md) or GitHub Issues
- **Bug Reports**: [GitHub Issues](https://github.com/Enreign/pagerunner/issues)

---

**Last Updated**: 2026-03-22
**Policy Version**: 1.0
