use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::Db;
use crate::security::PolicySummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub kind: AuditEventKind,
}

impl AuditEvent {
    pub fn new(kind: AuditEventKind) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuditEventKind {
    SessionOpened {
        session_id: String,
        profile: String,
        stealth: bool,
        policy_summary: PolicySummary,
    },
    SessionClosed {
        session_id: String,
    },
    ToolCall {
        session_id: Option<String>,
        tool: String,
        args_summary: String,
        outcome: ToolOutcome,
        /// true when a SecurityEvent was also recorded for this same invocation
        security_violation: bool,
    },
    SecurityEvent {
        session_id: Option<String>,
        kind: SecurityEventKind,
        detail: String,
    },
    ContentAnonymized {
        session_id: String,
        target_id: String,
        mode: String, // "tokenize" or "redact"
        entity_counts: std::collections::HashMap<String, usize>,
    },
    AdapterRegistered {
        origin: String,
        name: String,
        trusted: bool,
    },
    AuthTokenDetected {
        origin: String,
        kind: String, // "bearer", "basic", "api_key", "session_cookie" — never the raw value
    },
    SiteApiCalled {
        origin: String,
        adapter_name: String,
    },
    /// A credential was auto-detected in page content and scrubbed before reaching the LLM.
    /// The raw value is NEVER logged — only the secret name and match count.
    SecretScrubbed {
        session_id: String,
        target_id: String,
        /// Number of credential patterns found and replaced
        count: usize,
    },
    /// A secret was explicitly extracted from a page or stored via store_as_secret.
    /// Value is NEVER logged — only the name and source.
    SecretStored {
        /// Human-readable name the secret was stored under in the sealed KV
        name: String,
        /// "extract_secret" | "store_as_secret"
        source: String,
    },
    /// A secret was consumed via the CLI use-secret command.
    /// Value is NEVER logged — only the name and the command binary (not full args).
    SecretUsed {
        name: String,
        /// First token of the command only (e.g. "gh") — never full args
        command: String,
    },
    /// PII or credential was detected in page content AFTER anonymization ran —
    /// meaning the anonymizer missed it. Content was blocked before reaching the LLM.
    /// Values are NEVER logged — only entity types, counts, and detection method.
    AnonymizationGap {
        session_id: String,
        target_id: String,
        /// Entity labels that survived with their counts (e.g. {"SECRET": 1, "EMAIL": 2})
        entity_counts: std::collections::HashMap<String, usize>,
        /// How the gap was detected: "residual_scan" | "entropy_heuristic"
        source: String,
    },
    RecordingStarted {
        session_id: String,
        recording_id: String,
        profile: String,
    },
    RecordingStopped {
        session_id: String,
        recording_id: String,
        duration_ms: Option<u64>,
        markers_count: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolOutcome {
    Success,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityEventKind {
    UrlBlocked,
    ToolNotPermitted,
    NavBudgetExceeded,
    InjectionDetected,
    ContentSanitized,
}

pub struct AuditLog {
    log_path: PathBuf,
    db: Arc<Db>,
}

impl AuditLog {
    pub fn new(log_path: PathBuf, db: Arc<Db>) -> Self {
        Self { log_path, db }
    }

    /// Record an audit event to both file and DB.
    /// Best-effort: failures emit a warning but never propagate to the caller.
    pub async fn record(&self, event: AuditEvent) {
        let json = match serde_json::to_string(&event) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("audit: failed to serialize event: {}", e);
                return;
            }
        };

        // File first (grep/jq fallback if DB fails)
        if let Err(e) = self.write_to_file(&json) {
            tracing::warn!("audit: file write failed (DB still attempted): {}", e);
        }

        // DB (pagerunner audit CLI fallback if file fails)
        let ts = event.timestamp.timestamp_micros().max(0) as u64;
        let key = format!("{:020}/{}", ts, event.id);
        if let Err(e) = self.db.put("audit", &key, json.as_bytes()) {
            tracing::warn!("audit: DB write failed: {}", e);
        }
    }

    fn write_to_file(&self, json: &str) -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&self.log_path)?;
        writeln!(file, "{}", json)
    }
}

/// Build a privacy-safe args summary for a tool call.
/// NEVER logs raw content, passwords (type_text, fill), JS expressions, or kv values.
pub fn build_args_summary(tool: &str, args: &serde_json::Value) -> String {
    match tool {
        "navigate" => args["url"].as_str().unwrap_or("?").to_string(),
        "new_tab" => args["url"].as_str().unwrap_or("about:blank").to_string(),
        // Domain not available at dispatch time — log target_id as identifier
        "get_content" | "evaluate" | "screenshot" => {
            let tid = args["target_id"].as_str().unwrap_or("?");
            format!("target={}", tid)
        }
        "open_session" => args["profile"].as_str().unwrap_or("?").to_string(),
        "close_session" => args["session_id"].as_str().unwrap_or("?").to_string(),
        "save_snapshot" | "restore_snapshot" => args["origin"].as_str().unwrap_or("?").to_string(),
        "delete_snapshot" => {
            let profile = args["profile"].as_str().unwrap_or("?");
            let origin = args["origin"].as_str().unwrap_or("?");
            format!("{}/{}", profile, origin)
        }
        // kv operations: log namespace/key, never the value
        "kv_get" | "kv_set" | "kv_delete" | "kv_list" => {
            let ns = args["namespace"].as_str().unwrap_or("?");
            let key = args["key"].as_str().unwrap_or("?");
            format!("{}/{}", ns, key)
        }
        "kv_clear" => args["namespace"].as_str().unwrap_or("?").to_string(),
        "save_tab_state" | "restore_tab_state" => {
            args["session_id"].as_str().unwrap_or("?").to_string()
        }
        "click" => args["selector"].as_str().unwrap_or("?").to_string(),
        "scroll" => {
            if let Some(sel) = args["selector"].as_str() {
                format!("selector={}", sel)
            } else {
                let x = args["x"].as_i64().unwrap_or(0);
                let y = args["y"].as_i64().unwrap_or(0);
                format!("({},{})", x, y)
            }
        }
        "wait_for" => {
            if args["selector"].is_string() {
                "selector".to_string()
            } else if args["url"].is_string() {
                "url".to_string()
            } else {
                "ms".to_string()
            }
        }
        "get_site_knowledge" => args["origin"].as_str().unwrap_or("?").to_string(),
        "register_adapter" => {
            let origin = args["origin"].as_str().unwrap_or("?");
            let name = args["name"].as_str().unwrap_or("?");
            format!("{}/{}", origin, name)
            // NOTE: js_code is deliberately excluded
        }
        "call_site_api" => {
            let origin = args["origin"].as_str().unwrap_or("?");
            let name = args["name"].as_str().unwrap_or("?");
            format!("{}/{}", origin, name)
            // NOTE: params is deliberately excluded
        }
        // NEVER log the text/value for these — could be passwords
        "type_text" | "fill" | "select" => tool.to_string(),
        _ => tool.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn make_db() -> Arc<crate::db::Db> {
        let dir = tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        Arc::new(
            crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap(),
        )
    }

    #[test]
    fn audit_event_roundtrip() {
        let event = AuditEvent::new(AuditEventKind::SessionClosed {
            session_id: "abc".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        let back: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, event.id);
    }

    #[test]
    fn timestamp_key_preserves_chronological_order() {
        let a = format!("{:020}", u64::MIN);
        let b = format!("{:020}", 1_000_000u64);
        let c = format!("{:020}", u64::MAX);
        assert!(a < b);
        assert!(b < c);
        assert_eq!(c.len(), 20);
    }

    #[test]
    fn build_args_summary_navigate_returns_url() {
        let args = serde_json::json!({ "url": "https://example.com", "session_id": "s1" });
        assert_eq!(build_args_summary("navigate", &args), "https://example.com");
    }

    #[test]
    fn build_args_summary_type_text_never_logs_text() {
        let args = serde_json::json!({ "text": "hunter2", "session_id": "s1" });
        let summary = build_args_summary("type_text", &args);
        assert!(!summary.contains("hunter2"));
    }

    #[test]
    fn build_args_summary_fill_never_logs_value() {
        let args =
            serde_json::json!({ "selector": "#pass", "value": "secret", "session_id": "s1" });
        let summary = build_args_summary("fill", &args);
        assert!(!summary.contains("secret"));
    }

    #[test]
    fn build_args_summary_kv_set_never_logs_value() {
        let args = serde_json::json!({ "namespace": "ns", "key": "k", "value": "v" });
        let summary = build_args_summary("kv_set", &args);
        assert!(!summary.contains("\"v\""));
        assert!(summary.contains("ns/k"));
    }

    #[tokio::test]
    async fn record_writes_to_db() {
        let db = make_db();
        let dir = tempdir().unwrap();
        let log = AuditLog::new(dir.path().join("audit.log"), Arc::clone(&db));
        let event = AuditEvent::new(AuditEventKind::SessionClosed {
            session_id: "s1".into(),
        });
        log.record(event).await;
        let entries = db.scan_prefix("audit", "").unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn record_writes_to_file() {
        let db = make_db();
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let log = AuditLog::new(log_path.clone(), Arc::clone(&db));
        log.record(AuditEvent::new(AuditEventKind::SessionClosed {
            session_id: "s2".into(),
        }))
        .await;
        let contents = std::fs::read_to_string(&log_path).unwrap();
        assert!(contents.contains("SessionClosed"));
        assert!(contents.contains("s2"));
    }

    #[test]
    fn test_content_anonymized_event_serialization() {
        use std::collections::HashMap;
        let kind = AuditEventKind::ContentAnonymized {
            session_id: "sess1".to_string(),
            target_id: "TAB456".to_string(),
            mode: "tokenize".to_string(),
            entity_counts: {
                let mut m = HashMap::new();
                m.insert("EMAIL".to_string(), 2usize);
                m.insert("PHONE".to_string(), 1usize);
                m
            },
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("ContentAnonymized"));
        assert!(json.contains("EMAIL"));
        assert!(!json.contains("user@example.com")); // values never logged
    }

    #[test]
    fn build_args_summary_get_site_knowledge_logs_origin() {
        let args = serde_json::json!({ "origin": "https://linear.app" });
        let s = build_args_summary("get_site_knowledge", &args);
        assert_eq!(s, "https://linear.app");
    }

    #[test]
    fn build_args_summary_register_adapter_never_logs_js_code() {
        let args = serde_json::json!({
            "origin": "https://linear.app",
            "name": "create-comment",
            "js_code": "const secret = 'fetch(evil.com)';"
        });
        let s = build_args_summary("register_adapter", &args);
        assert!(s.contains("linear.app"));
        assert!(s.contains("create-comment"));
        assert!(!s.contains("secret"));
        assert!(!s.contains("evil"));
    }

    #[test]
    fn build_args_summary_call_site_api_never_logs_params() {
        let args = serde_json::json!({
            "origin": "https://linear.app",
            "name": "create-comment",
            "params": { "secret_key": "hunter2" }
        });
        let s = build_args_summary("call_site_api", &args);
        assert!(s.contains("linear.app"));
        assert!(s.contains("create-comment"));
        assert!(!s.contains("hunter2"));
    }

    #[test]
    fn audit_event_adapter_registered_serializes() {
        let kind = AuditEventKind::AdapterRegistered {
            origin: "https://linear.app".into(),
            name: "create-comment".into(),
            trusted: false,
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("AdapterRegistered"));
        assert!(json.contains("linear.app"));
        assert!(!json.contains("js_code"));
        assert!(json.contains("trusted"));
    }

    #[test]
    fn audit_event_auth_token_detected_serializes() {
        let kind = AuditEventKind::AuthTokenDetected {
            origin: "https://github.com".into(),
            kind: "bearer".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("AuthTokenDetected"));
        assert!(!json.contains("token_value"));
        assert!(json.contains("bearer")); // kind field is present in serialized output
    }

    #[test]
    fn audit_event_site_api_called_serializes() {
        let kind = AuditEventKind::SiteApiCalled {
            origin: "https://linear.app".into(),
            adapter_name: "create-comment".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("SiteApiCalled"));
        assert!(!json.contains("params"));
    }

    #[tokio::test]
    async fn record_multiple_events_appends() {
        let db = make_db();
        let dir = tempdir().unwrap();
        let log = AuditLog::new(dir.path().join("audit.log"), Arc::clone(&db));
        for i in 0..5 {
            log.record(AuditEvent::new(AuditEventKind::SessionClosed {
                session_id: format!("s{}", i),
            }))
            .await;
        }
        let entries = db.scan_prefix("audit", "").unwrap();
        assert_eq!(entries.len(), 5);
    }
}
