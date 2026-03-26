use crate::db::Db;
use crate::error::{PagerunnerError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdapterEntry {
    pub js_code: String,
    pub description: String,
    pub params_schema: Option<serde_json::Value>,
    pub trusted: bool,
    pub created_at: u64,
    pub last_used: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SelectorEntry {
    pub successes: u32,
    pub failures: u32,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthTokenEntry {
    pub vault_ref: String, // e.g. "site_vault:a3f9b2"
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiteKnowledgeEntry {
    pub adapters: HashMap<String, AdapterEntry>,
    pub selectors: HashMap<String, SelectorEntry>,
    pub auth_tokens: HashMap<String, AuthTokenEntry>,
    pub last_updated: u64,
}

pub struct SiteKnowledgeStore {
    db: Arc<Db>,
    site_vault_key: [u8; 32],
}

impl SiteKnowledgeStore {
    pub fn new(db: Arc<Db>, master_key: [u8; 32]) -> Self {
        let site_vault_key = Self::derive_site_vault_key(&master_key);
        Self { db, site_vault_key }
    }

    pub fn derive_site_vault_key(master_key: &[u8; 32]) -> [u8; 32] {
        const SALT: &[u8] = b"site_knowledge_auth_tokens_v1";
        let mut hasher = Sha256::new();
        hasher.update(master_key);
        hasher.update(SALT);
        hasher.finalize().into()
    }

    pub fn get(&self, origin: &str) -> Result<Option<SiteKnowledgeEntry>> {
        let key = urlencoded_origin(origin);
        match self.db.get("site_knowledge", &key)? {
            None => Ok(None),
            Some(bytes) => {
                let entry: SiteKnowledgeEntry = serde_json::from_slice(&bytes)
                    .map_err(|e| PagerunnerError::Config(format!("site_knowledge parse error: {}", e)))?;
                Ok(Some(entry))
            }
        }
    }

    pub fn put(&self, origin: &str, entry: &SiteKnowledgeEntry) -> Result<()> {
        let key = urlencoded_origin(origin);
        let bytes = serde_json::to_vec(entry)
            .map_err(|e| PagerunnerError::Config(format!("site_knowledge serialize error: {}", e)))?;
        self.db.put("site_knowledge", &key, &bytes)
    }

    pub fn delete(&self, origin: &str) -> Result<()> {
        let key = urlencoded_origin(origin);
        self.db.delete("site_knowledge", &key)
    }

    pub fn vault_token(&self, token: &str) -> Result<String> {
        let encrypted = Db::encrypt(&self.site_vault_key, token.as_bytes())?;
        let mut hasher = Sha256::new();
        hasher.update(&encrypted);
        let hash = hasher.finalize();
        let short = hex::encode(&hash[..6]);
        let vault_ref = format!("site_vault:{}", short);
        self.db.put("site_vault", &short, &encrypted)?;
        Ok(vault_ref)
    }

    pub fn reliability_score(sel: &SelectorEntry) -> Option<f64> {
        let total = sel.successes + sel.failures;
        if total < 5 {
            return None;
        }
        Some(sel.successes as f64 / total as f64)
    }

    pub fn is_fragile(sel: &SelectorEntry) -> bool {
        let total = sel.successes + sel.failures;
        if total < 5 {
            return false;
        }
        let failure_rate = sel.failures as f64 / total as f64;
        failure_rate > 0.30
    }

    pub fn is_expired(entry: &SiteKnowledgeEntry, now_micros: u64) -> bool {
        const NINETY_DAYS_MICROS: u64 = 90 * 24 * 60 * 60 * 1_000_000;
        now_micros.saturating_sub(entry.last_updated) > NINETY_DAYS_MICROS
    }

    pub fn prune_stale_adapters(entry: &mut SiteKnowledgeEntry, now_micros: u64) -> bool {
        const THIRTY_DAYS_MICROS: u64 = 30 * 24 * 60 * 60 * 1_000_000;
        let before = entry.adapters.len();
        entry.adapters.retain(|_, adapter| {
            if adapter.trusted {
                return true;
            }
            if adapter.last_used > 0 {
                return true;
            }
            now_micros.saturating_sub(adapter.created_at) <= THIRTY_DAYS_MICROS
        });
        entry.adapters.len() < before
    }
}

fn urlencoded_origin(origin: &str) -> String {
    origin.replace("://", "_").replace('/', "_").replace(':', "_")
}

pub fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn make_store() -> SiteKnowledgeStore {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Arc::new(Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap());
        SiteKnowledgeStore::new(db, key)
    }

    #[test]
    fn derive_site_vault_key_is_deterministic() {
        let master = [0u8; 32];
        let k1 = SiteKnowledgeStore::derive_site_vault_key(&master);
        let k2 = SiteKnowledgeStore::derive_site_vault_key(&master);
        assert_eq!(k1, k2);
    }

    #[test]
    fn derive_site_vault_key_differs_from_master() {
        let master = [42u8; 32];
        let derived = SiteKnowledgeStore::derive_site_vault_key(&master);
        assert_ne!(derived, master);
    }

    #[test]
    fn get_unknown_origin_returns_none() {
        let store = make_store();
        assert!(store.get("https://unknown.example.com").unwrap().is_none());
    }

    #[test]
    fn put_and_get_roundtrip() {
        let store = make_store();
        let mut entry = SiteKnowledgeEntry::default();
        entry.last_updated = 1_000_000;
        store.put("https://linear.app", &entry).unwrap();
        let got = store.get("https://linear.app").unwrap().unwrap();
        assert_eq!(got.last_updated, 1_000_000);
    }

    #[test]
    fn delete_removes_entry() {
        let store = make_store();
        let entry = SiteKnowledgeEntry::default();
        store.put("https://linear.app", &entry).unwrap();
        store.delete("https://linear.app").unwrap();
        assert!(store.get("https://linear.app").unwrap().is_none());
    }

    #[test]
    fn vault_token_returns_site_vault_prefix() {
        let store = make_store();
        let vref = store.vault_token("Bearer mytoken123").unwrap();
        assert!(vref.starts_with("site_vault:"), "got: {}", vref);
    }

    #[test]
    fn vault_token_same_value_different_refs() {
        let store = make_store();
        let r1 = store.vault_token("same_token").unwrap();
        let r2 = store.vault_token("same_token").unwrap();
        assert_ne!(r1, r2);
    }

    #[test]
    fn reliability_score_none_below_5_samples() {
        let sel = SelectorEntry { successes: 2, failures: 1, last_seen: 0 };
        assert!(SiteKnowledgeStore::reliability_score(&sel).is_none());
    }

    #[test]
    fn reliability_score_computed_with_5_or_more() {
        let sel = SelectorEntry { successes: 8, failures: 2, last_seen: 0 };
        let score = SiteKnowledgeStore::reliability_score(&sel).unwrap();
        assert!((score - 0.8).abs() < 0.001);
    }

    #[test]
    fn is_fragile_true_when_over_30_percent_failures_with_5_samples() {
        let sel = SelectorEntry { successes: 3, failures: 7, last_seen: 0 };
        assert!(SiteKnowledgeStore::is_fragile(&sel));
    }

    #[test]
    fn is_fragile_false_when_under_30_percent() {
        let sel = SelectorEntry { successes: 8, failures: 1, last_seen: 0 };
        assert!(!SiteKnowledgeStore::is_fragile(&sel));
    }

    #[test]
    fn is_fragile_false_below_5_samples_even_if_all_failures() {
        let sel = SelectorEntry { successes: 0, failures: 4, last_seen: 0 };
        assert!(!SiteKnowledgeStore::is_fragile(&sel));
    }

    #[test]
    fn is_expired_true_after_90_days() {
        let entry = SiteKnowledgeEntry { last_updated: 0, ..Default::default() };
        let ninety_one_days_micros: u64 = 91 * 24 * 60 * 60 * 1_000_000;
        assert!(SiteKnowledgeStore::is_expired(&entry, ninety_one_days_micros));
    }

    #[test]
    fn is_expired_false_within_90_days() {
        let now = now_micros();
        let entry = SiteKnowledgeEntry { last_updated: now, ..Default::default() };
        assert!(!SiteKnowledgeStore::is_expired(&entry, now));
    }

    #[test]
    fn prune_stale_adapters_removes_never_used_after_30_days() {
        let mut entry = SiteKnowledgeEntry::default();
        let thirty_one_days_ago: u64 = now_micros().saturating_sub(31 * 24 * 60 * 60 * 1_000_000);
        entry.adapters.insert("old".into(), AdapterEntry {
            created_at: thirty_one_days_ago,
            last_used: 0,
            trusted: false,
            ..Default::default()
        });
        entry.adapters.insert("recent".into(), AdapterEntry {
            created_at: now_micros(),
            last_used: 0,
            trusted: false,
            ..Default::default()
        });
        let pruned = SiteKnowledgeStore::prune_stale_adapters(&mut entry, now_micros());
        assert!(pruned);
        assert!(!entry.adapters.contains_key("old"));
        assert!(entry.adapters.contains_key("recent"));
    }

    #[test]
    fn prune_stale_adapters_keeps_trusted_even_if_old() {
        let mut entry = SiteKnowledgeEntry::default();
        entry.adapters.insert("seed".into(), AdapterEntry {
            created_at: 0,
            last_used: 0,
            trusted: true,
            ..Default::default()
        });
        let pruned = SiteKnowledgeStore::prune_stale_adapters(&mut entry, now_micros());
        assert!(!pruned);
        assert!(entry.adapters.contains_key("seed"));
    }
}
