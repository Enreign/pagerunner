use crate::browser::attach_to_target;
use crate::db::Db;
use crate::error::{PagerunnerError, Result};
use crate::session::Session;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub profile: String,
    pub origin: String,
    pub cookies: Vec<Value>,
    pub local_storage: HashMap<String, String>,
    pub saved_at: u64, // Unix microseconds
}

/// Base key prefix for a profile+origin combination (no timestamp).
pub fn snapshot_key_prefix(profile: &str, origin: &str) -> String {
    format!("{}/{}", profile, origin)
}

/// Full versioned key including timestamp.
pub fn snapshot_key(profile: &str, origin: &str, saved_at: u64) -> String {
    format!("{}/{}/{}", profile, origin, saved_at)
}

/// Capture cookies + localStorage for `origin` and persist to DB.
pub async fn save_snapshot(
    session: &mut Session,
    target_id: &str,
    origin: &str,
    db: &Db,
    max_versions: usize,
) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;

    let cookies_result = session
        .cdp
        .send_on_session(
            "Network.getCookies",
            json!({ "urls": [origin] }),
            Some(session_id.clone()),
        )
        .await?;
    let cookies = cookies_result["cookies"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let ls_result = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": "JSON.stringify(Object.fromEntries(Object.entries(localStorage)))",
                "returnByValue": true
            }),
            Some(session_id),
        )
        .await?;
    let ls_str = ls_result["result"]["value"].as_str().unwrap_or("{}");
    let local_storage: HashMap<String, String> = serde_json::from_str(ls_str).unwrap_or_default();

    let saved_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    let snap = Snapshot {
        profile: session.profile_name.clone(),
        origin: origin.into(),
        cookies,
        local_storage,
        saved_at,
    };

    let key = snapshot_key(&session.profile_name, origin, saved_at);
    let bytes = serde_json::to_vec(&snap).map_err(|e| PagerunnerError::Config(e.to_string()))?;
    db.put("snapshots", &key, &bytes)?;

    prune_snapshots(db, &session.profile_name, origin, max_versions)
}

/// Capture cookies for all domains in the current browser session and save
/// one snapshot per distinct origin. Returns the list of origins saved.
pub async fn save_all_snapshots(
    session: &mut Session,
    target_id: &str,
    db: &Db,
    max_versions: usize,
) -> Result<Vec<String>> {
    let session_id = attach_to_target(session, target_id).await?;

    // Get ALL cookies (no URL filter)
    let cookies_result = session
        .cdp
        .send_on_session("Network.getCookies", json!({}), Some(session_id.clone()))
        .await?;
    let all_cookies = cookies_result["cookies"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Group by scheme+domain → origin
    let mut by_origin: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();
    for cookie in &all_cookies {
        if let Some(domain) = cookie["domain"].as_str() {
            let clean = domain.trim_start_matches('.');
            let origin = format!("https://{}", clean);
            by_origin.entry(origin).or_default().push(cookie.clone());
        }
    }

    let ls_result = session
        .cdp
        .send_on_session(
            "Runtime.evaluate",
            json!({
                "expression": "JSON.stringify(Object.fromEntries(Object.entries(localStorage)))",
                "returnByValue": true
            }),
            Some(session_id),
        )
        .await?;
    let ls_str = ls_result["result"]["value"].as_str().unwrap_or("{}");
    let local_storage: HashMap<String, String> = serde_json::from_str(ls_str).unwrap_or_default();

    let saved_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    let mut saved_origins = Vec::new();
    for (origin, cookies) in by_origin {
        let snap = Snapshot {
            profile: session.profile_name.clone(),
            origin: origin.clone(),
            cookies,
            // localStorage is per-origin at the JS level; we include it for the
            // current page's origin only (agent can call per-origin save for others)
            local_storage: local_storage.clone(),
            saved_at,
        };
        let key = snapshot_key(&session.profile_name, &origin, saved_at);
        let bytes =
            serde_json::to_vec(&snap).map_err(|e| PagerunnerError::Config(e.to_string()))?;
        db.put("snapshots", &key, &bytes)?;
        prune_snapshots(db, &session.profile_name, &origin, max_versions)?;
        saved_origins.push(origin);
    }
    saved_origins.sort();
    Ok(saved_origins)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    pub scroll_y: f64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TabState {
    pub profile: String,
    pub saved_at: u64,
    pub tabs: Vec<SavedTab>,
}

/// Save the URL, title, and scroll position of every open tab in the session.
pub async fn save_tab_state(session: &mut Session, db: &Db) -> Result<usize> {
    let tabs = crate::browser::list_tabs(&session.cdp).await?;
    let mut saved_tabs = Vec::new();

    for tab in &tabs {
        // Attach to each tab and get scroll position
        let scroll_y = if let Ok(sid) = attach_to_target(session, &tab.target_id).await {
            let r = session
                .cdp
                .send_on_session(
                    "Runtime.evaluate",
                    json!({ "expression": "window.scrollY", "returnByValue": true }),
                    Some(sid),
                )
                .await
                .unwrap_or(json!({}));
            r["result"]["value"].as_f64().unwrap_or(0.0)
        } else {
            0.0
        };
        saved_tabs.push(SavedTab {
            url: tab.url.clone(),
            title: tab.title.clone(),
            scroll_y,
        });
    }

    let saved_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    let state = TabState {
        profile: session.profile_name.clone(),
        saved_at,
        tabs: saved_tabs,
    };
    let key = format!("{}/{}", session.profile_name, saved_at);
    let bytes = serde_json::to_vec(&state).map_err(|e| PagerunnerError::Config(e.to_string()))?;
    db.put("tab_state", &key, &bytes)?;

    // Keep only the 3 most recent tab states per profile
    let prefix = format!("{}/", session.profile_name);
    let mut entries = db.scan_prefix("tab_state", &prefix)?;
    entries.sort_by_key(|(k, _)| k.clone());
    if entries.len() > 3 {
        for (k, _) in &entries[..entries.len() - 3] {
            db.delete("tab_state", k)?;
        }
    }

    Ok(state.tabs.len())
}

/// Restore the most recently saved tab state: open a new tab for each saved URL
/// and scroll to the saved position.
pub async fn restore_tab_state(session: &mut Session, db: &Db) -> Result<Vec<String>> {
    let prefix = format!("{}/", session.profile_name);
    let mut entries = db.scan_prefix("tab_state", &prefix)?;
    if entries.is_empty() {
        return Err(PagerunnerError::Config(format!(
            "No saved tab state for profile '{}'",
            session.profile_name
        )));
    }
    entries.sort_by_key(|(k, _)| k.clone());
    let (_, bytes) = entries.pop().unwrap();
    let state: TabState =
        serde_json::from_slice(&bytes).map_err(|e| PagerunnerError::Config(e.to_string()))?;

    let mut restored_urls = Vec::new();
    for saved_tab in &state.tabs {
        if saved_tab.url == "about:blank" || saved_tab.url.is_empty() {
            continue;
        }
        // Check the URL against the session policy before restoring.
        // Saved state may have been written under a different (less restrictive) policy.
        if let Some(policy) = &session.security_policy {
            if let Err(reason) = policy.check_navigate(&saved_tab.url) {
                tracing::warn!(url = %saved_tab.url, reason = %reason, "Skipping blocked URL during tab state restore");
                continue;
            }
        }
        let tab = crate::browser::new_tab(&session.cdp, &saved_tab.url).await?;
        if saved_tab.scroll_y > 0.0 {
            if let Ok(sid) = attach_to_target(session, &tab.target_id).await {
                let _ = session
                    .cdp
                    .send_on_session(
                        "Runtime.evaluate",
                        json!({
                            "expression": format!("window.scrollTo(0, {})", saved_tab.scroll_y),
                            "returnByValue": true
                        }),
                        Some(sid),
                    )
                    .await;
            }
        }
        restored_urls.push(saved_tab.url.clone());
    }

    Ok(restored_urls)
}

#[cfg(test)]
pub(crate) fn count_restorable(tabs: &[SavedTab]) -> usize {
    tabs.iter()
        .filter(|t| t.url != "about:blank" && !t.url.is_empty())
        .count()
}

/// Delete all but the `max_versions` most recent versioned snapshots for a profile+origin.
/// If `max_versions` is 0, no pruning is performed (unlimited retention).
fn prune_snapshots(db: &Db, profile: &str, origin: &str, max_versions: usize) -> Result<()> {
    if max_versions == 0 {
        return Ok(());
    }
    let prefix = format!("{}/", snapshot_key_prefix(profile, origin));
    let mut entries = db.scan_prefix("snapshots", &prefix)?;
    // Keys end in "/TIMESTAMP" — sort ascending, delete all but last `max_versions`
    entries.sort_by_key(|(k, _)| k.clone());
    if entries.len() > max_versions {
        for (key, _) in &entries[..entries.len() - max_versions] {
            db.delete("snapshots", key)?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
pub struct SnapshotInfo {
    pub profile: String,
    pub origin: String,
    pub saved_at: u64,
    pub saved_at_iso: String,
    pub cookie_count: usize,
    pub ls_key_count: usize,
}

/// List all saved snapshots across all profiles and origins.
pub fn list_snapshots(
    db: &Db,
    latest_only: bool,
    profile_filter: Option<&str>,
) -> Result<Vec<SnapshotInfo>> {
    let prefix = match profile_filter {
        Some(p) => format!("{}/", p),
        None => String::new(),
    };
    let entries = db.scan_prefix("snapshots", &prefix)?;
    let mut out = Vec::new();
    for (_, bytes) in entries {
        if let Ok(snap) = serde_json::from_slice::<Snapshot>(&bytes) {
            let secs = snap.saved_at / 1_000_000;
            let saved_at_iso = format_timestamp(secs);
            out.push(SnapshotInfo {
                profile: snap.profile.clone(),
                origin: snap.origin.clone(),
                saved_at: snap.saved_at,
                saved_at_iso,
                cookie_count: snap.cookies.len(),
                ls_key_count: snap.local_storage.len(),
            });
        }
    }
    // Sort newest first
    out.sort_by_key(|s| std::cmp::Reverse(s.saved_at));
    if latest_only {
        // Keep only the newest entry per (profile, origin) pair
        let mut seen = std::collections::HashSet::new();
        out.retain(|s| seen.insert((s.profile.clone(), s.origin.clone())));
    }
    Ok(out)
}

/// Delete snapshot(s) for a profile+origin.
/// If `saved_at` is `Some(ts)`, deletes only that specific version (errors if not found).
/// If `saved_at` is `None`, deletes all versions (returns the count deleted).
pub fn delete_snapshot(
    db: &Db,
    profile: &str,
    origin: &str,
    saved_at: Option<u64>,
) -> Result<usize> {
    match saved_at {
        Some(ts) => {
            let key = snapshot_key(profile, origin, ts);
            if db.get("snapshots", &key)?.is_some() {
                db.delete("snapshots", &key)?;
                Ok(1)
            } else {
                Err(PagerunnerError::Config(format!(
                    "Snapshot version {} not found for {} / {}",
                    ts, profile, origin
                )))
            }
        }
        None => {
            let prefix = format!("{}/", snapshot_key_prefix(profile, origin));
            let entries = db.scan_prefix("snapshots", &prefix)?;
            let count = entries.len();
            for (key, _) in entries {
                db.delete("snapshots", &key)?;
            }
            Ok(count)
        }
    }
}

fn format_timestamp(secs: u64) -> String {
    // Format as "YYYY-MM-DD HH:MM:SS UTC" using manual Gregorian calendar arithmetic.
    // No chrono dep needed — this covers all display needs.
    let s = secs;
    let (y, mo, d, h, mi, sec) = epoch_to_ymd_hms(s);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        y, mo, d, h, mi, sec
    )
}

fn epoch_to_ymd_hms(mut secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = secs % 60;
    secs /= 60;
    let min = secs % 60;
    secs /= 60;
    let hour = secs % 24;
    secs /= 24;
    // Days since 1970-01-01
    let mut days = secs;
    let mut year = 1970u64;
    loop {
        let dy =
            if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
                366
            } else {
                365
            };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let month_days = [
        31u64,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1, hour, min, sec)
}

/// Restore cookies + localStorage for `origin` from DB into the live browser.
/// `from_profile` — if Some, load the snapshot saved by that profile instead of the current one.
pub async fn restore_snapshot(
    session: &mut Session,
    target_id: &str,
    origin: &str,
    from_profile: Option<&str>,
    db: &Db,
) -> Result<()> {
    let profile = from_profile.unwrap_or(&session.profile_name);
    let prefix = format!("{}/", snapshot_key_prefix(profile, origin));
    let mut entries = db.scan_prefix("snapshots", &prefix)?;
    if entries.is_empty() {
        return Err(PagerunnerError::Config(format!(
            "No snapshot for profile '{}' origin '{}'",
            profile, origin
        )));
    }
    // Most recent = highest timestamp = last after sort
    entries.sort_by_key(|(k, _)| k.clone());
    let (_, bytes) = entries.pop().unwrap();
    let snap: Snapshot =
        serde_json::from_slice(&bytes).map_err(|e| PagerunnerError::Config(e.to_string()))?;

    // Validate the stored origin matches what was requested.
    // Mismatch indicates DB corruption or a key-structure bug.
    if snap.origin != origin {
        return Err(PagerunnerError::Config(format!(
            "Snapshot origin mismatch: requested '{}' but stored snapshot is for '{}'. \
             The DB entry at key prefix '{}/{}' is corrupt.",
            origin, snap.origin, profile, origin
        )));
    }

    let session_id = attach_to_target(session, target_id).await?;

    for cookie in &snap.cookies {
        let _ = session
            .cdp
            .send_on_session(
                "Network.setCookie",
                cookie.clone(),
                Some(session_id.clone()),
            )
            .await;
    }

    if !snap.local_storage.is_empty() {
        let entries: Vec<String> = snap
            .local_storage
            .iter()
            .map(|(k, v)| {
                format!(
                    "localStorage.setItem({}, {})",
                    serde_json::to_string(k).unwrap(),
                    serde_json::to_string(v).unwrap(),
                )
            })
            .collect();
        let js = entries.join("; ");
        let _ = session
            .cdp
            .send_on_session(
                "Runtime.evaluate",
                json!({ "expression": js, "returnByValue": true }),
                Some(session_id),
            )
            .await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_restore_tab_state_count_excludes_blank() {
        // Verify the actual-restored count helper is correct
        let tabs = vec![
            SavedTab {
                url: "https://example.com".into(),
                title: "Ex".into(),
                scroll_y: 0.0,
            },
            SavedTab {
                url: "about:blank".into(),
                title: "".into(),
                scroll_y: 0.0,
            },
            SavedTab {
                url: "".into(),
                title: "".into(),
                scroll_y: 0.0,
            },
        ];
        let count = count_restorable(&tabs);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_snapshot_key_format() {
        let key = snapshot_key("personal", "https://github.com", 1234567890);
        assert_eq!(key, "personal/https://github.com/1234567890");
    }

    #[test]
    fn test_snapshot_key_prefix_format() {
        let prefix = snapshot_key_prefix("personal", "https://github.com");
        assert_eq!(prefix, "personal/https://github.com");
    }

    #[test]
    fn test_list_and_delete_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), [0u8; 32])
            .unwrap();

        // Insert two fake snapshot entries
        let snap1 = Snapshot {
            profile: "alpha".into(),
            origin: "https://example.com".into(),
            cookies: vec![],
            local_storage: Default::default(),
            saved_at: 1_000_000,
        };
        let snap2 = Snapshot {
            profile: "alpha".into(),
            origin: "https://example.com".into(),
            cookies: vec![serde_json::json!({"name":"sid"})],
            local_storage: Default::default(),
            saved_at: 2_000_000,
        };
        db.put(
            "snapshots",
            "alpha/https://example.com/1000000",
            &serde_json::to_vec(&snap1).unwrap(),
        )
        .unwrap();
        db.put(
            "snapshots",
            "alpha/https://example.com/2000000",
            &serde_json::to_vec(&snap2).unwrap(),
        )
        .unwrap();

        let list = list_snapshots(&db, false, None).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].saved_at, 2_000_000); // newest first

        let deleted = delete_snapshot(&db, "alpha", "https://example.com", None).unwrap();
        assert_eq!(deleted, 2);
        assert!(list_snapshots(&db, false, None).unwrap().is_empty());
    }

    #[test]
    fn test_snapshot_key_prefix_used_for_scan() {
        // Verify that snapshot_key_prefix produces the prefix scan_prefix expects
        let prefix = format!("{}/", snapshot_key_prefix("alpha", "https://example.com"));
        assert_eq!(prefix, "alpha/https://example.com/");
        // And that a versioned key starts with this prefix
        let key = snapshot_key("alpha", "https://example.com", 1_000_000);
        assert!(key.starts_with(&prefix));
    }

    #[test]
    fn test_tab_state_serialise_roundtrip() {
        let state = TabState {
            profile: "beta".into(),
            saved_at: 1_000,
            tabs: vec![SavedTab {
                url: "https://example.com".into(),
                title: "Example".into(),
                scroll_y: 150.0,
            }],
        };
        let bytes = serde_json::to_vec(&state).unwrap();
        let decoded: TabState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.profile, "beta");
        assert_eq!(decoded.tabs[0].scroll_y, 150.0);
    }

    #[test]
    fn test_list_snapshots_latest_only() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), [0u8; 32])
            .unwrap();

        for ts in [1_000_000u64, 2_000_000, 3_000_000] {
            let snap = Snapshot {
                profile: "alpha".into(),
                origin: "https://example.com".into(),
                cookies: vec![],
                local_storage: Default::default(),
                saved_at: ts,
            };
            db.put(
                "snapshots",
                &snapshot_key("alpha", "https://example.com", ts),
                &serde_json::to_vec(&snap).unwrap(),
            )
            .unwrap();
        }

        // latest_only=true → 1 entry (the newest)
        let list = list_snapshots(&db, true, None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].saved_at, 3_000_000);

        // latest_only=false → 3 entries
        let list = list_snapshots(&db, false, None).unwrap();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn test_delete_snapshot_specific_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), [0u8; 32])
            .unwrap();

        let snap1 = Snapshot {
            profile: "alpha".into(),
            origin: "https://example.com".into(),
            cookies: vec![],
            local_storage: Default::default(),
            saved_at: 1_000_000,
        };
        let snap2 = Snapshot {
            profile: "alpha".into(),
            origin: "https://example.com".into(),
            cookies: vec![],
            local_storage: Default::default(),
            saved_at: 2_000_000,
        };
        db.put(
            "snapshots",
            "alpha/https://example.com/1000000",
            &serde_json::to_vec(&snap1).unwrap(),
        )
        .unwrap();
        db.put(
            "snapshots",
            "alpha/https://example.com/2000000",
            &serde_json::to_vec(&snap2).unwrap(),
        )
        .unwrap();

        // Delete only version 1_000_000
        let n = delete_snapshot(&db, "alpha", "https://example.com", Some(1_000_000)).unwrap();
        assert_eq!(n, 1);

        // Version 2_000_000 still exists
        let list = list_snapshots(&db, false, None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].saved_at, 2_000_000);

        // Delete all remaining
        let n = delete_snapshot(&db, "alpha", "https://example.com", None).unwrap();
        assert_eq!(n, 1);
        assert!(list_snapshots(&db, false, None).unwrap().is_empty());

        // Deleting a non-existent version returns an error
        assert!(delete_snapshot(&db, "alpha", "https://example.com", Some(9_999_999)).is_err());
    }

    #[test]
    fn test_list_snapshots_profile_filter() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), [0u8; 32])
            .unwrap();

        // Save snapshots for two different profiles
        for (profile, origin, ts) in [
            ("alice", "https://example.com", 1_000_000u64),
            ("bob", "https://example.com", 2_000_000u64),
            ("alice", "https://other.com", 3_000_000u64),
        ] {
            let snap = Snapshot {
                profile: profile.into(),
                origin: origin.into(),
                cookies: vec![],
                local_storage: Default::default(),
                saved_at: ts,
            };
            db.put(
                "snapshots",
                &snapshot_key(profile, origin, ts),
                &serde_json::to_vec(&snap).unwrap(),
            )
            .unwrap();
        }

        // No filter → all 3
        let all = list_snapshots(&db, false, None).unwrap();
        assert_eq!(all.len(), 3);

        // Filter to alice → 2 (example.com + other.com)
        let alice = list_snapshots(&db, false, Some("alice")).unwrap();
        assert_eq!(alice.len(), 2);
        assert!(alice.iter().all(|s| s.profile == "alice"));

        // Filter to bob → 1
        let bob = list_snapshots(&db, false, Some("bob")).unwrap();
        assert_eq!(bob.len(), 1);
        assert_eq!(bob[0].profile, "bob");
    }

    #[test]
    fn test_snapshot_serialise_roundtrip() {
        let snap = Snapshot {
            profile: "personal".into(),
            origin: "https://github.com".into(),
            cookies: vec![serde_json::json!({"name": "sid", "value": "abc"})],
            local_storage: std::collections::HashMap::from([("key1".into(), "val1".into())]),
            saved_at: 1_700_000_000_000_000,
        };
        let bytes = serde_json::to_vec(&snap).unwrap();
        let decoded: Snapshot = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.origin, "https://github.com");
        assert_eq!(decoded.cookies.len(), 1);
        assert_eq!(decoded.saved_at, 1_700_000_000_000_000);
    }

    #[test]
    fn test_snapshot_origin_field_preserved_in_list() {
        // Documents that list_snapshots reports the origin STORED IN THE SNAPSHOT,
        // not the key used for storage. This is the invariant restore_snapshot relies
        // on when checking snap.origin == requested origin.
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), [0u8; 32])
            .unwrap();

        let snap = Snapshot {
            profile: "alpha".into(),
            origin: "https://evil.com".into(), // stored origin — would mismatch if key said example.com
            cookies: vec![],
            local_storage: Default::default(),
            saved_at: 1_000_000,
        };
        // Write under a key that disagrees with the stored origin (simulates DB corruption)
        db.put(
            "snapshots",
            "alpha/https://example.com/1000000",
            &serde_json::to_vec(&snap).unwrap(),
        )
        .unwrap();

        let list = list_snapshots(&db, false, None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].origin, "https://evil.com"); // origin from snap, not from key
    }
}
