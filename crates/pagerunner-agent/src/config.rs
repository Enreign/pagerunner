//! Agent-level configuration combining provider, model, budget, and policy.

use serde::{Deserialize, Serialize};

use crate::autonomy::AutonomyPolicy;
use crate::budget::BudgetConfig;
use crate::context::ContextConfig;

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
    }
}
