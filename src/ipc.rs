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

// ---------------------------------------------------------------------------
// Agent IPC types
// ---------------------------------------------------------------------------

/// Extended message envelope — used for agent commands alongside tool calls.
///
/// The daemon tries to parse incoming lines as `DaemonMessage` first; if that
/// fails it falls back to the legacy `DaemonRequest` format for backward
/// compatibility.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonMessage {
    /// Existing tool call (wrapped for forward compat).
    ToolCall(DaemonRequest),
    /// Start an autonomous agent run.
    AgentRun {
        id: String,
        goal: String,
        #[serde(default)]
        config: Option<pagerunner_agent::AgentConfig>,
    },
    /// Approve or deny a pending agent action.
    AgentApprove {
        id: String,
        run_id: String,
        approved: bool,
    },
    /// Interrupt a running agent.
    AgentInterrupt { id: String, run_id: String },
}

/// An event streamed from daemon to client during an agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonEvent {
    pub run_id: String,
    pub event: pagerunner_agent::AgentEvent,
}

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

    #[test]
    fn test_daemon_message_tool_call_roundtrip() {
        let msg = DaemonMessage::ToolCall(DaemonRequest {
            id: "t1".into(),
            tool: "list_sessions".into(),
            args: serde_json::json!({}),
        });
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("tool_call"));
        let decoded: DaemonMessage = serde_json::from_str(&s).unwrap();
        match decoded {
            DaemonMessage::ToolCall(req) => assert_eq!(req.tool, "list_sessions"),
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn test_daemon_message_agent_run_roundtrip() {
        let msg = DaemonMessage::AgentRun {
            id: "a1".into(),
            goal: "Navigate to example.com".into(),
            config: None,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("agent_run"));
        let decoded: DaemonMessage = serde_json::from_str(&s).unwrap();
        match decoded {
            DaemonMessage::AgentRun { id, goal, config } => {
                assert_eq!(id, "a1");
                assert_eq!(goal, "Navigate to example.com");
                assert!(config.is_none());
            }
            _ => panic!("expected AgentRun"),
        }
    }

    #[test]
    fn test_daemon_message_agent_run_with_config() {
        let cfg = pagerunner_agent::AgentConfig {
            model: "llama3".into(),
            ..pagerunner_agent::AgentConfig::default()
        };
        let msg = DaemonMessage::AgentRun {
            id: "a2".into(),
            goal: "Do something".into(),
            config: Some(cfg),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let decoded: DaemonMessage = serde_json::from_str(&s).unwrap();
        match decoded {
            DaemonMessage::AgentRun { config, .. } => {
                assert_eq!(config.unwrap().model, "llama3");
            }
            _ => panic!("expected AgentRun"),
        }
    }

    #[test]
    fn test_daemon_message_agent_approve() {
        let msg = DaemonMessage::AgentApprove {
            id: "x".into(),
            run_id: "run-1".into(),
            approved: true,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("agent_approve"));
        let decoded: DaemonMessage = serde_json::from_str(&s).unwrap();
        match decoded {
            DaemonMessage::AgentApprove {
                id,
                run_id,
                approved,
            } => {
                assert_eq!(id, "x");
                assert_eq!(run_id, "run-1");
                assert!(approved);
            }
            _ => panic!("expected AgentApprove"),
        }
    }

    #[test]
    fn test_daemon_message_agent_interrupt() {
        let msg = DaemonMessage::AgentInterrupt {
            id: "i1".into(),
            run_id: "run-2".into(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("agent_interrupt"));
        let decoded: DaemonMessage = serde_json::from_str(&s).unwrap();
        match decoded {
            DaemonMessage::AgentInterrupt { id, run_id } => {
                assert_eq!(id, "i1");
                assert_eq!(run_id, "run-2");
            }
            _ => panic!("expected AgentInterrupt"),
        }
    }

    #[test]
    fn test_daemon_event_serialise() {
        let ev = DaemonEvent {
            run_id: "run-1".into(),
            event: pagerunner_agent::AgentEvent::Progress {
                message: "step 1".into(),
            },
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("run-1"));
        assert!(s.contains("step 1"));
        let decoded: DaemonEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(decoded.run_id, "run-1");
    }

    #[test]
    fn test_legacy_daemon_request_still_parses() {
        // Legacy format: plain DaemonRequest without "type" field
        let legacy = r#"{"id":"abc","tool":"open_session","args":{"profile":"personal"}}"#;
        // Should NOT parse as DaemonMessage (requires "type" field)
        let msg_result: std::result::Result<DaemonMessage, _> = serde_json::from_str(legacy);
        assert!(
            msg_result.is_err(),
            "legacy format should not parse as DaemonMessage"
        );
        // But SHOULD parse as DaemonRequest
        let req: DaemonRequest = serde_json::from_str(legacy).unwrap();
        assert_eq!(req.tool, "open_session");
    }
}
