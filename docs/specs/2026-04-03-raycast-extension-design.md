# Raycast Extension — Design Spec

**Date:** 2026-04-03
**Status:** Draft

## Overview

A Raycast extension for managing pagerunner from a keyboard-driven launcher. Covers session lifecycle, tab browsing, KV store, secrets, site knowledge, and checkpoints — all backed by the existing `pagerunner` CLI (JSON output, exit 1 on error). No daemon changes required.

## Motivation

The macOS menu bar app provides ambient session awareness, but actions still require switching context. A Raycast extension gives keyboard-first access: search sessions, grab screenshots, look up KV values — without leaving the current workflow.

Raycast was chosen over building a native command palette because:
- It handles the launcher UI, hotkeys, fuzzy search, and keyboard navigation
- Pagerunner's ICP (developers, AI engineers on macOS) skews heavily toward Raycast users
- Extension development is TypeScript/React — fast to build and iterate

## Architecture

```
Raycast Extension (TypeScript/React)
  └── execCommand("pagerunner", [...args])
        └── parses JSON stdout
              └── renders as Raycast List / Detail / Form
```

**Transport:** `child_process.execFile("pagerunner", ...)` for every operation. All 38 CLI subcommands return JSON to stdout, errors to stderr with exit 1.

**Error handling:** stderr content surfaces as a Raycast Toast notification.

**Caching:** Raycast's built-in `useCachedPromise` for session and tab lists. Lists auto-refresh when the Raycast window regains focus.

**Binary location:** Detected via `which pagerunner`, overridable in extension preferences.

## Commands

Seven top-level commands, each searchable independently in Raycast:

### Sessions

List all open sessions. Each row shows profile name, session ID, and tab count.

**Inline actions (Cmd+K):**
- Close Session → `pagerunner close-session <sid>`
- Save Checkpoint → `pagerunner save-session-checkpoint <sid>`
- Open Tabs → navigates to the Tabs command filtered to this session

**CLI:** `pagerunner list-sessions`

### New Session

Profile picker with optional configuration.

**Flow:**
1. `pagerunner list-profiles` → render as selectable list
2. User selects a profile
3. Form appears with toggles: Stealth Mode, Anonymize
4. `pagerunner open-session <profile> [--stealth] [--anonymize]`
5. Toast: "Session opened" with session ID
6. Auto-navigate to Tabs view for the new session

### Tabs

List tabs for a selected session. Each row shows tab title and URL.

**Inline actions:**
- Screenshot → `pagerunner screenshot <sid> <tid> --base64` → decode PNG → display in Raycast Detail panel via temp file
- Navigate → Form with URL input → `pagerunner navigate <sid> <tid> <url>`
- Close Tab → `pagerunner close-tab <sid> <tid>`
- Get Content → `pagerunner get-content <sid> <tid>` → display in Detail panel

**Entry:** Accessed from Sessions list or directly (prompts for session selection).

### KV Browser

Hierarchical browser: namespace → keys → values.

**Flow:**
1. `pagerunner kv-list` with namespace selection
2. List keys with prefix filtering
3. Select key → show value in Detail panel

**Inline actions:**
- Copy Value → clipboard
- Edit → Form → `pagerunner kv-set <ns> <key> <value>`
- Delete → `pagerunner kv-delete <ns> <key>`
- New Key → Form → `pagerunner kv-set <ns> <key> <value>`

### Secrets

List secret names. Values are never displayed (security boundary).

**Inline actions:**
- Delete → `pagerunner delete-secret <name>` with confirmation

**CLI:** `pagerunner list-secrets`

### Site Knowledge

Search site knowledge by origin.

**Flow:**
1. Text input for origin URL
2. `pagerunner get-site-knowledge <origin>`
3. Display knowledge content in Detail panel

### Checkpoints

List and restore session checkpoints.

**Flow:**
1. Select a profile
2. `pagerunner list-session-checkpoints --profile <name>`
3. List checkpoints with names and timestamps

**Inline actions:**
- Restore → select a session → `pagerunner restore-session-checkpoint <sid> <checkpoint-id>`

## Screenshot in Detail Panel

Screenshots are the highest-value visual feature in the extension. When a user triggers Screenshot on a tab:

1. Run `pagerunner screenshot <sid> <tid> --base64`
2. Decode base64 PNG
3. Write to a temp file
4. Display in Raycast's Detail panel via markdown: `![](file:///tmp/pagerunner-screenshot-<tid>.png)`

This gives instant visual feedback of what Chrome is rendering without leaving Raycast.

## Extension Preferences

Configurable in Raycast's extension settings:

| Preference | Type | Default | Description |
|---|---|---|---|
| `pagerunnerPath` | string | auto-detect via `which` | Path to the `pagerunner` binary |
| `daemonAutoStart` | boolean | `true` | Start daemon automatically if not running |

## Project Structure

```
extensions/raycast/
├── package.json              # Raycast extension manifest + commands
├── tsconfig.json
├── src/
│   ├── utils/
│   │   └── pagerunner.ts     # execCommand wrapper, JSON parse, error handling
│   ├── sessions.tsx          # Sessions list command
│   ├── new-session.tsx       # New Session command (profile picker + form)
│   ├── tabs.tsx              # Tabs list command
│   ├── kv-browser.tsx        # KV Browser command
│   ├── secrets.tsx           # Secrets list command
│   ├── site-knowledge.tsx    # Site Knowledge command
│   └── checkpoints.tsx       # Checkpoints list/restore command
└── assets/
    └── icon.png              # Extension icon
```

Located at `extensions/raycast/` in the repo root — separate from `apps/menubar/` since Raycast extensions are standalone npm packages with their own build lifecycle.

## Distribution

**Phase 1:** Manual install — clone repo, `cd extensions/raycast && npm install && npm run dev`.

**Phase 2:** Submit to Raycast Store for public discoverability once the extension is stable.

## Out of Scope

- Direct daemon socket communication (CLI subprocess is sufficient)
- Real-time streaming or WebSocket updates
- Tab content editing or form filling from Raycast (too complex for a launcher UI)
- Mobile companion or notch HUD (separate future explorations)
