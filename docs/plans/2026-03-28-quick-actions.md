# Quick Actions — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add right-click context menus to session (window) rows and tab rows in the macOS menu bar app, making every visible item directly actionable without a terminal.

**Architecture:** SwiftUI `.contextMenu` modifiers on `TabRowView` and the active session header in `SessionBlockView`. A small `originFrom(url:)` helper extracts the scheme+host from a tab URL for `save_snapshot`. All backend tools are already implemented — this is pure UI wiring.

**Tech Stack:** Swift 6, SwiftUI, macOS 14+, Swift Testing framework (`@Suite`/`@Test`), `NSPasteboard`, `DaemonClient` via `@Environment(\.daemonClient)`

---

## Spec

`docs/specs/2026-03-28-quick-actions-design.md`

## Key Files

All paths are relative to the **feature branch worktree root** (`.worktrees/feature/site-intelligence-tier/`). The menubar app lives under `apps/menubar/`.

> **Shell note:** All `swift build` and `swift test` commands must be run from inside `apps/menubar/`. Either `cd` there first, or prefix every command with `cd .worktrees/feature/site-intelligence-tier/apps/menubar &&`.

| File | Change |
|------|--------|
| `apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift` | Add `.contextMenu` to active session header; add `@State private var showCloseConfirm`; `TabRowView` already exists here, add `.contextMenu` to it |
| `apps/menubar/Sources/PagerunnerCore/URLHelpers.swift` | **Create** — `originFrom(url:)` helper |
| `apps/menubar/Tests/PagerunnerCoreTests/URLHelpersTests.swift` | **Create** — unit tests for `originFrom` |
| `apps/menubar/Tests/PagerunnerCoreTests/AppStateTests.swift` | **Create** — unit test for `AppState.checkpointsFor(profile:)` |

---

## Task 1: Unit tests for `checkpointsFor` and URL origin extraction

Write all tests before any implementation. `URLHelpersTests` will fail to compile until Task 2 adds the helper.

**Files:**
- Create: `apps/menubar/Tests/PagerunnerCoreTests/URLHelpersTests.swift`
- Create: `apps/menubar/Tests/PagerunnerCoreTests/AppStateTests.swift`

- [ ] **Step 1: Write `URLHelpersTests.swift`**

```swift
import Testing
@testable import PagerunnerCore

@Suite("URLHelpers")
struct URLHelpersTests {

    @Test("extracts scheme+host from https URL with path")
    func extractsOriginFromHTTPSWithPath() {
        #expect(originFrom(url: "https://linear.app/pagerunner/issues") == "https://linear.app")
    }

    @Test("extracts scheme+host from URL without path")
    func extractsOriginFromURLNoPath() {
        #expect(originFrom(url: "https://app.growthmate.io") == "https://app.growthmate.io")
    }

    @Test("handles http scheme with port")
    func extractsOriginHTTPWithPort() {
        #expect(originFrom(url: "http://localhost:3000/dashboard") == "http://localhost:3000")
    }

    @Test("returns nil for chrome:// URLs")
    func returnsNilForChromeURLs() {
        #expect(originFrom(url: "chrome://newtab") == nil)
    }

    @Test("returns nil for empty string")
    func returnsNilForEmptyString() {
        #expect(originFrom(url: "") == nil)
    }

    @Test("returns nil for malformed URL")
    func returnsNilForMalformed() {
        #expect(originFrom(url: "not a url") == nil)
    }
}
```

- [ ] **Step 2: Read `Models.swift` to verify `Checkpoint` field types before writing the test**

```bash
cat .worktrees/feature/site-intelligence-tier/apps/menubar/Sources/PagerunnerCore/Models.swift | grep -A 15 "struct Checkpoint"
```

Note the exact field names and types — particularly whether `savedAt` is `Date` or `Int` and whether `name` is `String` or `String?`. Use whatever types the model actually declares.

- [ ] **Step 3: Write `AppStateTests.swift`** (adjust `savedAt` type to match what you saw in Step 2)

```swift
import Testing
@testable import PagerunnerBar
@testable import PagerunnerCore

@Suite("AppState")
@MainActor
struct AppStateTests {

    @Test("checkpointsFor returns checkpoints for matching profile only")
    func checkpointsForFiltersCorrectly() {
        let state = AppState()
        // Use Int unix timestamp for savedAt (adjust if model uses Date)
        let cp1 = Checkpoint(checkpointId: "cp1", name: "morning", profile: "personal",
                             savedAt: 1_700_000_000, tabCount: 2, origins: ["https://linear.app"])
        let cp2 = Checkpoint(checkpointId: "cp2", name: "afternoon", profile: "personal",
                             savedAt: 1_700_001_000, tabCount: 1, origins: ["https://github.com"])
        let cp3 = Checkpoint(checkpointId: "cp3", name: "agent-save", profile: "agent-1",
                             savedAt: 1_700_002_000, tabCount: 3, origins: [])
        state.checkpoints = ["personal": [cp1, cp2], "agent-1": [cp3]]

        let result = state.checkpointsFor(profile: "personal")
        #expect(result.count == 2)
        #expect(result.map(\.checkpointId).contains("cp1"))
        #expect(result.map(\.checkpointId).contains("cp2"))
    }

    @Test("checkpointsFor returns empty array when profile has no checkpoints")
    func checkpointsForReturnsEmptyForMissingProfile() {
        let state = AppState()
        state.checkpoints = [:]
        #expect(state.checkpointsFor(profile: "nonexistent").isEmpty)
    }
}
```

- [ ] **Step 4: Run tests to confirm `URLHelpersTests` fails to compile (expected) and `AppStateTests` passes**

```bash
cd .worktrees/feature/site-intelligence-tier/apps/menubar
swift test --filter URLHelpersTests 2>&1 | head -20
swift test --filter AppStateTests
```

Expected: `URLHelpersTests` fails with "use of unresolved identifier 'originFrom'". `AppStateTests` passes (2 tests).

- [ ] **Step 5: Commit failing tests**

```bash
git add apps/menubar/Tests/PagerunnerCoreTests/URLHelpersTests.swift
git add apps/menubar/Tests/PagerunnerCoreTests/AppStateTests.swift
git commit -m "test: add unit tests for URL origin helper and AppState.checkpointsFor"
```

---

## Task 2: Implement `originFrom(url:)` helper

**Files:**
- Create: `apps/menubar/Sources/PagerunnerCore/URLHelpers.swift`

- [ ] **Step 1: Create the helper**

```swift
// apps/menubar/Sources/PagerunnerCore/URLHelpers.swift
import Foundation

/// Extracts the origin (scheme + host + optional port) from a URL string.
/// Returns nil for non-http(s) URLs or malformed strings.
/// Examples:
///   "https://linear.app/foo/bar" → "https://linear.app"
///   "http://localhost:3000/x"    → "http://localhost:3000"
///   "chrome://newtab"            → nil
public func originFrom(url urlString: String) -> String? {
    guard let url = URL(string: urlString),
          let scheme = url.scheme,
          scheme == "https" || scheme == "http",
          let host = url.host else { return nil }
    if let port = url.port {
        return "\(scheme)://\(host):\(port)"
    }
    return "\(scheme)://\(host)"
}
```

- [ ] **Step 2: Run all tests to confirm they pass**

```bash
cd .worktrees/feature/site-intelligence-tier/apps/menubar
swift test --filter URLHelpersTests
swift test --filter AppStateTests
```

Expected: all 8 tests pass (6 URL + 2 AppState).

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerCore/URLHelpers.swift
git commit -m "feat: add originFrom(url:) helper for snapshot origin extraction"
```

---

## Task 3: Add context menu to `TabRowView`

`TabRowView` is defined at the bottom of `SessionBlockView.swift`. Add `.contextMenu` to its `body`.

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift`

> **Before you start:** Read `SessionBlockView.swift` fully. The relevant section is the `Button { controller.focusTab(...); controller.closePopover() } label: { HStack { ... } }` in `TabRowView.body`. The `.contextMenu` goes after `.onHover { isHovered = $0 }`.

**Tab context menu structure:**
```
Focus tab
────────────
Snapshot this tab
Copy URL
────────────
Close tab          ← destructive (red), disabled when only 1 tab
```

- [ ] **Step 1: Add context menu to `TabRowView.body`**

After `.onHover { isHovered = $0 }`, add:

```swift
.contextMenu {
    Button("Focus tab") {
        controller.focusTab(sessionId: sessionId, targetId: tab.targetId)
        controller.closePopover()
    }

    Divider()

    Button("Snapshot this tab") {
        guard let origin = originFrom(url: tab.url) else { return }
        Task { @MainActor in
            _ = try? await daemon.call(
                tool: "save_snapshot",
                args: ["session_id": sessionId, "target_id": tab.targetId, "origin": origin]
            )
        }
    }

    Button("Copy URL") {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(tab.url, forType: .string)
    }

    Divider()

    // Hidden (not disabled) when only one tab — .disabled() is unreliable
    // on context menu buttons in SwiftUI on macOS.
    if tabs.count > 1 {
        Button("Close tab", role: .destructive) {
            Task { @MainActor in
                _ = try? await daemon.call(
                    tool: "close_tab",
                    args: ["session_id": sessionId, "target_id": tab.targetId]
                )
            }
        }
    }
}
```

`daemon`, `tabs`, `sessionId`, and `controller` are all already available on `TabRowView`.

- [ ] **Step 2: Build to confirm it compiles**

```bash
cd .worktrees/feature/site-intelligence-tier/apps/menubar
swift build
```

Expected: Build succeeded.

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift
git commit -m "feat: add right-click context menu to tab rows"
```

---

## Task 4: Add context menu + close confirmation to `SessionBlockView`

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift`

> **Before you start:** Re-read `SessionBlockView.swift`. Find the active window `HStack` containing `"Window \(index + 1)"` and the chevron. It ends with `.contentShape(Rectangle()).onTapGesture { ... }`. The `.contextMenu` goes after `.onTapGesture`. The `.confirmationDialog` goes on the outer `VStack`.
>
> `SessionBlockView` already has `@Environment(\.daemonClient) private var daemon` — no need to add it.
> `AppState` already has `checkpointsFor(profile:)` — no need to add it (the unit test in Task 1 tests existing code).

**Session context menu structure:**
```
Save checkpoint
Restore checkpoint ▸  (hidden when checkpointsForSession is empty)
────────────────────
View session log
────────────────────
Close session         ← destructive (red)
```

- [ ] **Step 1: Add `@State` property and computed helper to `SessionBlockView`**

After `@State private var isCollapsed = false`, add:

```swift
@State private var showCloseConfirm = false

private var checkpointsForSession: [Checkpoint] {
    appState.checkpointsFor(profile: session.profile)
}
```

Using a computed property avoids a `let` binding inside the `@ViewBuilder` closure, which can cause compiler errors in some Xcode versions.

- [ ] **Step 2: Add `.contextMenu` to the active session header `HStack`**

After `.onTapGesture { withAnimation(.easeInOut(duration: 0.15)) { isCollapsed.toggle() } }`, add:

```swift
.contextMenu {
    Button("Save checkpoint") {
        Task { @MainActor in
            _ = try? await daemon.call(
                tool: "save_session_checkpoint",
                args: ["session_id": session.id]
            )
        }
    }

    if !checkpointsForSession.isEmpty {
        Menu("Restore checkpoint…") {
            ForEach(checkpointsForSession, id: \.checkpointId) { cp in
                Button(cp.name) {
                    Task { @MainActor in
                        _ = try? await daemon.call(
                            tool: "restore_session_checkpoint",
                            args: [
                                "session_id": session.id,
                                "checkpoint_id": cp.checkpointId
                            ]
                        )
                    }
                }
            }
        }
    }

    Divider()

    Button("View session log") {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(
            "pagerunner audit --session \(session.id)",
            forType: .string
        )
        // Post a "Command copied to clipboard" notification using the same
        // NotificationService pattern used elsewhere in the app.
        // Look up how NotificationService is called in PollingService.swift
        // or App.swift and follow that pattern here.
    }

    Divider()

    Button("Close session", role: .destructive) {
        showCloseConfirm = true
    }
}
```

> **Note on `cp.name`:** If the `Checkpoint` model has `name: String?` (optional), use `cp.name ?? "Saved \(cp.checkpointId.prefix(6))"` as a fallback. Check `Models.swift` to confirm.

- [ ] **Step 3: Add `.confirmationDialog` to the outer `VStack`**

At the end of `SessionBlockView.body`'s outer `VStack`, add:

```swift
.confirmationDialog(
    "Close session?",
    isPresented: $showCloseConfirm,
    titleVisibility: .visible
) {
    Button("Close session", role: .destructive) {
        Task { @MainActor in
            _ = try? await daemon.call(
                tool: "close_session",
                args: ["session_id": session.id]
            )
        }
    }
    Button("Cancel", role: .cancel) {}
} message: {
    Text("This will close all tabs in Window \(index + 1).")
}
```

- [ ] **Step 4: Add `import AppKit` if not present**

`NSPasteboard` requires `AppKit`. Check the imports at the top of `SessionBlockView.swift` — add `import AppKit` if it's not already there.

- [ ] **Step 5: Build to confirm it compiles**

```bash
cd .worktrees/feature/site-intelligence-tier/apps/menubar
swift build
```

Expected: Build succeeded.

- [ ] **Step 6: Run all unit tests**

```bash
swift test
```

Expected: all tests pass (8 new + existing tests). If the count doesn't match, investigate before proceeding.

- [ ] **Step 7: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift
git commit -m "feat: add right-click context menu to session rows with close confirmation"
```

---

## Task 5: Manual QA

Run against a live `pagerunner daemon` with at least one active session.

**Setup:**
```bash
pagerunner daemon &
pagerunner open-session personal
```

Open the menu bar app. Navigate to the profile with the active session.

- [ ] **QA-1:** Right-click Window 1 → Save checkpoint → checkpoint appears in Restore submenu on next right-click
- [ ] **QA-2:** Right-click Window 1 → Restore checkpoint → pick one → tabs restore
- [ ] **QA-3:** Right-click Window 1 with no checkpoints → "Restore checkpoint" item is absent
- [ ] **QA-4:** Right-click Window 1 → View session log → clipboard contains `pagerunner audit --session <id>` (paste to verify)
- [ ] **QA-5:** Right-click Window 1 → Close session → confirmation dialog appears → confirm → session disappears
- [ ] **QA-6:** Right-click a tab → Focus tab → Chrome comes to front on that tab
- [ ] **QA-7:** Right-click a tab → Snapshot this tab → no crash; verify with `pagerunner audit --tail 5`
- [ ] **QA-8:** Right-click a tab → Copy URL → paste into text editor to verify
- [ ] **QA-9:** Right-click a tab → Close tab → tab disappears from list
- [ ] **QA-10:** With only one tab open, right-click → "Close tab" item is **absent** from the menu (hidden, not greyed out)
- [ ] **QA-11:** Stop daemon (`pkill -f "pagerunner daemon"`) → right-click → Save checkpoint → no crash
- [ ] **QA-12:** Force a restore failure by passing a deleted checkpoint ID directly via CLI → menu bar shows no crash
- [ ] **QA-13:** Complete QA-1 (save checkpoint) → right-click same session again → Restore submenu is now visible

- [ ] **Record results in `docs/test-runs/YYYY-MM-DD-run-N.md`**

- [ ] **Final commit**

```bash
git add docs/test-runs/
git commit -m "docs: record quick actions manual QA test run"
```
