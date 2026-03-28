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
   - Update `display_name` in the matching `[[profiles]]` block in `~/.pagerunner/config.toml`.
   - Call `reloadConfig()` on the daemon (or restart it) so the change takes effect without a full quit.
   - Refresh the profile list in AppState.

**Validation:** Name must be non-empty after trimming. OK button disabled until valid.

### Remove a Profile

**Entry points:**
- Right-click on profile row in Overview → **Remove…**
- Settings → Profiles section → **Remove** button on the row

**Flow:**
1. Confirmation sheet: *"Remove [name]? Any active sessions for this profile will be closed."*
2. On confirm:
   - Close any live sessions belonging to this profile (call `close_session` for each).
   - Remove the matching `[[profiles]]` block from `~/.pagerunner/config.toml`.
   - Restart the daemon via `restartDaemon()` (already implemented in `AddProfileView`).
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

### Discovery Timing

- Triggered on panel open (NSPopover `popoverWillShow`).
- Runs as a background `async` Task; does not block the panel.
- Results cached for 30 seconds — subsequent panel opens within the window use cached results.
- Cache invalidated on manual "Refresh" or after 30s.

### Overview Layout

Discovered instances appear **inline below the last profile row** (Option B — macOS-native pattern, matches WiFi "Other Networks"):

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
3. On success: the attached session appears in the normal sessions list. The discovered row is removed (or grayed out with "Attached" label).
4. On failure: brief inline error label on the row ("Could not connect").

### Remote Flag

Sessions attached from ports where the owning process is `gvproxy` are flagged as **remote** (`is_remote = true` in AppState).

Remote sessions:
- Display a small "VM" badge in the session header.
- Hide the **Focus Tab** action (requires AppleScript, which cannot cross VM boundary).
- All other actions (navigate, screenshot, evaluate, etc.) work normally.

### Save as Profile

After attaching, the user may want to persist the connection as a named profile so it appears automatically next time.

**Entry point:** Right-click on the newly-attached session's profile row → **Add to Profiles…**

**Flow:**
1. Same rename sheet — text input pre-filled with the auto-generated label (e.g. `"chrome-9225"`).
2. On confirm: write a new `[[profiles]]` entry to `config.toml` with the chosen name and the debug port as metadata. Reload profiles.

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

### New/Modified Swift Files

| File | Change |
|------|--------|
| `Sources/PagerunnerCore/Models.swift` | Add `isRemote: Bool` to `Session`; add `DiscoveredInstance` model |
| `Sources/PagerunnerCore/DiscoveryService.swift` | **New** — port probe + gvproxy detection, 30s cache |
| `Sources/PagerunnerCore/DaemonClient.swift` | Add `attachSession`, `reloadConfig` calls |
| `Sources/PagerunnerBar/AppState.swift` | Add `discoveredInstances: [DiscoveredInstance]`, trigger discovery on panel open |
| `Sources/PagerunnerBar/Views/OverviewView.swift` | Render discovered rows below profile list |
| `Sources/PagerunnerBar/Views/ProfileRowView.swift` | Add right-click context menu (Rename / Remove) |
| `Sources/PagerunnerBar/Views/SettingsView.swift` | Add Profiles section |
| `Sources/PagerunnerBar/Views/RenameSheet.swift` | **New** — reusable sheet with text input, OK/Cancel |
| `Sources/PagerunnerBar/ConfigEditor.swift` | **New** — read/write `~/.pagerunner/config.toml` for rename/remove operations |

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

    func probe() async -> [DiscoveredInstance]
    // probes ports 9222–9239, checks /json/version, runs ps to detect gvproxy parent
    // returns cached result if within TTL
}
```

### ConfigEditor

```swift
struct ConfigEditor {
    static func renameProfile(name: String, newDisplayName: String) throws
    static func removeProfile(name: String) throws
    static func addProfile(name: String, displayName: String, port: Int) throws
    // reads ~/.pagerunner/config.toml, patches the relevant [[profiles]] block, writes back
}
```

---

## Out of Scope

- Discovery of Chrome via `--remote-debugging-pipe` processes not owned by pagerunner (complex IPC; deferred).
- Profile reordering.
- Multiple display names / aliases per profile.
- Windows / Linux support (macOS only).

---

## Testing

### Unit Tests (PagerunnerCoreTests — no live daemon)

- `DiscoveryService` with mocked HTTP responses: found instances, empty, gvproxy detection.
- `ConfigEditor`: rename updates correct block, remove deletes correct block, add appends.
- `DiscoveredInstance` model encoding/decoding.

### Manual QA Checklist

- [ ] Right-click profile → Rename → name updates in Overview and daemon
- [ ] Right-click profile → Remove → confirmation shown, profile gone after confirm
- [ ] Settings Profiles section lists all profiles with working Rename/Remove
- [ ] Panel open triggers background discovery (no UI freeze)
- [ ] Discovered row appears for Chrome on port 9225 (non-pagerunner)
- [ ] VM badge shown for gvproxy-forwarded port
- [ ] Attach button → session appears in session list
- [ ] Remote session hides Focus Tab action
- [ ] Save as profile → profile persists across app restart
- [ ] Cache: second panel open within 30s skips re-probe
- [ ] No discovered section rendered when nothing found
