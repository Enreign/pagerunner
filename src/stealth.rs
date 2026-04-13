use crate::cdp::CdpConn;
use crate::error::Result;
use serde_json::json;

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
pub async fn inject(cdp: &CdpConn, session_id: &str) -> Result<()> {
    cdp.send_on_session(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": STEALTH_JS }),
        Some(session_id.to_string()),
    )
    .await?;

    // Also evaluate immediately on the current page
    cdp.send_on_session(
        "Runtime.evaluate",
        json!({
            "expression": STEALTH_JS,
            "returnByValue": true,
        }),
        Some(session_id.to_string()),
    )
    .await?;

    Ok(())
}

/// Random delay between `min_ms` and `max_ms` milliseconds.
pub async fn human_delay(min_ms: u64, max_ms: u64) {
    use rand::Rng;
    let ms = rand::rng().random_range(min_ms..=max_ms);
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}
