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
}

pub type Result<T> = std::result::Result<T, PagerunnerError>;

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
