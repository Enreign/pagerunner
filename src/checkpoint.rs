use serde::{Deserialize, Serialize};
use crate::db::Db;
use crate::error::Result;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CheckpointTab {
    pub url: String,
    pub origin: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionCheckpoint {
    pub checkpoint_id: String,
    pub name: String,
    pub saved_at: u64,   // Unix seconds
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
    let key = checkpoint_key(&checkpoint.profile, checkpoint.saved_at, &checkpoint.checkpoint_id);
    let bytes = serde_json::to_vec(checkpoint)
        .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
    db.put("checkpoints", &key, &bytes)
}

pub fn load_checkpoint(
    db: &Db,
    profile: &str,
    checkpoint_id: &str,
) -> Result<SessionCheckpoint> {
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
                CheckpointTab { url: "https://github.com/foo".into(), origin: "https://github.com".into() },
                CheckpointTab { url: "https://linear.app/team".into(), origin: "https://linear.app".into() },
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
}
