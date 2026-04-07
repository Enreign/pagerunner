# Security Gaps Remaining Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close five remaining security gaps in the pagerunner security layer: document evaluate's JS bypass, filter list_snapshots by profile, validate restore_snapshot origin, intercept redirects via CDP Network.setBlockedURLs, and validate wait_for_url's final URL.

**Architecture:** All changes are either documentation-only (Task 1), DB-layer filter additions (Tasks 2-3), or dispatch-level policy enforcement (Tasks 4-5). Task 4 is the most impactful: adds a `blocked_url_patterns()` factory to `NetworkGuard` and calls `Network.enable` + `Network.setBlockedURLs` on each fresh CDP session attachment, blocking Chrome-level requests to private IPs including redirect targets. No event-streaming is needed — Chrome rejects blocked requests before they complete.

**Tech Stack:** Rust, `url` crate (2), CDP (Chrome DevTools Protocol), redb encrypted DB

---

## File Map

| File | Change |
|------|--------|
| `src/mcp_server.rs` | evaluate description update; list_snapshots schema+dispatch; wait_for policy check after URL match |
| `src/snapshot.rs` | `list_snapshots` signature adds `profile_filter: Option<&str>`; `restore_snapshot` validates snap.origin |
| `src/network_guard.rs` | Add `pub fn blocked_url_patterns() -> Vec<String>` |
| `src/browser.rs` | `fresh_attach` calls network blocking setup when policy present; add `enable_network_blocking` fn |

---

### Task 1: Document `evaluate` tool's JS bypass limitation

**Files:**
- Modify: `src/mcp_server.rs:124-134`

The `evaluate` tool lets agents run arbitrary JavaScript in a tab. JS `fetch()`, `XMLHttpRequest`, and other network APIs bypass NetworkGuard entirely (they go through the browser's network stack, not our Rust layer). Until redirect interception (Task 4) is in place, this is the main escape hatch. We document it so AI consumers of the MCP schema know not to rely on evaluate for security-sensitive tasks.

- [ ] **Step 1: Update the evaluate tool description**

In `src/mcp_server.rs`, change the evaluate `description` from:
```
"Execute JavaScript in a tab and return the result. Use for DOM queries, clicking elements, filling forms, or any page interaction."
```
to:
```
"Execute JavaScript in a tab and return the result. Use for DOM queries, clicking elements, filling forms, or any page interaction. Note: JavaScript runs in the page context and can make network requests (fetch, XHR) to any destination, bypassing server-level URL access controls."
```

- [ ] **Step 2: Verify the change compiles**

```bash
cargo check 2>&1 | head -20
```
Expected: no errors.

- [ ] **Step 3: Verify the tool list test still passes**

```bash
cargo test test_tools_list_response -- --nocapture
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/mcp_server.rs
git commit -m "docs: warn in evaluate tool description that JS bypasses network guard"
```

---

### Task 2: Filter `list_snapshots` by profile

**Files:**
- Modify: `src/snapshot.rs:296-320`
- Modify: `src/mcp_server.rs` (list_snapshots tool schema + dispatch)

Currently `list_snapshots` returns snapshots from ALL profiles, leaking browsing history across profiles (gap #9). Add an optional `profile` parameter so agents can filter to their session's profile. The tool still accepts cross-profile queries (for admin use), but the default is no filter — agents should pass their profile explicitly.

- [ ] **Step 1: Write the failing test**

In `src/snapshot.rs` tests section, add:

```rust
#[test]
fn test_list_snapshots_profile_filter() {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Db::open_with_key(
        dir.path().join("t.db").to_str().unwrap(), [0u8; 32]
    ).unwrap();

    // Save snapshots for two different profiles
    for (profile, origin, ts) in [
        ("alice", "https://example.com", 1_000_000u64),
        ("bob", "https://example.com", 2_000_000u64),
        ("alice", "https://other.com", 3_000_000u64),
    ] {
        let snap = Snapshot {
            profile: profile.into(), origin: origin.into(),
            cookies: vec![], local_storage: Default::default(), saved_at: ts,
        };
        db.put("snapshots", &snapshot_key(profile, origin, ts),
               &serde_json::to_vec(&snap).unwrap()).unwrap();
    }

    // No filter → all 3
    let all = list_snapshots(&db, false, None).unwrap();
    assert_eq!(all.len(), 3);

    // Filter to alice → 2 (example.com + other.com)
    let alice = list_snapshots(&db, false, Some("alice")).unwrap();
    assert_eq!(alice.len(), 2);
    assert!(alice.iter().all(|s| s.profile == "alice"));

    // Filter to bob → 1
    let bob = list_snapshots(&db, false, Some("bob")).unwrap();
    assert_eq!(bob.len(), 1);
    assert_eq!(bob[0].profile, "bob");
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test test_list_snapshots_profile_filter 2>&1 | tail -5
```
Expected: compile error (wrong number of arguments to `list_snapshots`).

- [ ] **Step 3: Update `list_snapshots` signature and implementation**

In `src/snapshot.rs`, change:
```rust
pub fn list_snapshots(db: &Db, latest_only: bool) -> Result<Vec<SnapshotInfo>> {
    let entries = db.scan_prefix("snapshots", "")?;
    let mut out = Vec::new();
    for (_, bytes) in entries {
        if let Ok(snap) = serde_json::from_slice::<Snapshot>(&bytes) {
```
to:
```rust
pub fn list_snapshots(db: &Db, latest_only: bool, profile_filter: Option<&str>) -> Result<Vec<SnapshotInfo>> {
    let prefix = match profile_filter {
        Some(p) => format!("{}/", p),
        None => String::new(),
    };
    let entries = db.scan_prefix("snapshots", &prefix)?;
    let mut out = Vec::new();
    for (_, bytes) in entries {
        if let Ok(snap) = serde_json::from_slice::<Snapshot>(&bytes) {
```

- [ ] **Step 4: Update the call sites**

In `src/mcp_server.rs`, change the `list_snapshots` call (around line 780):
```rust
"list_snapshots" => {
    let latest_only = args["latest_only"].as_bool().unwrap_or(true);
    let infos = crate::snapshot::list_snapshots(&db, latest_only)?;
```
to:
```rust
"list_snapshots" => {
    let latest_only = args["latest_only"].as_bool().unwrap_or(true);
    let profile_filter = args["profile"].as_str();
    let infos = crate::snapshot::list_snapshots(&db, latest_only, profile_filter)?;
```

Also update the `list_snapshots` tool schema in `all_tools()` to document the new `profile` parameter (find the `list_snapshots` json! block around line 249 and add to its properties):
```rust
"profile": {
    "type": "string",
    "description": "Filter snapshots to this profile name only. Omit to list all profiles."
},
```

Update all existing test call sites in `src/snapshot.rs` that call the old 2-arg signature. These are all the lines that must change (exact line numbers from current source):
- Line 486: `list_snapshots(&db, false)` → `list_snapshots(&db, false, None)`
- Line 492: `list_snapshots(&db, false)` → `list_snapshots(&db, false, None)`
- Line 537: `list_snapshots(&db, true)` → `list_snapshots(&db, true, None)`
- Line 542: `list_snapshots(&db, false)` → `list_snapshots(&db, false, None)`
- Line 571: `list_snapshots(&db, false)` → `list_snapshots(&db, false, None)`
- Line 578: `list_snapshots(&db, false)` → `list_snapshots(&db, false, None)`

Run `grep -n "list_snapshots(" src/snapshot.rs` to confirm no 2-arg calls remain.

- [ ] **Step 5: Run all snapshot tests**

```bash
cargo test -p pagerunner -- snapshot 2>&1 | tail -20
```
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/snapshot.rs src/mcp_server.rs
git commit -m "feat: add optional profile filter to list_snapshots"
```

---

### Task 3: Validate `restore_snapshot` origin against stored snapshot

**Files:**
- Modify: `src/snapshot.rs:384-434`

After deserializing the snapshot, verify its `origin` field matches what was requested. Without this, a DB key collision or future DB key structure change could silently inject wrong-origin cookies.

- [ ] **Step 1: Write the documentation test and add the guard**

The origin validation in `restore_snapshot` requires a live Chrome session (async, needs `Session`), so we can't write a hermetic unit test for the full function. Instead:

1. Add a test that documents the DB state we're defending against (a snapshot stored under a mismatched key), and
2. Add the guard code directly (Step 2) — the protection is verified by `cargo check` + reading the code.

In `src/snapshot.rs` tests:
```rust
#[test]
fn test_snapshot_origin_field_preserved_in_list() {
    // Documents that list_snapshots reports the origin STORED IN THE SNAPSHOT,
    // not the key used for storage. This is the invariant restore_snapshot relies
    // on when checking snap.origin == requested origin.
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Db::open_with_key(
        dir.path().join("t.db").to_str().unwrap(), [0u8; 32]
    ).unwrap();

    let snap = Snapshot {
        profile: "alpha".into(),
        origin: "https://evil.com".into(),  // stored origin — would mismatch if key said example.com
        cookies: vec![],
        local_storage: Default::default(),
        saved_at: 1_000_000,
    };
    // Write under a key that disagrees with the stored origin (simulates DB corruption)
    db.put("snapshots", "alpha/https://example.com/1000000",
           &serde_json::to_vec(&snap).unwrap()).unwrap();

    let list = list_snapshots(&db, false, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].origin, "https://evil.com"); // origin from snap, not from key
}
```

Add the validation code:

- [ ] **Step 2: Add origin validation in `restore_snapshot`**

In `src/snapshot.rs`, `restore_snapshot` function, after deserializing the snapshot (after line 404: `let snap: Snapshot = ...`), add:

```rust
    // Validate the stored origin matches what was requested.
    // Mismatch indicates DB corruption or a key-structure bug.
    if snap.origin != origin {
        return Err(PagerunnerError::Config(format!(
            "Snapshot origin mismatch: requested '{}' but stored snapshot is for '{}'. \
             The DB entry at key prefix '{}/{}' is corrupt.",
            origin, snap.origin, profile, origin
        )));
    }
```

- [ ] **Step 3: Verify the code compiles**

```bash
cargo check 2>&1 | head -10
```
Expected: no errors.

- [ ] **Step 4: Run all snapshot tests**

```bash
cargo test -p pagerunner -- snapshot 2>&1 | tail -20
```
Expected: all PASS (existing tests won't trigger mismatch since they use consistent key/origin).

- [ ] **Step 5: Commit**

```bash
git add src/snapshot.rs
git commit -m "feat: validate restore_snapshot origin matches stored snapshot origin"
```

---

### Task 4: CDP-level redirect interception via `Network.setBlockedURLs`

**Files:**
- Modify: `src/network_guard.rs` (add `blocked_url_patterns()`)
- Modify: `src/browser.rs` (`fresh_attach` sets up network blocking; new helper fn)

**What this does:** When a tab's CDP session is established (`fresh_attach`), if the pagerunner session has a security policy, call `Network.enable` + `Network.setBlockedURLs` with glob patterns covering all private IP ranges. Chrome will fail any request to these hosts with `net::ERR_BLOCKED_BY_CLIENT`, including redirect targets. This is the only way to catch SSRF via HTTP 301/302 redirects.

`Network.setBlockedURLs` is an experimental CDP API (not in the stable spec) but has been present in Chrome/Chromium since at least version 72 and remains functional as of 2026.

**Blocked patterns cover:** 127.x.x.x, ::1, localhost, 10.x.x.x, 172.16-31.x.x, 192.168.x.x, 169.254.x.x (AWS metadata), metadata.google.internal.

- [ ] **Step 1: Write the failing test for `blocked_url_patterns`**

In `src/network_guard.rs` tests:
```rust
#[test]
fn blocked_url_patterns_covers_all_private_ranges() {
    let patterns = NetworkGuard::blocked_url_patterns();
    // Should have at least: loopback, 10.*, 192.168.*, 169.254.*, 16 entries for 172.16-31.*
    assert!(patterns.len() >= 20, "expected ≥20 patterns, got {}", patterns.len());
    // Must include the 172.16-31 range (with /* path suffix)
    for i in 16u8..=31u8 {
        let pat = format!("*://172.{}.*/*", i);
        assert!(patterns.contains(&pat), "missing pattern: {pat}");
    }
    // Must include loopback and link-local
    assert!(patterns.iter().any(|p| p.contains("127.")));
    assert!(patterns.iter().any(|p| p.contains("169.254.")));
    assert!(patterns.iter().any(|p| p.contains("localhost")));
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test blocked_url_patterns_covers_all_private_ranges 2>&1 | tail -5
```
Expected: compile error (method not found).

- [ ] **Step 3: Implement `blocked_url_patterns` in NetworkGuard**

In `src/network_guard.rs`, add a public associated function after the `check_redirect` method:

```rust
/// Returns Chrome-compatible URL glob patterns for all private/loopback IP ranges.
/// Used with CDP `Network.setBlockedURLs` to block Chrome from requesting these
/// hosts, including as redirect destinations.
///
/// All patterns include a `/*` path suffix — Chrome's pattern engine requires
/// a path component to match reliably (bare `*://10.*` may not match `http://10.0.0.1/path`).
pub fn blocked_url_patterns() -> Vec<String> {
    let mut patterns = vec![
        // Loopback / unspecified
        "*://127.*/*".to_string(),
        "*://0.0.0.0/*".to_string(),
        "*://[::1]/*".to_string(),
        "*://[::1]:*/*".to_string(),
        // Localhost by name (all variants NetworkGuard blocks)
        "*://localhost/*".to_string(),
        "*://localhost:*/*".to_string(),
        "*://*.localhost/*".to_string(),
        "*://*.localhost:*/*".to_string(),
        "*://localhost.localdomain/*".to_string(),
        // 10.0.0.0/8
        "*://10.*/*".to_string(),
        // 192.168.0.0/16
        "*://192.168.*/*".to_string(),
        // 169.254.0.0/16 (link-local / AWS instance metadata)
        "*://169.254.*/*".to_string(),
        // Google Cloud metadata endpoint
        "*://metadata.google.internal/*".to_string(),
    ];
    // 172.16.0.0/12 (172.16.0.0 – 172.31.255.255)
    for i in 16u8..=31u8 {
        patterns.push(format!("*://172.{}.*/*", i));
    }
    patterns
}
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test blocked_url_patterns_covers_all_private_ranges -- --nocapture
```
Expected: PASS.

- [ ] **Step 5: Write the test for network blocking being set up on fresh attach**

In `src/browser.rs` (or `src/network_guard.rs`), the best we can test without Chrome is that the helper function would call the right CDP methods. We verify this via the pattern coverage test above (already passing). The integration verification is: `cargo test` passes and the e2e test below works.

For unit coverage of the CDP call sequence, add this comment to `enable_network_blocking`:
```rust
// Tested indirectly: blocked_url_patterns_covers_all_private_ranges verifies
// the pattern list; e2e redirect tests verify the CDP integration.
```

- [ ] **Step 6: Add `enable_network_blocking` helper and call from `fresh_attach`**

In `src/browser.rs`, add this function before `fresh_attach`:

```rust
/// Enable CDP Network domain and block all private IP ranges for this session.
/// Called on every fresh CDP session attach when the pagerunner session has a policy.
async fn enable_network_blocking(cdp: &mut CdpConn, session_id: &str) -> Result<()> {
    cdp.send_on_session("Network.enable", serde_json::json!({}), Some(session_id.to_string())).await?;
    let patterns = crate::network_guard::NetworkGuard::blocked_url_patterns();
    cdp.send_on_session(
        "Network.setBlockedURLs",
        serde_json::json!({ "urls": patterns }),
        Some(session_id.to_string()),
    ).await?;
    Ok(())
}
```

In `fresh_attach`, add the call after stealth injection:

```rust
async fn fresh_attach(session: &mut Session, target_id: &str) -> Result<String> {
    let attach = session.cdp.send("Target.attachToTarget", json!({
        "targetId": target_id,
        "flatten": true
    })).await?;
    let session_id = attach["sessionId"].as_str()
        .ok_or_else(|| PagerunnerError::Cdp("No sessionId from attachToTarget".into()))?
        .to_string();

    if session.stealth {
        crate::stealth::inject(&mut session.cdp, &session_id).await?;
    }
    if session.security_policy.is_some() {
        enable_network_blocking(&mut session.cdp, &session_id).await?;
    }

    session.cdp_sessions.insert(target_id.to_string(), session_id.clone());
    Ok(session_id)
}
```

- [ ] **Step 7: Verify the code compiles**

```bash
cargo check 2>&1 | head -20
```
Expected: no errors.

- [ ] **Step 8: Run all tests**

```bash
cargo test 2>&1 | tail -20
```
Expected: all PASS (no Chrome-dependent tests will run in CI).

- [ ] **Step 9: Commit**

```bash
git add src/network_guard.rs src/browser.rs
git commit -m "feat: block private IP ranges via Network.setBlockedURLs on CDP session attach"
```

---

### Task 5: Validate URL after `wait_for_url` completes

**Files:**
- Modify: `src/mcp_server.rs` (the `wait_for` dispatch arm, `url` branch)

`wait_for_url` uses substring matching (`window.location.href.includes(pattern)`). After the match succeeds, the actual current URL might differ from what the agent expected (e.g., `wait_for url="github.com"` could match `evil-github.com`). More importantly, if the page redirected to a URL not covered by `Network.setBlockedURLs` (e.g., an allowed domain that the policy denies), we need to catch it.

**Approach:** After `wait_for_url` returns, read the actual current URL via `list_tabs` (no extra CDP session needed), then run it through `check_navigate`. If blocked, navigate to `about:blank` and error. Also update `tab_urls` to the actual URL.

- [ ] **Step 1: Write the failing test**

In `src/mcp_server.rs` tests, add:

```rust
#[test]
fn wait_for_url_policy_check_logic() {
    // Verify check_navigate would block a URL that passes substring match
    use crate::security::SecurityPolicy;
    use crate::config::SecurityConfig;
    let policy = SecurityPolicy::from_config_with_overrides(
        &SecurityConfig::default(),
        Some(vec!["github.com".into()]),
        None, None, None,
    );
    // The pattern "github" matches, but the domain "evil-github.com" is not allowed
    let actual_url = "https://evil-github.com/page";
    assert!(policy.check_navigate(actual_url).is_err(),
            "domain allowlist should block evil-github.com");
    // Whereas real github.com is allowed
    assert!(policy.check_navigate("https://github.com/page").is_ok());
}
```

- [ ] **Step 2: Run the test to verify it passes already**

```bash
cargo test wait_for_url_policy_check_logic -- --nocapture
```
Expected: PASS (this tests the policy logic, which already works; the dispatch integration is verified manually).

- [ ] **Step 3: Add URL validation after `wait_for_url` in dispatch**

In `src/mcp_server.rs`, find the `wait_for` dispatch arm. The `url` branch currently reads:
```rust
} else if let Some(url) = args["url"].as_str() {
    browser::wait_for_url(session, tid, url, timeout_ms).await?;
    Ok(format!("URL matched: {}", url))
```

Replace with:
```rust
} else if let Some(url_pattern) = args["url"].as_str() {
    browser::wait_for_url(session, tid, url_pattern, timeout_ms).await?;

    // After the URL pattern matched, validate the actual current URL against policy.
    // wait_for_url uses substring matching, so the actual URL may be different from
    // what the agent intended (e.g. "github.com" matches "evil-github.com").
    let actual_url = browser::list_tabs(&mut session.cdp).await?
        .into_iter()
        .find(|t| t.target_id == tid)
        .map(|t| t.url);

    if let Some(ref actual) = actual_url {
        let blocked = session.security_policy.as_ref()
            .and_then(|p| p.check_navigate(actual).err());
        if let Some(reason) = blocked {
            // Navigate away from the blocked page before returning error.
            let _ = browser::navigate_to_blank(&mut session.cdp, tid).await;
            // Evict stale cdp_sessions entry so next attach goes through fresh_attach cleanly.
            session.cdp_sessions.remove(tid);
            session.tab_urls.insert(tid.to_string(), "about:blank".to_string());
            return Err(crate::error::PagerunnerError::Config(
                format!("wait_for URL matched but destination is blocked: {}", reason)
            ));
        }
        // Update tab_urls with the actual URL for correct domain labeling in get_content.
        session.tab_urls.insert(tid.to_string(), actual.clone());
    }

    Ok(format!("URL matched: {}", url_pattern))
```

- [ ] **Step 4: Add `navigate_to_blank` helper in `browser.rs`**

In `src/browser.rs`, add:

```rust
/// Navigate a target to about:blank (cleanup after blocked redirect detection).
/// Does not go through policy checks — used only for cleanup.
pub async fn navigate_to_blank(cdp: &mut CdpConn, target_id: &str) -> Result<()> {
    cdp.send("Target.activateTarget", json!({ "targetId": target_id })).await.ok();
    // Use createTarget approach since we may not have a session on the blocked page.
    // Simplest: send Page.navigate on the browser-level (no session needed for about:blank).
    // Actually, we need a session. Use a raw attach via Target.attachToTarget.
    let attach = cdp.send("Target.attachToTarget", json!({
        "targetId": target_id,
        "flatten": true
    })).await?;
    let session_id = attach["sessionId"].as_str()
        .ok_or_else(|| PagerunnerError::Cdp("No sessionId".into()))?
        .to_string();
    cdp.send_on_session("Page.navigate", json!({ "url": "about:blank" }), Some(session_id)).await?;
    Ok(())
}
```

- [ ] **Step 5: Verify the code compiles**

```bash
cargo check 2>&1 | head -20
```
Expected: no errors. The borrow is safe: `session.security_policy.as_ref().and_then(|p| p.check_navigate(...).err())` returns an owned `Option<String>` — no lifetime escapes into `blocked`. Rust NLL resolves the field borrows correctly.

- [ ] **Step 6: Run all tests**

```bash
cargo test 2>&1 | tail -20
```
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add src/mcp_server.rs src/browser.rs
git commit -m "feat: validate actual URL against policy after wait_for url match"
```

---

## Verification

After all tasks:

```bash
cargo test 2>&1 | grep -E "(test .* ok|FAILED|error)"
```
Expected: all tests pass, no errors.

```bash
cargo clippy -- -D warnings 2>&1 | head -20
```
Expected: no warnings.

The redirect interception (Task 4) is verified by the existing E2E test: open a session, navigate to a URL that redirects to `http://192.168.1.1/`, and verify Chrome fails the request (the test from the previous plan that passed all 15 blocking tests should still pass; Task 4 strengthens it at the network layer).

---

## Gaps NOT addressed in this plan (require separate work)

- **Gap #4 (MEDIUM)**: `screenshot` captures hidden content. Out of scope — screenshots are intentionally full-page captures; a separate `sanitize_screenshot` mode would require vision model integration.
- **Gap #2 full CDP event streaming**: `Network.setBlockedURLs` (Task 4) handles the common case. Full event-driven `Fetch.enable` interception requires adding async event dispatch to `CdpConn`, which is a separate architectural effort.
