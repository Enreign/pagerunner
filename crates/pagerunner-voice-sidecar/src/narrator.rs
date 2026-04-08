//! Maps agent events to short spoken phrases for TTS narration.
//!
//! The narrator intentionally keeps output terse — full tool results are
//! skipped since they are too verbose for speech.

use serde_json::Value;

/// Convert an agent event into an optional spoken phrase.
///
/// Returns `None` for events that should not be narrated (e.g. raw tool
/// results, interruptions).
pub fn narrate(event_type: &str, event_json: &Value) -> Option<String> {
    match event_type {
        "thinking" => event_json["text"].as_str().map(|s| s.to_string()),

        "tool_call" => {
            let name = event_json["name"].as_str().unwrap_or("unknown");
            Some(match name {
                "navigate" => format!(
                    "Navigating to {}.",
                    event_json["args"]["url"].as_str().unwrap_or("a page")
                ),
                "screenshot" => "Taking a screenshot.".to_string(),
                "get_content" => "Reading the page.".to_string(),
                "click" => "Clicking.".to_string(),
                "fill" => "Filling in the form.".to_string(),
                _ => format!("Running {}.", name),
            })
        }

        "tool_result" => None, // Don't narrate raw results

        "progress" => event_json["message"].as_str().map(|s| s.to_string()),

        "done" => event_json["summary"].as_str().map(|s| s.to_string()),

        "error" => event_json["message"]
            .as_str()
            .map(|s| format!("Error: {}", s)),

        "approval_required" => event_json["description"]
            .as_str()
            .map(|s| format!("Should I {}?", s)),

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn narrate_thinking() {
        let v = json!({"text": "Let me check the page"});
        assert_eq!(
            narrate("thinking", &v),
            Some("Let me check the page".into())
        );
    }

    #[test]
    fn narrate_tool_call_navigate() {
        let v = json!({"name": "navigate", "args": {"url": "https://example.com"}});
        assert_eq!(
            narrate("tool_call", &v),
            Some("Navigating to https://example.com.".into())
        );
    }

    #[test]
    fn narrate_tool_call_screenshot() {
        let v = json!({"name": "screenshot", "args": {}});
        assert_eq!(
            narrate("tool_call", &v),
            Some("Taking a screenshot.".into())
        );
    }

    #[test]
    fn narrate_tool_call_unknown() {
        let v = json!({"name": "save_snapshot", "args": {}});
        assert_eq!(
            narrate("tool_call", &v),
            Some("Running save_snapshot.".into())
        );
    }

    #[test]
    fn narrate_tool_result_is_none() {
        let v = json!({"name": "navigate", "result": "<html>..."});
        assert_eq!(narrate("tool_result", &v), None);
    }

    #[test]
    fn narrate_done() {
        let v = json!({"summary": "I found the answer."});
        assert_eq!(narrate("done", &v), Some("I found the answer.".into()));
    }

    #[test]
    fn narrate_error() {
        let v = json!({"message": "Session expired", "recoverable": false});
        assert_eq!(
            narrate("error", &v),
            Some("Error: Session expired".into())
        );
    }

    #[test]
    fn narrate_approval_required() {
        let v = json!({"description": "delete the file", "run_id": "r1", "action": "delete"});
        assert_eq!(
            narrate("approval_required", &v),
            Some("Should I delete the file?".into())
        );
    }

    #[test]
    fn narrate_unknown_event_is_none() {
        assert_eq!(narrate("interrupted", &json!({})), None);
    }

    #[test]
    fn narrate_progress() {
        let v = json!({"message": "Step 3 of 5"});
        assert_eq!(narrate("progress", &v), Some("Step 3 of 5".into()));
    }
}
