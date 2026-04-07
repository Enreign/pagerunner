//! Agent event types emitted during an agent run.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use pagerunner_llm::Usage;

// ---------------------------------------------------------------------------
// AgentEvent
// ---------------------------------------------------------------------------

/// An artifact produced by the agent (screenshot, text, file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Artifact {
    Screenshot { data: Vec<u8> },
    Text { content: String },
    File { name: String, data: Vec<u8> },
}

/// Events emitted by the agent loop to inform callers of progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// The model produced an internal reasoning / thinking chunk.
    Thinking { text: String },

    /// The model requested a tool call.
    ToolCall { name: String, args: Value },

    /// A tool call completed (or errored).
    ToolResult {
        name: String,
        result: String,
        is_error: bool,
    },

    /// Generic progress message.
    Progress { message: String },

    /// A tool action requires human approval before proceeding.
    ApprovalRequired {
        run_id: String,
        action: String,
        description: String,
    },

    /// User responded to an approval request.
    ApprovalResponse {
        run_id: String,
        approved: bool,
    },

    /// The agent completed successfully.
    Done {
        summary: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        artifacts: Vec<Artifact>,
    },

    /// An error occurred.
    Error { message: String, recoverable: bool },

    /// The agent run was interrupted.
    Interrupted,

    /// The agent exceeded its configured budget.
    BudgetExceeded { reason: String },
}

// ---------------------------------------------------------------------------
// AgentOutcome
// ---------------------------------------------------------------------------

/// High-level outcome of an agent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOutcome {
    /// The agent completed its goal.
    Completed,
    /// The agent stopped because it reached its maximum step limit.
    MaxStepsReached,
    /// The agent stopped because it exceeded its token/cost budget.
    BudgetExceeded,
    /// The agent run was interrupted externally.
    Interrupted,
    /// The agent stopped due to an unrecoverable error.
    Error,
}

// ---------------------------------------------------------------------------
// AgentResult
// ---------------------------------------------------------------------------

/// Final result returned after an agent run finishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentResult {
    /// How the run ended.
    pub outcome: AgentOutcome,
    /// Optional summary produced by the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Total number of steps (LLM completions) executed.
    pub total_steps: u32,
    /// Cumulative token usage for the entire run.
    pub usage: Usage,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(
        value: &T,
    ) -> T {
        let json = serde_json::to_string(value).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    // --- AgentEvent variants ---

    #[test]
    fn thinking_event_serialization() {
        let ev = AgentEvent::Thinking {
            text: "considering options".to_string(),
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "thinking");
        assert_eq!(v["text"], "considering options");
    }

    #[test]
    fn tool_call_event_serialization() {
        let ev = AgentEvent::ToolCall {
            name: "navigate".to_string(),
            args: json!({"url": "https://example.com"}),
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "tool_call");
        assert_eq!(v["name"], "navigate");
        assert_eq!(v["args"]["url"], "https://example.com");
    }

    #[test]
    fn tool_result_event_serialization() {
        let ev = AgentEvent::ToolResult {
            name: "screenshot".to_string(),
            result: "ok".to_string(),
            is_error: false,
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["is_error"], false);
    }

    #[test]
    fn progress_event_serialization() {
        let ev = AgentEvent::Progress {
            message: "step 2 of 5".to_string(),
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "progress");
        assert_eq!(v["message"], "step 2 of 5");
    }

    #[test]
    fn approval_required_event_serialization() {
        let ev = AgentEvent::ApprovalRequired {
            run_id: "run-001".to_string(),
            action: "navigate".to_string(),
            description: "Navigate to payment page".to_string(),
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "approval_required");
        assert_eq!(v["run_id"], "run-001");
        assert_eq!(v["action"], "navigate");
    }

    #[test]
    fn done_event_serialization() {
        let ev = AgentEvent::Done {
            summary: "Task completed successfully".to_string(),
            artifacts: vec![],
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "done");
        assert_eq!(v["summary"], "Task completed successfully");
        // Empty artifacts should be omitted
        assert!(v.get("artifacts").is_none());
    }

    #[test]
    fn done_event_with_artifacts() {
        let ev = AgentEvent::Done {
            summary: "Done".to_string(),
            artifacts: vec![
                Artifact::Text { content: "hello".into() },
                Artifact::Screenshot { data: vec![1, 2, 3] },
            ],
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["artifacts"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn approval_response_event_serialization() {
        let ev = AgentEvent::ApprovalResponse {
            run_id: "run-1".to_string(),
            approved: true,
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "approval_response");
        assert_eq!(v["approved"], true);
    }

    #[test]
    fn error_event_serialization() {
        let ev = AgentEvent::Error {
            message: "network timeout".to_string(),
            recoverable: true,
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["recoverable"], true);
    }

    #[test]
    fn interrupted_event_serialization() {
        let ev = AgentEvent::Interrupted;
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "interrupted");
    }

    #[test]
    fn budget_exceeded_event_serialization() {
        let ev = AgentEvent::BudgetExceeded {
            reason: "max_steps reached".to_string(),
        };
        let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "budget_exceeded");
        assert_eq!(v["reason"], "max_steps reached");
    }

    // --- Roundtrip for all variants ---

    #[test]
    fn all_event_variants_roundtrip() {
        let events = vec![
            AgentEvent::Thinking {
                text: "t".to_string(),
            },
            AgentEvent::ToolCall {
                name: "click".to_string(),
                args: json!({"selector": "#btn"}),
            },
            AgentEvent::ToolResult {
                name: "click".to_string(),
                result: "clicked".to_string(),
                is_error: false,
            },
            AgentEvent::Progress {
                message: "working".to_string(),
            },
            AgentEvent::ApprovalRequired {
                run_id: "r1".to_string(),
                action: "fill".to_string(),
                description: "Fill form".to_string(),
            },
            AgentEvent::Done {
                summary: "done".to_string(),
                artifacts: vec![],
            },
            AgentEvent::ApprovalResponse {
                run_id: "r1".to_string(),
                approved: false,
            },
            AgentEvent::Error {
                message: "oops".to_string(),
                recoverable: false,
            },
            AgentEvent::Interrupted,
            AgentEvent::BudgetExceeded {
                reason: "too many tokens".to_string(),
            },
        ];
        for ev in &events {
            assert_eq!(round_trip(ev), *ev, "roundtrip failed for {ev:?}");
        }
    }

    // --- AgentOutcome serialization ---

    #[test]
    fn outcome_variants_serialize_snake_case() {
        let cases = [
            (AgentOutcome::Completed, "completed"),
            (AgentOutcome::MaxStepsReached, "max_steps_reached"),
            (AgentOutcome::BudgetExceeded, "budget_exceeded"),
            (AgentOutcome::Interrupted, "interrupted"),
            (AgentOutcome::Error, "error"),
        ];
        for (outcome, expected) in &cases {
            let s = serde_json::to_string(outcome).unwrap();
            assert_eq!(s, format!("\"{expected}\""), "failed for {outcome:?}");
        }
    }

    #[test]
    fn outcome_roundtrip() {
        for outcome in [
            AgentOutcome::Completed,
            AgentOutcome::MaxStepsReached,
            AgentOutcome::BudgetExceeded,
            AgentOutcome::Interrupted,
            AgentOutcome::Error,
        ] {
            assert_eq!(round_trip(&outcome), outcome);
        }
    }

    // --- AgentResult ---

    #[test]
    fn agent_result_serialization() {
        let result = AgentResult {
            outcome: AgentOutcome::Completed,
            summary: Some("All done".to_string()),
            total_steps: 5,
            usage: Usage {
                input_tokens: 1000,
                output_tokens: 200,
            },
        };
        let v: serde_json::Value = serde_json::to_value(&result).unwrap();
        assert_eq!(v["outcome"], "completed");
        assert_eq!(v["total_steps"], 5);
        assert_eq!(v["summary"], "All done");
        assert_eq!(v["usage"]["input_tokens"], 1000);
    }

    #[test]
    fn agent_result_roundtrip() {
        let result = AgentResult {
            outcome: AgentOutcome::MaxStepsReached,
            summary: None,
            total_steps: 50,
            usage: Usage {
                input_tokens: 5000,
                output_tokens: 1000,
            },
        };
        let rt = round_trip(&result);
        assert_eq!(rt, result);
        // summary=None should be omitted from JSON
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("summary"));
    }
}
