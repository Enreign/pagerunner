# Stealth Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `stealth` mode to Conduit sessions that makes Chrome automation undetectable — clean browser fingerprint at launch, anti-detection JS injected before every page, and human-like behavioral delays on interactions.

**Architecture:** Stealth is a per-session boolean, passed to `open_session`. It flows through three layers: (1) `chrome.rs` uses minimal launch flags instead of automation-telltale flags, (2) after session opens, `Page.addScriptToEvaluateOnNewDocument` injects anti-fingerprint JS that runs before any page script, (3) `browser.rs` functions check session stealth flag and add realistic delays before clicks/typing/navigation. The stealth flag is stored on `Session` and checked by browser functions.

**Tech Stack:** Rust, tokio, serde_json, Chrome DevTools Protocol (Page, Runtime, Input domains), `rand` crate for jittered delays

---

## File Structure

| File | Responsibility | Changes |
|------|---------------|---------|
| `src/stealth.rs` | **NEW** — Anti-detection JS injection + human delay helpers |
| `src/chrome.rs` | Chrome launch flags differ based on stealth bool |
| `src/session.rs` | Session stores `stealth: bool`, passes it through |
| `src/browser.rs` | Functions add delays when session is stealth |
| `src/mcp_server.rs` | `open_session` accepts optional `stealth` param, passes to browser fns |
| `Cargo.toml` | Add `rand` dependency |

---

### Task 1: Add `rand` dependency and stealth flag to Session

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/session.rs`

- [ ] **Step 1: Add `rand` to Cargo.toml**

Add to `[dependencies]`:
```toml
rand = "0.8"
```

- [ ] **Step 2: Add `stealth` field to `Session` and `SessionInfo`**

In `src/session.rs`, add `pub stealth: bool` to both `Session` (after `profile_display_name`) and `SessionInfo` (after `profile_display_name`).

In `Session`:
```rust
pub struct Session {
    pub id: SessionId,
    pub profile_name: String,
    pub profile_display_name: String,
    pub stealth: bool,
    chrome: ChromeProcess,
    pub cdp: CdpConn,
}
```

In `SessionInfo`:
```rust
pub struct SessionInfo {
    pub id: SessionId,
    pub profile_name: String,
    pub profile_display_name: String,
    pub stealth: bool,
}
```

- [ ] **Step 3: Update `SessionManager::open` to accept stealth parameter**

Change signature and body:
```rust
pub async fn open(&mut self, profile: &ChromeProfile, stealth: bool) -> Result<SessionId> {
    let result = crate::chrome::ChromeProcess::spawn(&profile.user_data_dir, stealth).await?;
    let cdp = CdpConn::new(result.cmd_write, result.evt_read);
    let id = Uuid::new_v4().to_string();
    self.sessions.insert(id.clone(), Session {
        id: id.clone(),
        profile_name: profile.name.clone(),
        profile_display_name: profile.display_name.clone(),
        stealth,
        chrome: result.process,
        cdp,
    });
    Ok(id)
}
```

- [ ] **Step 4: Update `list()` to include stealth**

```rust
pub fn list(&self) -> Vec<SessionInfo> {
    self.sessions.values().map(|s| SessionInfo {
        id: s.id.clone(),
        profile_name: s.profile_name.clone(),
        profile_display_name: s.profile_display_name.clone(),
        stealth: s.stealth,
    }).collect()
}
```

- [ ] **Step 5: Fix tests in session.rs**

Update both tests to pass `false` as stealth:
```rust
let id = manager.open(&test_profile(), false).await
```

- [ ] **Step 6: Build (will fail on chrome.rs — expected)**

Run: `cargo check 2>&1`
Expected: Error in `chrome.rs` — `spawn` doesn't accept stealth param yet. This confirms the wiring is correct.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/session.rs
git commit -m "feat: add stealth flag to Session and SessionManager"
```

---

### Task 2: Stealth-aware Chrome launch flags

**Files:**
- Modify: `src/chrome.rs`

- [ ] **Step 1: Update `ChromeProcess::spawn` to accept stealth parameter**

Change signature to:
```rust
pub async fn spawn(user_data_dir: &str, stealth: bool) -> Result<SpawnResult> {
```

- [ ] **Step 2: Replace the hardcoded args block with conditional flags**

Replace the `cmd.args([...])` block (lines 39-50) with:

```rust
        let mut args: Vec<String> = vec![
            "--remote-debugging-pipe".into(),
            format!("--user-data-dir={}", user_data_dir),
            "about:blank".into(),
        ];

        if stealth {
            // Minimal flags — look like a normal Chrome launch
            args.push("--disable-blink-features=AutomationControlled".into());
        } else {
            // Transparent mode — standard automation flags
            args.push("--no-first-run".into());
            args.push("--no-default-browser-check".into());
            args.push("--disable-sync".into());
            args.push("--disable-extensions".into());
        }

        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
```

- [ ] **Step 3: Fix tests in chrome.rs and browser.rs**

Update `test_spawn_and_kill_chrome` in `src/chrome.rs` to pass `false`:
```rust
let mut result = ChromeProcess::spawn(profile_dir.path().to_str().unwrap(), false)
```

Also update `test_list_tabs_returns_at_least_one` in `src/browser.rs` (line 185):
```rust
let result = crate::chrome::ChromeProcess::spawn("/tmp/conduit-browser-test", false).await.unwrap();
```

- [ ] **Step 4: Build and run tests**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/chrome.rs
git commit -m "feat: stealth-aware Chrome launch flags"
```

---

### Task 3: Create `stealth.rs` — anti-detection JS injection

**Files:**
- Create: `src/stealth.rs`
- Modify: `src/main.rs` (add `mod stealth;`)

- [ ] **Step 1: Create `src/stealth.rs` with the anti-detection injection script**

```rust
use serde_json::json;
use crate::cdp::CdpConn;
use crate::error::Result;

/// JavaScript injected via Page.addScriptToEvaluateOnNewDocument.
/// Runs before ANY page script on every navigation.
const STEALTH_JS: &str = r#"
// 1. Hide navigator.webdriver
Object.defineProperty(navigator, 'webdriver', {
    get: () => false,
    configurable: true,
});

// 2. Ensure navigator.plugins is populated (empty = bot signal)
if (navigator.plugins.length === 0) {
    Object.defineProperty(navigator, 'plugins', {
        get: () => [
            { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer' },
            { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai' },
            { name: 'Native Client', filename: 'internal-nacl-plugin' },
        ],
        configurable: true,
    });
}

// 3. Ensure navigator.languages is populated
if (!navigator.languages || navigator.languages.length === 0) {
    Object.defineProperty(navigator, 'languages', {
        get: () => ['en-US', 'en'],
        configurable: true,
    });
}

// 4. Patch Permissions.query to hide "denied" notification status
// (automation mode denies notifications by default — detectable)
const originalQuery = window.Permissions?.prototype?.query;
if (originalQuery) {
    window.Permissions.prototype.query = function(parameters) {
        if (parameters.name === 'notifications') {
            return Promise.resolve({ state: Notification.permission });
        }
        return originalQuery.call(this, parameters);
    };
}

// 5. Scrub CDP artifacts (window.cdc_ prefixed variables)
for (const key of Object.keys(window)) {
    if (key.startsWith('cdc_') || key.startsWith('__cdc_')) {
        delete window[key];
    }
}
"#;

/// Inject stealth scripts into a CDP session so they run before every page load.
/// Must be called once after session opens — persists across navigations.
pub async fn inject(cdp: &mut CdpConn, session_id: &str) -> Result<()> {
    // Need to attach to the browser-level target first to set up
    // addScriptToEvaluateOnNewDocument on a page session
    cdp.send_on_session(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": STEALTH_JS }),
        Some(session_id.to_string()),
    ).await?;

    // Also evaluate immediately on the current page
    cdp.send_on_session(
        "Runtime.evaluate",
        json!({
            "expression": STEALTH_JS,
            "returnByValue": true,
        }),
        Some(session_id.to_string()),
    ).await?;

    Ok(())
}

/// Random delay between `min_ms` and `max_ms` milliseconds.
pub async fn human_delay(min_ms: u64, max_ms: u64) {
    use rand::Rng;
    let ms = rand::thread_rng().gen_range(min_ms..=max_ms);
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}
```

- [ ] **Step 2: Add `mod stealth;` to `src/main.rs`**

Add after the other `mod` declarations.

- [ ] **Step 3: Build**

Run: `cargo check 2>&1`
Expected: Compiles.

- [ ] **Step 4: Commit**

```bash
git add src/stealth.rs src/main.rs
git commit -m "feat: stealth.rs — anti-detection JS injection and human delay"
```

---

### Task 4: Make `attach_to_target` stealth-aware

The key insight: every browser function calls `attach_to_target` before interacting with a tab. By injecting stealth scripts here, **every tab automatically gets stealth** — including tabs created via `new_tab`. This also removes the need for separate injection during session open.

**Files:**
- Modify: `src/browser.rs` (update `attach_to_target` to accept stealth and inject)

- [ ] **Step 1: Make `attach_to_target` public and stealth-aware**

Replace the existing `attach_to_target` in `src/browser.rs` with:

```rust
pub async fn attach_to_target(cdp: &mut CdpConn, target_id: &str, stealth: bool) -> Result<String> {
    let attach = cdp.send("Target.attachToTarget", json!({
        "targetId": target_id,
        "flatten": true
    })).await?;
    let session_id = attach["sessionId"].as_str()
        .ok_or_else(|| ConduitError::Cdp("No sessionId from attachToTarget".into()))?
        .to_string();

    if stealth {
        crate::stealth::inject(cdp, &session_id).await?;
    }

    Ok(session_id)
}
```

- [ ] **Step 2: Update all callers of `attach_to_target` within browser.rs**

For functions that DON'T need stealth delays (`get_content`, `evaluate`, `screenshot`), pass `false`:
```rust
let session_id = attach_to_target(cdp, target_id, false).await?;
```

For functions that WILL get stealth (navigate, click, type_text) — these will be updated in Task 5 to accept and pass the stealth flag.

Temporarily pass `false` for all callers so it compiles:
```rust
// navigate, get_content, evaluate, screenshot, click, type_text — all pass false for now
let session_id = attach_to_target(cdp, target_id, false).await?;
```

- [ ] **Step 3: Build and test**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/browser.rs
git commit -m "feat: make attach_to_target stealth-aware for per-tab injection"
```

---

### Task 5: Add human-like delays to browser functions in stealth mode

**Files:**
- Modify: `src/browser.rs`

The key insight: browser functions currently take `&mut CdpConn`. To check stealth, they need the stealth flag. Change their signatures to accept `stealth: bool` — the caller (dispatch_tool) reads it from the session.

- [ ] **Step 1: Update `navigate` to add post-navigation settle delay in stealth mode**

```rust
pub async fn navigate(cdp: &mut CdpConn, target_id: &str, url: &str, stealth: bool) -> Result<()> {
    let session_id = attach_to_target(cdp, target_id, stealth).await?;
    cdp.send_on_session("Page.navigate", json!({ "url": url }), Some(session_id)).await?;
    if stealth {
        // Wait for page to settle like a human would
        crate::stealth::human_delay(800, 2000).await;
    }
    Ok(())
}
```

- [ ] **Step 2: Update `click` to add pre-click delay in stealth mode**

Add before the mousePressed event and after coordinate resolution:

```rust
pub async fn click(cdp: &mut CdpConn, target_id: &str, selector: &str, stealth: bool) -> Result<()> {
    let session_id = attach_to_target(cdp, target_id, stealth).await?;

    let js = format!(
        r#"(() => {{
            const el = document.querySelector({});
            if (!el) return null;
            const r = el.getBoundingClientRect();
            return {{ x: r.x + r.width / 2, y: r.y + r.height / 2 }};
        }})()"#,
        serde_json::to_string(selector).unwrap_or_else(|_| format!("\"{}\"", selector))
    );

    let result = cdp.send_on_session("Runtime.evaluate", json!({
        "expression": js,
        "returnByValue": true
    }), Some(session_id.clone())).await?;

    let value = &result["result"]["value"];
    if value.is_null() {
        return Err(ConduitError::Cdp(format!("Selector not found: {}", selector)));
    }

    let x = value["x"].as_f64()
        .ok_or_else(|| ConduitError::Cdp("No x coordinate".into()))?;
    let y = value["y"].as_f64()
        .ok_or_else(|| ConduitError::Cdp("No y coordinate".into()))?;

    if stealth {
        // Human hesitation before clicking
        crate::stealth::human_delay(100, 400).await;
    }

    cdp.send_on_session("Input.dispatchMouseEvent", json!({
        "type": "mousePressed",
        "x": x, "y": y, "button": "left", "clickCount": 1
    }), Some(session_id.clone())).await?;

    if stealth {
        // Humans hold the button briefly
        crate::stealth::human_delay(50, 150).await;
    }

    cdp.send_on_session("Input.dispatchMouseEvent", json!({
        "type": "mouseReleased",
        "x": x, "y": y, "button": "left", "clickCount": 1
    }), Some(session_id)).await?;

    Ok(())
}
```

- [ ] **Step 3: Update `type_text` to type character-by-character in stealth mode**

In stealth mode, use per-character `Input.dispatchKeyEvent` with jittered delays instead of instant `Input.insertText`:

```rust
pub async fn type_text(cdp: &mut CdpConn, target_id: &str, text: &str, selector: Option<&str>, stealth: bool) -> Result<()> {
    let session_id = attach_to_target(cdp, target_id, stealth).await?;

    if let Some(sel) = selector {
        let js = format!(
            r#"(() => {{
                const el = document.querySelector({});
                if (!el) return false;
                el.focus();
                return true;
            }})()"#,
            serde_json::to_string(sel).unwrap_or_else(|_| format!("\"{}\"", sel))
        );
        let result = cdp.send_on_session("Runtime.evaluate", json!({
            "expression": js,
            "returnByValue": true
        }), Some(session_id.clone())).await?;

        if result["result"]["value"].as_bool() != Some(true) {
            return Err(ConduitError::Cdp(format!("Selector not found: {}", sel)));
        }

        if stealth {
            crate::stealth::human_delay(100, 300).await;
        }
    }

    if stealth {
        // Type character by character with realistic delays
        for ch in text.chars() {
            cdp.send_on_session("Input.dispatchKeyEvent", json!({
                "type": "keyDown",
                "text": ch.to_string()
            }), Some(session_id.clone())).await?;

            cdp.send_on_session("Input.dispatchKeyEvent", json!({
                "type": "keyUp",
                "text": ch.to_string()
            }), Some(session_id.clone())).await?;

            // 30-120ms between keystrokes (real human typing speed)
            crate::stealth::human_delay(30, 120).await;
        }
    } else {
        cdp.send_on_session("Input.insertText", json!({
            "text": text
        }), Some(session_id)).await?;
    }

    Ok(())
}
```

- [ ] **Step 4: Keep other functions unchanged**

`list_tabs`, `new_tab`, `get_content`, `evaluate`, `screenshot` do NOT need stealth delays — they are observation tools, not interaction tools.

- [ ] **Step 5: Build (will fail — callers need updating)**

Run: `cargo check 2>&1`
Expected: Errors in `mcp_server.rs` — `navigate`, `click`, `type_text` now require `stealth` param.

- [ ] **Step 6: Commit**

```bash
git add src/browser.rs
git commit -m "feat: human-like delays in browser functions when stealth is on"
```

---

### Task 6: Wire stealth through MCP server

**Files:**
- Modify: `src/mcp_server.rs`

- [ ] **Step 1: Update `open_session` to accept `stealth` parameter**

```rust
"open_session" => {
    let profile_name = args["profile"].as_str()
        .ok_or_else(|| crate::error::ConduitError::Config("Missing profile".into()))?;
    let stealth = args["stealth"].as_bool().unwrap_or(false);
    let profile = config.find_profile(profile_name)
        .ok_or_else(|| crate::error::ConduitError::ProfileNotFound(profile_name.into()))?
        .clone();
    let mut mgr = sessions.lock().await;
    let id = mgr.open(&profile, stealth).await?;
    Ok(format!("Session opened: {} (stealth: {})", id, stealth))
}
```

- [ ] **Step 2: Update `open_session` tool definition to include stealth parameter**

Add to the `open_session` inputSchema properties:
```rust
"stealth": { "type": "boolean", "description": "Enable stealth mode — hides automation signals and adds human-like delays (default: false)" }
```

- [ ] **Step 3: Update `list_sessions` to show stealth flag**

```rust
"list_sessions" => {
    let mgr = sessions.lock().await;
    let list: Vec<Value> = mgr.list().iter().map(|s| json!({
        "id": s.id,
        "profile": s.profile_name,
        "display_name": s.profile_display_name,
        "stealth": s.stealth,
    })).collect();
    Ok(serde_json::to_string_pretty(&list)?)
}
```

- [ ] **Step 4: Update `navigate`, `click`, `type_text` dispatch cases to pass stealth**

For each of these three tool dispatch arms, add stealth extraction from the session:

For `navigate`:
```rust
"navigate" => {
    let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
    let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
    let url = args["url"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing url".into()))?;
    let mut mgr = sessions.lock().await;
    let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
    let stealth = session.stealth;
    browser::navigate(&mut session.cdp, tid, url, stealth).await?;
    Ok(format!("Navigated {} to {}", tid, url))
}
```

For `click`:
```rust
"click" => {
    let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
    let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
    let selector = args["selector"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing selector".into()))?;
    let mut mgr = sessions.lock().await;
    let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
    let stealth = session.stealth;
    browser::click(&mut session.cdp, tid, selector, stealth).await?;
    Ok(format!("Clicked: {}", selector))
}
```

For `type_text`:
```rust
"type_text" => {
    let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
    let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
    let text = args["text"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing text".into()))?;
    let selector = args["selector"].as_str();
    let mut mgr = sessions.lock().await;
    let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
    let stealth = session.stealth;
    browser::type_text(&mut session.cdp, tid, text, selector, stealth).await?;
    Ok(format!("Typed {} chars", text.len()))
}
```

- [ ] **Step 5: Build and run all tests**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/mcp_server.rs
git commit -m "feat: wire stealth mode through MCP server"
```

---

### Task 7: Build release and verify

**Files:**
- None (build + verification)

- [ ] **Step 1: Build release binary**

Run: `cargo build --release 2>&1`
Expected: Compiles with no errors.

- [ ] **Step 2: Verify tools list shows stealth parameter on open_session**

Run: `printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}\n' | /Users/stas/Code/conduit/target/release/conduit mcp 2>/dev/null | tail -1 | python3 -c "import json,sys; d=json.load(sys.stdin); t=[x for x in d['result']['tools'] if x['name']=='open_session'][0]; print(json.dumps(t['inputSchema'], indent=2))"`

Expected: `stealth` boolean property visible in schema.

- [ ] **Step 3: Commit (if cleanup needed)**

```bash
git add -A && git commit -m "chore: release build with stealth mode"
```
