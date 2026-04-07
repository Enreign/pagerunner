//! Autonomy policy — decides whether a tool call requires human approval.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ToolDecision
// ---------------------------------------------------------------------------

/// Decision made by the autonomy policy for a single tool invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolDecision {
    /// Execute the tool automatically without asking the user.
    AutoApprove,
    /// Pause and ask the user for approval before executing.
    RequireApproval,
    /// Refuse to execute the tool entirely.
    Block,
}

// ---------------------------------------------------------------------------
// AutonomyPolicy
// ---------------------------------------------------------------------------

/// Policy that maps tool names to approval decisions.
///
/// Rules are evaluated in priority order: block > require_approval >
/// auto_approve > default (AutoApprove).
///
/// Each list entry is a tool name or `"*"` (wildcard that matches any tool).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AutonomyPolicy {
    /// Tools that are always blocked (highest priority).
    #[serde(default)]
    pub block: Vec<String>,

    /// Tools that require explicit human approval.
    #[serde(default)]
    pub require_approval: Vec<String>,

    /// Tools that are always auto-approved.
    #[serde(default)]
    pub auto_approve: Vec<String>,
}

impl AutonomyPolicy {
    /// Decide what to do with a tool invocation.
    ///
    /// Priority: block > require_approval > auto_approve > default AutoApprove.
    pub fn decide(&self, tool: &str) -> ToolDecision {
        if Self::matches_any(&self.block, tool) {
            return ToolDecision::Block;
        }
        if Self::matches_any(&self.require_approval, tool) {
            return ToolDecision::RequireApproval;
        }
        if Self::matches_any(&self.auto_approve, tool) {
            return ToolDecision::AutoApprove;
        }
        // Default: auto-approve anything not explicitly listed.
        ToolDecision::AutoApprove
    }

    /// Check whether `tool` matches any entry in `patterns`.
    ///
    /// A pattern of `"*"` matches any tool name.  All other patterns are
    /// matched as exact strings (case-sensitive).
    fn matches_any(patterns: &[String], tool: &str) -> bool {
        patterns
            .iter()
            .any(|p| p == "*" || p.as_str() == tool)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Empty policy ---

    #[test]
    fn empty_policy_auto_approves_everything() {
        let policy = AutonomyPolicy::default();
        assert_eq!(policy.decide("navigate"), ToolDecision::AutoApprove);
        assert_eq!(policy.decide("click"), ToolDecision::AutoApprove);
        assert_eq!(policy.decide("fill"), ToolDecision::AutoApprove);
    }

    // --- Block takes priority ---

    #[test]
    fn block_takes_priority_over_require_approval() {
        let policy = AutonomyPolicy {
            block: vec!["dangerous_tool".to_string()],
            require_approval: vec!["dangerous_tool".to_string()],
            auto_approve: vec![],
        };
        assert_eq!(policy.decide("dangerous_tool"), ToolDecision::Block);
    }

    #[test]
    fn block_takes_priority_over_auto_approve() {
        let policy = AutonomyPolicy {
            block: vec!["bad_tool".to_string()],
            require_approval: vec![],
            auto_approve: vec!["bad_tool".to_string()],
        };
        assert_eq!(policy.decide("bad_tool"), ToolDecision::Block);
    }

    // --- Wildcard matching ---

    #[test]
    fn wildcard_in_block_blocks_all() {
        let policy = AutonomyPolicy {
            block: vec!["*".to_string()],
            require_approval: vec![],
            auto_approve: vec![],
        };
        assert_eq!(policy.decide("navigate"), ToolDecision::Block);
        assert_eq!(policy.decide("click"), ToolDecision::Block);
        assert_eq!(policy.decide("anything"), ToolDecision::Block);
    }

    #[test]
    fn wildcard_in_require_approval_unless_blocked() {
        let policy = AutonomyPolicy {
            block: vec!["super_dangerous".to_string()],
            require_approval: vec!["*".to_string()],
            auto_approve: vec![],
        };
        // blocked tool still gets blocked
        assert_eq!(policy.decide("super_dangerous"), ToolDecision::Block);
        // everything else needs approval
        assert_eq!(policy.decide("navigate"), ToolDecision::RequireApproval);
        assert_eq!(policy.decide("click"), ToolDecision::RequireApproval);
    }

    #[test]
    fn wildcard_in_auto_approve() {
        let policy = AutonomyPolicy {
            block: vec![],
            require_approval: vec![],
            auto_approve: vec!["*".to_string()],
        };
        assert_eq!(policy.decide("anything"), ToolDecision::AutoApprove);
    }

    // --- Mixed policy ---

    #[test]
    fn mixed_policy_correct_priority() {
        let policy = AutonomyPolicy {
            block: vec!["exec".to_string()],
            require_approval: vec!["fill".to_string(), "click".to_string()],
            auto_approve: vec!["navigate".to_string(), "screenshot".to_string()],
        };
        assert_eq!(policy.decide("exec"), ToolDecision::Block);
        assert_eq!(policy.decide("fill"), ToolDecision::RequireApproval);
        assert_eq!(policy.decide("click"), ToolDecision::RequireApproval);
        assert_eq!(policy.decide("navigate"), ToolDecision::AutoApprove);
        assert_eq!(policy.decide("screenshot"), ToolDecision::AutoApprove);
        // unlisted tool defaults to AutoApprove
        assert_eq!(policy.decide("get_content"), ToolDecision::AutoApprove);
    }

    // --- TOML parsing ---

    #[test]
    fn toml_parsing_empty_policy() {
        let policy: AutonomyPolicy = toml::from_str("").unwrap();
        assert!(policy.block.is_empty());
        assert!(policy.require_approval.is_empty());
        assert!(policy.auto_approve.is_empty());
    }

    #[test]
    fn toml_parsing_full_policy() {
        let toml = r#"
            block = ["exec", "eval"]
            require_approval = ["fill", "click"]
            auto_approve = ["navigate", "screenshot"]
        "#;
        let policy: AutonomyPolicy = toml::from_str(toml).unwrap();
        assert_eq!(policy.block, vec!["exec", "eval"]);
        assert_eq!(policy.require_approval, vec!["fill", "click"]);
        assert_eq!(policy.auto_approve, vec!["navigate", "screenshot"]);
    }

    #[test]
    fn toml_partial_policy_fills_defaults() {
        let toml = r#"
            block = ["exec"]
        "#;
        let policy: AutonomyPolicy = toml::from_str(toml).unwrap();
        assert_eq!(policy.block, vec!["exec"]);
        assert!(policy.require_approval.is_empty());
        assert!(policy.auto_approve.is_empty());
    }

    // --- Serde roundtrip ---

    #[test]
    fn serde_roundtrip() {
        let original = AutonomyPolicy {
            block: vec!["exec".to_string()],
            require_approval: vec!["fill".to_string()],
            auto_approve: vec!["navigate".to_string()],
        };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: AutonomyPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn require_approval_does_not_block() {
        let policy = AutonomyPolicy {
            block: vec![],
            require_approval: vec!["fill".to_string()],
            auto_approve: vec![],
        };
        assert_eq!(policy.decide("fill"), ToolDecision::RequireApproval);
        assert_ne!(policy.decide("fill"), ToolDecision::Block);
    }
}
