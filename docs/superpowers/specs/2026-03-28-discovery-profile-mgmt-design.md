# Auto-Discovery + Profile Management Design Spec

**Date:** 2026-03-28
**Status:** Approved
**Scope:** macOS menu bar app (`apps/menubar/`)

---

## Goal

Two complementary features:
1. **Profile Management** — rename and remove existing profiles from within the app (no manual config.toml editing required).
2. **Auto-Discovery** — surface unmanaged Chrome instances running on the machine, let the user attach to them with one click, and optionally save them as named profiles.

---

## Background

Profiles are stored as `[[profiles]]` entries in `~/.pagerunner/config.toml`. Currently users must edit this file manually to rename or remove a profile. There is also no way to discover Chrome instances that weren't launched by pagerunner (e.g. Chrome started with `--remote-debugging-port`, or container Chrome forwarded via gvproxy/Podman). The `attach_session` MCP tool already handles these cases — the gap is discoverability and UX.

---

## Feature 1: Profile Management

### Rename a Profile

**Entry points:**
- Right-click (context menu) on any profile row in Overview → **Rename…**
- Settings → Profiles section → **Rename** button on the row

**Flow:**
1. A small sheet appears with a pre-filled text input containing the current `display_name`.
2. User edits the name and confirms (OK / Return) or cancels (Cancel / Esc).
3. On confirm:
   - Update `display_name` in the matching `[[profiles]]` block in `~/.pagerunner/config.toml` via `ConfigEditor.renameProfile`.
   - Restart the daemon via `restartDaemon()` (same pattern as `AddProfileView.appendProfileToConfig`). There is no `reload_config` MCP tool in the daemon — a restart is the only safe way to pick up config changes.
   - Refresh the profile list in AppState after the daemon restarts.

**Validation:** Name must be non-empty after trimming. OK button disabled until valid.

### Remove a Profile

**Entry points:**
- Right-click on profile row in Overview → **Remove…**
- Settings → Profiles section → **Remove** button on the row

**Flow:**
1. Confirmation sheet: *"Remove [name]? Any active sessions for this profile will be closed."*
2. On confirm:
   - Read `appState.sessions` (in-memory cache is authoritative; no extra `list_sessions` call needed since AppState is refreshed on every poll cycle).
   - For each session where `session.profile == profile.name`, call `close_session`. If a `close_session` call fails (e.g. Chrome already gone), log the error but continue — do not abort the removal.
   - Remove the matching `[[profiles]]` block from `~/.pagerunner/config.toml` via `ConfigEditor.removeProfile`.
   - Restart the daemon via `restartDaemon()`.
   - Remove the profile from AppState.

**Destructive action styling:** Remove button uses red/danger color in both the context menu and the Settings list.

---

## Feature 2: Auto-Discovery

### What Gets Discovered

Two categories of unmanaged Chrome:

| Category | Detection method | Example |
|----------|-----------------|---------|
| TCP debug port | HTTP probe `GET /json/version` on ports 9222–9239 | Chrome with `--remote-debugging-port=9225` |
| VM/container Chrome | Same TCP probe; owning process is `gvproxy` | Podman Linux container Chrome forwarded to host |

**Not discovered:** Chrome launched by pagerunner via `--remote-debugging-pipe` (pipe-based, no HTTP port) — these appear as normal sessions already.

**Port range rationale:** 9222 is Chrome's default debug port. The range 9222–9239 (18 ports) covers common manual usage patterns (e.g. multiple profiles each on successive ports). Broader scanning (e.g. full 1024–65535) is deferred — the narrow range keeps probe latency bounded.

**Self-discovery exclusion:** All 18 ports are probed. After probes complete, the results are post-filtered in `AppState`: any discovered instance whose `/json/version` `webSocketDebuggerUrl` matches a URL already present in a live session is dropped before updating `discoveredInstances`. This means `DiscoveryService.probe()` takes no `excludingPorts` parameter — filtering is done by the caller (AppState) after the fact.

**Port already covered by a profile:** If a discovered port's `/json/version` response matches the `user_data_dir` of an existing `[[profiles]]` entry, suppress that row — the user already has it as a named profile.

### Discovery Timing

- Triggered on panel open (NSPopover `popoverWillShow`).
- Runs as a background `async` Task; does not block the panel.
- Results cached for 30 seconds — subsequent panel opens within the window use cached results.
- Cache invalidated on manual "Refresh" or after 30s.

### Probe Implementation Details

- All 18 port probes run **concurrently** via `withTaskGroup` to keep total latency low (worst-case ≈ 1 × timeout, not 18×).
- Per-probe HTTP timeout: **400 ms** (via `URLSession` with `timeoutIntervalForRequest: 0.4`). Ports that don't respond within 400ms are silently skipped.
- A port that responds with non-200 status or malformed JSON is silently skipped (not shown, not logged as an error).
- gvproxy detection: after a port responds, run `lsof -i tcp:<port> -n -P` and check if the process name or parent process name contains `gvproxy`. If yes, `isVM = true`.

### Overview Layout

Discovered instances appear **inline below the last profile row** (macOS-native pattern, matches WiFi "Other Networks"):

```
┌─────────────────────────────────────┐
│ ● Pagerunner is live  2w · 5 tabs   │
├─────────────────────────────────────┤
│ [avatar] growthmate.io              │
│          stas@growthmate.io  [1w] › │
├─────────────────────────────────────┤
│ [avatar] luminary.tools             │
│          stas@luminary.tools      › │
├─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┤  ← hairline separator
│ [⊙] :9225  Chrome (no profile)     │  ← 70% opacity
│     3 tabs                [Attach]  │
│ [⊙] :9222  Chrome          [VM]    │  ← VM badge for gvproxy
│     2 tabs                [Attach]  │
└─────────────────────────────────────┘
```

**Discovered row styling:**
- `opacity: 0.7` on the row
- Icon: plain browser circle (⊙), no colored avatar gradient
- Primary label: `:9225` or process name; subtitle: tab count
- **VM badge**: small pill badge shown when owning process is `gvproxy` (indicates container Chrome)
- **Attach button**: blue, right-aligned; tapping calls `attach_session`

**Empty state:** If no instances are discovered, the hairline separator and discovered rows are not rendered at all.

### Attach Flow

1. User taps **Attach** on a discovered row.
2. App calls `attach_session` with `debug_port: <port>` and an auto-generated label (e.g. `"chrome-9225"`).
3. On success: the attached session appears in the normal sessions list. The discovered row updates its `attachState` to `.attached` and renders a grayed-out "Attached" label in place of the Attach button.
4. On failure: `attachState` becomes `.failed("Could not connect")` — brief inline error label shown on the row.

### Remote Flag

Sessions attached from ports where the owning process is `gvproxy` are flagged as **remote** (`isRemote = true` in AppState). This flag is set when the attach succeeds, derived from the `DiscoveredInstance.isVM` value.

Remote sessions:
- Display a small "VM" badge in the session header.
- Hide the **Focus Tab** action (requires AppleScript, which cannot cross VM boundary).
- All other actions (navigate, screenshot, evaluate, etc.) work normally.

### Save as Profile

After attaching, the user may want the connection to persist as a named profile entry so it appears automatically on the next panel open.

**Important schema constraint:** Normal `[[profiles]]` entries have a `user_data_dir` field that pagerunner uses to launch Chrome. A port-attached profile has no `user_data_dir` — it is a bookmark to a running Chrome, not a launchable one. Therefore:
- The saved entry uses `kind = "attached"` and stores `debug_port` instead of `user_data_dir`.
- The app handles `kind = "attached"` profiles by calling `attach_session` (not `open_session`) when the user taps the profile row.
- The daemon does not need changes — `attach_session` already accepts `debug_port`.
- The profile row shows a "plug" icon instead of a colored avatar to signal it is attach-only.

**Entry point:** Right-click on the newly-attached session's profile row → **Add to Profiles…**

**Flow:**
1. Same rename sheet — text input pre-filled with the auto-generated label (e.g. `"chrome-9225"`).
2. On confirm: `ConfigEditor.addAttachedProfile(name:displayName:port:)` appends a new `[[profiles]]` block with `kind = "attached"` and `debug_port = <port>` to `config.toml`. Daemon is restarted to pick up the new entry.

**config.toml example for an attached profile:**
```toml
[[profiles]]
name = "chrome-9225"
display_name = "Chrome :9225"
kind = "attached"
debug_port = 9225
```

**Required Rust backend change:** `src/config.rs` `ChromeProfile` struct currently has `user_data_dir: String` (required). This must be changed to `user_data_dir: Option<String>` and `debug_port: Option<u16>` added. This is a backward-compatible change — existing profiles still deserialize correctly since `user_data_dir` will just be `Some(...)`. The daemon's `open_session` logic must check: if `kind == "attached"`, route through `attach_session` using `debug_port` instead of launching Chrome via `user_data_dir`.

---

## Settings Page Changes

Add a **Profiles** section above the existing **Behavior** section:

```
PROFILES
  ● growthmate.io          [Rename]  [Remove]
  ● luminary.tools         [Rename]  [Remove]

BEHAVIOR
  ⚡ Launch at login               Toggle
  ...
```

- Rename → same sheet flow as context menu.
- Remove → same confirmation flow as context menu.
- This section is purely a convenience for users who prefer Settings over right-click.

---

## Architecture

### Required Rust Changes

| File | Change |
|------|--------|
| `src/config.rs` | `ChromeProfile.user_data_dir: String` → `Option<String>`; add `debug_port: Option<u16>` |
| `src/mcp_server.rs` | `open_session` handler: if `profile.kind == Some("attached")`, call `attach_session` logic using `debug_port` instead of launching Chrome |

The Rust changes are backward-compatible. Existing profiles with `user_data_dir` set continue to work unchanged. Tests for the new `kind = "attached"` deserialization must be added to `src/config.rs`.

### New/Modified Swift Files

| File | Change |
|------|--------|
| `Sources/PagerunnerCore/Models.swift` | Add `isRemote: Bool` to `Session`; add `DiscoveredInstance` model |
| `Sources/PagerunnerCore/DiscoveryService.swift` | **New** — port probe + gvproxy detection, 30s cache |
| `Sources/PagerunnerCore/DaemonClient.swift` | Add `attachSession` call (no `reloadConfig` — daemon restart used instead) |
| `Sources/PagerunnerBar/AppState.swift` | Add `discoveredInstances: [DiscoveredInstance]`, trigger discovery on panel open |
| `Sources/PagerunnerBar/Views/OverviewView.swift` | Render discovered rows below profile list |
| `Sources/PagerunnerBar/Views/ProfileRowView.swift` | **New** — extract `ProfileRow` from `OverviewView.swift` into its own file, then add right-click context menu (Rename / Remove) |
| `Sources/PagerunnerBar/Views/SettingsView.swift` | Add Profiles section |
| `Sources/PagerunnerBar/Views/RenameSheet.swift` | **New** — reusable sheet with text input, OK/Cancel |
| `Sources/PagerunnerBar/ConfigEditor.swift` | **New** — read/write `~/.pagerunner/config.toml` for rename/remove/addAttached operations |

### DiscoveredInstance Model

```swift
struct DiscoveredInstance: Identifiable {
    let id: String           // "port-9225"
    let port: Int
    let tabCount: Int
    let isVM: Bool           // true if owning process is gvproxy
    var attachState: AttachState  // .idle | .attaching | .attached | .failed(String)
}
```

### DiscoveryService

```swift
actor DiscoveryService {
    private var cache: [DiscoveredInstance] = []
    private var lastProbeAt: Date?
    private let cacheTTL: TimeInterval = 30
    private let probeTimeout: TimeInterval = 0.4
    private let portRange: ClosedRange<Int> = 9222...9239

    func probe() async -> [DiscoveredInstance]
    // runs all probes concurrently via withTaskGroup
    // each probe: URLSession GET /json/version with 400ms timeout
    // non-200 / malformed JSON / timeout → skip silently
    // on success: lsof to detect gvproxy parent
    // returns cached result if within TTL
    // post-filtering (self-discovery exclusion) is done by AppState after calling probe()
}
```

### ConfigEditor

All methods operate on `~/.pagerunner/config.toml`. Tests pass a `URL` parameter pointing to a temp file so no test ever touches the real config.

```swift
struct ConfigEditor {
    static func renameProfile(name: String, newDisplayName: String, configURL: URL = .pagerunnerConfig) throws
    static func removeProfile(name: String, configURL: URL = .pagerunnerConfig) throws
    static func addAttachedProfile(name: String, displayName: String, port: Int, configURL: URL = .pagerunnerConfig) throws
    // reads TOML, patches the relevant [[profiles]] block, writes back atomically
    // atomic write: write to .tmp file, then rename to replace original (avoids partial writes)
}

extension URL {
    static let pagerunnerConfig = URL(fileURLWithPath: NSHomeDirectory())
        .appendingPathComponent(".pagerunner/config.toml")
}
```

---

## Out of Scope

- Discovery of Chrome via `--remote-debugging-pipe` processes not owned by pagerunner (complex IPC; deferred).
- Profile reordering.
- Multiple display names / aliases per profile.
- Windows / Linux support (macOS only).
- Daemon-side `reload_config` tool (restart is sufficient for v1).

---

## Testing

### Unit Tests (PagerunnerCoreTests — no live daemon)

All `ConfigEditor` tests use `FileManager.default.temporaryDirectory` for the config URL — never `~/.pagerunner/config.toml`.

- `ConfigEditor.renameProfile`: updates correct `[[profiles]]` block, leaves others untouched.
- `ConfigEditor.removeProfile`: removes correct block, leaves others untouched.
- `ConfigEditor.addAttachedProfile`: appends block with `kind = "attached"` and `debug_port`.
- `DiscoveryService` with mocked `URLSession`: found instances, empty result, gvproxy detection, timeout handling, cache hit within 30s, cache miss after 30s.
- `DiscoveredInstance` model: all `AttachState` transitions.

### Manual QA Checklist

- [ ] Right-click profile → Rename → name updates in Overview and daemon
- [ ] Right-click profile → Remove → confirmation shown, profile gone after confirm
- [ ] Settings Profiles section lists all profiles with working Rename/Remove
- [ ] Panel open triggers background discovery (no UI freeze, panel opens immediately)
- [ ] Discovered row appears for Chrome on port 9225 (non-pagerunner)
- [ ] VM badge shown for gvproxy-forwarded port
- [ ] Attach button → session appears in session list; discovered row shows "Attached"
- [ ] Remote session hides Focus Tab action
- [ ] Right-click attached session → Add to Profiles → profile with plug icon persists across app restart
- [ ] Reopen app → attached-kind profile row shows plug icon; tapping attaches session
- [ ] Cache: second panel open within 30s skips re-probe
- [ ] No discovered section rendered when nothing found
- [ ] Port occupied by non-HTTP service does not hang panel open (400ms timeout)
