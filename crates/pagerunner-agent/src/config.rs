//! Agent-level configuration combining provider, model, budget, and policy.

use serde::{Deserialize, Serialize};

use crate::autonomy::AutonomyPolicy;
use crate::budget::BudgetConfig;
use crate::context::ContextConfig;

// ---------------------------------------------------------------------------
// SessionContext — auto-injected session_id / target_id
// ---------------------------------------------------------------------------

/// Pre-resolved browser session context. When set, `session_id` and
/// `target_id` are automatically injected into tool args and stripped from
/// tool schemas so the LLM never needs to generate them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionContext {
    pub session_id: String,
    pub target_id: String,
}

// ---------------------------------------------------------------------------
// Default helpers
// ---------------------------------------------------------------------------

fn default_provider() -> String {
    "anthropic".to_string()
}

fn default_model() -> String {
    "claude-haiku-4-5-20251001".to_string()
}

// ---------------------------------------------------------------------------
// AgentConfig
// ---------------------------------------------------------------------------

/// Full configuration for a single agent run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Provider name (e.g. `"anthropic"`, `"openai-compat"`, `"ollama"`).
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Model identifier to use for completions.
    #[serde(default = "default_model")]
    pub model: String,

    /// Resource budget limits for the run.
    #[serde(default)]
    pub budget: BudgetConfig,

    /// Autonomy policy — which tools need approval.
    #[serde(default)]
    pub autonomy: AutonomyPolicy,

    /// Context management — truncation and compaction limits.
    #[serde(default)]
    pub context: ContextConfig,

    /// Optional Chrome session profile name to attach to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_profile: Option<String>,

    /// Extra text appended to the agent's system prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt_extra: Option<String>,

    /// Pre-injected session context. If set, session_id and target_id are
    /// auto-injected into tool args and stripped from tool schemas so the
    /// LLM never sees or generates these parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_context: Option<SessionContext>,

    /// Multi-tab Scope (see `crate::scope::Scope`). Rendered into the system
    /// prompt when present so the agent knows which tabs are in play.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<crate::scope::Scope>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            budget: BudgetConfig::default(),
            autonomy: AutonomyPolicy::default(),
            context: ContextConfig::default(),
            session_profile: None,
            system_prompt_extra: None,
            session_context: None,
            scope: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_config_defaults() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.model, "claude-haiku-4-5-20251001");
        assert_eq!(cfg.budget.max_steps, 50);
        assert!(cfg.session_profile.is_none());
        assert!(cfg.system_prompt_extra.is_none());
        assert!(cfg.session_context.is_none());
    }

    #[test]
    fn agent_config_toml_minimal() {
        let cfg: AgentConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn agent_config_toml_full() {
        let toml = r#"
            provider = "ollama"
            model = "llama3"
            session_profile = "personal"
            system_prompt_extra = "Always respond in JSON."

            [budget]
            max_steps = 20
            total_token_budget = 10000

            [autonomy]
            block = ["exec"]
            require_approval = ["fill"]
        "#;
        let cfg: AgentConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.provider, "ollama");
        assert_eq!(cfg.model, "llama3");
        assert_eq!(cfg.session_profile.as_deref(), Some("personal"));
        assert_eq!(
            cfg.system_prompt_extra.as_deref(),
            Some("Always respond in JSON.")
        );
        assert_eq!(cfg.budget.max_steps, 20);
        assert_eq!(cfg.budget.total_token_budget, 10000);
        assert_eq!(cfg.autonomy.block, vec!["exec"]);
        assert_eq!(cfg.autonomy.require_approval, vec!["fill"]);
    }

    #[test]
    fn agent_config_serde_roundtrip() {
        let original = AgentConfig {
            provider: "ollama".to_string(),
            model: "mistral".to_string(),
            budget: BudgetConfig {
                max_steps: 10,
                ..BudgetConfig::default()
            },
            autonomy: AutonomyPolicy {
                block: vec!["exec".to_string()],
                ..AutonomyPolicy::default()
            },
            context: ContextConfig::default(),
            session_profile: Some("work".to_string()),
            system_prompt_extra: None,
            session_context: None,
            scope: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn agent_config_optional_fields_omitted_when_none() {
        let cfg = AgentConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("session_profile"));
        assert!(!json.contains("system_prompt_extra"));
        assert!(!json.contains("session_context"));
        assert!(!json.contains("scope"));
    }

    #[test]
    fn session_context_serde_roundtrip() {
        let ctx = SessionContext {
            session_id: "sess-123".to_string(),
            target_id: "tab-456".to_string(),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let decoded: SessionContext = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, ctx);
    }

    #[test]
    fn agent_config_with_session_context() {
        let cfg = AgentConfig {
            session_context: Some(SessionContext {
                session_id: "s1".to_string(),
                target_id: "t1".to_string(),
            }),
            ..AgentConfig::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("session_context"));
        assert!(json.contains("s1"));
        let decoded: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.session_context, cfg.session_context);
    }

    #[test]
    fn agent_config_with_scope_roundtrip() {
        use crate::scope::{Scope, ScopeTab};
        let cfg = AgentConfig {
            scope: Some(Scope {
                tabs: vec![ScopeTab {
                    session_id: "s-1".into(),
                    target_id: Some("t-a".into()),
                    label: "Notion".into(),
                    purpose: None,
                    digest: None,
                }],
                goal: Some("weekly review".into()),
                ..Default::default()
            }),
            ..AgentConfig::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"scope\""));
        assert!(json.contains("\"weekly review\""));
        let decoded: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.scope, cfg.scope);
    }

    #[test]
    fn agent_config_scope_none_by_default() {
        let cfg = AgentConfig::default();
        assert!(cfg.scope.is_none());
    }
}
