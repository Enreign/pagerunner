# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅ Yes    |
| < 0.1.0 | ❌ No     |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Report vulnerabilities via GitHub's private advisory system:
[https://github.com/Enreign/pagerunner/security/advisories/new](https://github.com/Enreign/pagerunner/security/advisories/new)

You'll receive a response within 5 business days. We follow a **90-day responsible disclosure** timeline — we'll work with you to fix and publicly disclose the issue within that window.

## Credential Hygiene

Pagerunner connects to real Chrome profiles that contain your cookies, saved passwords, and browsing history. To limit exposure:

- **Use a dedicated Chrome profile for automation** rather than your primary profile containing sensitive accounts (banking, email, work SSO)
- Create a Chrome profile used exclusively for automation tasks
- Do not store credentials for sensitive services in the profile Pagerunner uses
- Use `pagerunner init` to configure which profiles are available — only list profiles you intend to automate

## Out of Scope

The following are **not** in scope for this security policy:

- Vulnerabilities in Chrome itself (report to [Google](https://bughunters.google.com/report))
- Vulnerabilities in websites being automated
- Third-party dependencies with no actively exploitable path in Pagerunner's threat model
- Issues only exploitable by an attacker with local machine access (Pagerunner is a local tool)

## Security Model

For a detailed description of what Pagerunner protects against and its known limitations, see [docs/security.md](docs/security.md).
