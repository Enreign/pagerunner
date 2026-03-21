# Contributing to Pagerunner

Thank you for your interest in contributing! Here's what you need to know.

## Getting Started

```bash
git clone https://github.com/Enreign/pagerunner
cd pagerunner
cargo build
```

## Before Submitting a PR

All three of these must pass:

```bash
cargo fmt --check          # formatting
cargo clippy -- -D warnings  # lints
cargo test                 # unit + CLI tests (no Chrome required)
```

Fix any failures before opening a PR. The CI will run the same checks.

## Pull Request Guidelines

- Keep PRs small and focused — one feature or fix per PR
- Every new feature needs tests; every bug fix needs a regression test
- Update `CHANGELOG.md` under `[Unreleased]` if your change is user-visible
- For larger changes, open an issue first to discuss the approach

## Reporting Bugs

[Open a bug report](https://github.com/Enreign/pagerunner/issues/new?template=bug_report.md) with:
- Steps to reproduce
- Expected vs. actual behavior
- OS, Chrome version, Pagerunner version (`pagerunner --version`)

## Feature Requests

[Open a feature request](https://github.com/Enreign/pagerunner/issues/new?template=feature_request.md) before writing any code — this avoids wasted effort if the feature doesn't fit the project's direction.

## Security Issues

Do **not** open a public issue for security vulnerabilities. See [SECURITY.md](SECURITY.md).
