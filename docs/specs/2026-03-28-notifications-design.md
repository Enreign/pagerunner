# Pagerunner Notification System — Design Spec

## Goal

Surface ambient awareness to users of the Pagerunner MCP server and menubar app. An agent calls `notify("Tests passed — 3 flows green")`, a macOS notification appears, the user taps "View", and the menubar opens to that session. This works with any MCP client (Claude Code, Cursor, Codex) without client-specific integration.

## Architecture

```
pagerunner mcp (Rust)          pagerunner daemon (Rust)         PagerunnerBar (Swift)
─────────────────────          ────────────────────────         ──────────────────────
Claude calls notify()   ────►  writes to notifications          NotificationPoller
                               table in ReDB                     polls every 2s (always,
                                                          ◄────  independent of popover)
                               marks as delivered         ────►  calls list_notifications
                                                                  fires UNUserNotificationCenter

AppState (existing)                                              Action tap → deep-link
  session crash detected ────► writes notification row           to profile/session
  daemon unexpected stop ────► fires directly from AppState
  agent idle (Swift)     ────► fires directly from PollingService
```

**Key principles:**
- The daemon is the notification store for agent-initiated events. The menubar is the delivery agent.
- The `notify` MCP tool is the only write path from MCP clients.
- Daemon crash and agent idle are detected client-side and bypass the DB (daemon may not be writable when crashing; idle is tracked in Swift).
- `NotificationPoller` runs on its own 2-second loop independent of `PollingService` (which slows to ~10s when popover is closed). This ensures `notify` tool results arrive promptly even when the user isn't looking at the panel.

---

## Tech Stack

- Rust: ReDB for notification queue, new `notify` and `list_notifications` tools in `mcp_server.rs`
- Swift: `NotificationPoller` class (new), `UNUserNotificationCenter`, `UserDefaults` for settings
- Existing: `NotificationService.swift` (extended + stubs wired), `AppState.swift` (wired), `PollingService.swift` (extended for idle detection), `SettingsView.swift` (extended)

---

## Component 1: `notify` MCP Tool (Rust)

**File:** `src/mcp_server.rs`

New tool available to any connected MCP client. Not subject to session security policy (allowed/blocked tools list) — it writes nothing to the browser.

### Input schema

```json
{
  "title": "Tests passed — 3 flows green",
  "body": "Signup, checkout, password reset all passed.",
  "level": "info",
  "session_id": "abc123"
}
```

| Field | Type | Required | Notes |
|---|---|---|---|
| `title` | String | Yes | Shown as notification title |
| `body` | String | No | Shown as notification subtitle |
| `level` | `"info"` \| `"warning"` \| `"error"` | No | Default `"info"`. Controls sound priority |
| `session_id` | String | No | Enables deep-link to session's profile |

### Behavior

1. If `session_id` is provided, resolve `profile_name` from the session store (best-effort; nil if session not found)
2. Write one row to the `notifications` table in ReDB
3. Return `{"ok": true}` immediately — fire-and-forget

### `list_notifications` tool (internal)

Called only by the menubar poller. Returns all `delivered = false` rows and marks them `delivered = true` atomically in the same ReDB write transaction. No second round-trip.

**Response shape:**
```json
{
  "notifications": [
    {
      "id": 42,
      "title": "Tests passed",
      "body": "3 flows green",
      "level": "info",
      "session_id": "abc123",
      "profile_name": "growthmate",
      "created_at": 1711641600000000
    }
  ]
}
```

### Notification DB schema

```
id:           u64       auto-increment
title:        String
body:         Option<String>
level:        String    "info" | "warning" | "error"
session_id:   Option<String>
profile_name: Option<String>    resolved from session_id; nil is valid
created_at:   i64       unix microseconds
delivered:    bool      default false
```

---

## Component 2: `NotificationPoller` (Swift)

**File:** `Sources/PagerunnerBar/NotificationPoller.swift` (new, lives in `PagerunnerBar` target — depends on both `DaemonClient` from Core and `NotificationService` from Bar)

`@MainActor final class NotificationPoller` — same pattern as `PollingService`. Required for safe calls to `NotificationService` which is also `@MainActor`.

### Lifecycle

- Instantiated in `App.swift` alongside `NotificationService`
- On first launch (or if authorization status is `.notDetermined`), calls `NotificationService.requestPermission()`
- Starts a `Task` loop immediately on init; never stopped while app is alive

### Polling loop

```
every 2 seconds:
  call list_notifications via DaemonClient
  for each row in response["notifications"]:
    if category == NOTIFY_TOOL → always fire (no settings gate)
    else → check per-profile settings for that category
  fire via NotificationService
```

### Automatic events (no DB — fired directly)

**Daemon unexpected stop** — fired from `AppState.recordFailure()` on the transition from `.running` or `.stale` → `.stopped` when `transition == .none`. Fires once per transition; subsequent failures return early (`guard daemonStatus != .stopped`). Calls `NotificationService.notifyDaemonStopped()`.

**Agent idle** — tracked in `PollingService`. For each session belonging to an agent profile: record tab count + timestamp at each poll. When the session has been `.alive` with no tab-count change for ≥ idle threshold, fire once. Use an in-memory `Set<String>` (sessionId) to avoid re-firing within the same app session. Re-firing on menubar restart is acceptable — the user may have missed the first one.

**"Tab count change" definition:** change in number of open tabs for that session. Navigation within an existing tab does not count as activity (tab count unchanged). Consistent with what the polling loop already tracks.

Calls `NotificationService.notifyAgentIdle(profileName:idleMinutes:)`.

---

## Component 3: `NotificationService` Extensions (Swift)

**File:** `Sources/PagerunnerBar/NotificationService.swift` (extend existing)

### Dependency injection

Add `configure(appState:controller:)` method (called from `App.swift` after init) to set weak references. Avoids circular init dependency.

```swift
weak var appState: AppState?
weak var controller: StatusItemController?

func configure(appState: AppState, controller: StatusItemController) {
    self.appState = appState
    self.controller = controller
}
```

### New method

```swift
func notifyExplicit(title: String, body: String?, level: String, profileName: String?, sessionId: String?)
```

Maps `level` → sound:
- `info` → no sound
- `warning` → `.default`
- `error` → `.default` (`.critical` requires a separate entitlement; fall back silently)

### Updated `notifyAgentIdle` signature

Existing stub must change signature to accept the configured threshold:
```swift
// Before (stub, hardcodes "30 min"):
func notifyAgentIdle(profileName: String)
// After:
func notifyAgentIdle(profileName: String, idleMinutes: Int)
```

### Notification categories — complete updated list

Replace `registerCategories()` in full. Final category list:

| Category | Actions | Notes |
|---|---|---|
| `NOTIFY_TOOL` | **View** | New |
| `SESSION_CRASHED` | **View**, **Restart** | Existing — add View action |
| `AGENT_IDLE` | **View**, **Close Session** | Existing — replace `keepSession` with View |
| `DAEMON_STOPPED` | **Restart Daemon** | Existing — unchanged |
| `SESSION_STARTED` | *(no actions)* | New |
| `CHECKPOINT_SAVED` | *(no actions)* | Existing — unchanged |

### `userInfo` keys

All notifications that support deep-linking carry:

| Key | Value |
|---|---|
| `notif.profileName` | Profile name string, or absent if unknown |
| `notif.sessionId` | Session ID string, or absent if not applicable |

### Action handling

Wire `userNotificationCenter(_:didReceive:withCompletionHandler:)`:

| Action identifier | Behavior |
|---|---|
| `VIEW` | `controller?.openPopover()` + `appState?.navigation = .profile(name)` if `notif.profileName` present, else `.overview` |
| `RESTART_SESSION` | Call `open_session` with `["profile": profileName]` — stealth/anonymize not preserved (intentional; add code comment at call site) |
| `CLOSE_SESSION` | Call `close_session` with `["session_id": sessionId]`; ignore error if session already gone |
| `RESTART_DAEMON` | Call `AppState.restartDaemon()` |

---

## Component 4: Notification Settings (Swift)

**File:** `Sources/PagerunnerBar/Views/SettingsView.swift` (extend existing)

New "Notifications" section. Global settings at top, per-profile list below with individual toggles.

### Settings

| Setting | Default | Scope | UserDefaults key |
|---|---|---|---|
| Notify on daemon health | On | Global | `notif.daemonHealth` |
| Explicit `notify` tool | Always on | Global | *(no toggle — label: "Agent-sent notifications always deliver")* |
| Notify on session crash | On | Per-profile | `notif.<name>.crash` |
| Notify on agent idle | On | Per-profile | `notif.<name>.idle` |
| Idle threshold | 30 min | Per-profile | `notif.<name>.idleMinutes` (options: 15 / 30 / 60) |
| Notify on session start | Off | Per-profile | `notif.<name>.start` |

Defaults applied via `UserDefaults.register(defaults:)` at app launch. Agent profiles default all-on; personal profiles default crash + idle only.

`NOTIFY_TOOL` rows always fire — the poller does not check any settings key for this category.

---

## Data Flow: `notify` tool end-to-end

1. Claude calls `notify(title: "Done", level: "info", session_id: "abc")`
2. MCP server resolves `profile_name = "growthmate"` from session store
3. Writes row to `notifications` table, returns `{"ok": true}`
4. Within ≤2s, `NotificationPoller` calls `list_notifications`
5. Daemon returns the row and marks it `delivered = true` atomically
6. Category is `NOTIFY_TOOL` — always fire, no settings check
7. Calls `notifyExplicit(...)` with no sound (level: info)
8. macOS notification appears: "Done" with **View** button
9. User taps View → `openPopover()` + `navigation = .profile("growthmate")`

---

## Error Handling

- `list_notifications` returns empty if daemon is unreachable — poller retries next cycle, no crash
- `notify` called with unknown `session_id` — writes row with `profile_name = nil`; View opens Overview
- Notification fired when app lacks permission — `add()` fails silently; permission requested on first launch
- `RESTART_SESSION` for a session that no longer exists — `open_session` creates a fresh one
- `CLOSE_SESSION` for an already-closed session — daemon returns error, silently ignored

---

## Testing

Following the project's tests-first rule:

**Rust unit tests (`src/mcp_server.rs`):**
- `notify` writes a row with correct fields including resolved `profile_name`
- `notify` with unknown `session_id` writes row with `profile_name = nil`
- `list_notifications` returns undelivered rows and marks them delivered atomically
- Calling `list_notifications` twice returns rows only on the first call

**Swift unit tests (`Tests/PagerunnerCoreTests/`):**
- Settings gate: `NOTIFY_TOOL` rows always fire; `SESSION_STARTED` rows respect `notif.<name>.start`
- Agent idle fires after threshold, not before; does not fire twice for the same session ID
- `UserDefaults.register(defaults:)` sets correct defaults for agent vs personal profiles

---

## Out of Scope

- **Monitoring/watch** (`watch_for` MCP tool with URL/content callbacks) — most powerful future direction, needs its own spec
- **Task-level auto-notifications** (navigate timeout, click failed) — excluded to protect signal-to-noise ratio
- **Notification history** in the menubar UI — future feature
- **Rich notifications** (images, progress indicators) — future feature
