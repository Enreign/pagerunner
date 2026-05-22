use crate::error::{PagerunnerError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChromeProfile {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub user_data_dir: Option<String>,
    #[serde(default)]
    pub debug_port: Option<u16>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    #[serde(default = "default_true")]
    pub sanitize_content: bool,
    #[serde(default = "default_true")]
    pub scan_injections: bool,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub max_navigations: Option<u32>,
    #[serde(default)]
    pub blocked_tools: Vec<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            sanitize_content: true,
            scan_injections: true,
            allowed_domains: Vec::new(),
            max_navigations: None,
            blocked_tools: Vec::new(),
            allowed_tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AnonMode {
    #[default]
    Tokenize,
    Redact,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntityTypeConfig {
    Email,
    Phone,
    CreditCard,
    Iban,
    Ssn,
    Ip,
    Person,
    Org,
    /// Scrub API keys, tokens, and credentials before content reaches the LLM.
    /// Covers ~18 Tier 1 service patterns (npm, GitHub, Stripe, OpenAI, AWS, etc.)
    /// plus JWT tokens and PEM private key headers.
    Secret,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomPattern {
    pub name: String,
    #[serde(flatten)]
    pub kind: CustomPatternKind,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomPatternKind {
    Pattern(String),
    Literal(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DomainAnonProfile {
    pub name: String,
    pub domains: Vec<String>,
    #[serde(default)]
    pub mode: AnonMode,
    #[serde(default)]
    pub entities: Vec<EntityTypeConfig>,
    #[serde(default)]
    pub custom_patterns: Vec<CustomPattern>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AnonymizationConfig {
    #[serde(default)]
    pub profiles: Vec<DomainAnonProfile>,
}

fn default_max_snapshot_versions() -> usize {
    10
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetentionConfig {
    /// Max snapshot versions to keep per (profile, origin). 0 = unlimited.
    /// Old default was 3; new default is 10.
    #[serde(default = "default_max_snapshot_versions")]
    pub max_snapshot_versions: usize,
    /// Days after which site knowledge entries expire. 0 = never.
    /// Old default was 90; new default is 0 (indefinite).
    #[serde(default)]
    pub site_knowledge_ttl_days: u64,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            max_snapshot_versions: default_max_snapshot_versions(),
            site_knowledge_ttl_days: 0,
        }
    }
}

fn default_buffer_capacity() -> usize {
    500
}

fn default_checkpoint_interval() -> u64 {
    300
} // 5 minutes

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CheckpointConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_checkpoint_interval")]
    pub interval_seconds: u64,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: default_checkpoint_interval(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConfig {
    #[serde(default = "default_buffer_capacity")]
    pub buffer_capacity: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            buffer_capacity: 500,
        }
    }
}

/// Global NER configuration.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct NerConfig {
    /// When Some(false): NER is disabled globally, even if entity list includes PERSON/ORG.
    /// When None or Some(true): NER runs if compiled with --features ner.
    pub enabled: Option<bool>,
}

fn default_recording_fps() -> u8 {
    10
}

fn default_output_fps() -> u8 {
    30
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingFormat {
    #[default]
    Mp4,
    Webm,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecordingConfig {
    #[serde(default)]
    pub storage_dir: Option<String>,
    #[serde(default)]
    pub retention_days: u64,
    #[serde(default)]
    pub max_size_mb: u64,
    #[serde(default)]
    pub format: RecordingFormat,
    #[serde(default)]
    pub auto_record: bool,
    /// Capture frames per second (1-15, default 10)
    #[serde(default = "default_recording_fps")]
    pub fps: u8,
    /// Output frame rate after motion interpolation (default 30)
    #[serde(default = "default_output_fps")]
    pub output_fps: u8,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            storage_dir: None,
            retention_days: 0,
            max_size_mb: 0,
            format: RecordingFormat::default(),
            auto_record: false,
            fps: default_recording_fps(),
            output_fps: default_output_fps(),
        }
    }
}

fn default_overlay_position() -> String {
    "bottom".to_string()
}
fn default_overlay_font() -> String {
    "Helvetica".to_string()
}
fn default_overlay_font_size() -> u32 {
    36
}
fn default_overlay_text_color() -> String {
    "white".to_string()
}
fn default_overlay_bg_color() -> String {
    "#000000AA".to_string()
}
fn default_overlay_bar_height() -> u32 {
    120
}

/// Configuration for text overlays rendered by `render_recording`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverlayConfig {
    /// Position: "top" or "bottom" (default: "bottom")
    #[serde(default = "default_overlay_position")]
    pub position: String,
    /// Font name (default: "Helvetica"). Must be available to ImageMagick.
    #[serde(default = "default_overlay_font")]
    pub font: String,
    /// Font size in points (default: 36)
    #[serde(default = "default_overlay_font_size")]
    pub font_size: u32,
    /// Text color — any ImageMagick color name or hex (default: "white")
    #[serde(default = "default_overlay_text_color")]
    pub text_color: String,
    /// Background color with optional alpha (default: "#000000AA")
    #[serde(default = "default_overlay_bg_color")]
    pub bg_color: String,
    /// Bar height in pixels (default: 120). Capped at 10% of video height.
    #[serde(default = "default_overlay_bar_height")]
    pub bar_height: u32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            position: default_overlay_position(),
            font: default_overlay_font(),
            font_size: default_overlay_font_size(),
            text_color: default_overlay_text_color(),
            bg_color: default_overlay_bg_color(),
            bar_height: default_overlay_bar_height(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    /// Shared bearer token. Default for backwards compatibility.
    #[default]
    Token,
    /// Use Tailscale LocalAPI `whois` to identify the caller by tailnet
    /// identity. Requires the daemon to bind to a Tailscale IP and the
    /// `tailscale` CLI to be on PATH.
    Tailscale,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpApiConfig {
    /// Enable the HTTP API server alongside the Unix socket (default: false).
    #[serde(default)]
    pub enabled: bool,
    /// Bind address — use a Tailscale IP (100.x.x.x) to restrict to your tailnet.
    #[serde(default = "default_http_bind")]
    pub bind_address: String,
    /// TCP port (default: 9876).
    #[serde(default = "default_http_port")]
    pub port: u16,
    /// Bearer token for authentication. Required when `auth = "token"`.
    #[serde(default)]
    pub token: String,
    /// Authentication mode. `"token"` (default) requires a shared bearer token;
    /// `"tailscale"` validates the caller against the local Tailscale daemon.
    #[serde(default)]
    pub auth: AuthMode,
    /// When `auth = "tailscale"`, optionally restrict access to specific
    /// tailnet login names (e.g. "alice@example.com"). Empty = any peer in
    /// the tailnet is allowed.
    #[serde(default)]
    pub tailscale_allowed_users: Vec<String>,
    /// When `auth = "tailscale"`, optionally restrict access to peers carrying
    /// a specific tag (e.g. "tag:pagerunner-phone"). Empty = tag is not
    /// checked.
    #[serde(default)]
    pub tailscale_allowed_tags: Vec<String>,
}

fn default_http_bind() -> String {
    "127.0.0.1".into()
}

fn default_http_port() -> u16 {
    9876
}

/// Addresses that bind to all interfaces — rejected for security.
const WILDCARD_ADDRS: &[&str] = &["0.0.0.0", "::", "[::]", "*"];

impl HttpApiConfig {
    /// Validate the config. Returns an error if the bind address is a wildcard,
    /// or if auth requirements for the selected mode aren't met.
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if WILDCARD_ADDRS.contains(&self.bind_address.as_str()) {
            return Err(PagerunnerError::Config(format!(
                "http_api.bind_address = \"{}\" binds to all interfaces and is not allowed. \
                 Use a specific IP (e.g. 127.0.0.1 for local, or your Tailscale IP 100.x.x.x for remote access).",
                self.bind_address
            )));
        }
        if self.auth == AuthMode::Token && self.token.is_empty() {
            return Err(PagerunnerError::Config(
                "http_api.auth = \"token\" but http_api.token is empty — \
                 set a token or switch auth to \"tailscale\"."
                    .into(),
            ));
        }
        if self.auth == AuthMode::Tailscale
            && (self.bind_address == "127.0.0.1" || self.bind_address == "::1")
        {
            return Err(PagerunnerError::Config(
                "http_api.auth = \"tailscale\" requires binding to a Tailscale IP \
                 (100.x.x.x), not loopback — Tailscale cannot identify local callers."
                    .into(),
            ));
        }
        Ok(())
    }
}

impl Default for HttpApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_address: default_http_bind(),
            port: default_http_port(),
            token: String::new(),
            auth: AuthMode::default(),
            tailscale_allowed_users: Vec::new(),
            tailscale_allowed_tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgentDaemonConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub llm: pagerunner_llm::AgentLlmConfig,
    #[serde(default)]
    pub default_config: pagerunner_agent::AgentConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PagerunnerConfig {
    #[serde(default)]
    pub profiles: Vec<ChromeProfile>,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub anonymization: AnonymizationConfig,
    #[serde(default)]
    pub ner: NerConfig, // NEW
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub checkpoints: CheckpointConfig,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub recording: RecordingConfig,
    #[serde(default)]
    pub overlay: OverlayConfig,
    #[serde(default)]
    pub agent: AgentDaemonConfig,
    #[serde(default)]
    pub http_api: HttpApiConfig,
}

impl PagerunnerConfig {
    pub fn load() -> Result<Self> {
        let path = if let Ok(p) = std::env::var("PAGERUNNER_CONFIG_PATH") {
            std::path::PathBuf::from(p)
        } else {
            dirs::home_dir()
                .ok_or_else(|| PagerunnerError::Config("Cannot find home dir".into()))?
                .join(".pagerunner/config.toml")
        };

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path).map_err(|e| {
            PagerunnerError::Config(format!("Cannot read {}: {}", path.display(), e))
        })?;

        toml::from_str(&content)
            .map_err(|e| PagerunnerError::Config(format!("Invalid config: {}", e)))
    }

    pub fn find_profile(&self, name: &str) -> Option<&ChromeProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anon_mode_default() {
        let mode = AnonMode::default();
        assert_eq!(mode, AnonMode::Tokenize);
    }

    #[test]
    fn test_entity_type_config_deserialization() {
        let json = r#""EMAIL""#;
        let e: EntityTypeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(e, EntityTypeConfig::Email);

        let json = r#""CREDIT_CARD""#;
        let e: EntityTypeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(e, EntityTypeConfig::CreditCard);
    }

    #[test]
    fn test_anon_mode_deserialization() {
        let json = r#""tokenize""#;
        let m: AnonMode = serde_json::from_str(json).unwrap();
        assert_eq!(m, AnonMode::Tokenize);

        let json = r#""redact""#;
        let m: AnonMode = serde_json::from_str(json).unwrap();
        assert_eq!(m, AnonMode::Redact);
    }

    #[test]
    fn test_custom_pattern_regex_deserialization() {
        let json = r#"{"name":"JIRA_CODE","pattern":"(?:PROJ|INFRA)-\\d+"}"#;
        let p: CustomPattern = serde_json::from_str(json).unwrap();
        assert_eq!(p.name, "JIRA_CODE");
        assert!(matches!(p.kind, CustomPatternKind::Pattern(_)));
    }

    #[test]
    fn test_custom_pattern_literal_deserialization() {
        let json = r#"{"name":"COMPANY","literal":"Acme Corp"}"#;
        let p: CustomPattern = serde_json::from_str(json).unwrap();
        assert_eq!(p.name, "COMPANY");
        assert!(matches!(p.kind, CustomPatternKind::Literal(_)));
    }

    #[test]
    fn test_domain_anon_profile_deserialization() {
        let toml = r#"
name = "jira-work"
domains = ["jira.acme.com", "*.atlassian.net"]
mode = "tokenize"
entities = ["EMAIL", "PHONE"]
"#;
        let p: DomainAnonProfile = toml::from_str(toml).unwrap();
        assert_eq!(p.name, "jira-work");
        assert_eq!(p.domains.len(), 2);
        assert_eq!(p.mode, AnonMode::Tokenize);
        assert_eq!(p.entities.len(), 2);
    }

    #[test]
    fn test_anonymization_config_empty_default() {
        let config = AnonymizationConfig::default();
        assert!(config.profiles.is_empty());
    }

    #[test]
    fn test_pagerunner_config_with_anonymization() {
        // Config without anonymization section still loads (default)
        let toml = r#"
[[profiles]]
name = "personal"
display_name = "Personal"
user_data_dir = "/tmp/chrome"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(cfg.anonymization.profiles.is_empty());
    }

    #[test]
    fn test_domain_anon_profile_with_custom_patterns_toml() {
        let toml = r#"
name = "work"
domains = ["jira.acme.com"]
entities = ["EMAIL"]

[[custom_patterns]]
name = "JIRA"
pattern = "PROJ-\\d+"

[[custom_patterns]]
name = "COMPANY"
literal = "Acme Corp"
"#;
        let p: DomainAnonProfile = toml::from_str(toml).unwrap();
        assert_eq!(p.custom_patterns.len(), 2);
        assert!(
            matches!(&p.custom_patterns[0].kind, CustomPatternKind::Pattern(s) if s == "PROJ-\\d+")
        );
        assert!(
            matches!(&p.custom_patterns[1].kind, CustomPatternKind::Literal(s) if s == "Acme Corp")
        );
    }

    #[test]
    fn test_domain_anon_profile_entities_default() {
        // entities field should default to empty vec when not specified
        let toml = r#"
name = "work"
domains = ["jira.acme.com"]

[[custom_patterns]]
name = "JIRA"
pattern = "PROJ-\\d+"
"#;
        let p: DomainAnonProfile = toml::from_str(toml).unwrap();
        assert!(p.entities.is_empty());
        assert_eq!(p.custom_patterns.len(), 1);
    }

    #[test]
    fn test_parse_valid_config() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test Profile"
user_data_dir = "/tmp/chrome-test"
"#;
        let config: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.profiles[0].name, "test");
        assert_eq!(
            config.profiles[0].user_data_dir.as_deref(),
            Some("/tmp/chrome-test")
        );
    }

    #[test]
    fn test_find_profile() {
        let config = PagerunnerConfig {
            profiles: vec![ChromeProfile {
                name: "a".into(),
                display_name: "A".into(),
                user_data_dir: Some("/tmp/a".into()),
                debug_port: None,
                kind: None,
            }],
            ..Default::default()
        };
        assert!(config.find_profile("a").is_some());
        assert!(config.find_profile("missing").is_none());
    }

    #[test]
    fn test_parse_security_config_defaults() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"
"#;
        let config: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(config.security.sanitize_content);
        assert!(config.security.scan_injections);
        assert!(config.security.allowed_domains.is_empty());
        assert!(config.security.max_navigations.is_none());
    }

    #[test]
    fn test_parse_security_config_explicit() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"

[security]
sanitize_content = false
scan_injections = false
allowed_domains = ["github.com", "docs.rs"]
max_navigations = 20
"#;
        let config: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(!config.security.sanitize_content);
        assert!(!config.security.scan_injections);
        assert_eq!(
            config.security.allowed_domains,
            vec!["github.com", "docs.rs"]
        );
        assert_eq!(config.security.max_navigations, Some(20));
    }

    #[test]
    fn security_config_tool_fields_default_empty() {
        let cfg = SecurityConfig::default();
        assert!(cfg.blocked_tools.is_empty());
        assert!(cfg.allowed_tools.is_empty());
    }

    #[test]
    fn security_config_tool_fields_parse_from_toml() {
        let toml = r#"
[security]
blocked_tools = ["evaluate", "screenshot"]
allowed_tools = ["navigate", "get_content"]
"#;
        let config: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.security.blocked_tools,
            vec!["evaluate", "screenshot"]
        );
        assert_eq!(
            config.security.allowed_tools,
            vec!["navigate", "get_content"]
        );
    }

    #[test]
    fn test_entity_type_config_person_org_deserialization() {
        let e: EntityTypeConfig = serde_json::from_str(r#""PERSON""#).unwrap();
        assert_eq!(e, EntityTypeConfig::Person);

        let e: EntityTypeConfig = serde_json::from_str(r#""ORG""#).unwrap();
        assert_eq!(e, EntityTypeConfig::Org);
    }

    #[test]
    fn test_ner_config_absent_gives_none() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(
            cfg.ner.enabled.is_none(),
            "absent [ner] should give enabled=None"
        );
    }

    #[test]
    fn test_network_config_defaults() {
        let cfg = NetworkConfig::default();
        assert_eq!(cfg.buffer_capacity, 500);
    }

    #[test]
    fn test_network_config_from_toml() {
        let toml = r#"
[network]
buffer_capacity = 100
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.network.buffer_capacity, 100);
    }

    #[test]
    fn test_network_config_absent_gives_default() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.network.buffer_capacity, 500);
    }

    #[test]
    fn test_ner_config_explicit_disabled() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"

[ner]
enabled = false
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.ner.enabled, Some(false));
    }

    #[test]
    fn test_checkpoint_config_defaults() {
        let config = PagerunnerConfig::default();
        assert_eq!(config.checkpoints.interval_seconds, 300); // 5 min default
        assert!(config.checkpoints.enabled);
    }

    #[test]
    fn test_checkpoint_config_from_toml() {
        let toml = r#"
[checkpoints]
enabled = false
interval_seconds = 60
"#;
        let config: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(!config.checkpoints.enabled);
        assert_eq!(config.checkpoints.interval_seconds, 60);
    }

    #[test]
    fn test_chrome_profile_kind_defaults_to_none() {
        let toml = r#"
[[profiles]]
name = "personal"
display_name = "Personal"
user_data_dir = "/tmp/chrome"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(cfg.profiles[0].kind.is_none());
    }

    #[test]
    fn test_chrome_profile_kind_agent() {
        let toml = r#"
[[profiles]]
name = "agent-1"
display_name = "Agent 1"
user_data_dir = "/tmp/chrome-agent"
kind = "agent"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.profiles[0].kind.as_deref(), Some("agent"));
    }

    #[test]
    fn test_retention_config_defaults() {
        let config = PagerunnerConfig::default();
        assert_eq!(config.retention.max_snapshot_versions, 10);
        assert_eq!(config.retention.site_knowledge_ttl_days, 0); // 0 = indefinite
    }

    #[test]
    fn test_retention_config_from_toml() {
        let toml_str = r#"
[retention]
max_snapshot_versions = 5
site_knowledge_ttl_days = 30
"#;
        let config: PagerunnerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.retention.max_snapshot_versions, 5);
        assert_eq!(config.retention.site_knowledge_ttl_days, 30);
    }

    #[test]
    fn test_attached_profile_parses_without_user_data_dir() {
        let toml = r#"
[[profiles]]
name = "chrome-9225"
display_name = "Chrome :9225"
kind = "attached"
debug_port = 9225
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.profiles[0].name, "chrome-9225");
        assert_eq!(cfg.profiles[0].kind.as_deref(), Some("attached"));
        assert_eq!(cfg.profiles[0].debug_port, Some(9225u16));
        assert!(cfg.profiles[0].user_data_dir.is_none());
    }

    #[test]
    fn test_existing_profile_still_parses_with_user_data_dir() {
        let toml = r#"
[[profiles]]
name = "personal"
display_name = "Personal"
user_data_dir = "/tmp/chrome"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            cfg.profiles[0].user_data_dir.as_deref(),
            Some("/tmp/chrome")
        );
        assert!(cfg.profiles[0].debug_port.is_none());
    }

    #[test]
    fn test_recording_config_defaults() {
        let config = PagerunnerConfig::default();
        assert!(config.recording.storage_dir.is_none());
        assert_eq!(config.recording.retention_days, 0);
        assert_eq!(config.recording.max_size_mb, 0);
        assert_eq!(config.recording.format, RecordingFormat::Mp4);
        assert!(!config.recording.auto_record);
        assert_eq!(config.recording.fps, 10);
    }

    #[test]
    fn test_recording_config_from_toml() {
        let toml = r#"
[recording]
storage_dir = "/tmp/recordings"
retention_days = 30
max_size_mb = 500
format = "webm"
auto_record = true
fps = 5
"#;
        let config: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.recording.storage_dir.as_deref(),
            Some("/tmp/recordings")
        );
        assert_eq!(config.recording.retention_days, 30);
        assert_eq!(config.recording.max_size_mb, 500);
        assert_eq!(config.recording.format, RecordingFormat::Webm);
        assert!(config.recording.auto_record);
        assert_eq!(config.recording.fps, 5);
    }

    #[test]
    fn test_recording_config_absent_gives_defaults() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(!cfg.recording.auto_record);
        assert_eq!(cfg.recording.fps, 10);
        assert_eq!(cfg.recording.output_fps, 30);
    }

    #[test]
    fn test_recording_config_partial_overrides() {
        let toml = r#"
[recording]
fps = 10
auto_record = true
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(cfg.recording.auto_record);
        assert_eq!(cfg.recording.fps, 10);
        // Rest should be defaults
        assert_eq!(cfg.recording.format, RecordingFormat::Mp4);
        assert!(cfg.recording.storage_dir.is_none());
        assert_eq!(cfg.recording.retention_days, 0);
        assert_eq!(cfg.recording.max_size_mb, 0);
    }

    #[test]
    fn test_recording_format_serialization_roundtrip() {
        let mp4: RecordingFormat = serde_json::from_str(r#""mp4""#).unwrap();
        assert_eq!(mp4, RecordingFormat::Mp4);
        let webm: RecordingFormat = serde_json::from_str(r#""webm""#).unwrap();
        assert_eq!(webm, RecordingFormat::Webm);
        let json = serde_json::to_string(&RecordingFormat::Webm).unwrap();
        assert_eq!(json, r#""webm""#);
    }

    #[test]
    fn test_agent_config_absent_gives_defaults() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(!cfg.agent.enabled);
        assert_eq!(cfg.agent.llm.default_provider, "anthropic");
        assert_eq!(cfg.agent.default_config.provider, "anthropic");
    }

    #[test]
    fn test_agent_config_from_toml() {
        let toml = r#"
[agent]
enabled = true

[agent.llm]
default_provider = "ollama"
default_model = "llama3"

[agent.default_config]
provider = "ollama"
model = "llama3"

[agent.default_config.budget]
max_steps = 20
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(cfg.agent.enabled);
        assert_eq!(cfg.agent.llm.default_provider, "ollama");
        assert_eq!(cfg.agent.llm.default_model, "llama3");
        assert_eq!(cfg.agent.default_config.provider, "ollama");
        assert_eq!(cfg.agent.default_config.model, "llama3");
        assert_eq!(cfg.agent.default_config.budget.max_steps, 20);
    }

    #[test]
    fn test_agent_config_partial() {
        let toml = r#"
[agent]
enabled = true
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(cfg.agent.enabled);
        // LLM and agent config should use defaults
        assert_eq!(cfg.agent.llm.default_provider, "anthropic");
        assert_eq!(cfg.agent.default_config.budget.max_steps, 50);
    }

    #[test]
    fn test_recording_config_with_all_fields() {
        let toml = r#"
[recording]
storage_dir = "/data/recordings"
retention_days = 90
max_size_mb = 1024
format = "webm"
auto_record = true
fps = 5
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            cfg.recording.storage_dir.as_deref(),
            Some("/data/recordings")
        );
        assert_eq!(cfg.recording.retention_days, 90);
        assert_eq!(cfg.recording.max_size_mb, 1024);
        assert_eq!(cfg.recording.format, RecordingFormat::Webm);
        assert!(cfg.recording.auto_record);
        assert_eq!(cfg.recording.fps, 5);
    }

    #[test]
    fn test_http_api_config_absent_gives_defaults() {
        let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(!cfg.http_api.enabled);
        assert_eq!(cfg.http_api.bind_address, "127.0.0.1");
        assert_eq!(cfg.http_api.port, 9876);
        assert!(cfg.http_api.token.is_empty());
    }

    #[test]
    fn test_http_api_config_from_toml() {
        let toml = r#"
[http_api]
enabled = true
bind_address = "100.64.0.1"
port = 9877
token = "my-secret-token"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(cfg.http_api.enabled);
        assert_eq!(cfg.http_api.bind_address, "100.64.0.1");
        assert_eq!(cfg.http_api.port, 9877);
        assert_eq!(cfg.http_api.token, "my-secret-token");
    }

    #[test]
    fn test_http_api_config_partial() {
        let toml = r#"
[http_api]
enabled = true
token = "abc"
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
        assert!(cfg.http_api.enabled);
        assert_eq!(cfg.http_api.bind_address, "127.0.0.1");
        assert_eq!(cfg.http_api.port, 9876);
        assert_eq!(cfg.http_api.token, "abc");
    }

    #[test]
    fn test_http_api_rejects_wildcard_0000() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "0.0.0.0".into(),
            port: 9876,
            token: "t".into(),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_http_api_rejects_wildcard_ipv6() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "::".into(),
            port: 9876,
            token: "t".into(),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_http_api_rejects_wildcard_bracketed() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "[::]".into(),
            port: 9876,
            token: "t".into(),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_http_api_rejects_wildcard_star() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "*".into(),
            port: 9876,
            token: "t".into(),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_http_api_allows_localhost() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "127.0.0.1".into(),
            port: 9876,
            token: "t".into(),
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_http_api_allows_tailscale_ip() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "100.64.0.1".into(),
            port: 9876,
            token: "t".into(),
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_http_api_allows_private_ip() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "192.168.1.50".into(),
            port: 9876,
            token: "t".into(),
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_auth_mode_deserialize() {
        let toml_str = r#"
[http_api]
enabled = true
bind_address = "100.64.0.1"
auth = "tailscale"
tailscale_allowed_users = ["alice@example.com"]
tailscale_allowed_tags = ["tag:phone"]
"#;
        let cfg: PagerunnerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.http_api.auth, AuthMode::Tailscale);
        assert_eq!(
            cfg.http_api.tailscale_allowed_users,
            vec!["alice@example.com"]
        );
        assert_eq!(cfg.http_api.tailscale_allowed_tags, vec!["tag:phone"]);
    }

    #[test]
    fn test_auth_mode_default_is_token() {
        let cfg = HttpApiConfig::default();
        assert_eq!(cfg.auth, AuthMode::Token);
    }

    #[test]
    fn test_validate_rejects_empty_token_in_token_mode() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "127.0.0.1".into(),
            port: 9876,
            token: String::new(),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_loopback_in_tailscale_mode() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "127.0.0.1".into(),
            port: 9876,
            token: String::new(),
            auth: AuthMode::Tailscale,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_accepts_tailscale_on_tailnet_ip() {
        let cfg = HttpApiConfig {
            enabled: true,
            bind_address: "100.64.0.1".into(),
            port: 9876,
            token: String::new(),
            auth: AuthMode::Tailscale,
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_http_api_disabled_skips_validation() {
        let cfg = HttpApiConfig {
            enabled: false,
            bind_address: "0.0.0.0".into(),
            port: 9876,
            token: "t".into(),
            ..Default::default()
        };
        // Validation only rejects wildcards when enabled
        assert!(cfg.validate().is_ok());
    }
}
