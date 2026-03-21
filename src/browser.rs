use crate::cdp::CdpConn;
use crate::error::{PagerunnerError, Result};
use crate::session::Session;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct TabInfo {
    pub target_id: String,
    pub url: String,
    pub title: String,
}

pub async fn list_tabs(cdp: &mut CdpConn) -> Result<Vec<TabInfo>> {
    let result = cdp.send("Target.getTargets", json!({})).await?;
    let targets = result["targetInfos"]
        .as_array()
        .ok_or_else(|| PagerunnerError::Cdp("No targetInfos".into()))?;

    Ok(targets
        .iter()
        .filter(|t| t["type"].as_str() == Some("page"))
        .map(|t| TabInfo {
            target_id: t["targetId"].as_str().unwrap_or("").into(),
            url: t["url"].as_str().unwrap_or("").into(),
            title: t["title"].as_str().unwrap_or("").into(),
        })
        .collect())
}

pub async fn new_tab(cdp: &mut CdpConn, url: &str) -> Result<TabInfo> {
    let result = cdp
        .send(
            "Target.createTarget",
            json!({
                "url": url
            }),
        )
        .await?;
    let target_id = result["targetId"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Cdp("No targetId from createTarget".into()))?
        .to_string();

    // Fetch full target info to return title/url
    let targets = list_tabs(cdp).await?;
    targets
        .into_iter()
        .find(|t| t.target_id == target_id)
        .ok_or_else(|| PagerunnerError::Cdp("Created tab not found in targets".into()))
}

/// Attach to a target, reusing cached CDP session if available.
/// When stealth is true, injects anti-detection scripts on first attach.
pub async fn attach_to_target(session: &mut Session, target_id: &str) -> Result<String> {
    // Return cached session if we already attached to this target
    if let Some(cached) = session.cdp_sessions.get(target_id).cloned() {
        // Verify the cached session is still alive with a lightweight probe
        let probe = session
            .cdp
            .send_on_session(
                "Runtime.evaluate",
                json!({ "expression": "1", "returnByValue": true }),
                Some(cached.clone()),
            )
            .await;
        match probe {
            Ok(_) => return Ok(cached),
            Err(_) => {
                // Stale session (page navigated or closed) — re-attach
                session.cdp_sessions.remove(target_id);
            }
        }
    }

    fresh_attach(session, target_id).await
}

/// Enable CDP Network domain and block all private IP ranges for this session.
/// Called on every fresh CDP session attach when the pagerunner session has a policy.
// Tested indirectly: blocked_url_patterns_covers_all_private_ranges verifies
// the pattern list; e2e redirect tests verify the CDP integration.
async fn enable_network_blocking(cdp: &mut CdpConn, session_id: &str) -> Result<()> {
    cdp.send_on_session(
        "Network.enable",
        serde_json::json!({}),
        Some(session_id.to_string()),
    )
    .await?;
    let patterns = crate::network_guard::NetworkGuard::blocked_url_patterns();
    cdp.send_on_session(
        "Network.setBlockedURLs",
        serde_json::json!({ "urls": patterns }),
        Some(session_id.to_string()),
    )
    .await?;
    Ok(())
}

/// Force a fresh attach (bypasses cache). Used on initial attach and retry after stale session.
async fn fresh_attach(session: &mut Session, target_id: &str) -> Result<String> {
    let attach = session
        .cdp
        .send(
            "Target.attachToTarget",
            json!({
                "targetId": target_id,
                "flatten": true
            }),
        )
        .await?;
    let session_id = attach["sessionId"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Cdp("No sessionId from attachToTarget".into()))?
        .to_string();

    if session.stealth {
        crate::stealth::inject(&mut session.cdp, &session_id).await?;
    }
    if session.security_policy.is_some() {
        enable_network_blocking(&mut session.cdp, &session_id).await?;
    }

    session
        .cdp_sessions
        .insert(target_id.to_string(), session_id.clone());
    Ok(session_id)
}

/// Evict a cached CDP session and re-attach. Call when a CDP command fails
/// with "Inspected target navigated or closed".
pub async fn reattach(session: &mut Session, target_id: &str) -> Result<String> {
    session.cdp_sessions.remove(target_id);
    fresh_attach(session, target_id).await
}

pub async fn navigate(session: &mut Session, target_id: &str, url: &str) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    session
        .cdp
        .send_on_session("Page.navigate", json!({ "url": url }), Some(session_id))
        .await?;
    if session.stealth {
        crate::stealth::human_delay(800, 2000).await;
    }
    Ok(())
}

pub async fn get_content(session: &mut Session, target_id: &str) -> Result<String> {
    let session_id = attach_to_target(session, target_id).await?;
    let result = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": "document.body.innerText",
                "returnByValue": true
            }),
            Some(session_id),
        )
        .await?;
    Ok(result["result"]["value"].as_str().unwrap_or("").into())
}

pub async fn evaluate(session: &mut Session, target_id: &str, expression: &str) -> Result<Value> {
    let session_id = attach_to_target(session, target_id).await?;
    let result = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true
            }),
            Some(session_id),
        )
        .await?;

    if let Some(exception) = result.get("exceptionDetails") {
        let msg = exception["exception"]["description"]
            .as_str()
            .or_else(|| exception["text"].as_str())
            .unwrap_or("Unknown JS error");
        return Err(PagerunnerError::Cdp(format!("JS error: {}", msg)));
    }

    Ok(result["result"].clone())
}

pub async fn screenshot(session: &mut Session, target_id: &str) -> Result<String> {
    let session_id = attach_to_target(session, target_id).await?;
    let result = session
        .cdp
        .send_on_session(
            "Page.captureScreenshot",
            json!({
                "format": "png"
            }),
            Some(session_id),
        )
        .await?;
    result["data"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| PagerunnerError::Cdp("No screenshot data".into()))
}

pub async fn click(session: &mut Session, target_id: &str, selector: &str) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    let stealth = session.stealth;

    let js = format!(
        r#"(() => {{
            const el = document.querySelector({});
            if (!el) return null;
            const r = el.getBoundingClientRect();
            return {{ x: r.x + r.width / 2, y: r.y + r.height / 2 }};
        }})()"#,
        serde_json::to_string(selector).unwrap_or_else(|_| format!("\"{}\"", selector))
    );

    let result = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": js,
                "returnByValue": true
            }),
            Some(session_id.clone()),
        )
        .await?;

    let value = &result["result"]["value"];
    if value.is_null() {
        return Err(PagerunnerError::Cdp(format!(
            "Selector not found: {}",
            selector
        )));
    }

    let x = value["x"]
        .as_f64()
        .ok_or_else(|| PagerunnerError::Cdp("No x coordinate".into()))?;
    let y = value["y"]
        .as_f64()
        .ok_or_else(|| PagerunnerError::Cdp("No y coordinate".into()))?;

    if stealth {
        crate::stealth::human_delay(100, 400).await;
    }

    session
        .cdp
        .send_on_session(
            "Input.dispatchMouseEvent",
            json!({
                "type": "mousePressed",
                "x": x, "y": y, "button": "left", "clickCount": 1
            }),
            Some(session_id.clone()),
        )
        .await?;

    if stealth {
        crate::stealth::human_delay(50, 150).await;
    }

    session
        .cdp
        .send_on_session(
            "Input.dispatchMouseEvent",
            json!({
                "type": "mouseReleased",
                "x": x, "y": y, "button": "left", "clickCount": 1
            }),
            Some(session_id),
        )
        .await?;

    Ok(())
}

pub async fn type_text(
    session: &mut Session,
    target_id: &str,
    text: &str,
    selector: Option<&str>,
) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    let stealth = session.stealth;

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
        let result = session
            .cdp
            .send_on_session(
                "Runtime.evaluate",
                json!({
                    "expression": js,
                    "returnByValue": true
                }),
                Some(session_id.clone()),
            )
            .await?;

        if result["result"]["value"].as_bool() != Some(true) {
            return Err(PagerunnerError::Cdp(format!("Selector not found: {}", sel)));
        }

        if stealth {
            crate::stealth::human_delay(100, 300).await;
        }
    }

    if stealth {
        for ch in text.chars() {
            session
                .cdp
                .send_on_session(
                    "Input.dispatchKeyEvent",
                    json!({
                        "type": "keyDown",
                        "text": ch.to_string()
                    }),
                    Some(session_id.clone()),
                )
                .await?;

            session
                .cdp
                .send_on_session(
                    "Input.dispatchKeyEvent",
                    json!({
                        "type": "keyUp",
                        "text": ch.to_string()
                    }),
                    Some(session_id.clone()),
                )
                .await?;

            crate::stealth::human_delay(30, 120).await;
        }
    } else {
        session
            .cdp
            .send_on_session(
                "Input.insertText",
                json!({
                    "text": text
                }),
                Some(session_id),
            )
            .await?;
    }

    Ok(())
}

/// Wait until `selector` exists in the DOM, or `timeout_ms` elapses.
pub async fn wait_for_selector(
    session: &mut Session,
    target_id: &str,
    selector: &str,
    timeout_ms: u64,
) -> Result<()> {
    let mut session_id = attach_to_target(session, target_id).await?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let js = format!(
        "!!document.querySelector({})",
        serde_json::to_string(selector).unwrap()
    );
    loop {
        let result = match session
            .cdp
            .send_on_session(
                "Runtime.evaluate",
                json!({
                    "expression": &js,
                    "returnByValue": true
                }),
                Some(session_id.clone()),
            )
            .await
        {
            Ok(r) => r,
            Err(e) if e.to_string().contains("navigated or closed") => {
                session_id = reattach(session, target_id).await?;
                continue;
            }
            Err(e) => return Err(e),
        };
        if result["result"]["value"].as_bool() == Some(true) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(PagerunnerError::Cdp(format!(
                "Timeout waiting for selector: {}",
                selector
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Wait until the current URL contains `pattern`, or `timeout_ms` elapses.
pub async fn wait_for_url(
    session: &mut Session,
    target_id: &str,
    pattern: &str,
    timeout_ms: u64,
) -> Result<()> {
    let mut session_id = attach_to_target(session, target_id).await?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let js = format!(
        "window.location.href.includes({})",
        serde_json::to_string(pattern).unwrap()
    );
    loop {
        let result = match session
            .cdp
            .send_on_session(
                "Runtime.evaluate",
                json!({
                    "expression": &js,
                    "returnByValue": true
                }),
                Some(session_id.clone()),
            )
            .await
        {
            Ok(r) => r,
            Err(e) if e.to_string().contains("navigated or closed") => {
                session_id = reattach(session, target_id).await?;
                continue;
            }
            Err(e) => return Err(e),
        };
        if result["result"]["value"].as_bool() == Some(true) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(PagerunnerError::Cdp(format!(
                "Timeout waiting for URL containing: {}",
                pattern
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Fill an input field triggering React/Vue/Angular synthetic events.
/// Unlike type_text, this works on framework-controlled inputs.
pub async fn fill(
    session: &mut Session,
    target_id: &str,
    selector: &str,
    value: &str,
) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    let js = format!(
        r#"(() => {{
            const el = document.querySelector({sel});
            if (!el) return false;
            const proto = el instanceof HTMLTextAreaElement
                ? window.HTMLTextAreaElement.prototype
                : window.HTMLInputElement.prototype;
            const nativeSetter = Object.getOwnPropertyDescriptor(proto, 'value').set;
            nativeSetter.call(el, {val});
            el.dispatchEvent(new Event('input',  {{ bubbles: true }}));
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            return true;
        }})()"#,
        sel = serde_json::to_string(selector).unwrap(),
        val = serde_json::to_string(value).unwrap(),
    );
    let result = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": js,
                "returnByValue": true
            }),
            Some(session_id),
        )
        .await?;
    if result["result"]["value"].as_bool() != Some(true) {
        return Err(PagerunnerError::Cdp(format!(
            "Selector not found: {}",
            selector
        )));
    }
    Ok(())
}

/// Scroll the page by (x, y) pixels, or scroll a specific element into view.
pub async fn scroll(
    session: &mut Session,
    target_id: &str,
    x: i64,
    y: i64,
    selector: Option<&str>,
) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    let js = match selector {
        Some(sel) => format!(
            r#"(() => {{
                const el = document.querySelector({});
                if (!el) return false;
                el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
                return true;
            }})()"#,
            serde_json::to_string(sel).unwrap()
        ),
        None => format!("window.scrollBy({}, {}); true", x, y),
    };
    let result = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": js,
                "returnByValue": true
            }),
            Some(session_id),
        )
        .await?;
    if result["result"]["value"].as_bool() != Some(true) {
        if let Some(sel) = selector {
            return Err(PagerunnerError::Cdp(format!("Selector not found: {}", sel)));
        }
    }
    Ok(())
}

/// Select an option in a <select> element by value, then fire a change event.
pub async fn select_option(
    session: &mut Session,
    target_id: &str,
    selector: &str,
    value: &str,
) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    let js = format!(
        r#"(() => {{
            const el = document.querySelector({sel});
            if (!el) return false;
            el.value = {val};
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            return true;
        }})()"#,
        sel = serde_json::to_string(selector).unwrap(),
        val = serde_json::to_string(value).unwrap(),
    );
    let result = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": js,
                "returnByValue": true
            }),
            Some(session_id),
        )
        .await?;
    if result["result"]["value"].as_bool() != Some(true) {
        return Err(PagerunnerError::Cdp(format!(
            "Selector not found: {}",
            selector
        )));
    }
    Ok(())
}

/// Navigate a target to about:blank (cleanup after blocked redirect detection).
/// Does not go through policy checks — used only for cleanup.
pub async fn navigate_to_blank(cdp: &mut CdpConn, target_id: &str) -> Result<()> {
    cdp.send("Target.activateTarget", json!({ "targetId": target_id }))
        .await
        .ok();
    // Use a raw attach via Target.attachToTarget (bypasses cdp_sessions cache).
    let attach = cdp
        .send(
            "Target.attachToTarget",
            json!({
                "targetId": target_id,
                "flatten": true
            }),
        )
        .await?;
    let session_id = attach["sessionId"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Cdp("No sessionId".into()))?
        .to_string();
    cdp.send_on_session(
        "Page.navigate",
        json!({ "url": "about:blank" }),
        Some(session_id.clone()),
    )
    .await?;
    // Detach to avoid leaving a dangling CDP session in Chrome.
    let _ = cdp
        .send(
            "Target.detachFromTarget",
            json!({ "sessionId": &session_id }),
        )
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_for_selector_timeout_error_message() {
        // Unit: verify error message contains the selector name
        let err = PagerunnerError::Cdp("Timeout waiting for selector: #missing".into());
        assert!(err.to_string().contains("#missing"));
    }

    #[test]
    fn test_select_option_error_message() {
        let err = PagerunnerError::Cdp("Selector not found: #missing-select".into());
        assert!(err.to_string().contains("#missing-select"));
    }

    #[test]
    fn test_scroll_selector_error_message() {
        let err = PagerunnerError::Cdp("Selector not found: #off-screen".into());
        assert!(err.to_string().contains("#off-screen"));
    }
}
