# Pagerunner Notification System — Design Spec

## Goal

Surface ambient awareness to users of the Pagerunner MCP server and menubar app. An agent calls `notify("Tests passed — 3 flows green")`, a macOS notification appears, the user taps "View", and the menubar opens to that session. This works with any MCP client (Claude Code, Cursor, Codex) without client-specific integration.

## Architecture

```
pagerunner mcp (Rust)          pagerunner daemon (Rust)         PagerunnerBar (Swift)
─────────────────────          ────────────────────────         ──────────────────────
Claude calls notify()   ────►  writes to notifications          NotificationPoller
                               table in ReDB                     polls every 2s (always)
                                                          ◄────  calls list_notifications
                               marks as delivered         ────►  returns undelivered rows
                                                                  fires UNUserNotificationCenter

AppState (existing)                                              Action tap → deep-link
  session crash detected ────► writes notification row           to profile/session
  daemon unexpected stop ────► fires directly (no DB)
  agent idle (Swift)     ────► fires directly (no DB)
```

**Key principle:** The daemon is the notification store. The menubar is the delivery agent. The `notify` MCP tool is the only write path from MCP clients. Daemon crash and agent idle are detected client-side and bypass the DB.

---

## Tech Stack

- Rust: ReDB for notification queue, new `notify` and `list_notifications` tools in `mcp_server.rs`
- Swift: `NotificationPoller` class, `UNUserNotificationCenter`, `UserDefaults` for settings
- Existing: `NotificationService.swift` (extended), `AppState.swift` (wired), `SettingsView.swift` (extended)

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

1. If `session_id` is provided, resolve `profile_name` from the session store
2. Write one row to the `notifications` table in ReDB
3. Return `{"ok": true}` immediately — fire-and-forget

### `list_notifications` tool (internal)

Called only by the menubar. Returns all `delivered = false` rows and marks them delivered atomically in the same ReDB transaction. No second round-trip.

### Notification DB schema

```
id:           u64       auto-increment
title:        String
body:         Option<String>
level:        String    "info" | "warning" | "error"
session_id:   Option<String>
profile_name: Option<String>    resolved from session_id
created_at:   i64       unix microseconds
delivered:    bool      default false
```

---

## Component 2: `NotificationPoller` (Swift)

**File:** `Sources/PagerunnerBar/NotificationPoller.swift` (new)

Lightweight class, started on app launch, runs independently of the panel polling cycle.

### Behavior

- Starts a `Task` loop immediately on init
- Calls `list_notifications` every **2 seconds** regardless of popover state
- On each non-empty result:
  - Reads per-profile notification settings from `UserDefaults`
  - Skips rows where the setting is disabled for that profile
  - Calls `NotificationService` to fire each notification
  - Level → sound mapping:
    - `info` → no sound
    - `warning` → `.default`
    - `error` → `.default` (`.critical` requires entitlement; fall back silently)

### Automatic events (no DB)

Two event types bypass the DB and are fired directly:

**Daemon unexpected stop** — fired from `AppState.recordFailure()` when `transition == .none` and status tips to `.stopped`. Calls `NotificationService.notifyDaemonStopped()`.

**Agent idle** — detected in the existing polling loop. When a session has been `.alive` for ≥ idle threshold minutes with no tab count change, calls `NotificationService.notifyAgentIdle(profileName:)`. Threshold is read from settings. Only fires once per session (tracked via a Set of already-notified session IDs).

---

## Component 3: `NotificationService` Extensions (Swift)

**File:** `Sources/PagerunnerBar/NotificationService.swift` (extend existing)

Add `notifyExplicit(title:body:level:profileName:sessionId:)` for rows from the `notify` tool.

### Notification types and actions

| Category | Title | Actions |
|---|---|---|
| `NOTIFY_TOOL` | From tool args | **View** (opens popover → profile) |
| `SESSION_CRASHED` | "Session crashed — \<profile\>" | **View**, **Restart** |
| `AGENT_IDLE` | "Agent \<profile\> idle \<N\>min" | **View**, **Close Session** |
| `DAEMON_STOPPED` | "Pagerunner stopped unexpectedly" | **Restart Daemon** |
| `SESSION_STARTED` | "\<profile\> session started" | *(dismiss only)* |

All notifications carry `profileName` and `sessionId` (where available) in the `userInfo` dict for use by the action handler.

### Action handling

Wire `userNotificationCenter(_:didReceive:withCompletionHandler:)`. `NotificationService` holds weak references to `AppState` and `StatusItemController`.

| Action identifier | Behavior |
|---|---|
| `VIEW` | `StatusItemController.openPopover()` + `appState.navigation = .profile(name)` |
| `RESTART_SESSION` | Call `open_session` for the profile (handles attached profiles correctly) |
| `CLOSE_SESSION` | Call `close_session` with session_id |
| `RESTART_DAEMON` | Call `AppState.restartDaemon()` |

---

## Component 4: Notification Settings (Swift)

**File:** `Sources/PagerunnerBar/Views/SettingsView.swift` (extend existing)

New "Notifications" section. Global settings at top, per-profile section below (collapsed list of profiles with individual toggles).

### Settings

| Setting | Default | Scope | UserDefaults key |
|---|---|---|---|
| Notify on daemon health | On | Global | `notif.daemonHealth` |
| Explicit `notify` tool | Always on | Global | *(no toggle — grayed out with label "Always delivered")* |
| Notify on session crash | On | Per-profile | `notif.<name>.crash` |
| Notify on agent idle | On | Per-profile | `notif.<name>.idle` |
| Idle threshold | 30 min | Per-profile | `notif.<name>.idleMinutes` (15 / 30 / 60) |
| Notify on session start | Off | Per-profile | `notif.<name>.start` |

**Defaults behaviour:** Agent profiles default all-on; personal profiles default crash + idle only. Applied at first-read via `UserDefaults.register(defaults:)`.

---

## Data Flow: `notify` tool end-to-end

1. Claude calls `notify(title: "Done", level: "info", session_id: "abc")`
2. MCP server resolves `profile_name = "growthmate"` from session store
3. Writes row to `notifications` table, returns `{"ok": true}`
4. Within ≤2s, `NotificationPoller` calls `list_notifications`
5. Daemon returns the row, marks it `delivered = true`
6. `NotificationPoller` checks `notif.growthmate.explicit` — always on
7. Calls `NotificationService.notifyExplicit(...)` with no sound (level: info)
8. macOS notification appears: "Done" with **View** button
9. User taps View → `openPopover()` + `navigation = .profile("growthmate")`

---

## Error Handling

- `list_notifications` returns empty if DB is unreachable — poller retries next cycle, no crash
- `notify` called with unknown `session_id` — writes row with `profile_name = nil`, notification fires without deep-link, View opens Overview
- Notification fired when menubar lacks permission — `UNUserNotificationCenter.add()` fails silently; permission is requested on first launch
- `RESTART_SESSION` action for a crashed session that no longer exists — `open_session` creates a new one, same as normal flow

---

## Out of Scope

- **Monitoring/watch** (`watch_for` tool with URL/content callbacks) — separate spec
- **Task-level auto-notifications** (navigate timeout, click failed) — intentionally excluded to protect signal-to-noise ratio
- **Notification history** in the menubar UI — future feature
- **Rich notifications** (images, progress) — future feature
