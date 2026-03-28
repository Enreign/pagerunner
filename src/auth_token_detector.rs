// src/auth_token_detector.rs

use crate::site_knowledge::SiteKnowledgeStore;
use std::collections::HashMap;

/// Detect auth tokens in HTTP request headers.
/// Returns a list of (kind, raw_token_value) pairs found in the headers.
/// Kinds: "bearer", "basic", "api_key", "session_cookie"
pub fn detect_tokens(headers: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut results = Vec::new();
    for (key, value) in headers {
        let key_lower = key.to_lowercase();
        match key_lower.as_str() {
            "authorization" => {
                if let Some(token) = value.strip_prefix("Bearer ") {
                    results.push(("bearer".into(), token.trim().into()));
                } else if let Some(token) = value.strip_prefix("Basic ") {
                    results.push(("basic".into(), token.trim().into()));
                }
            }
            "x-api-key" | "x-auth-token" => {
                if !value.is_empty() {
                    results.push(("api_key".into(), value.clone()));
                }
            }
            "cookie" => {
                for part in value.split(';') {
                    let part = part.trim();
                    if let Some((cname, _cval)) = part.split_once('=') {
                        let cname_lower = cname.trim().to_lowercase();
                        if matches!(cname_lower.as_str(), "session" | "token" | "auth") {
                            // Store the full "name=value" string as the token value (not just the value portion).
                            // This is intentional: the full cookie pair is what gets vaulted for replay purposes.
                            results.push(("session_cookie".into(), part.into()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    results
}

/// Returns a copy of headers with detected auth token values replaced by "[REDACTED]".
/// Used on vault encryption failure to ensure raw values never reach the ring buffer.
pub fn redact_detected_tokens(headers: &HashMap<String, String>) -> HashMap<String, String> {
    let tokens = detect_tokens(headers);
    if tokens.is_empty() {
        return headers.clone();
    }
    let mut result = headers.clone();
    for (key, value) in headers {
        let key_lower = key.to_lowercase();
        match key_lower.as_str() {
            "authorization" => {
                if value.starts_with("Bearer ") {
                    result.insert(key.clone(), "Bearer [REDACTED]".into());
                } else if value.starts_with("Basic ") {
                    result.insert(key.clone(), "Basic [REDACTED]".into());
                }
            }
            "x-api-key" | "x-auth-token" => {
                result.insert(key.clone(), "[REDACTED]".into());
            }
            "cookie" => {
                let redacted = value
                    .split(';')
                    .map(|part| {
                        let part = part.trim();
                        if let Some((cname, _)) = part.split_once('=') {
                            let cname_lower = cname.trim().to_lowercase();
                            if matches!(cname_lower.as_str(), "session" | "token" | "auth") {
                                return format!("{}=[REDACTED]", cname.trim());
                            }
                        }
                        part.to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                result.insert(key.clone(), redacted);
            }
            _ => {}
        }
    }
    result
}

/// For each detected token, vault it and update the origin's site_knowledge entry.
/// Returns the (possibly redacted) headers:
/// - On success: returns headers unchanged
/// - On any encryption error: returns headers with detected token values replaced by [REDACTED]
/// Best-effort: logs warnings on vault/storage failure, never propagates errors.
pub fn detect_and_vault(
    headers: &HashMap<String, String>,
    origin: &str,
    store: &SiteKnowledgeStore,
) -> HashMap<String, String> {
    let tokens = detect_tokens(headers);
    if tokens.is_empty() {
        return headers.clone();
    }

    let mut entry = store.get(origin).unwrap_or_default().unwrap_or_default();
    let mut changed = false;
    let mut had_error = false;

    for (kind, raw_value) in &tokens {
        match store.vault_token(raw_value) {
            Ok(vault_ref) => {
                entry.auth_tokens.insert(
                    kind.clone(),
                    crate::site_knowledge::AuthTokenEntry { vault_ref },
                );
                changed = true;
            }
            Err(e) => {
                tracing::warn!(
                    "auth_token_detector: failed to vault token for {}: {}",
                    origin,
                    e
                );
                had_error = true;
            }
        }
    }

    if changed {
        entry.last_updated = crate::site_knowledge::now_micros();
        if let Err(e) = store.put(origin, &entry) {
            // Note: put failure does NOT set had_error — the spec says redaction is triggered
            // by vault *encryption* errors only. A DB write failure after successful
            // encryption means the vault ref was not persisted, but the token itself was
            // not exposed in plain text. On the next request, a new vault ref will be created.
            tracing::warn!(
                "auth_token_detector: failed to persist site_knowledge for {}: {}",
                origin,
                e
            );
        }
    }

    // On any encryption error: return redacted headers so caller can replace raw values
    // before writing to ring buffer (spec: "ring buffer event still written with token
    // value redacted to [REDACTED]")
    if had_error {
        redact_detected_tokens(headers)
    } else {
        headers.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn detects_bearer_token() {
        let h = headers(&[("authorization", "Bearer mytoken123")]);
        let tokens = detect_tokens(&h);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "bearer");
        assert_eq!(tokens[0].1, "mytoken123");
    }

    #[test]
    fn detects_basic_auth() {
        let h = headers(&[("authorization", "Basic dXNlcjpwYXNz")]);
        let tokens = detect_tokens(&h);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "basic");
        assert_eq!(tokens[0].1, "dXNlcjpwYXNz");
    }

    #[test]
    fn detects_x_api_key() {
        let h = headers(&[("x-api-key", "sk-test-abc123")]);
        let tokens = detect_tokens(&h);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "api_key");
        assert_eq!(tokens[0].1, "sk-test-abc123");
    }

    #[test]
    fn detects_x_auth_token() {
        let h = headers(&[("x-auth-token", "token456")]);
        let tokens = detect_tokens(&h);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "api_key");
        assert_eq!(tokens[0].1, "token456");
    }

    #[test]
    fn detects_session_cookie() {
        let h = headers(&[("cookie", "session=abc123; other=value")]);
        let tokens = detect_tokens(&h);
        let session_token = tokens.iter().find(|(k, _)| k == "session_cookie");
        assert!(session_token.is_some());
        assert!(session_token.unwrap().1.contains("abc123"));
    }

    #[test]
    fn no_false_positives_on_plain_content_type() {
        let h = headers(&[("content-type", "application/json")]);
        let tokens = detect_tokens(&h);
        assert!(tokens.is_empty());
    }

    #[test]
    fn case_insensitive_header_matching() {
        let h = headers(&[("Authorization", "Bearer uppercase123")]);
        let tokens = detect_tokens(&h);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "bearer");
    }

    #[test]
    fn empty_headers_produces_no_tokens() {
        let tokens = detect_tokens(&HashMap::new());
        assert!(tokens.is_empty());
    }

    #[test]
    fn redact_detected_tokens_replaces_bearer_value() {
        let h = headers(&[
            ("authorization", "Bearer secret123"),
            ("content-type", "application/json"),
        ]);
        let redacted = redact_detected_tokens(&h);
        let auth = redacted.get("authorization").unwrap();
        assert!(auth.contains("[REDACTED]"), "got: {}", auth);
        assert!(!auth.contains("secret123"));
        // Non-auth header unchanged
        assert_eq!(redacted.get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn redact_detected_tokens_replaces_cookie_values() {
        let h = headers(&[("cookie", "session=abc123; other=value; token=xyz789")]);
        let redacted = redact_detected_tokens(&h);
        let cookie = redacted.get("cookie").unwrap();
        assert!(cookie.contains("[REDACTED]"));
        assert!(!cookie.contains("abc123"));
        assert!(!cookie.contains("xyz789"));
        // Non-auth cookie preserved
        assert!(cookie.contains("other=value"));
    }

    #[test]
    fn redact_detected_tokens_no_change_when_no_tokens() {
        let h = headers(&[
            ("content-type", "application/json"),
            ("accept", "text/html"),
        ]);
        let redacted = redact_detected_tokens(&h);
        assert_eq!(redacted.get("content-type").unwrap(), "application/json");
        assert_eq!(redacted.get("accept").unwrap(), "text/html");
    }

    #[test]
    fn detect_and_vault_stores_vault_ref_and_returns_original_headers() {
        use crate::site_knowledge::SiteKnowledgeStore;
        use std::sync::Arc;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = Arc::new(
            crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap(),
        );
        let store = SiteKnowledgeStore::new(db, key);

        let h = headers(&[
            ("authorization", "Bearer mytoken456"),
            ("content-type", "application/json"),
        ]);

        let result = detect_and_vault(&h, "https://example.com", &store);

        // On success, returns headers unchanged
        assert_eq!(result.get("authorization").unwrap(), "Bearer mytoken456");
        assert_eq!(result.get("content-type").unwrap(), "application/json");

        // Vault ref is stored in site_knowledge
        let entry = store.get("https://example.com").unwrap().unwrap();
        let bearer_ref = &entry.auth_tokens.get("bearer").unwrap().vault_ref;
        assert!(bearer_ref.starts_with("site_vault:"), "got: {}", bearer_ref);
    }
}
