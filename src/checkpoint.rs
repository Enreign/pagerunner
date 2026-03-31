use crate::db::Db;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CheckpointTab {
    pub url: String,
    pub origin: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionCheckpoint {
    pub checkpoint_id: String,
    pub name: String,
    pub saved_at: u64, // Unix seconds
    pub profile: String,
    pub tabs: Vec<CheckpointTab>,
}

/// DB key: "checkpoints/{profile}/{saved_at_secs}/{checkpoint_id}"
/// Allows scanning by profile prefix and sorting by saved_at.
pub fn checkpoint_key(profile: &str, saved_at: u64, checkpoint_id: &str) -> String {
    format!("checkpoints/{}/{}/{}", profile, saved_at, checkpoint_id)
}

pub fn checkpoint_profile_prefix(profile: &str) -> String {
    format!("checkpoints/{}/", profile)
}

pub fn save_checkpoint(db: &Db, checkpoint: &SessionCheckpoint) -> Result<()> {
    let key = checkpoint_key(
        &checkpoint.profile,
        checkpoint.saved_at,
        &checkpoint.checkpoint_id,
    );
    let bytes = serde_json::to_vec(checkpoint)
        .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
    db.put("checkpoints", &key, &bytes)
}

pub fn load_checkpoint(db: &Db, profile: &str, checkpoint_id: &str) -> Result<SessionCheckpoint> {
    let prefix = checkpoint_profile_prefix(profile);
    let entries = db.scan_prefix("checkpoints", &prefix)?;
    for (_, bytes) in entries {
        if let Ok(ckpt) = serde_json::from_slice::<SessionCheckpoint>(&bytes) {
            if ckpt.checkpoint_id == checkpoint_id {
                return Ok(ckpt);
            }
        }
    }
    Err(crate::error::PagerunnerError::Config(format!(
        "Checkpoint '{}' not found for profile '{}'",
        checkpoint_id, profile
    )))
}

pub fn list_checkpoints(db: &Db, profile: &str) -> Result<Vec<SessionCheckpoint>> {
    let prefix = checkpoint_profile_prefix(profile);
    let mut entries = db.scan_prefix("checkpoints", &prefix)?;
    entries.sort_by_key(|(k, _)| std::cmp::Reverse(k.clone()));
    let mut out = Vec::new();
    for (_, bytes) in entries {
        if let Ok(ckpt) = serde_json::from_slice::<SessionCheckpoint>(&bytes) {
            out.push(ckpt);
        }
    }
    Ok(out)
}

pub fn delete_checkpoint(db: &Db, profile: &str, checkpoint_id: &str) -> Result<()> {
    let prefix = checkpoint_profile_prefix(profile);
    let entries = db.scan_prefix("checkpoints", &prefix)?;
    for (key, bytes) in entries {
        if let Ok(ckpt) = serde_json::from_slice::<SessionCheckpoint>(&bytes) {
            if ckpt.checkpoint_id == checkpoint_id {
                return db.delete("checkpoints", &key);
            }
        }
    }
    Err(crate::error::PagerunnerError::Config(format!(
        "Checkpoint '{}' not found for profile '{}'",
        checkpoint_id, profile
    )))
}

/// Extract the HTTPS/HTTP origin from a URL string (scheme + host, no path).
/// Returns None for blank/chrome-internal URLs.
pub fn extract_origin(url: &str) -> Option<String> {
    if url.is_empty() || url == "about:blank" || url.starts_with("chrome-") {
        return None;
    }
    let after_scheme = url.find("://").map(|i| i + 3)?;
    let rest = &url[after_scheme..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..host_end];
    if host.is_empty() {
        return None;
    }
    let scheme = &url[..url.find("://").unwrap()];
    Some(format!("{}://{}", scheme, host))
}

/// Save the current session state as a named checkpoint.
pub async fn save_session_checkpoint(
    session: &mut crate::session::Session,
    name: Option<&str>,
    db: &crate::db::Db,
    max_snapshot_versions: usize,
) -> Result<SessionCheckpoint> {
    // Step 1: get current tabs
    let tabs = crate::browser::list_tabs(&session.cdp).await?;

    // Step 2: save tab URL list for recovery
    crate::snapshot::save_tab_state(session, db).await?;

    // Step 3: collect tabs with origins, dedup origins for snapshot saving
    let mut checkpoint_tabs: Vec<CheckpointTab> = Vec::new();
    let mut seen_origins: std::collections::HashMap<String, String> =
        std::collections::HashMap::new(); // origin → target_id of first tab

    for tab in &tabs {
        if let Some(origin) = extract_origin(&tab.url) {
            seen_origins
                .entry(origin.clone())
                .or_insert_with(|| tab.target_id.clone());
            checkpoint_tabs.push(CheckpointTab {
                url: tab.url.clone(),
                origin: origin.clone(),
            });
        }
    }

    // Step 4: save snapshot for each unique origin (best-effort)
    for (origin, target_id) in &seen_origins {
        if let Err(e) =
            crate::snapshot::save_snapshot(session, target_id, origin, db, max_snapshot_versions)
                .await
        {
            tracing::warn!(origin = %origin, error = %e, "save_session_checkpoint: snapshot failed");
        }
    }

    // Step 5: build and store checkpoint
    let saved_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let checkpoint_name = match name {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => {
            let days = saved_at / 86400;
            let year = 1970 + days / 365;
            format!("Autosave · {}", year)
        }
    };

    let ckpt = SessionCheckpoint {
        checkpoint_id: uuid::Uuid::new_v4().to_string(),
        name: checkpoint_name,
        saved_at,
        profile: session.profile_name.clone(),
        tabs: checkpoint_tabs,
    };
    save_checkpoint(db, &ckpt)?;
    Ok(ckpt)
}

/// Restore a session to a previously saved checkpoint.
/// 1. Closes all existing tabs (navigates the last one to about:blank to avoid killing Chrome)
/// 2. Opens a new tab for each URL in the checkpoint
/// 3. Restores per-origin auth state (cookies + localStorage) for each unique origin
pub async fn restore_session_checkpoint(
    session: &mut crate::session::Session,
    checkpoint_id: &str,
    db: &crate::db::Db,
) -> Result<serde_json::Value> {
    let ckpt = load_checkpoint(db, &session.profile_name, checkpoint_id)?;

    // Step 1: close existing tabs (keep last one to avoid killing Chrome window)
    let existing_tabs = crate::browser::list_tabs(&session.cdp).await?;
    let tab_count = existing_tabs.len();

    if tab_count > 1 {
        for tab in &existing_tabs[..tab_count - 1] {
            let _ = session
                .cdp
                .send(
                    "Target.closeTarget",
                    serde_json::json!({ "targetId": tab.target_id }),
                )
                .await;
        }
    }
    // Navigate last tab to about:blank to avoid killing the Chrome window
    if let Some(last) = existing_tabs.last() {
        let _ = crate::browser::navigate_to_blank(&session.cdp, &last.target_id).await;
    }

    // Step 2: open tabs from checkpoint
    let mut tabs_restored = 0usize;
    let mut opened: Vec<(String, String)> = Vec::new(); // (target_id, origin)

    for tab in &ckpt.tabs {
        if tab.url.is_empty() || tab.url == "about:blank" {
            continue;
        }
        match crate::browser::new_tab(&session.cdp, &tab.url).await {
            Ok(t) => {
                tabs_restored += 1;
                opened.push((t.target_id, tab.origin.clone()));
            }
            Err(e) => {
                tracing::warn!(url = %tab.url, error = %e, "restore_session_checkpoint: failed to open tab");
            }
        }
    }

    // Step 3: restore auth state for each unique origin
    let mut snapshots_restored = 0usize;
    let mut seen_origins = std::collections::HashSet::new();
    for (target_id, origin) in &opened {
        if seen_origins.insert(origin.clone()) {
            if let Ok(()) =
                crate::snapshot::restore_snapshot(session, target_id, origin, None, db).await
            {
                snapshots_restored += 1;
            }
        }
    }

    Ok(serde_json::json!({
        "ok": true,
        "tabs_restored": tabs_restored,
        "snapshots_restored": snapshots_restored,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db() -> (Db, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), [0u8; 32]).unwrap();
        (db, dir)
    }

    #[test]
    fn test_checkpoint_key_format() {
        let key = checkpoint_key("personal", 1711500000, "abc-123");
        assert_eq!(key, "checkpoints/personal/1711500000/abc-123");
    }

    #[test]
    fn test_checkpoint_serialise_roundtrip() {
        let (db, _dir) = make_db();
        let ckpt = SessionCheckpoint {
            checkpoint_id: "abc-123".into(),
            name: "Research sprint".into(),
            saved_at: 1711500000,
            profile: "personal".into(),
            tabs: vec![
                CheckpointTab {
                    url: "https://github.com/foo".into(),
                    origin: "https://github.com".into(),
                },
                CheckpointTab {
                    url: "https://linear.app/team".into(),
                    origin: "https://linear.app".into(),
                },
            ],
        };
        save_checkpoint(&db, &ckpt).unwrap();
        let loaded = load_checkpoint(&db, "personal", "abc-123").unwrap();
        assert_eq!(loaded.name, "Research sprint");
        assert_eq!(loaded.tabs.len(), 2);
    }

    #[test]
    fn test_list_checkpoints_sorted_newest_first() {
        let (db, _dir) = make_db();
        for (ts, id) in [(100u64, "old"), (200u64, "mid"), (300u64, "new")] {
            let ckpt = SessionCheckpoint {
                checkpoint_id: id.into(),
                name: format!("Checkpoint {}", id),
                saved_at: ts,
                profile: "personal".into(),
                tabs: vec![],
            };
            save_checkpoint(&db, &ckpt).unwrap();
        }
        let list = list_checkpoints(&db, "personal").unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].checkpoint_id, "new");
        assert_eq!(list[2].checkpoint_id, "old");
    }

    #[test]
    fn test_delete_checkpoint() {
        let (db, _dir) = make_db();
        let ckpt = SessionCheckpoint {
            checkpoint_id: "to-delete".into(),
            name: "test".into(),
            saved_at: 100,
            profile: "personal".into(),
            tabs: vec![],
        };
        save_checkpoint(&db, &ckpt).unwrap();
        assert_eq!(list_checkpoints(&db, "personal").unwrap().len(), 1);
        delete_checkpoint(&db, "personal", "to-delete").unwrap();
        assert_eq!(list_checkpoints(&db, "personal").unwrap().len(), 0);
    }

    #[test]
    fn test_delete_checkpoint_not_found_returns_error() {
        let (db, _dir) = make_db();
        assert!(delete_checkpoint(&db, "personal", "nonexistent").is_err());
    }

    #[test]
    fn test_load_checkpoint_not_found_returns_error() {
        let (db, _dir) = make_db();
        assert!(load_checkpoint(&db, "personal", "missing").is_err());
    }

    #[test]
    fn test_extract_origin_happy_path() {
        assert_eq!(
            extract_origin("https://github.com/foo/bar"),
            Some("https://github.com".into())
        );
        assert_eq!(
            extract_origin("http://example.com/path?q=1"),
            Some("http://example.com".into())
        );
        // No trailing path
        assert_eq!(
            extract_origin("https://linear.app"),
            Some("https://linear.app".into())
        );
    }

    #[test]
    fn test_extract_origin_returns_none_for_blank_and_chrome() {
        assert_eq!(extract_origin(""), None);
        assert_eq!(extract_origin("about:blank"), None);
        assert_eq!(extract_origin("chrome-extension://abc/page.html"), None);
        assert_eq!(extract_origin("chrome-search://local-ntp/"), None);
    }
}
