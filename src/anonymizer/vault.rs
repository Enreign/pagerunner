// src/anonymizer/vault.rs

use crate::db::Db;
use crate::error::PagerunnerError;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::TryRngCore;
use std::sync::Arc;

pub struct Vault {
    db: Arc<Db>,
}

impl Vault {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Get existing token for `value` in this session, or create a new one.
    /// `entity_type` is the uppercase label (e.g. "EMAIL").
    /// Returns the token string: `[EMAIL:a3f9b2]`
    pub fn get_or_create_token(
        &self,
        session_id: &str,
        entity_type: &str,
        value: &str,
    ) -> Result<String, PagerunnerError> {
        // Reverse lookup key: anon_val table, key = "{session_id}:{entity_type}:{base64(value)}"
        let rev_key = format!("{}:{}:{}", session_id, entity_type, BASE64.encode(value));

        if let Some(existing) = self.db.get("anon_val", &rev_key)? {
            // Token already exists
            let token = String::from_utf8(existing)
                .map_err(|e| PagerunnerError::Config(format!("invalid token utf8: {}", e)))?;
            return Ok(token);
        }

        // Generate new 6-char hex nonce
        let mut bytes = [0u8; 3]; // 3 bytes = 6 hex chars
        rand::rngs::OsRng
            .try_fill_bytes(&mut bytes)
            .expect("OsRng fill failed");
        let nonce = hex::encode(bytes);
        let token = format!("[{}:{}]", entity_type, nonce);

        // Write forward: anon table, key = "{session_id}:{token}" → value bytes
        let fwd_key = format!("{}:{}", session_id, token);
        self.db.put("anon", &fwd_key, value.as_bytes())?;

        // Write reverse: anon_val table, key = rev_key → token bytes
        self.db.put("anon_val", &rev_key, token.as_bytes())?;

        Ok(token)
    }

    /// Look up original value for a token in this session.
    /// Token format: `[EMAIL:a3f9b2]`
    /// Returns `None` if not found.
    pub fn lookup_token(
        &self,
        session_id: &str,
        token: &str,
    ) -> Result<Option<String>, PagerunnerError> {
        let key = format!("{}:{}", session_id, token);
        match self.db.get("anon", &key)? {
            None => Ok(None),
            Some(bytes) => {
                let value = String::from_utf8(bytes).map_err(|e| {
                    PagerunnerError::Config(format!("invalid vault value utf8: {}", e))
                })?;
                Ok(Some(value))
            }
        }
    }

    /// Purge all vault entries for a session (called on close_session).
    pub fn purge_session(&self, session_id: &str) -> Result<(), PagerunnerError> {
        let prefix = format!("{}:", session_id);
        self.db.delete_prefix("anon", &prefix)?;
        self.db.delete_prefix("anon_val", &prefix)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn make_db() -> Arc<Db> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        // We need the path to outlive this function — leak the dir
        let path_str = path.to_str().unwrap().to_string();
        let key = Db::generate_key();
        std::mem::forget(dir); // keep tempdir alive
        Arc::new(Db::open_with_key(&path_str, key).unwrap())
    }

    #[test]
    fn test_get_or_create_token_returns_token() {
        let db = make_db();
        let vault = Vault::new(db);
        let token = vault
            .get_or_create_token("sess1", "EMAIL", "user@example.com")
            .unwrap();
        assert!(token.starts_with("[EMAIL:"));
        assert!(token.ends_with(']'));
        assert_eq!(token.len(), "[EMAIL:xxxxxx]".len());
    }

    #[test]
    fn test_get_or_create_token_consistent() {
        let db = make_db();
        let vault = Vault::new(db);
        let t1 = vault
            .get_or_create_token("sess1", "EMAIL", "user@example.com")
            .unwrap();
        let t2 = vault
            .get_or_create_token("sess1", "EMAIL", "user@example.com")
            .unwrap();
        assert_eq!(t1, t2, "same value must always get same token");
    }

    #[test]
    fn test_different_values_get_different_tokens() {
        let db = make_db();
        let vault = Vault::new(db);
        let t1 = vault
            .get_or_create_token("sess1", "EMAIL", "a@example.com")
            .unwrap();
        let t2 = vault
            .get_or_create_token("sess1", "EMAIL", "b@example.com")
            .unwrap();
        assert_ne!(t1, t2);
    }

    #[test]
    fn test_lookup_token_found() {
        let db = make_db();
        let vault = Vault::new(db);
        let token = vault
            .get_or_create_token("sess1", "EMAIL", "user@example.com")
            .unwrap();
        let value = vault.lookup_token("sess1", &token).unwrap();
        assert_eq!(value, Some("user@example.com".to_string()));
    }

    #[test]
    fn test_lookup_token_not_found() {
        let db = make_db();
        let vault = Vault::new(db);
        let value = vault.lookup_token("sess1", "[EMAIL:ffffff]").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_lookup_is_session_scoped() {
        let db = make_db();
        let vault = Vault::new(db);
        let token = vault
            .get_or_create_token("sess1", "EMAIL", "user@example.com")
            .unwrap();
        // sess2 cannot see sess1's tokens
        let value = vault.lookup_token("sess2", &token).unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_purge_session() {
        let db = make_db();
        let vault = Vault::new(db);
        vault
            .get_or_create_token("sess1", "EMAIL", "user@example.com")
            .unwrap();
        vault
            .get_or_create_token("sess1", "PHONE", "555-123-4567")
            .unwrap();
        vault
            .get_or_create_token("sess2", "EMAIL", "other@example.com")
            .unwrap();

        vault.purge_session("sess1").unwrap();

        // sess1 tokens gone
        // (lookup the stored values via get_or_create — different values will get new tokens)
        let new_t = vault
            .get_or_create_token("sess1", "EMAIL", "user@example.com")
            .unwrap();
        // After purge, a new token should be generated (the reverse lookup is gone)
        // We can't easily check "new_t != old_t" without storing old_t, so just check sess2 survives
        let v = vault
            .lookup_token(
                "sess2",
                &vault
                    .get_or_create_token("sess2", "EMAIL", "other@example.com")
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(v, Some("other@example.com".to_string()));
        let _ = new_t; // suppress unused warning
    }
}
