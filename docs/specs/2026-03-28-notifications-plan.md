# Pagerunner Notification System — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `notify` MCP tool + automatic agent lifecycle notifications + action-button deep-links into the menubar.

**Architecture:** The Rust daemon stores notifications in ReDB; the Swift menubar polls `list_notifications` every 2s and fires them via `UNUserNotificationCenter`. Daemon crash and agent idle are detected client-side and bypass the DB. See `docs/specs/2026-03-28-notifications-design.md` for full design.

**Tech Stack:** Rust (redb, serde_json), Swift 6 (`@MainActor`, `UNUserNotificationCenter`, `UserDefaults`)

---

## File Map

**Create:**
- `src/notification.rs` — `Notification` struct, `push_notification()`, `drain_notifications()`
- `apps/menubar/Sources/PagerunnerBar/NotificationPoller.swift` — 2s poll loop
- `apps/menubar/Sources/PagerunnerBar/NotificationSettings.swift` — UserDefaults wrapper

**Modify:**
- `src/mcp_server.rs` — add `notify` to tools/list + dispatch; add `list_notifications` to dispatch only
- `apps/menubar/Sources/PagerunnerBar/NotificationService.swift` — configure(), notifyExplicit(), updated notifyAgentIdle(), full registerCategories(), action handling
- `apps/menubar/Sources/PagerunnerBar/App.swift` — instantiate NotificationPoller, wire configure(), add idle detection, fire notifyDaemonStopped() on unexpected stop
- `apps/menubar/Sources/PagerunnerBar/Views/SettingsView.swift` — Notifications section

---

## Task 1: Rust — `notification.rs` module

**Files:**
- Create: `src/notification.rs`
- Modify: `src/mcp_server.rs` (add `mod notification;`)

The `Db` struct stores everything in one KV table with compound keys. Notifications use key format `{timestamp_micros}:{uuid}` to allow scanning and natural-order sorting.

- [ ] **Step 1: Write failing tests in `src/notification.rs`**

Create `src/notification.rs` with tests first:

```rust
use serde::{Deserialize, Serialize};
use crate::db::Db;
use crate::error::Result;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Notification {
    pub id: String,           // "{timestamp_micros}:{uuid4_short}"
    pub title: String,
    pub body: Option<String>,
    pub level: String,        // "info" | "warning" | "error"
    pub session_id: Option<String>,
    pub profile_name: Option<String>,
    pub created_at: u64,      // Unix microseconds
    pub delivered: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_db() -> Db {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap()
    }

    #[test]
    fn test_push_and_drain() {
        let db = test_db();
        push_notification(&db, "Hello", None, "info", None, None).unwrap();
        let drained = drain_notifications(&db).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].title, "Hello");
        assert_eq!(drained[0].level, "info");
        assert!(!drained[0].delivered); // delivered flag is set in DB after drain
    }

    #[test]
    fn test_drain_marks_delivered() {
        let db = test_db();
        push_notification(&db, "Once", None, "info", None, None).unwrap();
        let first = drain_notifications(&db).unwrap();
        assert_eq!(first.len(), 1);
        let second = drain_notifications(&db).unwrap();
        assert!(second.is_empty(), "second drain must be empty");
    }

    #[test]
    fn test_push_stores_fields() {
        let db = test_db();
        push_notification(
            &db,
            "Test",
            Some("body text"),
            "error",
            Some("sess-1"),
            Some("growthmate"),
        ).unwrap();
        let drained = drain_notifications(&db).unwrap();
        assert_eq!(drained[0].body.as_deref(), Some("body text"));
        assert_eq!(drained[0].level, "error");
        assert_eq!(drained[0].session_id.as_deref(), Some("sess-1"));
        assert_eq!(drained[0].profile_name.as_deref(), Some("growthmate"));
    }

    #[test]
    fn test_drain_multiple_returns_all_undelivered() {
        let db = test_db();
        push_notification(&db, "A", None, "info", None, None).unwrap();
        push_notification(&db, "B", None, "warning", None, None).unwrap();
        push_notification(&db, "C", None, "error", None, None).unwrap();
        let drained = drain_notifications(&db).unwrap();
        assert_eq!(drained.len(), 3);
    }
}
```

- [ ] **Step 2: Run tests — confirm they fail**

```bash
cd /path/to/worktree
cargo test notification -- --nocapture 2>&1 | head -30
```

Expected: compile error — `push_notification` and `drain_notifications` not defined.

- [ ] **Step 3: Implement `push_notification` and `drain_notifications`**

Add to `src/notification.rs` after the struct:

```rust
/// DB key: "notif/{timestamp_micros}:{short_id}"
/// Undelivered = stored with delivered=false. drain_notifications re-writes with delivered=true.
fn notif_key(created_at: u64, id: &str) -> String {
    format!("{:020}:{}", created_at, id)
}

pub fn push_notification(
    db: &Db,
    title: &str,
    body: Option<&str>,
    level: &str,
    session_id: Option<&str>,
    profile_name: Option<&str>,
) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;
    // Short random suffix to avoid collisions on same microsecond
    let suffix: u32 = rand::random();
    let id = format!("{:08x}", suffix);
    let notif = Notification {
        id: format!("{}:{}", now, id),
        title: title.to_string(),
        body: body.map(|s| s.to_string()),
        level: level.to_string(),
        session_id: session_id.map(|s| s.to_string()),
        profile_name: profile_name.map(|s| s.to_string()),
        created_at: now,
        delivered: false,
    };
    let key = notif_key(now, &id);
    let bytes = serde_json::to_vec(&notif)
        .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
    db.put("notif", &key, &bytes)
}

/// Scan all undelivered notifications, re-write each as delivered, return the rows.
/// Not strictly atomic (scan then write) but safe: menubar polls every 2s,
/// daemon processes one request at a time — no concurrent drains possible.
pub fn drain_notifications(db: &Db) -> Result<Vec<Notification>> {
    let entries = db.scan_prefix("notif", "")?;
    let mut result = Vec::new();
    for (key, bytes) in entries {
        let mut notif: Notification = serde_json::from_slice(&bytes)
            .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
        if !notif.delivered {
            notif.delivered = true;
            let updated = serde_json::to_vec(&notif)
                .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
            db.put("notif", &key, &updated)?;
            notif.delivered = false; // Return with delivered=false so caller sees original state
            result.push(notif);
        }
    }
    Ok(result)
}
```

Add `rand` to `Cargo.toml` if not already present:
```toml
rand = "0.8"
```

Check if `rand` is already in Cargo.toml first: `grep rand Cargo.toml`

- [ ] **Step 4: Add `mod notification;` to `src/mcp_server.rs`**

Find the top of `mcp_server.rs` where other modules are used (look for `use crate::checkpoint` or similar) and add:
```rust
// near top of mcp_server.rs, with other crate imports
use crate::notification;
```

Also add `pub mod notification;` to `src/main.rs` (check where other mods are declared):
```rust
pub mod notification;
```

- [ ] **Step 5: Run tests — confirm they pass**

```bash
cargo test notification
```

Expected: 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/notification.rs src/mcp_server.rs src/main.rs Cargo.toml Cargo.lock
git commit -m "feat(rust): add notification module with push/drain"
```

---

## Task 2: Rust — `notify` MCP tool

**Files:**
- Modify: `src/mcp_server.rs`

- [ ] **Step 1: Write failing test**

In `src/mcp_server.rs`, find the `#[cfg(test)]` block near the bottom and add:

```rust
#[test]
fn test_notify_tool_writes_notification() {
    let dir = tempfile::tempdir().unwrap();
    let key = crate::db::Db::generate_key();
    let db = crate::db::Db::open_with_key(
        dir.path().join("test.db").to_str().unwrap(), key
    ).unwrap();

    // Simulate tool call with no session_id
    let args = serde_json::json!({
        "title": "Tests passed",
        "body": "All 3 flows green",
        "level": "info"
    });
    // Call handle_notify directly (extract to function in step 3)
    handle_notify(&db, &args, None).unwrap();

    let drained = crate::notification::drain_notifications(&db).unwrap();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].title, "Tests passed");
    assert_eq!(drained[0].body.as_deref(), Some("All 3 flows green"));
    assert_eq!(drained[0].level, "info");
    assert!(drained[0].profile_name.is_none());
}

#[test]
fn test_notify_tool_unknown_session_id_writes_nil_profile() {
    let dir = tempfile::tempdir().unwrap();
    let key = crate::db::Db::generate_key();
    let db = crate::db::Db::open_with_key(
        dir.path().join("test.db").to_str().unwrap(), key
    ).unwrap();

    let args = serde_json::json!({
        "title": "Done",
        "session_id": "nonexistent-session-id"
    });
    handle_notify(&db, &args, None).unwrap(); // sessions map is None = no session lookup

    let drained = crate::notification::drain_notifications(&db).unwrap();
    assert_eq!(drained.len(), 1);
    assert!(drained[0].profile_name.is_none());
}
```

- [ ] **Step 2: Run tests — confirm they fail**

```bash
cargo test test_notify_tool
```

Expected: compile error — `handle_notify` not defined.

- [ ] **Step 3: Add `notify` tool definition to `tools/list`**

In `mcp_server.rs`, find the `tools/list` JSON array (around line 763). Add after `list_session_checkpoints`:

```rust
{
    "name": "notify",
    "description": "Send a macOS notification via the Pagerunner menu bar. Use this to alert the user when a task is done, an error occurred, or any event worth surfacing. The notification appears immediately and can deep-link back to the current session.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "title": {
                "type": "string",
                "description": "Notification title (required). E.g. 'Tests passed — 3 flows green'"
            },
            "body": {
                "type": "string",
                "description": "Optional detail text shown below the title."
            },
            "level": {
                "type": "string",
                "enum": ["info", "warning", "error"],
                "description": "Urgency level. Default: 'info'. 'error' plays a louder sound."
            },
            "session_id": {
                "type": "string",
                "description": "Optional. If provided, tapping 'View' in the notification opens the menu bar to this session's profile."
            }
        },
        "required": ["title"]
    }
},
```

- [ ] **Step 4: Add `handle_notify` helper function and tool dispatch**

Add a helper function near the other `handle_*` functions or just inline it in the match arm. Add before the `#[cfg(test)]` block:

```rust
fn handle_notify(
    db: &crate::db::Db,
    args: &serde_json::Value,
    profile_name: Option<String>,
) -> crate::error::Result<String> {
    let title = args["title"]
        .as_str()
        .ok_or_else(|| crate::error::PagerunnerError::Config("Missing title".into()))?;
    let body = args["body"].as_str();
    let level = args["level"].as_str().unwrap_or("info");
    if !["info", "warning", "error"].contains(&level) {
        return Err(crate::error::PagerunnerError::Config(
            "level must be 'info', 'warning', or 'error'".into(),
        ));
    }
    let session_id = args["session_id"].as_str();
    crate::notification::push_notification(db, title, body, level, session_id, profile_name.as_deref())?;
    Ok(serde_json::json!({"ok": true}).to_string())
}
```

In the tool dispatch `match tool` block (around line 2690 area), add:

```rust
"notify" => {
    // Resolve profile_name from session_id if provided
    let profile_name = if let Some(sid) = args.get("session_id").and_then(|v| v.as_str()) {
        let mgr = sessions.lock().await;
        mgr.get(sid).and_then(|s| s.profile_name().map(|p| p.to_string()))
    } else {
        None
    };
    handle_notify(&db, &args, profile_name)?
}
```

Note: `session.profile_name()` — check what the Session struct exposes. Look at `src/session.rs` for the field name. It may be `profile` or `profile_name`. Use whatever field stores the profile name string.

Also add `"notify"` to the audit `build_args_summary` allowlist (check `src/audit.rs`) so it gets recorded.

- [ ] **Step 5: Run tests — confirm they pass**

```bash
cargo test test_notify_tool
```

Expected: 2 tests pass.

- [ ] **Step 6: Build to confirm compilation**

```bash
cargo build
```

- [ ] **Step 7: Commit**

```bash
git add src/mcp_server.rs
git commit -m "feat(rust): add notify MCP tool"
```

---

## Task 3: Rust — `list_notifications` internal tool

**Files:**
- Modify: `src/mcp_server.rs`

This tool is called only by the Swift menubar. It does NOT appear in `tools/list` (not shown to MCP clients).

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_list_notifications_drains_and_returns_json() {
    let dir = tempfile::tempdir().unwrap();
    let key = crate::db::Db::generate_key();
    let db = crate::db::Db::open_with_key(
        dir.path().join("test.db").to_str().unwrap(), key
    ).unwrap();

    crate::notification::push_notification(&db, "N1", None, "info", None, None).unwrap();
    crate::notification::push_notification(&db, "N2", None, "warning", None, Some("myprofile")).unwrap();

    // Simulate list_notifications response parsing
    let result = handle_list_notifications(&db).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let notifs = parsed["notifications"].as_array().unwrap();
    assert_eq!(notifs.len(), 2);
    assert!(notifs.iter().any(|n| n["title"] == "N1"));
    assert!(notifs.iter().any(|n| n["profile_name"] == "myprofile"));

    // Second call returns empty
    let result2 = handle_list_notifications(&db).unwrap();
    let parsed2: serde_json::Value = serde_json::from_str(&result2).unwrap();
    assert!(parsed2["notifications"].as_array().unwrap().is_empty());
}
```

- [ ] **Step 2: Run — confirm compile error**

```bash
cargo test test_list_notifications
```

- [ ] **Step 3: Add `handle_list_notifications` and dispatch**

Helper:
```rust
fn handle_list_notifications(db: &crate::db::Db) -> crate::error::Result<String> {
    let notifs = crate::notification::drain_notifications(db)?;
    let json_notifs: Vec<serde_json::Value> = notifs
        .iter()
        .map(|n| serde_json::json!({
            "id": n.id,
            "title": n.title,
            "body": n.body,
            "level": n.level,
            "session_id": n.session_id,
            "profile_name": n.profile_name,
            "created_at": n.created_at,
        }))
        .collect();
    Ok(serde_json::json!({"notifications": json_notifs}).to_string())
}
```

In the match block, add:
```rust
"list_notifications" => {
    handle_list_notifications(&db)?
}
```

This goes in the dispatch but NOT in the `tools/list` JSON array.

- [ ] **Step 4: Run tests**

```bash
cargo test test_list_notifications
```

Expected: passes.

- [ ] **Step 5: Build**

```bash
cargo build
```

- [ ] **Step 6: Commit**

```bash
git add src/mcp_server.rs
git commit -m "feat(rust): add list_notifications internal tool"
```

---

## Task 4: Swift — `NotificationSettings`

**Files:**
- Create: `apps/menubar/Sources/PagerunnerBar/NotificationSettings.swift`

UserDefaults-backed settings. No UI yet (that's Task 10).

- [ ] **Step 1: Create `NotificationSettings.swift`**

```swift
import Foundation

/// UserDefaults-backed notification preferences.
/// Keys: "notif.daemonHealth", "notif.<profileName>.crash", etc.
struct NotificationSettings {

    static func registerDefaults(profileNames: [String], agentProfiles: Set<String>) {
        var defaults: [String: Any] = [
            "notif.daemonHealth": true,
        ]
        for name in profileNames {
            let isAgent = agentProfiles.contains(name)
            defaults["notif.\(name).crash"] = true          // always on
            defaults["notif.\(name).idle"] = true           // always on
            defaults["notif.\(name).idleMinutes"] = 30
            defaults["notif.\(name).start"] = isAgent       // on for agents, off for personal
        }
        UserDefaults.standard.register(defaults: defaults)
    }

    static func notifyOnDaemonHealth() -> Bool {
        UserDefaults.standard.bool(forKey: "notif.daemonHealth")
    }

    static func notifyOnCrash(profile: String) -> Bool {
        UserDefaults.standard.bool(forKey: "notif.\(profile).crash")
    }

    static func notifyOnIdle(profile: String) -> Bool {
        UserDefaults.standard.bool(forKey: "notif.\(profile).idle")
    }

    static func idleThresholdMinutes(profile: String) -> Int {
        let v = UserDefaults.standard.integer(forKey: "notif.\(profile).idleMinutes")
        return v > 0 ? v : 30
    }

    static func notifyOnStart(profile: String) -> Bool {
        UserDefaults.standard.bool(forKey: "notif.\(profile).start")
    }

    static func setNotifyOnCrash(_ value: Bool, profile: String) {
        UserDefaults.standard.set(value, forKey: "notif.\(profile).crash")
    }

    static func setNotifyOnIdle(_ value: Bool, profile: String) {
        UserDefaults.standard.set(value, forKey: "notif.\(profile).idle")
    }

    static func setIdleThresholdMinutes(_ value: Int, profile: String) {
        UserDefaults.standard.set(value, forKey: "notif.\(profile).idleMinutes")
    }

    static func setNotifyOnStart(_ value: Bool, profile: String) {
        UserDefaults.standard.set(value, forKey: "notif.\(profile).start")
    }

    static func setNotifyOnDaemonHealth(_ value: Bool) {
        UserDefaults.standard.set(value, forKey: "notif.daemonHealth")
    }
}
```

- [ ] **Step 2: Build to confirm compilation**

```bash
cd apps/menubar && swift build
```

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/NotificationSettings.swift
git commit -m "feat(swift): add NotificationSettings UserDefaults wrapper"
```

---

## Task 5: Swift — `NotificationService` updates

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/NotificationService.swift`

- [ ] **Step 1: Add `configure()`, `notifyExplicit()`, update `notifyAgentIdle()`**

Replace the entire file content with:

```swift
import UserNotifications
import Foundation
import PagerunnerCore

@MainActor
final class NotificationService: NSObject, UNUserNotificationCenterDelegate {
    private let center = UNUserNotificationCenter.current()

    // Set via configure() after init to avoid circular dependency
    weak var appState: AppState?
    weak var controller: StatusItemController?

    override init() {
        super.init()
        center.delegate = self
    }

    func configure(appState: AppState, controller: StatusItemController) {
        self.appState = appState
        self.controller = controller
    }

    func requestPermission() async {
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
        registerCategories()
    }

    // MARK: - Notification types

    func notifyExplicit(title: String, body: String?, level: String, profileName: String?, sessionId: String?) {
        let content = UNMutableNotificationContent()
        content.title = title
        if let body { content.body = body }
        content.sound = sound(for: level)
        content.categoryIdentifier = "NOTIFY_TOOL"
        content.userInfo = userInfo(profileName: profileName, sessionId: sessionId)
        schedule(content, id: "notify-\(UUID().uuidString)")
    }

    func notifySessionCrashed(profile: String, sessionId: String) {
        let content = UNMutableNotificationContent()
        content.title = "Session crashed — \(profile)"
        content.body = "Tap Restart to reopen."
        content.sound = .default
        content.categoryIdentifier = "SESSION_CRASHED"
        content.userInfo = userInfo(profileName: profile, sessionId: sessionId)
        schedule(content, id: "crash-\(sessionId)")
    }

    func notifyDaemonStopped() {
        let content = UNMutableNotificationContent()
        content.title = "Pagerunner stopped unexpectedly"
        content.body = "Tap Restart Daemon to recover."
        content.sound = .default
        content.categoryIdentifier = "DAEMON_STOPPED"
        schedule(content, id: "daemon-stopped-\(Date().timeIntervalSince1970)")
    }

    func notifyCheckpointSaved(name: String) {
        let content = UNMutableNotificationContent()
        content.title = "Checkpoint saved"
        content.body = "\"\(name)\" saved successfully."
        content.categoryIdentifier = "CHECKPOINT_SAVED"
        schedule(content, id: "ckpt-saved-\(UUID().uuidString)")
    }

    func notifyAgentIdle(profileName: String, idleMinutes: Int) {
        let content = UNMutableNotificationContent()
        content.title = "Agent \(profileName) idle \(idleMinutes)min"
        content.body = "No tab activity detected."
        content.sound = .default
        content.categoryIdentifier = "AGENT_IDLE"
        content.userInfo = userInfo(profileName: profileName, sessionId: nil)
        schedule(content, id: "agent-idle-\(profileName)")
    }

    func notifySessionStarted(profileName: String) {
        let content = UNMutableNotificationContent()
        content.title = "\(profileName) session started"
        content.categoryIdentifier = "SESSION_STARTED"
        content.userInfo = userInfo(profileName: profileName, sessionId: nil)
        schedule(content, id: "session-start-\(profileName)-\(Date().timeIntervalSince1970)")
    }

    // MARK: - Categories

    private func registerCategories() {
        let view = UNNotificationAction(identifier: "VIEW", title: "View", options: .foreground)
        let restart = UNNotificationAction(identifier: "RESTART_SESSION", title: "Restart", options: .foreground)
        let restartDaemon = UNNotificationAction(identifier: "RESTART_DAEMON", title: "Restart Daemon", options: .foreground)
        let closeSession = UNNotificationAction(identifier: "CLOSE_SESSION", title: "Close Session", options: .destructive)
        let dismiss = UNNotificationAction(identifier: "DISMISS", title: "Dismiss", options: .destructive)

        center.setNotificationCategories([
            UNNotificationCategory(identifier: "NOTIFY_TOOL",     actions: [view],                intentIdentifiers: []),
            UNNotificationCategory(identifier: "SESSION_CRASHED", actions: [view, restart],       intentIdentifiers: []),
            UNNotificationCategory(identifier: "AGENT_IDLE",      actions: [view, closeSession],  intentIdentifiers: []),
            UNNotificationCategory(identifier: "DAEMON_STOPPED",  actions: [restartDaemon, dismiss], intentIdentifiers: []),
            UNNotificationCategory(identifier: "SESSION_STARTED", actions: [],                    intentIdentifiers: []),
            UNNotificationCategory(identifier: "CHECKPOINT_SAVED",actions: [],                    intentIdentifiers: []),
        ])
    }

    // MARK: - Delegate

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let userInfo = response.notification.request.content.userInfo
        let profileName = userInfo["notif.profileName"] as? String
        let sessionId = userInfo["notif.sessionId"] as? String

        Task { @MainActor [weak self] in
            guard let self else { completionHandler(); return }
            switch response.actionIdentifier {
            case "VIEW", UNNotificationDefaultActionIdentifier:
                self.controller?.openPopover()
                if let name = profileName {
                    self.appState?.navigation = .profile(name)
                } else {
                    self.appState?.navigation = .overview
                }
            case "RESTART_SESSION":
                if let name = profileName {
                    // Note: stealth/anonymize not preserved — intentional
                    _ = try? await DaemonClient().call(
                        tool: "open_session",
                        args: ["profile": name]
                    )
                }
            case "CLOSE_SESSION":
                if let sid = sessionId {
                    _ = try? await DaemonClient().call(
                        tool: "close_session",
                        args: ["session_id": sid]
                    )
                }
            case "RESTART_DAEMON":
                await self.appState?.restartDaemon()
            default:
                break
            }
            completionHandler()
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }

    // MARK: - Helpers

    private func schedule(_ content: UNMutableNotificationContent, id: String) {
        let request = UNNotificationRequest(identifier: id, content: content, trigger: nil)
        center.add(request)
    }

    private func sound(for level: String) -> UNNotificationSound? {
        switch level {
        case "warning", "error": return .default
        default: return nil
        }
    }

    private func userInfo(profileName: String?, sessionId: String?) -> [AnyHashable: Any] {
        var info: [AnyHashable: Any] = [:]
        if let p = profileName { info["notif.profileName"] = p }
        if let s = sessionId { info["notif.sessionId"] = s }
        return info
    }
}
```

- [ ] **Step 2: Build**

```bash
swift build
```

Fix any Swift 6 concurrency errors if they arise. The `nonisolated` delegate methods calling `Task { @MainActor }` is the correct pattern.

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/NotificationService.swift
git commit -m "feat(swift): update NotificationService — configure(), notifyExplicit(), action handling"
```

---

## Task 6: Swift — `NotificationPoller`

**Files:**
- Create: `apps/menubar/Sources/PagerunnerBar/NotificationPoller.swift`

- [ ] **Step 1: Create `NotificationPoller.swift`**

```swift
import Foundation
import PagerunnerCore

/// Polls the daemon every 2 seconds for pending notifications and fires them
/// via NotificationService. Runs independently of PollingService — stays at
/// 2s even when the popover is closed so notify() tool results arrive promptly.
@MainActor
final class NotificationPoller {
    private let daemon = DaemonClient()
    private let notificationService: NotificationService

    init(notificationService: NotificationService) {
        self.notificationService = notificationService
    }

    func start() {
        Task { @MainActor [weak self] in
            while !Task.isCancelled {
                await self?.poll()
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    private func poll() async {
        guard let raw = try? await daemon.call(tool: "list_notifications") else { return }
        guard let notifs = raw["notifications"]?.arrayValue else { return }

        for item in notifs {
            guard let obj = item.objectValue,
                  let title = obj["title"]?.stringValue,
                  let level = obj["level"]?.stringValue else { continue }

            let body = obj["body"]?.stringValue
            let profileName = obj["profile_name"]?.stringValue
            let sessionId = obj["session_id"]?.stringValue

            // NOTIFY_TOOL rows always fire — no settings gate
            notificationService.notifyExplicit(
                title: title,
                body: body,
                level: level,
                profileName: profileName,
                sessionId: sessionId
            )
        }
    }
}
```

- [ ] **Step 2: Build**

```bash
swift build
```

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/NotificationPoller.swift
git commit -m "feat(swift): add NotificationPoller — 2s poll for notify tool results"
```

---

## Task 7: Swift — Wire everything in `App.swift`

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/App.swift`

- [ ] **Step 1: Add `NotificationPoller` instantiation and `configure()` call**

In `AppDelegate`, add:
```swift
private var notificationPoller: NotificationPoller!
```

In `applicationDidFinishLaunching`, after `notificationService = NotificationService()`:

```swift
// Start notification poller (runs every 2s regardless of popover state)
notificationPoller = NotificationPoller(notificationService: notificationService)
notificationPoller.start()
```

After `statusItemController = StatusItemController(...)`:

```swift
// Wire notification service to appState + controller for action handling
notificationService.configure(appState: appState, controller: statusItemController)
```

Call `NotificationSettings.registerDefaults` once profiles are known. The best place is at the end of the first successful poll. In the `poll(client:)` function, after `appState.profiles` is updated, add:

```swift
// Register notification defaults (idempotent — safe to call on every poll)
let agentNames = Set(appState.agentProfiles.map { $0.name })
NotificationSettings.registerDefaults(
    profileNames: appState.profiles.map { $0.name },
    agentProfiles: agentNames
)
```

- [ ] **Step 2: Add unexpected daemon stop notification**

In `poll(client:)`, in the `catch` block:

```swift
} catch {
    let wasRunningOrStale = appState.daemonStatus != .stopped
    appState.recordFailure()
    // Fire notification on unexpected stop (not intentional stop)
    if wasRunningOrStale && appState.daemonStatus == .stopped
       && appState.transition == .none
       && NotificationSettings.notifyOnDaemonHealth() {
        notificationService.notifyDaemonStopped()
    }
}
```

- [ ] **Step 3: Build**

```bash
swift build
```

- [ ] **Step 4: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/App.swift
git commit -m "feat(swift): wire NotificationPoller, configure NotificationService, daemon-stop notification"
```

---

## Task 8: Swift — Agent idle detection in `App.swift`

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/App.swift`

Idle detection tracks when tab count last changed per agent session.

- [ ] **Step 1: Add tracking state to `AppDelegate`**

```swift
// Idle detection: tracks last tab-count-change time per session
private var sessionIdleTracker: [String: (tabCount: Int, stableFrom: Date)] = [:]
private var idleNotifiedSessions: Set<String> = []
```

- [ ] **Step 2: Add idle check to the poll function**

After the tab-fetching loop (step 2 in `poll`), add:

```swift
// Idle detection for agent sessions
let now = Date()
for session in sessions where session.status == .alive {
    guard let profile = appState.profiles.first(where: { $0.name == session.profile }),
          profile.kind == "agent" else { continue }
    guard NotificationSettings.notifyOnIdle(profile: session.profile) else { continue }

    let currentTabCount = appState.tabs[session.id]?.count ?? 0
    let threshold = NotificationSettings.idleThresholdMinutes(profile: session.profile)

    if let tracker = sessionIdleTracker[session.id] {
        if tracker.tabCount != currentTabCount {
            // Tab count changed — reset timer, clear idle notification flag
            sessionIdleTracker[session.id] = (tabCount: currentTabCount, stableFrom: now)
            idleNotifiedSessions.remove(session.id)
        } else if !idleNotifiedSessions.contains(session.id) {
            let minutesIdle = now.timeIntervalSince(tracker.stableFrom) / 60
            if minutesIdle >= Double(threshold) {
                notificationService.notifyAgentIdle(
                    profileName: profile.displayName,
                    idleMinutes: threshold
                )
                idleNotifiedSessions.insert(session.id)
            }
        }
    } else {
        sessionIdleTracker[session.id] = (tabCount: currentTabCount, stableFrom: now)
    }
}

// Clean up tracker entries for sessions that no longer exist
let activeIds = Set(sessions.map { $0.id })
sessionIdleTracker = sessionIdleTracker.filter { activeIds.contains($0.key) }
idleNotifiedSessions = idleNotifiedSessions.filter { activeIds.contains($0) }
```

- [ ] **Step 3: Build**

```bash
swift build
```

- [ ] **Step 4: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/App.swift
git commit -m "feat(swift): add agent idle detection + notification"
```

---

## Task 9: Swift — Notifications section in `SettingsView`

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/SettingsView.swift`

- [ ] **Step 1: Add notifications section**

In `SettingsView`, add state for per-profile settings binding and a `notificationsSection` view builder.

In `contentArea`, add after `behaviorSection`:

```swift
Divider()
notificationsSection
```

Add the section:

```swift
@ViewBuilder
private var notificationsSection: some View {
    VStack(alignment: .leading, spacing: 8) {
        Text("NOTIFICATIONS")
            .font(.system(size: 9, weight: .semibold))
            .foregroundStyle(.secondary)
            .kerning(0.5)

        // Global: daemon health
        HStack {
            Toggle(isOn: Binding(
                get: { NotificationSettings.notifyOnDaemonHealth() },
                set: { NotificationSettings.setNotifyOnDaemonHealth($0) }
            )) {
                Text("Daemon health alerts")
                    .font(.system(size: 12))
            }
            .toggleStyle(.switch)
            .controlSize(.small)
        }

        // Global: explicit notify tool (always on — informational only)
        HStack {
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 11))
                .foregroundColor(.secondary)
            Text("Agent-sent notifications always deliver")
                .font(.system(size: 11))
                .foregroundColor(.secondary)
        }

        if !appState.profiles.isEmpty {
            Divider()
            Text("PER PROFILE")
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(.secondary)
                .kerning(0.5)

            ForEach(appState.profiles, id: \.id) { profile in
                NotificationProfileRow(profile: profile)
            }
        }
    }
}
```

Add a new private view for each profile row:

```swift
private struct NotificationProfileRow: View {
    let profile: Profile
    @State private var crash: Bool = true
    @State private var idle: Bool = true
    @State private var start: Bool = false
    @State private var idleMinutes: Int = 30

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(profile.displayName)
                .font(.system(size: 11, weight: .medium))
            HStack(spacing: 12) {
                Toggle("Crash", isOn: $crash)
                    .onChange(of: crash) { NotificationSettings.setNotifyOnCrash(crash, profile: profile.name) }
                Toggle("Idle", isOn: $idle)
                    .onChange(of: idle) { NotificationSettings.setNotifyOnIdle(idle, profile: profile.name) }
                if idle {
                    Picker("", selection: $idleMinutes) {
                        Text("15m").tag(15)
                        Text("30m").tag(30)
                        Text("60m").tag(60)
                    }
                    .pickerStyle(.segmented)
                    .frame(width: 110)
                    .onChange(of: idleMinutes) {
                        NotificationSettings.setIdleThresholdMinutes(idleMinutes, profile: profile.name)
                    }
                }
                Toggle("Start", isOn: $start)
                    .onChange(of: start) { NotificationSettings.setNotifyOnStart(start, profile: profile.name) }
            }
            .font(.system(size: 11))
            .toggleStyle(.checkbox)
            .controlSize(.small)
        }
        .padding(.vertical, 2)
        .onAppear {
            crash = NotificationSettings.notifyOnCrash(profile: profile.name)
            idle = NotificationSettings.notifyOnIdle(profile: profile.name)
            start = NotificationSettings.notifyOnStart(profile: profile.name)
            idleMinutes = NotificationSettings.idleThresholdMinutes(profile: profile.name)
        }
    }
}
```

- [ ] **Step 2: Build**

```bash
swift build
```

Fix any layout issues.

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/SettingsView.swift
git commit -m "feat(swift): add Notifications section to SettingsView"
```

---

## Task 10: Release build + smoke test

- [ ] **Step 1: Run Rust tests**

```bash
cargo test notification
cargo test test_notify_tool
cargo test test_list_notifications
```

Expected: all pass.

- [ ] **Step 2: Run Swift tests**

```bash
cd apps/menubar && swift test
```

Expected: existing PagerunnerCoreTests still pass.

- [ ] **Step 3: Release build**

```bash
cargo build --release
cd apps/menubar
swift build -c release
cp .build/release/PagerunnerBar scripts/Pagerunner.app/Contents/MacOS/PagerunnerBar
```

- [ ] **Step 4: Manual smoke test**

1. Start daemon: `pagerunner daemon &`
2. Open Pagerunner.app
3. In Claude Code, call the `notify` tool:
   ```
   Use the pagerunner notify tool with title "Hello from Claude" and level "info"
   ```
4. Verify macOS notification appears within 2 seconds with a "View" button
5. Tap "View" — verify menubar opens
6. Stop daemon unexpectedly (`pkill -f "pagerunner daemon"`)
7. Verify "Pagerunner stopped unexpectedly" notification fires
8. Open Settings → verify Notifications section shows profiles with toggles

- [ ] **Step 5: Final commit if anything was fixed during smoke test**

```bash
git add -A
git commit -m "fix: smoke test fixes for notification system"
```
