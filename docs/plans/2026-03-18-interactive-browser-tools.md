# Interactive Browser Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `new_tab`, `evaluate`, `click`, and `type_text` MCP tools to Conduit so AI agents can create tabs, run JavaScript, click elements, and type into pages.

**Architecture:** Each new tool follows the existing pattern: a function in `browser.rs` that calls CDP methods via `CdpConn`, a tool definition in `all_tools()`, and a dispatch case in `dispatch_tool()`. All four tools require a `session_id` and `target_id` (except `new_tab` which only needs `session_id`). The `click` and `type_text` tools use CDP `Input.dispatch*Event` methods. A helper function `attach_to_target` is extracted to DRY up the repeated attach pattern.

**Tech Stack:** Rust, tokio, serde_json, Chrome DevTools Protocol (Target, Runtime, Input, Page domains)

---

### Task 1: Extract `attach_to_target` helper

Every existing browser function repeats the same 5-line attach-and-extract-sessionId pattern. Extract it before adding more tools.

**Files:**
- Modify: `src/browser.rs:27-64` (navigate, get_content, screenshot all have identical attach blocks)

- [ ] **Step 1: Write the failing test**

```rust
// Add to src/browser.rs #[cfg(test)] mod tests
#[test]
fn test_all_tools_count() {
    // After refactor, the module should still compile and export the same functions
    // This is a compile-time check — if attach_to_target signature is wrong, tests won't build
    let _: fn(&mut CdpConn, &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + '_>> = |cdp, tid| Box::pin(attach_to_target(cdp, tid));
}
```

Actually, since this is a pure refactor with no behavior change, skip the new test — existing tests cover it.

- [ ] **Step 1: Extract the helper function**

In `src/browser.rs`, add this function before `navigate`:

```rust
async fn attach_to_target(cdp: &mut CdpConn, target_id: &str) -> Result<String> {
    let attach = cdp.send("Target.attachToTarget", json!({
        "targetId": target_id,
        "flatten": true
    })).await?;
    attach["sessionId"].as_str()
        .ok_or_else(|| ConduitError::Cdp("No sessionId from attachToTarget".into()))
        .map(|s| s.to_string())
}
```

- [ ] **Step 2: Update `navigate` to use the helper**

Replace lines 28-34 of navigate with:

```rust
pub async fn navigate(cdp: &mut CdpConn, target_id: &str, url: &str) -> Result<()> {
    let session_id = attach_to_target(cdp, target_id).await?;
    cdp.send_on_session("Page.navigate", json!({ "url": url }), Some(session_id)).await?;
    Ok(())
}
```

- [ ] **Step 3: Update `get_content` to use the helper**

```rust
pub async fn get_content(cdp: &mut CdpConn, target_id: &str) -> Result<String> {
    let session_id = attach_to_target(cdp, target_id).await?;
    let result = cdp.send_on_session("Runtime.evaluate", json!({
        "expression": "document.body.innerText",
        "returnByValue": true
    }), Some(session_id)).await?;
    Ok(result["result"]["value"].as_str().unwrap_or("").into())
}
```

- [ ] **Step 4: Update `screenshot` to use the helper**

```rust
pub async fn screenshot(cdp: &mut CdpConn, target_id: &str) -> Result<String> {
    let session_id = attach_to_target(cdp, target_id).await?;
    let result = cdp.send_on_session("Page.captureScreenshot", json!({
        "format": "png"
    }), Some(session_id)).await?;
    result["data"].as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| ConduitError::Cdp("No screenshot data".into()))
}
```

- [ ] **Step 5: Build and run existing tests**

Run: `cargo test -p conduit 2>&1`
Expected: All existing tests pass, no compilation errors.

- [ ] **Step 6: Commit**

```bash
git add src/browser.rs
git commit -m "refactor: extract attach_to_target helper in browser.rs"
```

---

### Task 2: Add `new_tab` tool

Creates a new tab in an existing Chrome session using `Target.createTarget`.

**Files:**
- Modify: `src/browser.rs` (add `new_tab` function)
- Modify: `src/mcp_server.rs` (add tool definition and dispatch)

- [ ] **Step 1: Write the unit test for tool definition**

In `src/mcp_server.rs` `mod tests`, update the existing test:

```rust
#[test]
fn test_tools_list_response() {
    let tools = all_tools();
    assert!(tools.iter().any(|t| t["name"] == "list_profiles"));
    assert!(tools.iter().any(|t| t["name"] == "open_session"));
    assert!(tools.iter().any(|t| t["name"] == "screenshot"));
    assert!(tools.iter().any(|t| t["name"] == "new_tab"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_tools_list_response -- --nocapture 2>&1`
Expected: FAIL — "new_tab" not found in tools list.

- [ ] **Step 3: Add `new_tab` function to `browser.rs`**

Add after `list_tabs`:

```rust
pub async fn new_tab(cdp: &mut CdpConn, url: &str) -> Result<TabInfo> {
    let result = cdp.send("Target.createTarget", json!({
        "url": url
    })).await?;
    let target_id = result["targetId"].as_str()
        .ok_or_else(|| ConduitError::Cdp("No targetId from createTarget".into()))?
        .to_string();

    // Fetch full target info to return title/url
    let targets = list_tabs(cdp).await?;
    targets.into_iter()
        .find(|t| t.target_id == target_id)
        .ok_or_else(|| ConduitError::Cdp("Created tab not found in targets".into()))
}
```

- [ ] **Step 4: Add tool definition to `all_tools()` in `mcp_server.rs`**

Add after the `list_tabs` tool definition (after line 53):

```rust
json!({
    "name": "new_tab",
    "description": "Open a new tab in a Chrome session. Returns target_id for the new tab.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "session_id": { "type": "string" },
            "url": { "type": "string", "description": "URL to open (default: about:blank)" }
        },
        "required": ["session_id"]
    }
}),
```

- [ ] **Step 5: Add dispatch case in `dispatch_tool()`**

Add after the `"list_tabs"` match arm (after line 243):

```rust
"new_tab" => {
    let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
    let url = args["url"].as_str().unwrap_or("about:blank");
    let mut mgr = sessions.lock().await;
    let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
    let tab = browser::new_tab(&mut session.cdp, url).await?;
    Ok(serde_json::to_string_pretty(&json!({
        "target_id": tab.target_id,
        "url": tab.url,
        "title": tab.title,
    }))?)
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1`
Expected: All tests pass including updated `test_tools_list_response`.

- [ ] **Step 7: Commit**

```bash
git add src/browser.rs src/mcp_server.rs
git commit -m "feat: add new_tab tool to create tabs in Chrome sessions"
```

---

### Task 3: Add `evaluate` tool

Runs arbitrary JavaScript in a tab via `Runtime.evaluate`. This is the most powerful tool — with JS you can query the DOM, click elements, fill forms, etc.

**Files:**
- Modify: `src/browser.rs` (add `evaluate` function)
- Modify: `src/mcp_server.rs` (add tool definition and dispatch)

- [ ] **Step 1: Add test for tool definition**

In `src/mcp_server.rs` `mod tests`, add to the existing test:

```rust
assert!(tools.iter().any(|t| t["name"] == "evaluate"));
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_tools_list_response -- --nocapture 2>&1`
Expected: FAIL

- [ ] **Step 3: Add `evaluate` function to `browser.rs`**

Add after `get_content`:

```rust
pub async fn evaluate(cdp: &mut CdpConn, target_id: &str, expression: &str) -> Result<Value> {
    let session_id = attach_to_target(cdp, target_id).await?;
    let result = cdp.send_on_session("Runtime.evaluate", json!({
        "expression": expression,
        "returnByValue": true,
        "awaitPromise": true
    }), Some(session_id)).await?;

    if let Some(exception) = result.get("exceptionDetails") {
        let msg = exception["exception"]["description"]
            .as_str()
            .or_else(|| exception["text"].as_str())
            .unwrap_or("Unknown JS error");
        return Err(ConduitError::Cdp(format!("JS error: {}", msg)));
    }

    Ok(result["result"].clone())
}
```

- [ ] **Step 4: Add tool definition to `all_tools()`**

```rust
json!({
    "name": "evaluate",
    "description": "Execute JavaScript in a tab and return the result. Use for DOM queries, clicking elements, filling forms, or any page interaction.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "session_id": { "type": "string" },
            "target_id": { "type": "string" },
            "expression": { "type": "string", "description": "JavaScript expression to evaluate" }
        },
        "required": ["session_id", "target_id", "expression"]
    }
}),
```

- [ ] **Step 5: Add dispatch case**

```rust
"evaluate" => {
    let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
    let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
    let expr = args["expression"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing expression".into()))?;
    let mut mgr = sessions.lock().await;
    let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
    let result = browser::evaluate(&mut session.cdp, tid, expr).await?;
    Ok(serde_json::to_string_pretty(&result)?)
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/browser.rs src/mcp_server.rs
git commit -m "feat: add evaluate tool for JavaScript execution in tabs"
```

---

### Task 4: Add `click` tool

Clicks at coordinates or on a CSS selector. Uses `Runtime.evaluate` to resolve selector to coordinates, then `Input.dispatchMouseEvent` for the actual click.

**Files:**
- Modify: `src/browser.rs` (add `click` function)
- Modify: `src/mcp_server.rs` (add tool definition and dispatch)

- [ ] **Step 1: Add test for tool definition**

```rust
assert!(tools.iter().any(|t| t["name"] == "click"));
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_tools_list_response -- --nocapture 2>&1`
Expected: FAIL

- [ ] **Step 3: Add `click` function to `browser.rs`**

```rust
pub async fn click(cdp: &mut CdpConn, target_id: &str, selector: &str) -> Result<()> {
    let session_id = attach_to_target(cdp, target_id).await?;

    // Resolve selector to coordinates via JS
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

    // mousePressed + mouseReleased = a full click
    cdp.send_on_session("Input.dispatchMouseEvent", json!({
        "type": "mousePressed",
        "x": x,
        "y": y,
        "button": "left",
        "clickCount": 1
    }), Some(session_id.clone())).await?;

    cdp.send_on_session("Input.dispatchMouseEvent", json!({
        "type": "mouseReleased",
        "x": x,
        "y": y,
        "button": "left",
        "clickCount": 1
    }), Some(session_id)).await?;

    Ok(())
}
```

- [ ] **Step 4: Add tool definition to `all_tools()`**

```rust
json!({
    "name": "click",
    "description": "Click on an element matching a CSS selector",
    "inputSchema": {
        "type": "object",
        "properties": {
            "session_id": { "type": "string" },
            "target_id": { "type": "string" },
            "selector": { "type": "string", "description": "CSS selector (e.g. \"button.submit\", \"#login\", \"a[href='/home']\")" }
        },
        "required": ["session_id", "target_id", "selector"]
    }
}),
```

- [ ] **Step 5: Add dispatch case**

```rust
"click" => {
    let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
    let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
    let selector = args["selector"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing selector".into()))?;
    let mut mgr = sessions.lock().await;
    let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
    browser::click(&mut session.cdp, tid, selector).await?;
    Ok(format!("Clicked: {}", selector))
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/browser.rs src/mcp_server.rs
git commit -m "feat: add click tool for CSS selector-based clicking"
```

---

### Task 5: Add `type_text` tool

Types text into the currently focused element or a specified selector. Uses `Input.dispatchKeyEvent` to send individual key events.

**Files:**
- Modify: `src/browser.rs` (add `type_text` function)
- Modify: `src/mcp_server.rs` (add tool definition and dispatch)

- [ ] **Step 1: Add test for tool definition**

```rust
assert!(tools.iter().any(|t| t["name"] == "type_text"));
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_tools_list_response -- --nocapture 2>&1`
Expected: FAIL

- [ ] **Step 3: Add `type_text` function to `browser.rs`**

```rust
pub async fn type_text(cdp: &mut CdpConn, target_id: &str, text: &str, selector: Option<&str>) -> Result<()> {
    let session_id = attach_to_target(cdp, target_id).await?;

    // If a selector is provided, focus it first
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
    }

    // Type each character as keyDown + keyUp
    for ch in text.chars() {
        cdp.send_on_session("Input.dispatchKeyEvent", json!({
            "type": "keyDown",
            "text": ch.to_string()
        }), Some(session_id.clone())).await?;

        cdp.send_on_session("Input.dispatchKeyEvent", json!({
            "type": "keyUp",
            "text": ch.to_string()
        }), Some(session_id.clone())).await?;
    }

    Ok(())
}
```

- [ ] **Step 4: Add tool definition to `all_tools()`**

```rust
json!({
    "name": "type_text",
    "description": "Type text into the focused element or a specific element by CSS selector",
    "inputSchema": {
        "type": "object",
        "properties": {
            "session_id": { "type": "string" },
            "target_id": { "type": "string" },
            "text": { "type": "string", "description": "Text to type" },
            "selector": { "type": "string", "description": "Optional CSS selector to focus before typing" }
        },
        "required": ["session_id", "target_id", "text"]
    }
}),
```

- [ ] **Step 5: Add dispatch case**

```rust
"type_text" => {
    let sid = args["session_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing session_id".into()))?;
    let tid = args["target_id"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing target_id".into()))?;
    let text = args["text"].as_str().ok_or_else(|| crate::error::ConduitError::Config("Missing text".into()))?;
    let selector = args["selector"].as_str();
    let mut mgr = sessions.lock().await;
    let session = mgr.get_mut(sid).ok_or_else(|| crate::error::ConduitError::SessionNotFound(sid.into()))?;
    browser::type_text(&mut session.cdp, tid, text, selector).await?;
    Ok(format!("Typed {} chars", text.len()))
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/browser.rs src/mcp_server.rs
git commit -m "feat: add type_text tool for keyboard input"
```

---

### Task 6: Build release binary and verify

**Files:**
- None (build + manual verification)

- [ ] **Step 1: Build release binary**

Run: `cargo build --release 2>&1`
Expected: Compiles with no errors (warnings OK).

- [ ] **Step 2: Verify all 12 tools are listed**

Run: `echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | /Users/stas/Code/conduit/target/release/conduit mcp 2>/dev/null | head -1`
Then: `echo '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | ...`

Expected: 12 tools in response (8 original + new_tab, evaluate, click, type_text).

- [ ] **Step 3: Commit (if any cleanup was needed)**

```bash
git add -A
git commit -m "chore: build release with interactive browser tools"
```
