use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A tool call sent from MCP server → daemon.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub id: String,
    pub tool: String,
    pub args: Value,
}

/// Result sent from daemon → MCP server.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub id: String,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// Path relative to $HOME for the Unix socket.
pub const SOCKET_SUBPATH: &str = ".pagerunner/daemon.sock";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialise() {
        let req = DaemonRequest {
            id: "abc".into(),
            tool: "open_session".into(),
            args: serde_json::json!({"profile": "personal"}),
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("open_session"));
        assert!(s.contains("abc"));
    }

    #[test]
    fn test_response_error_serialise() {
        let resp = DaemonResponse {
            id: "abc".into(),
            result: None,
            error: Some("Profile not found".into()),
        };
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("Profile not found"));
    }
}
