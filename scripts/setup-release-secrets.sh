#!/usr/bin/env bash
# setup-release-secrets.sh — interactively set GitHub secrets needed for the release pipeline.
#
# Tokens are captured with `read -rs` (no echo) and piped directly to `gh secret set`.
# They are never written to disk, never printed, and never passed through an LLM.
#
# Requirements: gh CLI authenticated, internet access

set -euo pipefail

REPO="${REPO:-Enreign/pagerunner}"
TAP_REPO="${TAP_REPO:-Enreign/homebrew-pagerunner}"

print_step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
print_ok()   { printf '\033[1;32m✓  %s\033[0m\n' "$*"; }
print_skip() { printf '\033[1;33m–  %s (skipped)\033[0m\n' "$*"; }
print_info() { printf '   %s\n' "$*"; }

check_existing() {
  local name="$1"
  # gh secret list exits 0 and prints names; grep is silent
  if gh secret list --repo "$REPO" 2>/dev/null | grep -q "^${name}\b"; then
    return 0  # exists
  fi
  return 1    # not set
}

set_secret() {
  local name="$1"
  local token="$2"
  printf '%s' "$token" | gh secret set "$name" --repos "$REPO"
}

print_step "Pagerunner release secrets setup"
print_info "Repo: $REPO"
print_info "This script sets: NPM_TOKEN, CRATES_IO_TOKEN, HOMEBREW_TAP_TOKEN"
print_info "Tokens are read with no echo and piped directly to gh — never stored or printed."

# ── NPM_TOKEN ──────────────────────────────────────────────────────────────
print_step "NPM_TOKEN"
if check_existing NPM_TOKEN; then
  print_info "Already set. Re-enter to replace, or press Enter to skip."
  read -rsp "npm Automation token (blank = keep existing): " NPM_TOKEN_VAL
  printf '\n'
  if [[ -n "$NPM_TOKEN_VAL" ]]; then
    set_secret NPM_TOKEN "$NPM_TOKEN_VAL"
    unset NPM_TOKEN_VAL
    print_ok "NPM_TOKEN updated"
  else
    print_skip "NPM_TOKEN"
  fi
else
  print_info "Create an Automation token at: https://www.npmjs.com/settings/~/tokens/new"
  print_info "Token type: Automation  (not Granular — classic Automation works for unscoped packages)"
  read -rsp "Paste token (input hidden): " NPM_TOKEN_VAL
  printf '\n'
  if [[ -z "$NPM_TOKEN_VAL" ]]; then
    print_skip "NPM_TOKEN (no input)"
  else
    set_secret NPM_TOKEN "$NPM_TOKEN_VAL"
    unset NPM_TOKEN_VAL
    print_ok "NPM_TOKEN set"
  fi
fi

# ── CRATES_IO_TOKEN ────────────────────────────────────────────────────────
print_step "CRATES_IO_TOKEN"
if check_existing CRATES_IO_TOKEN; then
  print_info "Already set. Re-enter to replace, or press Enter to skip."
  read -rsp "crates.io token (blank = keep existing): " CRATES_TOKEN_VAL
  printf '\n'
  if [[ -n "$CRATES_TOKEN_VAL" ]]; then
    set_secret CRATES_IO_TOKEN "$CRATES_TOKEN_VAL"
    unset CRATES_TOKEN_VAL
    print_ok "CRATES_IO_TOKEN updated"
  else
    print_skip "CRATES_IO_TOKEN"
  fi
else
  print_info "Create a token at: https://crates.io/settings/tokens"
  print_info "Scope: publish-update"
  read -rsp "Paste token (input hidden): " CRATES_TOKEN_VAL
  printf '\n'
  if [[ -z "$CRATES_TOKEN_VAL" ]]; then
    print_skip "CRATES_IO_TOKEN (no input)"
  else
    set_secret CRATES_IO_TOKEN "$CRATES_TOKEN_VAL"
    unset CRATES_TOKEN_VAL
    print_ok "CRATES_IO_TOKEN set"
  fi
fi

# ── HOMEBREW_TAP_TOKEN ─────────────────────────────────────────────────────
print_step "HOMEBREW_TAP_TOKEN"
if check_existing HOMEBREW_TAP_TOKEN; then
  print_info "Already set. Re-enter to replace, or press Enter to skip."
  read -rsp "GitHub PAT (blank = keep existing): " HB_TOKEN_VAL
  printf '\n'
  if [[ -n "$HB_TOKEN_VAL" ]]; then
    set_secret HOMEBREW_TAP_TOKEN "$HB_TOKEN_VAL"
    unset HB_TOKEN_VAL
    print_ok "HOMEBREW_TAP_TOKEN updated"
  else
    print_skip "HOMEBREW_TAP_TOKEN"
  fi
else
  print_info "Create a fine-grained PAT at: https://github.com/settings/personal-access-tokens/new"
  print_info "Repository access: $TAP_REPO only"
  print_info "Permissions: Contents (read+write), Pull requests (read+write)"
  read -rsp "Paste token (input hidden): " HB_TOKEN_VAL
  printf '\n'
  if [[ -z "$HB_TOKEN_VAL" ]]; then
    print_skip "HOMEBREW_TAP_TOKEN (no input)"
  else
    set_secret HOMEBREW_TAP_TOKEN "$HB_TOKEN_VAL"
    unset HB_TOKEN_VAL
    print_ok "HOMEBREW_TAP_TOKEN set"
  fi
fi

# ── Summary ────────────────────────────────────────────────────────────────
print_step "Current secrets in $REPO"
gh secret list --repo "$REPO"
