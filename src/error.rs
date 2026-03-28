use thiserror::Error;

#[derive(Debug, Error)]
pub enum PagerunnerError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
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
}

pub type Result<T> = std::result::Result<T, PagerunnerError>;

impl PagerunnerError {
    /// Short machine-readable error category for structured responses.
    pub fn error_type(&self) -> &'static str {
        match self {
            PagerunnerError::SessionNotFound(_) => "SESSION_NOT_FOUND",
            PagerunnerError::Chrome(_) => "CHROME_ERROR",
            PagerunnerError::Cdp(_) => "CDP_ERROR",
            PagerunnerError::Config(_) => "CONFIG_ERROR",
            PagerunnerError::ToolNotPermitted(_) => "TOOL_NOT_PERMITTED",
            PagerunnerError::Io(_) => "IO_ERROR",
            PagerunnerError::Json(_) => "JSON_ERROR",
        }
    }

    /// Human-readable recovery hint for structured error responses.
    pub fn recovery_hint(&self) -> &'static str {
        match self {
            PagerunnerError::SessionNotFound(_) => {
                "Call open_session to create a new session, then retry."
            }
            PagerunnerError::Chrome(_) => {
                "Chrome may have crashed or closed. Try closing and reopening the session."
            }
            PagerunnerError::Cdp(_) => {
                "Check that the tab is still open and the expression is valid JavaScript."
            }
            PagerunnerError::Config(_) => "Check the required parameters and try again.",
            PagerunnerError::ToolNotPermitted(_) => {
                "This tool is blocked by the session security policy."
            }
            PagerunnerError::Io(_) => "An I/O error occurred. Check file permissions.",
            PagerunnerError::Json(_) => "A JSON serialization error occurred.",
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
}
