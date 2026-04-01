use thiserror::Error;

#[derive(Debug, Error)]
pub enum PagerunnerError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Session {0} has crashed (Chrome process is no longer running)")]
    SessionDead(String),
    #[error("Chrome error: {0}")]
    Chrome(String),
    #[error("CDP error: {0}")]
    Cdp(String),
    #[error("Config error: {0}")]
    Config(String),
    #[error("Tool '{0}' is not permitted in this session")]
    ToolNotPermitted(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// PII or credential survived anonymization and was blocked before reaching the LLM.
    /// Carries per-entity counts so callers can emit structured audit events.
    #[error("Anonymization gap: {count} entity(s) survived — blocked before reaching LLM")]
    ResidualPiiDetected {
        /// Map of entity label → count for what survived (e.g. {"SECRET": 1, "EMAIL": 2})
        entity_counts: std::collections::HashMap<String, usize>,
        count: usize,
    },
}

pub type Result<T> = std::result::Result<T, PagerunnerError>;

impl PagerunnerError {
    pub fn error_type(&self) -> &'static str {
        match self {
            Self::SessionNotFound(_) => "session_not_found",
            Self::Chrome(_) => "chrome_unreachable",
            Self::Cdp(msg) => {
                let lower = msg.to_ascii_lowercase();
                if lower.contains("selector")
                    || lower.contains("element not found")
                    || lower.contains("no element")
                {
                    "selector_not_found"
                } else if lower.contains("timeout")
                    || lower.contains("timed out")
                    || lower.contains("navigation timeout")
                {
                    "navigation_timeout"
                } else {
                    "cdp_error"
                }
            }
            Self::ToolNotPermitted(_) => "permission_denied",
            Self::Config(msg) => {
                let lower = msg.to_ascii_lowercase();
                if lower.contains("blocked")
                    || lower.contains("domain")
                    || lower.contains("not allowed")
                {
                    "permission_denied"
                } else {
                    "validation_error"
                }
            }
            Self::Io(_) => "io_error",
            Self::Json(_) => "internal_error",
            Self::SessionDead(_) => "session_dead",
            Self::ResidualPiiDetected { .. } => "anonymization_gap",
        }
    }

    pub fn recovery_hint(&self) -> &'static str {
        match self.error_type() {
            "session_not_found" => "Call open_session to start a new browser session, then retry with the new session_id.",
            "chrome_unreachable" => "Chrome may have crashed. Run pagerunner status to check, then call open_session again.",
            "selector_not_found" => "Use screenshot to see current page state and verify the CSS selector is correct.",
            "navigation_timeout" => "The page may be slow to load. Retry navigate or use wait_for with a longer --ms value.",
            "permission_denied" => "This action is blocked by the session security policy. Check allowed_domains or allowed_tools in open_session.",
            "validation_error" => "Check that all required parameters are provided and have valid values.",
            "io_error" => "A filesystem error occurred. Check disk space and permissions.",
            "cdp_error" => "A Chrome DevTools Protocol error occurred. Use screenshot to inspect current page state.",
            _ => "An unexpected error occurred. Run pagerunner status to diagnose.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_not_permitted_message() {
        let e = PagerunnerError::ToolNotPermitted("evaluate".to_string());
        assert_eq!(
            e.to_string(),
            "Tool 'evaluate' is not permitted in this session"
        );
    }

    #[test]
    fn session_not_found_maps_to_correct_type() {
        let e = PagerunnerError::SessionNotFound("abc".into());
        assert_eq!(e.error_type(), "session_not_found");
        assert!(!e.recovery_hint().is_empty());
    }

    #[test]
    fn chrome_error_maps_correctly() {
        let e = PagerunnerError::Chrome("crashed".into());
        assert_eq!(e.error_type(), "chrome_unreachable");
    }

    #[test]
    fn tool_not_permitted_maps_to_permission_denied() {
        let e = PagerunnerError::ToolNotPermitted("screenshot".into());
        assert_eq!(e.error_type(), "permission_denied");
    }

    #[test]
    fn config_error_maps_to_validation_error() {
        let e = PagerunnerError::Config("Missing session_id".into());
        assert_eq!(e.error_type(), "validation_error");
    }

    #[test]
    fn cdp_selector_error_maps_correctly() {
        let e = PagerunnerError::Cdp("Element not found for selector: .btn".into());
        assert_eq!(e.error_type(), "selector_not_found");
    }

    #[test]
    fn cdp_timeout_maps_correctly() {
        let e = PagerunnerError::Cdp("Navigation timeout exceeded".into());
        assert_eq!(e.error_type(), "navigation_timeout");
    }

    #[test]
    fn blocked_config_maps_to_permission_denied() {
        let e = PagerunnerError::Config("URL blocked by allowed_domains policy".into());
        assert_eq!(e.error_type(), "permission_denied");
    }

    #[test]
    fn io_error_maps_to_io_error() {
        let e = PagerunnerError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert_eq!(e.error_type(), "io_error");
        assert!(!e.recovery_hint().is_empty());
    }

    #[test]
    fn json_error_maps_to_internal_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let e = PagerunnerError::Json(json_err);
        assert_eq!(e.error_type(), "internal_error");
    }

    #[test]
    fn cdp_generic_error_maps_to_cdp_error() {
        let e = PagerunnerError::Cdp("Protocol error occurred".into());
        assert_eq!(e.error_type(), "cdp_error");
    }
}
