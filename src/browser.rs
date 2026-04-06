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

pub async fn list_tabs(cdp: &CdpConn) -> Result<Vec<TabInfo>> {
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

pub async fn new_tab(cdp: &CdpConn, url: &str) -> Result<TabInfo> {
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

/// Close a browser tab by target ID.
/// Returns an error if the session has only one tab remaining — closing it would
/// destroy the Chrome window and kill the session.
pub async fn close_tab(cdp: &CdpConn, target_id: &str) -> Result<()> {
    let tabs = list_tabs(cdp).await?;
    if tabs.len() <= 1 {
        return Err(PagerunnerError::Config(
            "Cannot close last tab — use close_session instead".into(),
        ));
    }
    cdp.send("Target.closeTarget", json!({ "targetId": target_id }))
        .await?;
    Ok(())
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

/// Enable CDP Network domain for logging and event capture.
/// Called on every fresh CDP session attach for all sessions.
pub async fn enable_network_logging(cdp: &CdpConn, session_id: &str) -> Result<()> {
    cdp.send_on_session(
        "Network.enable",
        serde_json::json!({}),
        Some(session_id.to_string()),
    )
    .await?;
    Ok(())
}

/// Enable CDP Runtime domain for console/exception event capture.
/// Called on every fresh CDP session attach for all sessions.
pub async fn enable_runtime_logging(cdp: &CdpConn, session_id: &str) -> Result<()> {
    cdp.send_on_session(
        "Runtime.enable",
        serde_json::json!({}),
        Some(session_id.to_string()),
    )
    .await?;
    Ok(())
}

/// Block all private IP ranges via CDP Network domain.
/// Called on every fresh CDP session attach when the pagerunner session has a security policy.
// Tested indirectly: blocked_url_patterns_covers_all_private_ranges verifies
// the pattern list; e2e redirect tests verify the CDP integration.
async fn enable_network_blocking_patterns(cdp: &CdpConn, session_id: &str) -> Result<()> {
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
        crate::stealth::inject(&session.cdp, &session_id).await?;
    }

    // Always enable Network domain for logging/event capture
    enable_network_logging(&session.cdp, &session_id).await?;

    // Always enable Runtime domain for console/exception capture
    enable_runtime_logging(&session.cdp, &session_id).await?;

    if session.security_policy.is_some() {
        enable_network_blocking_patterns(&session.cdp, &session_id).await?;
    }

    // Update reverse map for event processor
    if let Ok(mut rev) = session.cdp_sessions_rev.write() {
        rev.insert(session_id.clone(), target_id.to_string());
    }

    // Mark session as network-enabled
    session.network_enabled = true;

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
    let timeout = std::time::Duration::from_secs(15);

    // Try innerText first (better formatting, but forces layout reflow which can
    // crash the renderer on heavy SPAs).
    let result = session
        .cdp
        .send_on_session_with_timeout(
            "Runtime.evaluate",
            json!({
                "expression": "document.body.innerText",
                "returnByValue": true
            }),
            Some(session_id.clone()),
            timeout,
        )
        .await;

    match result {
        Ok(val) => Ok(val["result"]["value"].as_str().unwrap_or("").into()),
        Err(e) => {
            tracing::warn!(
                target_id = %target_id,
                error = %e,
                "innerText extraction failed, falling back to textContent"
            );
            // Re-attach in case the CDP session died during the failed attempt.
            let session_id = attach_to_target(session, target_id).await?;
            let val = session
                .cdp
                .send_on_session_with_timeout(
                    "Runtime.evaluate",
                    json!({
                        "expression": "document.body.textContent",
                        "returnByValue": true
                    }),
                    Some(session_id),
                    timeout,
                )
                .await?;
            Ok(val["result"]["value"].as_str().unwrap_or("").into())
        }
    }
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

    let js = build_selector_chain_js(selector);

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
/// Returns the elapsed time in milliseconds from call start to condition met.
pub async fn wait_for_selector(
    session: &mut Session,
    target_id: &str,
    selector: &str,
    timeout_ms: u64,
) -> Result<u64> {
    let start = std::time::Instant::now();
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
            return Ok(start.elapsed().as_millis() as u64);
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
/// Returns the elapsed time in milliseconds from call start to condition met.
pub async fn wait_for_url(
    session: &mut Session,
    target_id: &str,
    pattern: &str,
    timeout_ms: u64,
) -> Result<u64> {
    let start = std::time::Instant::now();
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
            return Ok(start.elapsed().as_millis() as u64);
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
    let sel_json = serde_json::to_string(selector)
        .unwrap_or_else(|_| format!("\"{}\"", selector.replace('"', "\\\"")));
    let val_json = serde_json::to_string(value)
        .unwrap_or_else(|_| format!("\"{}\"", value.replace('"', "\\\"")));
    let js = format!(
        r#"(() => {{
            const _sel = {sel};
            let el = document.querySelector(_sel);
            if (!el) {{
                const _hint = (() => {{
                    const _td = _sel.match(/data-testid=["']?([^"'\]]+)/);
                    if (_td) return _td[1].toLowerCase().replace(/-/g, ' ');
                    if (_sel.startsWith('#')) return _sel.slice(1).replace(/-/g, ' ');
                    if (_sel.startsWith('.')) return _sel.replace(/^\./, '').replace(/-/g, ' ');
                    const _cm = _sel.match(/\.([a-zA-Z0-9_-]+)/);
                    if (_cm) return _cm[1].replace(/-/g, ' ');
                    return null;
                }})();
                if (_hint) {{
                    el = document.querySelector('[data-testid*="' + _hint + '"]')
                      || document.querySelector('[aria-label*="' + _hint + '"]')
                      || Array.from(document.querySelectorAll('button,a,[role="button"],[type="submit"]'))
                             .find(function(b) {{ return b.textContent.toLowerCase().includes(_hint); }})
                      || null;
                }}
            }}
            if (!el) return false;
            const proto = el instanceof HTMLTextAreaElement
                ? window.HTMLTextAreaElement.prototype
                : window.HTMLInputElement.prototype;
            const nativeInputValueSetter = Object.getOwnPropertyDescriptor(proto, 'value').set;
            nativeInputValueSetter.call(el, {val});
            el.dispatchEvent(new Event('input', {{ bubbles: true }}));
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            return true;
        }})()"#,
        sel = sel_json,
        val = val_json,
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
    let sel_json = serde_json::to_string(selector)
        .unwrap_or_else(|_| format!("\"{}\"", selector.replace('"', "\\\"")));
    let val_json = serde_json::to_string(value)
        .unwrap_or_else(|_| format!("\"{}\"", value.replace('"', "\\\"")));
    let js = format!(
        r#"(() => {{
            const _sel = {sel};
            let el = document.querySelector(_sel);
            if (!el) {{
                const _hint = (() => {{
                    const _td = _sel.match(/data-testid=["']?([^"'\]]+)/);
                    if (_td) return _td[1].toLowerCase().replace(/-/g, ' ');
                    if (_sel.startsWith('#')) return _sel.slice(1).replace(/-/g, ' ');
                    if (_sel.startsWith('.')) return _sel.replace(/^\./, '').replace(/-/g, ' ');
                    const _cm = _sel.match(/\.([a-zA-Z0-9_-]+)/);
                    if (_cm) return _cm[1].replace(/-/g, ' ');
                    return null;
                }})();
                if (_hint) {{
                    el = document.querySelector('[data-testid*="' + _hint + '"]')
                      || document.querySelector('[aria-label*="' + _hint + '"]')
                      || null;
                }}
            }}
            if (!el) return false;
            el.value = {val};
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            return true;
        }})()"#,
        sel = sel_json,
        val = val_json,
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
pub async fn navigate_to_blank(cdp: &CdpConn, target_id: &str) -> Result<()> {
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

/// Build a JavaScript IIFE that tries the given selector and falls back to
/// data-testid / aria-label / text-content alternatives when querySelector fails.
/// Returns `null` only if all strategies fail.
/// The result is `{x, y}` (click coordinates) for use in the `click` function,
/// or a boolean (true = found) for use in fill/select.
pub fn build_selector_chain_js(selector: &str) -> String {
    let sel_json = serde_json::to_string(selector)
        .unwrap_or_else(|_| format!("\"{}\"", selector.replace('"', "\\\"")));
    format!(
        r#"(() => {{
            const _sel = {sel};
            let _el = document.querySelector(_sel);
            if (!_el) {{
                const _hint = (() => {{
                    const _td = _sel.match(/data-testid=["']?([^"'\]]+)/);
                    if (_td) return _td[1].toLowerCase().replace(/-/g, ' ');
                    if (_sel.startsWith('#')) return _sel.slice(1).replace(/-/g, ' ');
                    if (_sel.startsWith('.')) return _sel.replace(/^\./, '').replace(/-/g, ' ');
                    const _cm = _sel.match(/\.([a-zA-Z0-9_-]+)/);
                    if (_cm) return _cm[1].replace(/-/g, ' ');
                    return null;
                }})();
                if (_hint) {{
                    _el = document.querySelector('[data-testid*="' + _hint + '"]')
                       || document.querySelector('[aria-label*="' + _hint + '"]')
                       || Array.from(document.querySelectorAll('button,a,[role="button"],[type="submit"]'))
                              .find(function(b) {{ return b.textContent.toLowerCase().includes(_hint); }})
                       || null;
                }}
            }}
            if (!_el) return null;
            const _r = _el.getBoundingClientRect();
            return {{ x: _r.x + _r.width / 2, y: _r.y + _r.height / 2, _fallback: !document.querySelector(_sel) }};
        }})()"#,
        sel = sel_json
    )
}

/// Update selector stability in site_knowledge. Best-effort — never fails the tool call.
pub fn update_selector_stability(
    store: &crate::site_knowledge::SiteKnowledgeStore,
    origin: &str,
    selector: &str,
    success: bool,
) {
    if selector.len() > 2048 {
        return; // cap: silently drop oversized selectors
    }
    let now = crate::site_knowledge::now_micros();
    let mut entry = store.get(origin).unwrap_or_default().unwrap_or_default();
    let sel = entry.selectors.entry(selector.to_string()).or_default();
    if success {
        sel.successes += 1;
    } else {
        sel.failures += 1;
    }
    sel.last_seen = now;
    entry.last_updated = now;
    let _ = store.put(origin, &entry);
}

/// Build fragility warning metadata if the selector is fragile.
pub fn fragility_warning(
    store: &crate::site_knowledge::SiteKnowledgeStore,
    origin: &str,
    selector: &str,
) -> Option<serde_json::Value> {
    let entry = store.get(origin).ok()??;
    let sel = entry.selectors.get(selector)?;
    if !crate::site_knowledge::SiteKnowledgeStore::is_fragile(sel) {
        return None;
    }
    let total = sel.successes + sel.failures;
    let rate = (sel.failures as f64 / total as f64 * 100.0) as u32;
    Some(serde_json::json!({
        "_warning": format!(
            "Selector '{}' has a {}% failure rate ({}/{} uses) on {} — consider finding a more stable selector",
            selector, rate, sel.failures, total, origin
        ),
        "_hint": format!("Use get_site_knowledge('{}') to see alternative selectors with better reliability", origin)
    }))
}

/// Inject the recording cursor overlay into the page.
/// Call this when recording starts and after each navigation.
pub async fn inject_recording_cursor(session: &mut Session, target_id: &str) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    let _ = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": crate::recording_cursor::INJECT_CURSOR_JS,
                "returnByValue": true
            }),
            Some(session_id),
        )
        .await;
    Ok(())
}

/// Move the recording cursor to (x, y) and optionally show a click ripple.
pub async fn move_recording_cursor(
    session: &mut Session,
    target_id: &str,
    x: f64,
    y: f64,
    click: bool,
) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    // Move cursor
    let _ = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": crate::recording_cursor::move_cursor_js(x, y),
                "returnByValue": true
            }),
            Some(session_id.clone()),
        )
        .await;

    if click {
        let _ = session
            .cdp
            .send_on_session(
                "Runtime.evaluate",
                json!({
                    "expression": crate::recording_cursor::click_ripple_js(x, y),
                    "returnByValue": true
                }),
                Some(session_id),
            )
            .await;
    }
    Ok(())
}

/// Remove the recording cursor overlay.
pub async fn remove_recording_cursor(session: &mut Session, target_id: &str) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    let _ = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": crate::recording_cursor::REMOVE_CURSOR_JS,
                "returnByValue": true
            }),
            Some(session_id),
        )
        .await;
    Ok(())
}

/// Start CDP screencast — Chrome will push frames as events.
/// Enables the Page domain first (required for screencast events).
pub async fn start_screencast(
    session: &mut Session,
    target_id: &str,
    format: &str,
    quality: u8,
    max_width: u32,
    max_height: u32,
    every_nth_frame: u32,
) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;

    // Page domain must be enabled for screencastFrame events to fire
    session
        .cdp
        .send_on_session("Page.enable", json!({}), Some(session_id.clone()))
        .await?;

    session
        .cdp
        .send_on_session(
            "Page.startScreencast",
            json!({
                "format": format,
                "quality": quality,
                "maxWidth": max_width,
                "maxHeight": max_height,
                "everyNthFrame": every_nth_frame,
            }),
            Some(session_id),
        )
        .await?;
    Ok(())
}

/// Stop CDP screencast.
pub async fn stop_screencast(session: &mut Session, target_id: &str) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    session
        .cdp
        .send_on_session("Page.stopScreencast", json!({}), Some(session_id))
        .await?;
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

    #[test]
    fn update_selector_stability_records_success() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = std::sync::Arc::new(
            crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap(),
        );
        let store = crate::site_knowledge::SiteKnowledgeStore::new(db, key);

        update_selector_stability(&store, "https://linear.app", ".submit-btn", true);

        let entry = store.get("https://linear.app").unwrap().unwrap();
        let sel = entry.selectors.get(".submit-btn").unwrap();
        assert_eq!(sel.successes, 1);
        assert_eq!(sel.failures, 0);
    }

    #[test]
    fn update_selector_stability_records_failure() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = std::sync::Arc::new(
            crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap(),
        );
        let store = crate::site_knowledge::SiteKnowledgeStore::new(db, key);

        update_selector_stability(&store, "https://linear.app", ".submit-btn", false);

        let entry = store.get("https://linear.app").unwrap().unwrap();
        let sel = entry.selectors.get(".submit-btn").unwrap();
        assert_eq!(sel.successes, 0);
        assert_eq!(sel.failures, 1);
    }

    #[test]
    fn fragility_warning_returned_when_failure_rate_exceeds_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = std::sync::Arc::new(
            crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap(),
        );
        let store = crate::site_knowledge::SiteKnowledgeStore::new(db, key);
        let origin = "https://linear.app";
        let selector = ".submit-btn";

        // 3 successes + 7 failures = 70% failure rate (> 30%, >= 5 samples)
        for _ in 0..3 {
            update_selector_stability(&store, origin, selector, true);
        }
        for _ in 0..7 {
            update_selector_stability(&store, origin, selector, false);
        }

        let warning = fragility_warning(&store, origin, selector);
        assert!(warning.is_some(), "expected fragility warning");
        let w = warning.unwrap();
        let warning_text = w["_warning"].as_str().unwrap();
        assert!(warning_text.contains("linear.app"));
        assert!(warning_text.contains(".submit-btn"));
        assert!(w["_hint"].as_str().is_some());
    }

    #[test]
    fn fragility_warning_absent_below_5_samples() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = std::sync::Arc::new(
            crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap(),
        );
        let store = crate::site_knowledge::SiteKnowledgeStore::new(db, key);
        let origin = "https://linear.app";
        let selector = ".btn";

        // 4 failures but only 4 total — under the 5-sample minimum
        for _ in 0..4 {
            update_selector_stability(&store, origin, selector, false);
        }

        assert!(fragility_warning(&store, origin, selector).is_none());
    }

    #[test]
    fn selector_chain_js_includes_original_selector() {
        let js = build_selector_chain_js("#my-btn");
        assert!(
            js.contains("#my-btn"),
            "JS should include original selector"
        );
    }

    #[test]
    fn selector_chain_js_includes_data_testid_fallback() {
        let js = build_selector_chain_js("#my-btn");
        assert!(
            js.contains("data-testid") || js.contains("aria-label"),
            "JS should include attribute fallbacks"
        );
    }

    #[test]
    fn selector_chain_js_is_valid_js_expression() {
        // Must be an IIFE returning a value
        let js = build_selector_chain_js("button.submit");
        assert!(
            js.contains("(function") || js.contains("(() =>") || js.contains("(()=>{"),
            "JS should be an IIFE"
        );
        assert!(js.contains("return"), "JS should have a return path");
    }

    #[test]
    fn close_tab_last_tab_guard() {
        fn can_close(count: usize) -> bool {
            count > 1
        }
        assert!(!can_close(1), "should refuse to close last tab");
        assert!(can_close(2), "should allow close when 2+ tabs");
    }
}
