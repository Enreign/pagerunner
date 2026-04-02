use crate::db::Db;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionRegistryEntry {
    pub session_id: String,
    pub profile_name: String,
    pub display_name: String,
    pub stealth: bool,
    pub debug_port: u16,
    /// WebSocket URL for faster reconnection (skip HTTP discovery).
    /// TODO: populate this when saving registry entries in mcp_server.rs
    pub ws_url: Option<String>,
    pub opened_at: u64, // Unix seconds
    /// JSON-serialized SecurityPolicy params for restore (allowed_domains, max_navigations, etc.)
    /// Stored as a serde_json::Value so we don't have a hard dep on SecurityPolicy here.
    pub security_params: serde_json::Value,
}

pub fn registry_key(session_id: &str) -> String {
    session_id.to_string()
}

pub fn save_entry(db: &Db, entry: &SessionRegistryEntry) -> Result<()> {
    let bytes = serde_json::to_vec(entry)
        .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
    db.put("session_reg", &registry_key(&entry.session_id), &bytes)
}

pub fn load_entry(db: &Db, session_id: &str) -> Result<Option<SessionRegistryEntry>> {
    match db.get("session_reg", &registry_key(session_id))? {
        None => Ok(None),
        Some(bytes) => {
            let entry = serde_json::from_slice(&bytes)
                .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
            Ok(Some(entry))
        }
    }
}

pub fn delete_entry(db: &Db, session_id: &str) -> Result<()> {
    db.delete("session_reg", &registry_key(session_id))
}

pub fn list_entries(db: &Db) -> Result<Vec<SessionRegistryEntry>> {
    let raw = db.scan_prefix("session_reg", "")?;
    let mut out = Vec::new();
    for (_, bytes) in raw {
        let entry = serde_json::from_slice::<SessionRegistryEntry>(&bytes)
            .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
        out.push(entry);
    }
    Ok(out)
}

/// On startup: attempt to reattach to all sessions in the registry.
/// Sessions whose Chrome port is unreachable are silently removed from the registry.
/// Returns the list of successfully reattached session IDs.
/// NOTE: Reattached sessions run with security_policy: None (not restored from registry yet).
pub async fn reconcile_sessions(
    db: &std::sync::Arc<crate::db::Db>,
    session_manager: &tokio::sync::Mutex<crate::session::SessionManager>,
    config: &crate::config::PagerunnerConfig,
    site_store: Option<std::sync::Arc<crate::site_knowledge::SiteKnowledgeStore>>,
) -> Vec<String> {
    // CRUD fns take &Db; bridge via db.as_ref()
    let entries = match list_entries(db.as_ref()) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    if entries.is_empty() {
        return vec![];
    }

    tracing::info!(
        "Session registry: {} entries, attempting reattach",
        entries.len()
    );
    let mut reattached = Vec::new();

    for entry in entries {
        let debug_url = format!("http://127.0.0.1:{}", entry.debug_port);
        let result = {
            let mut sm = session_manager.lock().await;
            sm.attach(
                &debug_url,
                Some(entry.profile_name.clone()),
                Some(entry.display_name.clone()),
                std::sync::Arc::clone(db),
                &config.network,
                site_store.clone(),
            )
            .await
        };

        match result {
            Ok(new_session_id) => {
                tracing::info!(
                    profile = %entry.profile_name,
                    port = entry.debug_port,
                    "Reattached session"
                );
                // Write new registry entry for the new session_id; use db.as_ref() for CRUD
                let new_entry = SessionRegistryEntry {
                    session_id: new_session_id.clone(),
                    ..entry.clone()
                };
                let _ = save_entry(db.as_ref(), &new_entry);
                // Delete old entry (different session_id)
                if new_session_id != entry.session_id {
                    let _ = delete_entry(db.as_ref(), &entry.session_id);
                }
                // TODO(security): restore security_policy from entry.security_params once
                // SessionManager::attach() accepts a security_policy parameter.
                reattached.push(new_session_id);
            }
            Err(e) => {
                tracing::debug!(
                    session_id = %entry.session_id,
                    profile = %entry.profile_name,
                    port = entry.debug_port,
                    error = %e,
                    "Chrome not found for session, removing from registry"
                );
                let _ = delete_entry(db.as_ref(), &entry.session_id);
            }
        }
    }

    reattached
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_db() -> (Db, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db = Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), [0u8; 32]).unwrap();
        (db, dir)
    }

    fn make_entry(session_id: &str, port: u16) -> SessionRegistryEntry {
        SessionRegistryEntry {
            session_id: session_id.to_string(),
            profile_name: "personal".to_string(),
            display_name: "Stas (personal)".to_string(),
            stealth: false,
            debug_port: port,
            ws_url: None,
            opened_at: 1711500000,
            security_params: serde_json::json!({"allowed_domains": [], "max_navigations": null}),
        }
    }

    #[test]
    fn test_save_and_load_entry() {
        let (db, _dir) = make_db();
        let entry = make_entry("sess-abc", 54321);
        save_entry(&db, &entry).unwrap();
        let loaded = load_entry(&db, "sess-abc").unwrap();
        assert_eq!(loaded, Some(entry));
    }

    #[test]
    fn test_load_missing_returns_none() {
        let (db, _dir) = make_db();
        assert_eq!(load_entry(&db, "nonexistent").unwrap(), None);
    }

    #[test]
    fn test_delete_entry() {
        let (db, _dir) = make_db();
        save_entry(&db, &make_entry("sess-del", 12345)).unwrap();
        delete_entry(&db, "sess-del").unwrap();
        assert_eq!(load_entry(&db, "sess-del").unwrap(), None);
    }

    #[test]
    fn test_delete_nonexistent_is_noop() {
        let (db, _dir) = make_db();
        delete_entry(&db, "not-here").unwrap(); // no error
    }

    #[test]
    fn test_list_entries_returns_all() {
        let (db, _dir) = make_db();
        save_entry(&db, &make_entry("s1", 1001)).unwrap();
        save_entry(&db, &make_entry("s2", 1002)).unwrap();
        let entries = list_entries(&db).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_list_entries_empty() {
        let (db, _dir) = make_db();
        assert!(list_entries(&db).unwrap().is_empty());
    }

    #[test]
    fn test_list_entries_after_partial_cleanup() {
        // Simulate a cleaned registry: save 3 entries, delete 1, list should show 2
        let (db, _dir) = make_db();
        save_entry(&db, &make_entry("s1", 1001)).unwrap();
        save_entry(&db, &make_entry("s2", 1002)).unwrap();
        save_entry(&db, &make_entry("s3", 1003)).unwrap();
        delete_entry(&db, "s2").unwrap();
        let entries = list_entries(&db).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.session_id == "s1"));
        assert!(entries.iter().any(|e| e.session_id == "s3"));
        assert!(!entries.iter().any(|e| e.session_id == "s2"));
    }

    #[test]
    fn test_overwrite_entry() {
        let (db, _dir) = make_db();
        save_entry(&db, &make_entry("sess-ow", 1000)).unwrap();
        let updated = SessionRegistryEntry {
            debug_port: 2000,
            ..make_entry("sess-ow", 1000)
        };
        save_entry(&db, &updated).unwrap();
        let loaded = load_entry(&db, "sess-ow").unwrap().unwrap();
        assert_eq!(loaded.debug_port, 2000);
    }
}
