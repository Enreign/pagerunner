//! Provider factory — creates `LlmProvider` instances from configuration.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::anthropic::AnthropicProvider;
use crate::error::{LlmError, Result};
use crate::ollama::OllamaProvider;
use crate::openai_compat::OpenAiCompatProvider;
use crate::provider::LlmProvider;

// ---------------------------------------------------------------------------
// ProviderConfig
// ---------------------------------------------------------------------------

/// Configuration for a single named LLM provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider identifier: `"anthropic"`, `"openai-compat"`, or `"ollama"`.
    pub name: String,

    /// Name of the environment variable that holds the API key.
    /// When absent the factory uses the provider's default env-var name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,

    /// Override the provider's default base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Default model to use with this provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl ProviderConfig {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            api_key_env: None,
            base_url: None,
            model: None,
        }
    }
}

// ---------------------------------------------------------------------------
// AgentLlmConfig
// ---------------------------------------------------------------------------

fn default_provider_name() -> String {
    "anthropic".to_string()
}

fn default_model_name() -> String {
    "claude-haiku-4-5-20251001".to_string()
}

/// Top-level LLM configuration for the agent system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLlmConfig {
    /// Name of the provider to use by default.
    #[serde(default = "default_provider_name")]
    pub default_provider: String,

    /// Model identifier to use when none is specified per-request.
    #[serde(default = "default_model_name")]
    pub default_model: String,

    /// All configured providers.
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

impl Default for AgentLlmConfig {
    fn default() -> Self {
        Self {
            default_provider: default_provider_name(),
            default_model: default_model_name(),
            providers: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Factory functions
// ---------------------------------------------------------------------------

/// Read an API key from the named environment variable.
///
/// Returns `LlmError::NotConfigured` when the variable is absent or empty.
fn read_api_key(env_var: &str) -> Result<String> {
    std::env::var(env_var)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            LlmError::NotConfigured(format!(
                "environment variable `{env_var}` is not set or empty"
            ))
        })
}

/// Instantiate an `LlmProvider` from a [`ProviderConfig`].
pub fn create_provider(config: &ProviderConfig) -> Result<Arc<dyn LlmProvider>> {
    match config.name.as_str() {
        "anthropic" => {
            let env_var = config
                .api_key_env
                .as_deref()
                .unwrap_or("ANTHROPIC_API_KEY");
            let api_key = read_api_key(env_var)?;
            let model = config
                .model
                .clone()
                .unwrap_or_else(|| default_model_name());
            Ok(Arc::new(AnthropicProvider::new(api_key, model)))
        }
        "openai-compat" => {
            let env_var = config.api_key_env.as_deref().unwrap_or("OPENAI_API_KEY");
            let api_key = read_api_key(env_var)?;
            let base_url = config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let model = config
                .model
                .clone()
                .unwrap_or_else(|| "gpt-4o".to_string());
            Ok(Arc::new(OpenAiCompatProvider::new(
                api_key, base_url, model,
            )))
        }
        "ollama" => {
            let model = config
                .model
                .clone()
                .unwrap_or_else(|| "llama3".to_string());
            Ok(Arc::new(OllamaProvider::new(
                config.base_url.as_deref().map(|s| s.to_string()),
                model,
            )))
        }
        other => Err(LlmError::NotConfigured(format!(
            "unknown provider `{other}`; supported: anthropic, openai-compat, ollama"
        ))),
    }
}

/// Find or synthesise a [`ProviderConfig`] for the default provider, then
/// instantiate it.
pub fn create_default_provider(config: &AgentLlmConfig) -> Result<Arc<dyn LlmProvider>> {
    // Look for a matching named config first.
    if let Some(provider_config) = config
        .providers
        .iter()
        .find(|p| p.name == config.default_provider)
    {
        // Use the global default_model as a fallback when the provider config
        // does not specify its own model.
        let mut effective = provider_config.clone();
        if effective.model.is_none() {
            effective.model = Some(config.default_model.clone());
        }
        create_provider(&effective)
    } else {
        // No matching entry — synthesise a minimal config from the top-level fields.
        let synthetic = ProviderConfig {
            name: config.default_provider.clone(),
            api_key_env: None,
            base_url: None,
            model: Some(config.default_model.clone()),
        };
        create_provider(&synthetic)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Config defaults ---

    #[test]
    fn agent_llm_config_defaults() {
        let cfg = AgentLlmConfig::default();
        assert_eq!(cfg.default_provider, "anthropic");
        assert_eq!(cfg.default_model, "claude-haiku-4-5-20251001");
        assert!(cfg.providers.is_empty());
    }

    #[test]
    fn provider_config_new() {
        let cfg = ProviderConfig::new("ollama");
        assert_eq!(cfg.name, "ollama");
        assert!(cfg.api_key_env.is_none());
        assert!(cfg.base_url.is_none());
        assert!(cfg.model.is_none());
    }

    // --- TOML parsing ---

    #[test]
    fn agent_llm_config_toml_minimal() {
        let toml = r#"
            default_provider = "ollama"
            default_model = "llama3"
        "#;
        let cfg: AgentLlmConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.default_provider, "ollama");
        assert_eq!(cfg.default_model, "llama3");
        assert!(cfg.providers.is_empty());
    }

    #[test]
    fn agent_llm_config_toml_with_providers() {
        let toml = r#"
            default_provider = "anthropic"
            default_model = "claude-haiku-4-5-20251001"

            [[providers]]
            name = "anthropic"
            api_key_env = "MY_ANTHROPIC_KEY"
            model = "claude-opus-4-5"

            [[providers]]
            name = "ollama"
            base_url = "http://localhost:11434"
            model = "llama3"
        "#;
        let cfg: AgentLlmConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.providers.len(), 2);
        assert_eq!(cfg.providers[0].api_key_env.as_deref(), Some("MY_ANTHROPIC_KEY"));
        assert_eq!(cfg.providers[1].base_url.as_deref(), Some("http://localhost:11434"));
    }

    // --- Ollama creation (no env var needed) ---

    #[test]
    fn create_ollama_no_env_var() {
        let cfg = ProviderConfig {
            name: "ollama".to_string(),
            api_key_env: None,
            base_url: None,
            model: Some("llama3".to_string()),
        };
        match create_provider(&cfg) {
            Ok(provider) => assert_eq!(provider.name(), "ollama"),
            Err(e) => panic!("expected Ok, got error: {e}"),
        }
    }

    #[test]
    fn create_ollama_with_custom_base_url() {
        let cfg = ProviderConfig {
            name: "ollama".to_string(),
            api_key_env: None,
            base_url: Some("http://gpu-box:11434".to_string()),
            model: Some("mistral".to_string()),
        };
        match create_provider(&cfg) {
            Ok(provider) => assert_eq!(provider.name(), "ollama"),
            Err(e) => panic!("expected Ok, got error: {e}"),
        }
    }

    // --- Unknown provider ---

    #[test]
    fn unknown_provider_returns_not_configured() {
        let cfg = ProviderConfig::new("grok");
        match create_provider(&cfg) {
            Err(LlmError::NotConfigured(msg)) => assert!(msg.contains("grok")),
            other => panic!("expected NotConfigured, got something else: is_ok={}", other.is_ok()),
        }
    }

    // --- Missing API key ---

    #[test]
    fn anthropic_missing_api_key_fails() {
        // Use a unique env var that is definitely not set.
        let cfg = ProviderConfig {
            name: "anthropic".to_string(),
            api_key_env: Some("__PAGERUNNER_TEST_MISSING_KEY_ANTHROPIC__".to_string()),
            base_url: None,
            model: None,
        };
        match create_provider(&cfg) {
            Err(LlmError::NotConfigured(_)) => {}
            other => panic!("expected NotConfigured, got is_ok={}", other.is_ok()),
        }
    }

    #[test]
    fn openai_compat_missing_api_key_fails() {
        let cfg = ProviderConfig {
            name: "openai-compat".to_string(),
            api_key_env: Some("__PAGERUNNER_TEST_MISSING_KEY_OPENAI__".to_string()),
            base_url: None,
            model: None,
        };
        match create_provider(&cfg) {
            Err(LlmError::NotConfigured(_)) => {}
            other => panic!("expected NotConfigured, got is_ok={}", other.is_ok()),
        }
    }

    // --- Default provider synthetic fallback ---

    #[test]
    fn create_default_provider_falls_back_to_synthetic_ollama() {
        let cfg = AgentLlmConfig {
            default_provider: "ollama".to_string(),
            default_model: "llama3".to_string(),
            providers: Vec::new(), // no matching entry
        };
        // Ollama needs no key — should succeed.
        match create_default_provider(&cfg) {
            Ok(provider) => assert_eq!(provider.name(), "ollama"),
            Err(e) => panic!("expected Ok, got error: {e}"),
        }
    }

    #[test]
    fn create_default_provider_uses_matching_entry() {
        let cfg = AgentLlmConfig {
            default_provider: "ollama".to_string(),
            default_model: "mistral".to_string(),
            providers: vec![ProviderConfig {
                name: "ollama".to_string(),
                api_key_env: None,
                base_url: Some("http://custom:11434".to_string()),
                model: None, // should inherit default_model
            }],
        };
        match create_default_provider(&cfg) {
            Ok(provider) => assert_eq!(provider.name(), "ollama"),
            Err(e) => panic!("expected Ok, got error: {e}"),
        }
    }

    // --- Serialization roundtrip ---

    #[test]
    fn provider_config_serde_roundtrip() {
        let original = ProviderConfig {
            name: "anthropic".to_string(),
            api_key_env: Some("MY_KEY".to_string()),
            base_url: None,
            model: Some("claude-haiku-4-5-20251001".to_string()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: ProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn agent_llm_config_serde_roundtrip() {
        let original = AgentLlmConfig {
            default_provider: "ollama".to_string(),
            default_model: "llama3".to_string(),
            providers: vec![ProviderConfig::new("ollama")],
        };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: AgentLlmConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn agent_llm_config_toml_defaults_when_empty() {
        // Completely empty TOML should produce default values.
        let cfg: AgentLlmConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.default_provider, "anthropic");
        assert_eq!(cfg.default_model, "claude-haiku-4-5-20251001");
    }
}
