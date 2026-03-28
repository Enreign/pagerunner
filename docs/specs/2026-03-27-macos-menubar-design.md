# macOS Menu Bar App — Design Spec

> **Linear:** LNY-194 — macOS menu bar MVP (Swift)
> **Branch:** feature/lny-194-macos-menu-bar-mvp-tauri-session-switcher-profile-status

---

## Goal

A native macOS menu bar companion for pagerunner. Shows all Chrome profiles, active sessions, open tabs, and daemon status at a glance. One-click open/close sessions, save/restore session checkpoints, and control the daemon — no terminal required.

---

## Architecture

Two-module Swift Package under `apps/menubar/`:

```
apps/menubar/
├── Package.swift
├── Sources/
│   ├── PagerunnerCore/          — zero UI, fully testable in isolation
│   │   ├── DaemonClient.swift   — Unix socket IPC (JSON-lines over ~/.pagerunner/daemon.sock)
│   │   ├── Models.swift         — Profile, Session, Tab, Checkpoint (Codable structs)
│   │   └── PollingService.swift — async/await polling loop, cancel/restart on panel show/hide
│   └── PagerunnerBar/           — app, tray, all SwiftUI/AppKit
│       ├── App.swift            — @main, NSApplicationDelegateAdaptor, LSUIElement (no Dock icon)
│       ├── StatusItemController.swift  — NSStatusItem, NSPopover, tray icon rendering
│       ├── AppState.swift       — @Observable, single source of truth
│       ├── NotificationService.swift   — UNUserNotificationCenter, actionable notifications
│       └── Views/
│           ├── PanelView.swift
│           ├── OverviewView.swift
│           ├── ProfileDetailView.swift
│           ├── SessionBlockView.swift
│           └── CheckpointListView.swift
└── Tests/
    └── PagerunnerCoreTests/     — DaemonClient, polling, models (no UI deps)
```

**Tech stack:**
- Swift 6, strict concurrency
- SwiftUI for all panel views; AppKit (`NSStatusItem`, `NSPopover`) for tray integration
- Swift Package Manager — no Xcode project files
- macOS 14.0+ (Sonoma)
- Dependencies: **Sparkle 2.x** (auto-update), **KeyboardShortcuts** (sindresorhus, global hotkey)

**Project location:** `apps/menubar/` inside the existing pagerunner repo. Swift and Cargo are independent build systems — no workspace changes to `Cargo.toml` are needed.

---

## Profile kinds (config.toml change)

`ChromeProfile` in `src/config.rs` gains an optional `kind` field:

```toml
[[profiles]]
name = "personal"
display_name = "Stas (personal)"
user_data_dir = "/Users/stas/Library/Application Support/Google/Chrome/Default"
# kind omitted → defaults to "personal"

[[profiles]]
name = "agent-1"
display_name = "Agent 1"
user_data_dir = "/Users/stas/Library/Application Support/Google/Chrome/Agent1"
kind = "agent"
```

Rust change: `pub kind: Option<String>` on `ChromeProfile`, defaulting to `None` (treated as `"personal"`). No existing configs break — `#[serde(default)]`.

`list_profiles` response gains a `"kind"` field per entry:

```json
{
  "ok": true,
  "data": [
    { "name": "personal", "display_name": "Stas (personal)", "kind": "personal" },
    { "name": "agent-1",  "display_name": "Agent 1",         "kind": "agent" }
  ]
}
```

The Swift `Profile` struct uses this to split the Overview into two sections and render distinct icon styles.

---

## Wire Protocol (IPC)

`PagerunnerCore.DaemonClient` connects to `~/.pagerunner/daemon.sock` (a Unix domain socket) using `FileHandle` or `NWConnection`. **One fresh connection per call** — no pooling. This matches the existing Rust `DaemonClient` exactly.

Request (one JSON line, `\n`-terminated):
```json
{"id":"<uuid>","tool":"list_sessions","args":{}}
```

Response (one JSON line):
```json
{"id":"<uuid>","result":"{…escaped JSON string…}","error":null}
```

`result` is a JSON-encoded string (double-serialised). `DaemonClient.call()` returns the parsed inner JSON value.

**Connection lifecycle per call:**
1. Open socket to `~/.pagerunner/daemon.sock`
2. Write request line
3. Read one response line
4. Close socket

**Daemon not running:** socket connect throws `ENOENT` or `ECONNREFUSED` → surface as `.daemonStopped` error to callers.

**Finding the binary:** check `~/.local/bin/pagerunner`, then resolve via `which pagerunner` (`Process` + `launchPath: "/usr/bin/env"`). If neither found, `AppState.binaryPath = nil` → Settings shows installation instructions.

**Starting the daemon:** `Process()` with `launchPath` = resolved binary, `arguments = ["daemon"]`. App holds a `Process` reference; `terminationHandler` → `AppState.daemonStatus = .stopped`.

---

## Response Shapes (for Models.swift)

### `list_profiles`
```json
{ "ok": true, "data": [{ "name": "personal", "display_name": "Stas", "kind": "personal" }] }
```

### `list_sessions`
```json
{
  "ok": true,
  "data": [
    { "id": "abc123", "profile": "personal", "display_name": "Stas", "stealth": false, "status": "alive" }
  ]
}
```
`status` is `"alive"` or `"crashed"`.

### `list_tabs`
```json
{
  "ok": true,
  "data": [
    { "target_id": "T1", "url": "https://github.com/anthropics", "title": "anthropics · GitHub" }
  ]
}
```

### `list_session_checkpoints` (new)
```json
{
  "ok": true,
  "data": [
    {
      "checkpoint_id": "ckpt-uuid",
      "name": "Research sprint",
      "saved_at": 1711500000,
      "profile": "personal",
      "tab_count": 3,
      "origins": ["github.com", "linear.app", "notion.so"]
    }
  ]
}
```

---

## UI Design

### Navigation structure

**Level 1 — Overview (default/home screen)**

Always lands here on panel open. Two sections: "Your profiles" (`kind = "personal"`) and "Agent profiles" (`kind = "agent"`). One row per profile:
- Profile icon (colorful Chrome-style for personal, muted grey for agent)
- Profile name
- Top active URL or "No open sessions"
- Session count badge (green if ≥1 alive, plain if idle)
- `›` chevron — tap to drill in

Scales identically for 1 to 20+ profiles (scrollable list).

**Level 2 — Profile detail**

Reached via Overview row tap or compact icon tab. Contains:
- **‹ Overview** back button
- **Session blocks** — one per session, with status badge (active/idle/dead), **Save** button, close ✕
  - **Tab rows** nested under each session — favicon + URL truncated + close ✕ (shown on hover)
  - Tab row click → focus Chrome window (see below)
- **＋ Open new session** (disabled when daemon stopped)
- **Saved sessions** collapsible — named checkpoints, tab origin preview, Restore + delete ✕

**Navigation strip**

- **Overview tab** — leftmost, fixed, icon + "Overview" label, `border-bottom` highlight when active
- **Compact icon tabs** — scrollable strip, icons only (profile name in tooltip via `help(_:)`). Colorful for personal, grey for agent. Vertical divider between groups. `‹ ›` scroll hint buttons when overflow exists.

### Focus Chrome window

When a tab row is tapped, `StatusItemController` runs:

```swift
let script = """
tell application "Google Chrome"
    set winList to every window
    repeat with w in winList
        set tabList to every tab of w
        repeat with t in tabList
            if URL of t is "\(url)" then
                set index of w to 1
                set active tab index of w to (get index of t)
                activate
                return
            end if
        end repeat
    end repeat
end tell
"""
var error: NSDictionary?
NSAppleScript(source: script)?.executeAndReturnError(&error)
```

Uses `url` from `list_tabs` response. Falls back silently if the tab has navigated away.

### Daemon banner (3 states)

- 🟢 **Running** — `rgba(34,197,94,0.1)` tint, "Daemon running", session + tab count, **Stop** button
- 🔴 **Stopped** — `rgba(239,68,68,0.08)` tint, "Daemon stopped", **Start daemon** button; sessions show "dead" badge; ＋ Open new session disabled
- 🟡 **Stale** — `rgba(245,158,11,0.08)` tint, "Last seen Xs ago" — after 3+ consecutive poll failures while prior data exists

### Tray icon

`NSStatusItem` with a template image (auto-adapts dark/light mode). Small status dot:
- Green — daemon running, ≥1 alive session
- Yellow — stale (3+ failures)
- Red — daemon stopped / unreachable

### Panel presentation

`NSPopover` anchored to `NSStatusItem.button`. Auto-positions for screen edges and notch. `NSVisualEffectView` with `.sidebar` material (native vibrancy). Width: 310pt. Max height: 560pt with internal `ScrollView`.

**No Dock icon** — `LSUIElement = YES` in `Info.plist`.

**Keepalive window** — 1×1 `NSWindow` with `isReleasedWhenClosed = false`, `orderOut` immediately on create. Keeps the run loop alive when the popover is closed.

---

## Polling

`PollingService` runs a `Task.detached(priority: .utility)` loop:

```swift
while !Task.isCancelled {
    try? await Task.sleep(for: .seconds(interval))
    await poll()
}
```

**Intervals:**
- Panel visible: 2s
- Panel hidden: 10s
- On panel open: cancel current task, start 2s task immediately (no initial sleep)
- On panel close: cancel 2s task, start 10s task

**`poll()` sequence:**
1. Call `list_sessions` → update `AppState.sessions`
2. For each session where `status == "alive"`: call `list_tabs(session_id:)` → update `AppState.tabs[sessionId]`
3. Calls are **serial** (not parallel) to avoid hammering the daemon socket
4. If `list_tabs` fails for one session (e.g., session died between step 1 and step 2): swallow the error, mark that session's tabs as `[]`, continue with the next session
5. On success: reset `consecutiveFailures = 0`
6. On any top-level failure (step 1 fails): `consecutiveFailures += 1` (see failure gate below)

**Failure gate:**

| `consecutiveFailures` | `AppState.daemonStatus` |
|---|---|
| 0 | `.running` |
| 1–2 | `.running` (last good data shown, no UI change) |
| 3–4 | `.stale(lastSeenAt: Date)` → yellow banner |
| 5+ | `.stopped` → red banner |

Resets to `.running` immediately on next successful `list_sessions`.

---

## Notifications

`NotificationService` observes `AppState` diffs using `withObservationTracking`. Requests `.alert + .sound` permission on first launch.

**Notification types and actions:**

| Event | Title | Actions |
|---|---|---|
| Session `status` flips to `"crashed"` | "Session crashed — \(profile)" | **Restart** (calls `open_session`), Dismiss |
| `daemonStatus` flips to `.stopped` unexpectedly | "Pagerunner daemon stopped" | **Restart daemon**, Dismiss |
| Agent session idle >30 min (no tab activity) | "Agent \(name) idle for 30 min" | **Close session**, Keep |
| Checkpoint saved | "Checkpoint saved" | (informational, no actions) |
| Future LNY-195 | Agent activity events | **View**, Dismiss |

Notification categories registered at launch with `UNUserNotificationCenter.current().setNotificationCategories(_:)`. Notification delivery respects system Focus modes automatically.

---

## New pagerunner Tools

Five new tools added to the Rust binary (same pattern as existing 33 — MCP + CLI subcommand):

### `close_tab`

```
close_tab(session_id, target_id)
```

Uses CDP `Target.closeTarget`. **Guard:** if the session has only one tab remaining, returns an error `{"ok":false,"error":"Cannot close last tab — use close_session instead","error_type":"validation_error"}` rather than potentially killing the Chrome window. The Swift UI disables the tab ✕ button when `tabs.count == 1`.

### `save_session_checkpoint`

```
save_session_checkpoint(session_id, name?: string)
```

1. Calls `save_tab_state(session_id)` internally
2. For each tab's origin (deduplicated): calls `save_snapshot(session_id, target_id)`
3. Stores a `SessionCheckpoint` record in `state.db` with: `checkpoint_id` (UUID), `name` (auto-named `"Autosave · {date}"` if omitted), `profile`, `saved_at` (Unix timestamp), list of `(url, origin)` pairs
4. Returns `{"ok":true,"checkpoint_id":"…","name":"…"}`

### `restore_session_checkpoint`

```
restore_session_checkpoint(session_id, checkpoint_id)
```

1. **Closes all existing tabs** in the session first (CDP `Target.closeTarget` per tab, except the last which gets navigated to `about:blank` to avoid killing the window)
2. Calls `restore_tab_state` with the saved tab URLs
3. For each origin in the checkpoint: calls `restore_snapshot(session_id, target_id, origin)` matching the new tab for that origin
4. Returns `{"ok":true,"tabs_restored":3,"snapshots_restored":2}`

### `list_session_checkpoints`

```
list_session_checkpoints(profile)
```

Returns checkpoints for the profile sorted newest-first. Response shape:
```json
{
  "ok": true,
  "data": [{
    "checkpoint_id": "uuid",
    "name": "Research sprint",
    "saved_at": 1711500000,
    "profile": "personal",
    "tab_count": 3,
    "origins": ["github.com", "linear.app", "notion.so"]
  }]
}
```

### `delete_session_checkpoint`

```
delete_session_checkpoint(profile, checkpoint_id)
```

Removes the `SessionCheckpoint` record from `state.db`. Does not delete the constituent `save_snapshot` entries (they may be referenced by other checkpoints or used independently). Returns `{"ok":true}`.

---

## Distribution & Launch at Login

**Build scripts** (`apps/menubar/scripts/`):
- `package.sh` — `swift build -c release`, bundles `.app`, produces signed `.zip`
- `notarize.sh` — submits to Apple notarization, staples ticket

**Required environment variables for notarization:**
```
APPLE_TEAM_ID          # e.g. ABC123DEF
APPLE_ID               # e.g. dev@example.com
NOTARIZE_PASSWORD      # app-specific password from appleid.apple.com
CODE_SIGN_IDENTITY     # e.g. "Developer ID Application: …"
```

**GitHub Actions:** `.github/workflows/menubar-release.yml` — triggers on `apps/menubar/**` tag push, runs `package.sh` + `notarize.sh`, attaches `.zip` to GitHub Release, updates Homebrew cask formula.

**Sparkle appcast:** `apps/menubar/scripts/make_appcast.sh` — generates `appcast.xml` from the notarized `.zip` and `CHANGELOG.md`, committed to `gh-pages` branch. Sparkle disabled when installed via Homebrew (detect `Bundle.main.bundlePath.contains("Cellar")`).

**Launch at login:** `SMAppService.mainApp.register()` / `.unregister()` — toggle in Settings panel (off by default). macOS 13+ API; no LaunchAgent plist.

**Global hotkey:** `KeyboardShortcuts` — user-configurable in Settings, no default.

---

## Testing

`PagerunnerCoreTests` (no UI, no live daemon):
- `DaemonClient` JSON request/response round-trips (using a mock Unix socket server)
- `PollingService` failure-gate state machine (mock `DaemonClient` that returns errors)
- `Models` Codable round-trips for all response shapes
- `save_session_checkpoint` Rust tool: verifies `save_tab_state` + `save_snapshot` are called per origin
- `restore_session_checkpoint` Rust tool: verifies existing tabs closed before re-opening
- `close_tab` Rust tool: verifies error returned when session has exactly 1 tab

Manual live tests (recorded in `docs/test-runs/`):
- Panel opens/closes, positions correctly under tray icon (including near notch)
- Overview shows all 10 profiles split into two sections; scrolls correctly
- Profile detail: session open/close/tab-close flows
- Checkpoint save → restore round-trip (tabs re-open, auth state restored)
- `close_tab` on last tab shows error, ✕ button disabled in UI
- Daemon Stop → banner goes red → Start → banner goes green
- Notifications fire for session crash and daemon stop, action buttons work
- Launch at login toggle persists across reboot
- Sparkle update check presents update sheet

---

## Out of Scope (this MVP)

- Windows / Linux support
- iPad / iOS
- Drag-to-reorder profiles (order follows `config.toml`)
- Inline tab URL editing
- Multiple daemon instances
- LNY-195 live agent activity feed (separate ticket; `NotificationService` hook already in place)
