//! CDP method dispatch — handles the 18 CDP methods pagerunner uses.

use crate::dom::NodeId;
use crate::html;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

/// Represents a browser target (tab).
#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub url: String,
    pub title: String,
    /// CDP session id for this target.
    pub session_id: Option<String>,
    /// The DOM tree for this target's current page.
    pub dom: Option<crate::dom::Dom>,
}

/// Handles CDP method calls against the browser state.
pub struct CdpHandler {
    pub headless: bool,
    pub targets: Vec<Target>,
    /// Map from CDP session_id → target_id.
    pub sessions: HashMap<String, String>,
    /// Cookies: origin → vec of cookie objects.
    pub cookies: HashMap<String, Vec<Value>>,
    /// Blocked URL patterns.
    pub blocked_urls: Vec<String>,
    /// Network logging enabled per session.
    pub network_enabled: HashMap<String, bool>,
    /// Runtime enabled per session.
    pub runtime_enabled: HashMap<String, bool>,
}

impl CdpHandler {
    pub fn new(headless: bool) -> Self {
        // Create one default target.
        let target_id = Uuid::new_v4().to_string().to_uppercase();
        let default_target = Target {
            id: target_id,
            url: "about:blank".into(),
            title: "".into(),
            session_id: None,
            dom: None,
        };

        CdpHandler {
            headless,
            targets: vec![default_target],
            sessions: HashMap::new(),
            cookies: HashMap::new(),
            blocked_urls: Vec::new(),
            network_enabled: HashMap::new(),
            runtime_enabled: HashMap::new(),
        }
    }

    /// List targets for /json/list endpoint.
    pub fn list_targets(&self) -> Vec<Value> {
        self.targets
            .iter()
            .map(|t| {
                json!({
                    "targetId": t.id,
                    "type": "page",
                    "title": t.title,
                    "url": t.url,
                })
            })
            .collect()
    }

    /// Create a new target.
    pub fn create_target(&mut self, url: &str) -> Value {
        let id = Uuid::new_v4().to_string().to_uppercase();
        let target = Target {
            id: id.clone(),
            url: url.into(),
            title: "".into(),
            session_id: None,
            dom: None,
        };
        self.targets.push(target);
        json!({ "targetId": id })
    }

    /// Main dispatch — handle a CDP method call.
    pub fn handle(&mut self, method: &str, params: Value, session_id: Option<&str>) -> Value {
        match method {
            // -- Target domain --
            "Target.getTargets" => self.target_get_targets(),
            "Target.createTarget" => {
                let url = params["url"].as_str().unwrap_or("about:blank");
                self.create_target(url)
            }
            "Target.closeTarget" => {
                let target_id = params["targetId"].as_str().unwrap_or("");
                self.target_close(target_id)
            }
            "Target.attachToTarget" => {
                let target_id = params["targetId"].as_str().unwrap_or("");
                self.target_attach(target_id)
            }
            "Target.activateTarget" => json!({}),
            "Target.detachFromTarget" => {
                let sid = params["sessionId"].as_str().unwrap_or("");
                self.sessions.remove(sid);
                json!({})
            }

            // -- Runtime domain --
            "Runtime.enable" => {
                if let Some(sid) = session_id {
                    self.runtime_enabled.insert(sid.to_string(), true);
                }
                json!({})
            }
            "Runtime.evaluate" => {
                let expression = params["expression"].as_str().unwrap_or("");
                self.runtime_evaluate(expression, session_id)
            }

            // -- Page domain --
            "Page.navigate" => {
                let url = params["url"].as_str().unwrap_or("about:blank");
                self.page_navigate(url, session_id)
            }
            "Page.captureScreenshot" => self.page_screenshot(session_id),
            "Page.enable" => json!({}),
            "Page.startScreencast" => json!({}),
            "Page.stopScreencast" => json!({}),
            "Page.addScriptToEvaluateOnNewDocument" => {
                json!({ "identifier": Uuid::new_v4().to_string() })
            }

            // -- Network domain --
            "Network.enable" => {
                if let Some(sid) = session_id {
                    self.network_enabled.insert(sid.to_string(), true);
                }
                json!({})
            }
            "Network.getCookies" => {
                let urls = params["urls"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                self.network_get_cookies(&urls)
            }
            "Network.setCookie" => {
                self.network_set_cookie(&params);
                json!({ "success": true })
            }
            "Network.setBlockedURLs" => {
                if let Some(urls) = params["urls"].as_array() {
                    self.blocked_urls = urls
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                }
                json!({})
            }
            "Network.getResponseBody" => {
                json!({ "body": "", "base64Encoded": false })
            }

            // -- Input domain --
            "Input.dispatchMouseEvent" => json!({}),
            "Input.dispatchKeyEvent" => json!({}),
            "Input.insertText" => json!({}),

            // -- Browser domain --
            "Browser.close" => {
                self.targets.clear();
                json!({})
            }

            _ => {
                tracing::warn!("unhandled CDP method: {}", method);
                json!({})
            }
        }
    }

    // -- Target handlers ----------------------------------------------------

    fn target_get_targets(&self) -> Value {
        json!({
            "targetInfos": self.targets.iter().map(|t| json!({
                "targetId": t.id,
                "type": "page",
                "title": t.title,
                "url": t.url,
                "attached": t.session_id.is_some(),
            })).collect::<Vec<_>>()
        })
    }

    fn target_close(&mut self, target_id: &str) -> Value {
        self.targets.retain(|t| t.id != target_id);
        json!({ "success": true })
    }

    fn target_attach(&mut self, target_id: &str) -> Value {
        let session_id = Uuid::new_v4().to_string().to_uppercase();
        if let Some(target) = self.targets.iter_mut().find(|t| t.id == target_id) {
            target.session_id = Some(session_id.clone());
        }
        self.sessions
            .insert(session_id.clone(), target_id.to_string());
        json!({ "sessionId": session_id })
    }

    // -- Page handlers ------------------------------------------------------

    fn page_navigate(&mut self, url: &str, session_id: Option<&str>) -> Value {
        let target_id = session_id
            .and_then(|sid| self.sessions.get(sid))
            .cloned();

        if let Some(tid) = target_id {
            if let Some(target) = self.targets.iter_mut().find(|t| t.id == tid) {
                target.url = url.to_string();
                // Parse any HTML content that will be fetched later.
                // For now, just clear the DOM — actual fetching happens in the net module.
                target.dom = None;
            }
        }

        json!({
            "frameId": Uuid::new_v4().to_string(),
            "loaderId": Uuid::new_v4().to_string(),
        })
    }

    fn page_screenshot(&self, _session_id: Option<&str>) -> Value {
        // Return a minimal 1x1 white PNG as placeholder.
        // Real rendering will use the layout engine + tiny-skia.
        json!({ "data": PLACEHOLDER_PNG_B64 })
    }

    // -- Runtime handlers ---------------------------------------------------

    fn runtime_evaluate(&mut self, expression: &str, session_id: Option<&str>) -> Value {
        let target = session_id
            .and_then(|sid| self.sessions.get(sid))
            .and_then(|tid| self.targets.iter().find(|t| t.id == *tid));

        // Handle common expressions that pagerunner uses.
        let result = match expression.trim() {
            "1" => json!({ "type": "number", "value": 1, "description": "1" }),
            e if e.contains("document.body.innerText") => {
                let text = target
                    .and_then(|t| t.dom.as_ref())
                    .and_then(|dom| dom.body_element())
                    .map(|body_id| {
                        target.unwrap().dom.as_ref().unwrap().inner_text(body_id)
                    })
                    .unwrap_or_default();
                json!({ "type": "string", "value": text })
            }
            e if e.contains("document.body.textContent") => {
                let text = target
                    .and_then(|t| t.dom.as_ref())
                    .and_then(|dom| dom.body_element())
                    .map(|body_id| {
                        target.unwrap().dom.as_ref().unwrap().inner_text(body_id)
                    })
                    .unwrap_or_default();
                json!({ "type": "string", "value": text })
            }
            e if e.contains("document.querySelector") => {
                // Extract selector from querySelector('...')
                let selector = extract_qs_selector(e);
                let found = target
                    .and_then(|t| t.dom.as_ref())
                    .and_then(|dom| {
                        selector.and_then(|sel| dom.query_selector(NodeId::DOCUMENT, sel))
                    })
                    .is_some();
                if found {
                    json!({ "type": "object", "subtype": "node", "description": "HTMLElement" })
                } else {
                    json!({ "type": "object", "subtype": "null", "value": null })
                }
            }
            e if e.contains("document.title") => {
                let title = target.map(|t| t.title.as_str()).unwrap_or("");
                json!({ "type": "string", "value": title })
            }
            e if e.contains("getBoundingClientRect") => {
                // Placeholder — real layout engine will compute this.
                json!({
                    "type": "object",
                    "value": { "x": 100, "y": 100, "width": 200, "height": 40 }
                })
            }
            e if e.contains("window.innerWidth") || e.contains("window.innerHeight") => {
                json!({
                    "type": "object",
                    "value": { "width": 1280, "height": 720 }
                })
            }
            e if e.contains("localStorage") => {
                json!({ "type": "object", "value": {} })
            }
            _ => {
                // Generic: return undefined for unknown expressions.
                json!({ "type": "undefined" })
            }
        };

        json!({ "result": result })
    }

    // -- Network handlers ---------------------------------------------------

    fn network_get_cookies(&self, urls: &[&str]) -> Value {
        let mut all_cookies = Vec::new();
        if urls.is_empty() {
            for cookies in self.cookies.values() {
                all_cookies.extend(cookies.iter().cloned());
            }
        } else {
            for url in urls {
                if let Some(cookies) = self.cookies.get(*url) {
                    all_cookies.extend(cookies.iter().cloned());
                }
            }
        }
        json!({ "cookies": all_cookies })
    }

    fn network_set_cookie(&mut self, params: &Value) {
        let domain = params["domain"]
            .as_str()
            .or_else(|| params["url"].as_str())
            .unwrap_or("unknown")
            .to_string();
        let entry = self.cookies.entry(domain).or_default();
        entry.push(params.clone());
    }
}

/// Load HTML content into a target's DOM.
pub fn load_html_into_target(target: &mut Target, html: &str) {
    let dom = html::parse(html);
    // Extract title from DOM.
    if let Some(head) = dom.head_element() {
        for &child_id in &dom.node(head).children {
            if dom.tag_name(child_id) == Some("title") {
                target.title = dom.inner_text(child_id).trim().to_string();
                break;
            }
        }
    }
    target.dom = Some(dom);
}

/// Extract selector string from a querySelector('selector') expression.
fn extract_qs_selector(expr: &str) -> Option<&str> {
    let start = expr.find("querySelector(")? + "querySelector(".len();
    let rest = &expr[start..];
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let inner = &rest[1..];
    let end = inner.find(quote)?;
    Some(&inner[..end])
}

/// Minimal 1x1 white PNG, base64 encoded.
const PLACEHOLDER_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_creates_targets() {
        let mut h = CdpHandler::new(true);
        assert_eq!(h.targets.len(), 1);
        h.create_target("https://example.com");
        assert_eq!(h.targets.len(), 2);
    }

    #[test]
    fn handler_attach_and_detach() {
        let mut h = CdpHandler::new(true);
        let tid = h.targets[0].id.clone();
        let result = h.target_attach(&tid);
        let sid = result["sessionId"].as_str().unwrap().to_string();
        assert!(h.sessions.contains_key(&sid));

        h.handle(
            "Target.detachFromTarget",
            json!({ "sessionId": sid.clone() }),
            None,
        );
        assert!(!h.sessions.contains_key(&sid));
    }

    #[test]
    fn handler_get_targets() {
        let h = CdpHandler::new(true);
        let result = h.target_get_targets();
        let infos = result["targetInfos"].as_array().unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0]["type"], "page");
    }

    #[test]
    fn handler_navigate() {
        let mut h = CdpHandler::new(true);
        let tid = h.targets[0].id.clone();
        let attach = h.target_attach(&tid);
        let sid = attach["sessionId"].as_str().unwrap();

        h.handle(
            "Page.navigate",
            json!({ "url": "https://example.com" }),
            Some(sid),
        );
        assert_eq!(h.targets[0].url, "https://example.com");
    }

    #[test]
    fn handler_runtime_evaluate_simple() {
        let mut h = CdpHandler::new(true);
        let result = h.handle("Runtime.evaluate", json!({ "expression": "1" }), None);
        assert_eq!(result["result"]["value"], 1);
    }

    #[test]
    fn handler_runtime_evaluate_innertext() {
        let mut h = CdpHandler::new(true);
        let tid = h.targets[0].id.clone();
        let attach = h.target_attach(&tid);
        let sid = attach["sessionId"].as_str().unwrap().to_string();

        // Load HTML into target.
        load_html_into_target(
            &mut h.targets[0],
            "<html><body><p>Hello World</p></body></html>",
        );

        let result = h.handle(
            "Runtime.evaluate",
            json!({ "expression": "document.body.innerText" }),
            Some(&sid),
        );
        let text = result["result"]["value"].as_str().unwrap();
        assert!(text.contains("Hello World"));
    }

    #[test]
    fn handler_cookies() {
        let mut h = CdpHandler::new(true);
        h.handle(
            "Network.setCookie",
            json!({ "domain": "example.com", "name": "sid", "value": "abc" }),
            None,
        );
        let result = h.network_get_cookies(&["example.com"]);
        let cookies = result["cookies"].as_array().unwrap();
        assert_eq!(cookies.len(), 1);
    }

    #[test]
    fn handler_blocked_urls() {
        let mut h = CdpHandler::new(true);
        h.handle(
            "Network.setBlockedURLs",
            json!({ "urls": ["*.internal.com", "10.*"] }),
            None,
        );
        assert_eq!(h.blocked_urls.len(), 2);
    }

    #[test]
    fn extract_qs_selector_works() {
        assert_eq!(
            extract_qs_selector("document.querySelector('.foo')"),
            Some(".foo")
        );
        assert_eq!(
            extract_qs_selector("document.querySelector(\"#bar\")"),
            Some("#bar")
        );
        assert_eq!(extract_qs_selector("something else"), None);
    }

    #[test]
    fn handler_load_html_extracts_title() {
        let mut target = Target {
            id: "test".into(),
            url: "about:blank".into(),
            title: "".into(),
            session_id: None,
            dom: None,
        };
        load_html_into_target(
            &mut target,
            "<html><head><title>My Page</title></head><body>Content</body></html>",
        );
        assert_eq!(target.title, "My Page");
        assert!(target.dom.is_some());
    }
}
