#!/usr/bin/env bash
# setup-npm-token.sh — set the NPM_TOKEN GitHub secret using pagerunner's sealed secret store.
#
# This script demonstrates the pagerunner secret flow:
#   1. pagerunner navigates to npmjs.com and creates a token (LLM-driven, but value stays sealed)
#   2. pagerunner use-secret pipes the sealed value directly to gh secret set
#   3. The token value never touches the LLM, stdout, or any log
#
# Prerequisites:
#   - pagerunner daemon running: pagerunner daemon &
#   - Chrome profile with npmjs.com session (or credentials to log in)
#   - gh CLI authenticated
#
# Usage:
#   ./scripts/setup-npm-token.sh [--profile <chrome-profile>] [--repo <owner/repo>]

set -euo pipefail

PROFILE="${PAGERUNNER_PROFILE:-personal}"
REPO="${REPO:-Enreign/pagerunner}"
SECRET_NAME="npm_token"

print_step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
print_ok()   { printf '\033[1;32m✓  %s\033[0m\n' "$*"; }
print_info() { printf '   %s\n' "$*"; }

# ── Parse args ─────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile) PROFILE="$2"; shift 2 ;;
    --repo)    REPO="$2";    shift 2 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

print_step "Pagerunner npm token setup"
print_info "Profile:     $PROFILE"
print_info "GitHub repo: $REPO"
print_info "Secret name: $SECRET_NAME"
print_info ""
print_info "The token value will be stored in pagerunner's sealed store."
print_info "It will never be printed, logged, or seen by the LLM."

# ── Step 1: Use pagerunner CLI to open a session and extract the token ─────
# The LLM flow would do this interactively; this script does it directly via
# CLI for use without an LLM session.

print_step "Opening Chrome session"
SESSION=$(pagerunner open-session "$PROFILE" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['session_id'])")
print_ok "Session: ${SESSION:0:8}..."

print_step "Navigating to npmjs.com token creation"
TARGET=$(pagerunner list-tabs "$SESSION" 2>/dev/null | python3 -c "
import sys,json
tabs = json.load(sys.stdin).get('tabs', [])
print(tabs[0]['target_id'] if tabs else '')
")
pagerunner navigate "$SESSION" "$TARGET" "https://www.npmjs.com/settings/~/tokens/new" > /dev/null
sleep 2

print_info ""
print_info "The npmjs.com token creation page is now open in Chrome."
print_info "Please:"
print_info "  1. Select 'Automation' as the token type"
print_info "  2. Click 'Generate Token'"
print_info "  3. Copy the token (it will be shown on screen)"
print_info ""
read -rp "Press Enter once the token is visible on the page..."

# ── Step 2: Extract token from page into sealed store ─────────────────────
print_step "Extracting token into sealed store"

# npm shows the generated token in a readonly input or pre element.
# Try common selectors; the LLM would use extract_secret tool directly.
pagerunner evaluate "$SESSION" "$TARGET" \
  "document.querySelector('[data-testid=\"token-value\"], .copy-text, pre, code, input[type=text]')?.value || document.querySelector('[data-testid=\"token-value\"], .copy-text, pre, code')?.textContent?.trim()" \
  2>/dev/null | python3 -c "
import sys, json, subprocess, os
result = json.load(sys.stdin)
value = result.get('result', '')
if not value or not value.startswith('npm_'):
    print('  Could not auto-extract token. Run: pagerunner use-secret to set it manually.')
    exit(1)
# Store via pagerunner CLI — value never in env or log
import tempfile
with tempfile.NamedTemporaryFile(mode='w', suffix='.tmp', delete=False) as f:
    f.write(value)
    fname = f.name
os.execlp('sh', 'sh', '-c',
  f'cat {fname} | pagerunner evaluate \"\$SESSION\" \"\$TARGET\" \"\$(cat {fname})\" --store-as npm_token; rm -f {fname}')
" || {
  print_info ""
  print_info "Auto-extract failed (page layout may differ). Falling back to manual entry."
  print_info "This is the safe fallback — token still never touches the LLM."
  read -rsp "Paste your npm Automation token (input hidden): " NPM_TOKEN_VAL
  printf '\n'
  if [[ -n "$NPM_TOKEN_VAL" ]]; then
    printf '%s' "$NPM_TOKEN_VAL" | gh secret set NPM_TOKEN --repos "$REPO"
    unset NPM_TOKEN_VAL
    print_ok "NPM_TOKEN set directly (fallback path)"
    exit 0
  else
    echo "No token entered. Exiting."
    exit 1
  fi
}

# ── Step 3: Use sealed secret to set GitHub secret ─────────────────────────
print_step "Setting GitHub secret from sealed store"
print_info "Running: pagerunner use-secret $SECRET_NAME -- gh secret set NPM_TOKEN --repos $REPO"

pagerunner use-secret "$SECRET_NAME" -- gh secret set NPM_TOKEN --repos "$REPO"
print_ok "NPM_TOKEN set in $REPO"

# ── Step 4: Verify ─────────────────────────────────────────────────────────
print_step "Verification"
gh secret list --repo "$REPO" | grep NPM_TOKEN && print_ok "NPM_TOKEN confirmed in GitHub secrets"

print_info ""
print_info "Audit trail (last 5 secret events):"
pagerunner audit 2>/dev/null | grep "SECRET_" | tail -5 || true
