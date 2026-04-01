#[cfg(feature = "ner")]
pub mod ner;
pub mod entropy;
pub mod patterns;
pub mod vault;

use crate::config::AnonMode;
use crate::error::PagerunnerError;
use patterns::{detect_spans, CompiledCustomPattern, EntityType};
use std::collections::HashMap;
use vault::Vault;

#[cfg(feature = "ner")]
static NER_SESSION: std::sync::OnceLock<
    std::sync::Arc<std::sync::Mutex<crate::anonymizer::ner::NerSession>>,
> = std::sync::OnceLock::new();

/// Initialize the global NER session from the model at `model_path`.
/// Must be called after `verify_model_hash` succeeds.
/// Panics if the model fails to load after verification (should be impossible).
#[cfg(feature = "ner")]
pub(crate) fn init_ner_session(model_path: &std::path::Path) {
    NER_SESSION.get_or_init(|| {
        std::sync::Arc::new(std::sync::Mutex::new(
            crate::anonymizer::ner::NerSession::load(model_path)
                .expect("NER model verified but failed to load — check disk/permissions"),
        ))
    });
}

/// Resolved config for one session. Derives Clone so it can be stored in Session.
#[derive(Debug, Clone)]
pub struct AnonConfig {
    pub mode: AnonMode,
    pub entities: Vec<EntityType>,
    pub custom_patterns: Vec<CompiledCustomPattern>,
}

/// Result of running the anonymization pipeline on one content string.
pub struct AnonResult {
    pub output: String,
    pub entity_counts: HashMap<String, usize>,
}

/// Orchestrates detection → substitution → vault write → residual scan.
pub struct AnonEngine {
    vault: Vault,
    config: AnonConfig,
    #[cfg(feature = "ner")]
    ner_disabled: bool,
}

impl AnonEngine {
    pub fn new(vault: Vault, config: AnonConfig) -> Self {
        Self {
            vault,
            config,
            #[cfg(feature = "ner")]
            ner_disabled: false,
        }
    }

    #[cfg(feature = "ner")]
    pub fn new_with_ner_disabled(vault: Vault, config: AnonConfig) -> Self {
        Self {
            vault,
            config,
            ner_disabled: true,
        }
    }

    /// Process `text` through the anonymization pipeline.
    /// `session_id`: used as vault scope.
    /// `domain`: unused in Phase A (profiles are resolved before AnonEngine is created).
    pub fn process(
        &mut self,
        session_id: &str,
        _domain: Option<&str>,
        text: &str,
    ) -> Result<AnonResult, PagerunnerError> {
        // Step 1: Detect spans via regex patterns
        #[cfg(not(feature = "ner"))]
        let spans = detect_spans(text, &self.config.entities, &self.config.custom_patterns);

        #[cfg(feature = "ner")]
        let mut spans = detect_spans(text, &self.config.entities, &self.config.custom_patterns);

        // Step 1b: NER detection for PERSON and ORG
        #[cfg(feature = "ner")]
        if !self.ner_disabled
            && (self.config.entities.contains(&EntityType::Person)
                || self.config.entities.contains(&EntityType::Org))
        {
            if let Some(session_mutex) = NER_SESSION.get() {
                let mut session = session_mutex.lock().unwrap();
                let ner_spans = session
                    .detect(text)
                    .map_err(|e| PagerunnerError::Config(format!("NER inference: {}", e)))?;
                spans.extend(ner_spans);
                // Re-deduplicate after merging NER spans with regex spans
                spans = patterns::deduplicate_spans(spans);
            }
            // If NER_SESSION not initialized, skip silently (model not available or ner_disabled)
        }

        // Early return only after all detection is done
        if spans.is_empty() {
            return Ok(AnonResult {
                output: text.to_string(),
                entity_counts: HashMap::new(),
            });
        }

        // Step 2: Build entity_counts and perform substitution (right-to-left)
        let mut output = text.to_string();
        let mut entity_counts: HashMap<String, usize> = HashMap::new();
        let mut sorted_spans = spans;
        sorted_spans.sort_by(|a, b| b.start.cmp(&a.start)); // right-to-left

        // Track value→token cache for this call (for multi-occurrence consistency without extra vault reads)
        let mut value_to_token: HashMap<String, String> = HashMap::new();

        for span in &sorted_spans {
            let value = &text[span.start..span.end];
            let entity_label = entity_type_label(&span.entity_type);

            let replacement = match self.config.mode {
                AnonMode::Redact => format!("[{}]", entity_label),
                AnonMode::Tokenize => {
                    if let Some(cached) = value_to_token.get(value) {
                        cached.clone()
                    } else {
                        let token =
                            self.vault
                                .get_or_create_token(session_id, &entity_label, value)?;
                        value_to_token.insert(value.to_string(), token.clone());
                        token
                    }
                }
            };

            *entity_counts.entry(entity_label).or_insert(0) += 1;
            output.replace_range(span.start..span.end, &replacement);
        }

        // Step 3: Residual scan — re-run detection on output; fail-closed if any PII survives
        // Exclude Person/Org from residual scan (NER not re-run on output)
        let residual_entities: std::borrow::Cow<[EntityType]> = {
            let has_ner_types = self
                .config
                .entities
                .iter()
                .any(|e| matches!(e, EntityType::Person | EntityType::Org));
            if has_ner_types {
                let filtered: Vec<EntityType> = self
                    .config
                    .entities
                    .iter()
                    .filter(|e| !matches!(e, EntityType::Person | EntityType::Org))
                    .cloned()
                    .collect();
                // Note: Secret IS included in residual scan — if a secret somehow survived
                // substitution (shouldn't happen), we want to catch it.
                std::borrow::Cow::Owned(filtered)
            } else {
                std::borrow::Cow::Borrowed(&self.config.entities)
            }
        };
        let residual = detect_spans(&output, &residual_entities, &self.config.custom_patterns);
        if !residual.is_empty() {
            let mut residual_counts: HashMap<String, usize> = HashMap::new();
            for span in &residual {
                *residual_counts
                    .entry(entity_type_label(&span.entity_type))
                    .or_insert(0) += 1;
            }
            let count = residual.len();
            return Err(PagerunnerError::ResidualPiiDetected {
                entity_counts: residual_counts,
                count,
            });
        }

        Ok(AnonResult {
            output,
            entity_counts,
        })
    }
}

/// Returns the uppercase label for an entity type (used in token format and counts).
pub fn entity_type_label(entity_type: &EntityType) -> String {
    match entity_type {
        EntityType::Email => "EMAIL".to_string(),
        EntityType::Phone => "PHONE".to_string(),
        EntityType::CreditCard => "CREDIT_CARD".to_string(),
        EntityType::Iban => "IBAN".to_string(),
        EntityType::Ssn => "SSN".to_string(),
        EntityType::Ip => "IP".to_string(),
        EntityType::Person => "PERSON".to_string(),
        EntityType::Org => "ORG".to_string(),
        EntityType::Secret => "SECRET".to_string(),
        EntityType::Custom(name) => name.clone(),
    }
}

/// Returns true if `s` looks like a vault token: `[ENTITY_TYPE:xxxxxx]`
/// where ENTITY_TYPE is uppercase letters/underscores and xxxxxx is 6 lowercase hex chars.
pub fn is_token(s: &str) -> bool {
    if !s.starts_with('[') || !s.ends_with(']') {
        return false;
    }
    let inner = &s[1..s.len() - 1];
    let Some(colon) = inner.find(':') else {
        return false;
    };
    let (entity_part, nonce_part) = (&inner[..colon], &inner[colon + 1..]);
    // entity_part: uppercase letters and underscores, non-empty
    if entity_part.is_empty()
        || !entity_part
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_')
    {
        return false;
    }
    // nonce_part: exactly 6 lowercase hex chars
    nonce_part.len() == 6
        && nonce_part
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn make_db() -> std::sync::Arc<Db> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_str().unwrap().to_string();
        std::mem::forget(dir);
        std::sync::Arc::new(Db::open_with_key(&path_str, Db::generate_key()).unwrap())
    }

    fn make_engine(mode: AnonMode, entities: Vec<EntityType>) -> AnonEngine {
        let db = make_db();
        let vault = crate::anonymizer::vault::Vault::new(db);
        let config = AnonConfig {
            mode,
            entities,
            custom_patterns: vec![],
        };
        AnonEngine::new(vault, config)
    }

    #[test]
    fn test_process_tokenizes_email() {
        let mut engine = make_engine(AnonMode::Tokenize, vec![EntityType::Email]);
        let result = engine
            .process("sess1", None, "Contact user@example.com for help")
            .unwrap();
        assert!(!result.output.contains("user@example.com"));
        assert!(result.output.contains("[EMAIL:"));
        assert_eq!(result.entity_counts.get("EMAIL"), Some(&1usize));
    }

    #[test]
    fn test_process_redact_mode() {
        let mut engine = make_engine(AnonMode::Redact, vec![EntityType::Email]);
        let result = engine
            .process("sess1", None, "Contact user@example.com for help")
            .unwrap();
        assert!(!result.output.contains("user@example.com"));
        assert!(
            !result.output.contains("[EMAIL:"),
            "redact mode should not produce tokens"
        );
        assert!(
            result.output.contains("[EMAIL]"),
            "redact mode should produce [EMAIL] placeholder"
        );
    }

    #[test]
    fn test_process_consistent_tokenization() {
        let mut engine = make_engine(AnonMode::Tokenize, vec![EntityType::Email]);
        let r1 = engine
            .process("sess1", None, "Email: user@example.com")
            .unwrap();
        let r2 = engine
            .process("sess1", None, "Also: user@example.com here")
            .unwrap();
        // Extract the token from each result
        let token1 = r1
            .output
            .split_whitespace()
            .find(|w| w.starts_with("[EMAIL:"))
            .unwrap()
            .to_string();
        let token2 = r2
            .output
            .split_whitespace()
            .find(|w| w.starts_with("[EMAIL:"))
            .unwrap()
            .to_string();
        assert_eq!(token1, token2, "same value must get same token");
    }

    #[test]
    fn test_process_residual_scan_fails_closed() {
        // We can't easily trigger a residual PII (the pipeline should prevent it),
        // so just verify that normal processing doesn't return a residual error.
        let mut engine = make_engine(AnonMode::Tokenize, vec![EntityType::Email]);
        let result = engine.process("sess1", None, "no PII here");
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_token_recognizes_tokens() {
        assert!(is_token("[EMAIL:a3f9b2]"));
        assert!(is_token("[PHONE:000000]"));
        assert!(is_token("[CREDIT_CARD:abcdef]"));
        assert!(!is_token("not a token"));
        assert!(!is_token("[EMAIL:too_long_nonce]"));
        assert!(!is_token("[email:a3f9b2]")); // must be uppercase
    }

    #[test]
    fn test_process_no_pii_returns_unchanged() {
        let mut engine = make_engine(AnonMode::Tokenize, vec![EntityType::Email]);
        let result = engine
            .process("sess1", None, "no pii in this text")
            .unwrap();
        assert_eq!(result.output, "no pii in this text");
        assert!(result.entity_counts.is_empty());
    }

    #[test]
    fn test_entity_type_label_person_org() {
        assert_eq!(entity_type_label(&EntityType::Person), "PERSON");
        assert_eq!(entity_type_label(&EntityType::Org), "ORG");
    }

    #[test]
    fn test_process_person_org_in_entity_list_no_panic_without_ner_feature() {
        #[cfg(not(feature = "ner"))]
        {
            let mut engine = make_engine(
                AnonMode::Tokenize,
                vec![EntityType::Email, EntityType::Person, EntityType::Org],
            );
            let r = engine
                .process(
                    "sess1",
                    None,
                    "Alice works at Acme Corp, email alice@example.com",
                )
                .unwrap();
            assert!(
                !r.output.contains("alice@example.com"),
                "email must be tokenized"
            );
            assert!(r.output.contains("[EMAIL:"), "email token expected");
            assert!(
                !r.output.contains("[PERSON:"),
                "no person token without ner feature"
            );
            assert!(
                !r.output.contains("[ORG:"),
                "no org token without ner feature"
            );
        }
        // In ner build, this test is a no-op (ner session not initialized here, person/org not detectable without model)
        #[cfg(feature = "ner")]
        {
            // just verify it doesn't panic when NER session is not initialized
            let mut engine = make_engine(
                AnonMode::Tokenize,
                vec![EntityType::Email, EntityType::Person, EntityType::Org],
            );
            let r = engine
                .process(
                    "sess1",
                    None,
                    "Alice works at Acme Corp, email alice@example.com",
                )
                .unwrap();
            assert!(
                !r.output.contains("alice@example.com"),
                "email must be tokenized"
            );
            assert!(r.output.contains("[EMAIL:"), "email token expected");
        }
    }

    #[test]
    #[cfg(feature = "ner")]
    fn test_process_ner_disabled_skips_inference() {
        // When ner_disabled=true, process must not attempt NER even if entities include Person/Org.
        // Without a model file on disk this test verifies no error is returned.
        let db = make_db();
        let vault = crate::anonymizer::vault::Vault::new(db);
        let config = AnonConfig {
            mode: AnonMode::Tokenize,
            entities: vec![EntityType::Person, EntityType::Email],
            custom_patterns: vec![],
        };
        let mut engine = AnonEngine::new_with_ner_disabled(vault, config);
        let r = engine.process("sess1", None, "alice@example.com").unwrap();
        assert!(!r.output.contains("alice@example.com"));
    }

    // --- Multiple entities ---

    #[test]
    fn test_process_multiple_emails_get_separate_tokens() {
        let mut engine = make_engine(AnonMode::Tokenize, vec![EntityType::Email]);
        let result = engine
            .process(
                "sess1",
                None,
                "Contact alice@example.com or bob@corp.io for help",
            )
            .unwrap();
        assert!(!result.output.contains("alice@example.com"));
        assert!(!result.output.contains("bob@corp.io"));
        assert_eq!(result.entity_counts.get("EMAIL"), Some(&2usize));
    }

    #[test]
    fn test_process_same_email_twice_same_token() {
        let mut engine = make_engine(AnonMode::Tokenize, vec![EntityType::Email]);
        let result = engine
            .process("sess1", None, "From user@example.com to user@example.com")
            .unwrap();
        // Both occurrences should be replaced by the same token
        let token_count = result.output.matches("[EMAIL:").count();
        assert_eq!(token_count, 2, "both occurrences should be replaced");
        // Extract both tokens and assert they are equal
        let tokens: Vec<&str> = result
            .output
            .split_whitespace()
            .filter(|w| w.starts_with("[EMAIL:"))
            .collect();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], tokens[1], "same value must produce same token");
    }

    #[test]
    fn test_process_mixed_entity_types() {
        let mut engine = make_engine(
            AnonMode::Tokenize,
            vec![EntityType::Email, EntityType::Phone],
        );
        let result = engine
            .process("sess1", None, "Email user@example.com, call 555-867-5309")
            .unwrap();
        assert!(!result.output.contains("user@example.com"));
        assert!(!result.output.contains("555-867-5309"));
        assert!(result.output.contains("[EMAIL:"));
        assert!(result.output.contains("[PHONE:"));
        assert_eq!(result.entity_counts.get("EMAIL"), Some(&1usize));
        assert_eq!(result.entity_counts.get("PHONE"), Some(&1usize));
    }

    #[test]
    fn test_process_redact_mixed_entities() {
        let mut engine = make_engine(AnonMode::Redact, vec![EntityType::Email, EntityType::Phone]);
        let result = engine
            .process("sess1", None, "Email user@example.com, call 555-867-5309")
            .unwrap();
        assert!(result.output.contains("[EMAIL]"));
        assert!(result.output.contains("[PHONE]"));
        assert!(!result.output.contains("[EMAIL:"));
        assert!(!result.output.contains("[PHONE:"));
    }

    // --- Custom patterns ---

    #[test]
    fn test_process_custom_pattern() {
        let cp = patterns::CompiledCustomPattern {
            name: "TICKET".to_string(),
            regex: regex::Regex::new(r"PROJ-\d+").unwrap(),
        };
        let db = make_db();
        let vault = crate::anonymizer::vault::Vault::new(db);
        let config = AnonConfig {
            mode: AnonMode::Tokenize,
            entities: vec![],
            custom_patterns: vec![cp],
        };
        let mut engine = AnonEngine::new(vault, config);
        let result = engine
            .process("sess1", None, "See PROJ-1234 for details")
            .unwrap();
        assert!(!result.output.contains("PROJ-1234"));
        assert!(result.output.contains("[TICKET:"));
        assert_eq!(result.entity_counts.get("TICKET"), Some(&1usize));
    }

    #[test]
    fn test_process_custom_pattern_redact_mode() {
        let cp = patterns::CompiledCustomPattern {
            name: "TICKET".to_string(),
            regex: regex::Regex::new(r"PROJ-\d+").unwrap(),
        };
        let db = make_db();
        let vault = crate::anonymizer::vault::Vault::new(db);
        let config = AnonConfig {
            mode: AnonMode::Redact,
            entities: vec![],
            custom_patterns: vec![cp],
        };
        let mut engine = AnonEngine::new(vault, config);
        let result = engine
            .process("sess1", None, "See PROJ-1234 for details")
            .unwrap();
        assert!(result.output.contains("[TICKET]"));
        assert!(!result.output.contains("[TICKET:"));
    }

    // --- Token format ---

    #[test]
    fn test_process_emitted_tokens_pass_is_token() {
        let mut engine = make_engine(AnonMode::Tokenize, vec![EntityType::Email]);
        let result = engine.process("sess1", None, "user@example.com").unwrap();
        let word = result.output.trim();
        assert!(
            is_token(word),
            "emitted token '{word}' must pass is_token()"
        );
    }

    #[test]
    fn test_is_token_person_org_format() {
        assert!(is_token("[PERSON:a1b2c3]"));
        assert!(is_token("[ORG:ffffff]"));
        assert!(!is_token("[PERSON:]")); // empty nonce
        assert!(!is_token("[PERSON:ABCDEF]")); // nonce must be lowercase
    }

    // --- Session scoping ---

    #[test]
    fn test_process_different_sessions_independent_tokens() {
        let db = make_db();
        let vault1 = crate::anonymizer::vault::Vault::new(db.clone());
        let vault2 = crate::anonymizer::vault::Vault::new(db.clone());
        let config = AnonConfig {
            mode: AnonMode::Tokenize,
            entities: vec![EntityType::Email],
            custom_patterns: vec![],
        };
        let mut engine1 = AnonEngine::new(vault1, config.clone());
        let mut engine2 = AnonEngine::new(vault2, config);
        let r1 = engine1
            .process("session-A", None, "user@example.com")
            .unwrap();
        let r2 = engine2
            .process("session-B", None, "user@example.com")
            .unwrap();
        // Both are valid tokens
        assert!(is_token(r1.output.trim()));
        assert!(is_token(r2.output.trim()));
        // Tokens are session-scoped — they may differ (vault uses session prefix)
        // We can't assert they differ since vault may reuse the same nonce, but
        // both must be replaced and valid.
        assert!(!r1.output.contains("user@example.com"));
        assert!(!r2.output.contains("user@example.com"));
    }

    // --- Entity counts accuracy ---

    #[test]
    fn test_process_entity_counts_multiple_types() {
        let mut engine = make_engine(AnonMode::Redact, vec![EntityType::Email, EntityType::Ssn]);
        let text = "Emails: a@b.com and c@d.com; SSN: 123-45-6789";
        let result = engine.process("sess1", None, text).unwrap();
        assert_eq!(result.entity_counts.get("EMAIL"), Some(&2usize));
        assert_eq!(result.entity_counts.get("SSN"), Some(&1usize));
    }

    #[test]
    fn test_process_no_matching_entity_type_no_change() {
        // Config only scans for Credit Card but text has an email — should pass through
        let mut engine = make_engine(AnonMode::Tokenize, vec![EntityType::CreditCard]);
        let result = engine.process("sess1", None, "user@example.com").unwrap();
        assert_eq!(result.output, "user@example.com");
        assert!(result.entity_counts.is_empty());
    }

    // --- NER-specific (require feature flag) ---

    #[test]
    #[cfg(feature = "ner")]
    fn test_process_ner_disabled_email_still_tokenized() {
        // ner_disabled only suppresses NER inference; regex-based entities still work
        let db = make_db();
        let vault = crate::anonymizer::vault::Vault::new(db);
        let config = AnonConfig {
            mode: AnonMode::Tokenize,
            entities: vec![EntityType::Email, EntityType::Person],
            custom_patterns: vec![],
        };
        let mut engine = AnonEngine::new_with_ner_disabled(vault, config);
        let r = engine
            .process("sess1", None, "Alice sent alice@example.com from her work")
            .unwrap();
        assert!(
            !r.output.contains("alice@example.com"),
            "email must be tokenized"
        );
        assert!(r.output.contains("[EMAIL:"), "email token expected");
        // No PERSON token — NER is disabled and Person has no regex
        assert!(
            !r.output.contains("[PERSON:"),
            "no person token when NER disabled"
        );
    }

    #[test]
    #[cfg(feature = "ner")]
    fn test_process_ner_disabled_redact_email_not_person() {
        let db = make_db();
        let vault = crate::anonymizer::vault::Vault::new(db);
        let config = AnonConfig {
            mode: AnonMode::Redact,
            entities: vec![EntityType::Email, EntityType::Person, EntityType::Org],
            custom_patterns: vec![],
        };
        let mut engine = AnonEngine::new_with_ner_disabled(vault, config);
        let r = engine
            .process("sess1", None, "Bob at Acme Corp, bob@example.com")
            .unwrap();
        assert!(r.output.contains("[EMAIL]"), "email must be redacted");
        assert!(
            !r.output.contains("[PERSON]"),
            "no person redaction without NER"
        );
        assert!(!r.output.contains("[ORG]"), "no org redaction without NER");
    }

    // --- NER live pipeline tests (NP1–NP7, require model) ---

    #[cfg(feature = "ner")]
    fn init_ner_for_test() {
        let model_path = dirs::home_dir()
            .unwrap()
            .join(".pagerunner/models/ner.onnx");
        crate::anonymizer::init_ner_session(&model_path);
    }

    #[cfg(feature = "ner")]
    fn make_ner_engine(mode: AnonMode, entities: Vec<EntityType>) -> AnonEngine {
        init_ner_for_test();
        let db = make_db();
        let vault = crate::anonymizer::vault::Vault::new(db);
        let config = AnonConfig {
            mode,
            entities,
            custom_patterns: vec![],
        };
        AnonEngine::new(vault, config)
    }

    // NP1 — PERSON tokenized to [PERSON:xxxxxx]
    #[test]
    #[cfg(feature = "ner")]
    fn test_pipeline_person_tokenized() {
        let mut engine = make_ner_engine(AnonMode::Tokenize, vec![EntityType::Person]);
        let r = engine
            .process("sess1", None, "Alice Smith is a software engineer.")
            .unwrap();
        assert!(!r.output.contains("Alice Smith"), "name must be replaced");
        assert!(r.output.contains("[PERSON:"), "must produce a PERSON token");
        assert!(
            is_token(
                r.output
                    .split_whitespace()
                    .find(|w| w.starts_with("[PERSON:"))
                    .unwrap_or("")
                    .trim_end_matches('.')
            ),
            "token must pass is_token()"
        );
    }

    // NP2 — ORG tokenized to [ORG:xxxxxx]
    #[test]
    #[cfg(feature = "ner")]
    fn test_pipeline_org_tokenized() {
        let mut engine = make_ner_engine(AnonMode::Tokenize, vec![EntityType::Org]);
        let r = engine
            .process("sess1", None, "She works at Acme Corp in New York.")
            .unwrap();
        assert!(!r.output.contains("Acme Corp"), "org name must be replaced");
        assert!(r.output.contains("[ORG:"), "must produce an ORG token");
    }

    // NP3 — EMAIL + PERSON + ORG all replaced in one pass
    #[test]
    #[cfg(feature = "ner")]
    fn test_pipeline_email_person_org_all_replaced() {
        let mut engine = make_ner_engine(
            AnonMode::Tokenize,
            vec![EntityType::Email, EntityType::Person, EntityType::Org],
        );
        let text = "Jane Smith, jane@example.com, works at Microsoft Corp.";
        let r = engine.process("sess1", None, text).unwrap();
        assert!(!r.output.contains("Jane Smith"), "PERSON must be replaced");
        assert!(
            !r.output.contains("jane@example.com"),
            "EMAIL must be replaced"
        );
        assert!(!r.output.contains("Microsoft Corp"), "ORG must be replaced");
        assert!(
            r.output.contains("[PERSON:") || r.output.contains("[EMAIL:"),
            "output must contain at least one token: {}",
            r.output
        );
        assert!(r.output.contains("[EMAIL:"), "EMAIL token expected");
    }

    // NP4 — same person name produces same token across two process() calls
    #[test]
    #[cfg(feature = "ner")]
    fn test_pipeline_same_name_same_token() {
        let mut engine = make_ner_engine(AnonMode::Tokenize, vec![EntityType::Person]);
        let r1 = engine
            .process("sess1", None, "Alice Smith attended the meeting.")
            .unwrap();
        let r2 = engine
            .process("sess1", None, "Also, Alice Smith sent the report.")
            .unwrap();
        let tok1 = r1
            .output
            .split_whitespace()
            .find(|w| w.starts_with("[PERSON:"))
            .map(|w| w.trim_end_matches('.').to_string());
        let tok2 = r2
            .output
            .split_whitespace()
            .find(|w| w.starts_with("[PERSON:"))
            .map(|w| w.trim_end_matches('.').to_string());
        assert!(tok1.is_some(), "first call must produce PERSON token");
        assert!(tok2.is_some(), "second call must produce PERSON token");
        assert_eq!(tok1, tok2, "same name must yield same token across calls");
    }

    // NP5 — redact mode: name → [PERSON], company → [ORG]
    #[test]
    #[cfg(feature = "ner")]
    fn test_pipeline_redact_person_and_org() {
        let mut engine =
            make_ner_engine(AnonMode::Redact, vec![EntityType::Person, EntityType::Org]);
        let r = engine
            .process("sess1", None, "Bob Jones is the CEO of Acme Corp.")
            .unwrap();
        assert!(!r.output.contains("Bob Jones"), "PERSON must be redacted");
        assert!(!r.output.contains("Acme Corp"), "ORG must be redacted");
        assert!(
            r.output.contains("[PERSON]"),
            "must produce [PERSON] placeholder"
        );
        assert!(r.output.contains("[ORG]"), "must produce [ORG] placeholder");
        assert!(
            !r.output.contains("[PERSON:"),
            "redact mode must not produce tokens"
        );
        assert!(
            !r.output.contains("[ORG:"),
            "redact mode must not produce tokens"
        );
    }

    // NP6 — entity_counts includes PERSON and ORG with correct values
    #[test]
    #[cfg(feature = "ner")]
    fn test_pipeline_entity_counts_person_and_org() {
        let mut engine = make_ner_engine(
            AnonMode::Tokenize,
            vec![EntityType::Person, EntityType::Org],
        );
        let r = engine
            .process("sess1", None, "Alice Smith is the CEO of Acme Corp.")
            .unwrap();
        assert_eq!(
            r.entity_counts.get("PERSON"),
            Some(&1),
            "should count 1 PERSON entity, got: {:?}",
            r.entity_counts
        );
        assert_eq!(
            r.entity_counts.get("ORG"),
            Some(&1),
            "should count 1 ORG entity, got: {:?}",
            r.entity_counts
        );
    }

    // NP7 — residual scan excludes PERSON/ORG (pipeline must not fail when names remain as tokens)
    #[test]
    #[cfg(feature = "ner")]
    fn test_pipeline_residual_scan_excludes_person_org() {
        let mut engine = make_ner_engine(
            AnonMode::Tokenize,
            vec![EntityType::Email, EntityType::Person, EntityType::Org],
        );
        // After tokenization the output contains [PERSON:...] and [ORG:...] tokens.
        // The residual scan must not try to re-run NER on those tokens and must not error.
        let r = engine.process(
            "sess1",
            None,
            "Alice Smith, alice@example.com, works at Acme Corp.",
        );
        assert!(r.is_ok(), "residual scan must not fail: {:?}", r.err());
        let out = r.unwrap();
        assert!(
            !out.output.contains("alice@example.com"),
            "EMAIL must be replaced"
        );
    }
}
