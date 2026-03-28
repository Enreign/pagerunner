use crate::db::Db;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub level: String,
    pub session_id: Option<String>,
    pub profile_name: Option<String>,
    pub created_at: u64,
    pub delivered: bool,
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

pub fn push_notification(
    db: &Db,
    title: &str,
    body: Option<&str>,
    level: &str,
    session_id: Option<&str>,
    profile_name: Option<&str>,
) -> Result<()> {
    let created_at = now_micros();
    let suffix: u32 = rand::random();
    let id = format!("{:020}:{:08x}", created_at, suffix);
    let notif = Notification {
        id: id.clone(),
        title: title.to_string(),
        body: body.map(|s| s.to_string()),
        level: level.to_string(),
        session_id: session_id.map(|s| s.to_string()),
        profile_name: profile_name.map(|s| s.to_string()),
        created_at,
        delivered: false,
    };
    let bytes = serde_json::to_vec(&notif)
        .map_err(|e| crate::error::PagerunnerError::Config(format!("Serialize error: {}", e)))?;
    db.put("notif", &id, &bytes)
}

pub fn drain_notifications(db: &Db) -> Result<Vec<Notification>> {
    let rows = db.scan_prefix("notif", "")?;
    let mut result = Vec::new();
    for (key, value) in rows {
        let mut notif: Notification = serde_json::from_slice(&value).map_err(|e| {
            crate::error::PagerunnerError::Config(format!("Deserialize error: {}", e))
        })?;
        if notif.delivered {
            continue;
        }
        // Mark delivered in DB
        let mut stored = notif.clone();
        stored.delivered = true;
        let bytes = serde_json::to_vec(&stored).map_err(|e| {
            crate::error::PagerunnerError::Config(format!("Serialize error: {}", e))
        })?;
        db.put("notif", &key, &bytes)?;
        // Return with delivered=false (pre-delivery state)
        notif.delivered = false;
        result.push(notif);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_db() -> Db {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap()
    }

    #[test]
    fn test_push_and_drain() {
        let db = test_db();
        push_notification(&db, "Hello", None, "info", None, None).unwrap();
        let drained = drain_notifications(&db).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].title, "Hello");
        assert_eq!(drained[0].level, "info");
    }

    #[test]
    fn test_drain_marks_delivered() {
        let db = test_db();
        push_notification(&db, "Once", None, "info", None, None).unwrap();
        let first = drain_notifications(&db).unwrap();
        assert_eq!(first.len(), 1);
        let second = drain_notifications(&db).unwrap();
        assert!(second.is_empty(), "second drain must be empty");
    }

    #[test]
    fn test_push_stores_fields() {
        let db = test_db();
        push_notification(
            &db,
            "Test",
            Some("body text"),
            "error",
            Some("sess-1"),
            Some("growthmate"),
        )
        .unwrap();
        let drained = drain_notifications(&db).unwrap();
        assert_eq!(drained[0].body.as_deref(), Some("body text"));
        assert_eq!(drained[0].level, "error");
        assert_eq!(drained[0].session_id.as_deref(), Some("sess-1"));
        assert_eq!(drained[0].profile_name.as_deref(), Some("growthmate"));
    }

    #[test]
    fn test_drain_multiple_returns_all_undelivered() {
        let db = test_db();
        push_notification(&db, "A", None, "info", None, None).unwrap();
        push_notification(&db, "B", None, "warning", None, None).unwrap();
        push_notification(&db, "C", None, "error", None, None).unwrap();
        let drained = drain_notifications(&db).unwrap();
        assert_eq!(drained.len(), 3);
    }
}
