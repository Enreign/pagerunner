use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "x-auth-token",
];

const RESPONSE_BODY_TRUNCATE_BYTES: usize = 2048;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub request_id: String,
    pub url: String,
    pub method: String,
    pub status: u16,
    pub duration_ms: u64,
    pub timestamp_ms: u64,
    pub request_headers: HashMap<String, String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub response_truncated: bool,
    pub tab_id: String,
}

#[derive(Debug, Clone)]
pub struct InFlightRequest {
    pub url: String,
    pub method: String,
    pub request_headers: HashMap<String, String>,
    pub request_body: Option<String>,
    pub start_timestamp_ms: u64,
    pub status: Option<u16>,
}

pub struct NetworkQuery {
    pub url_pattern: Option<String>,
    pub method: Option<String>,
    pub status_min: Option<u16>,
    pub status_max: Option<u16>,
    pub lookback_ms: Option<u64>,
    pub limit: usize,
    pub include_request_body: bool,
    pub full_response: bool,
    pub all_tabs: bool,
}

impl Default for NetworkQuery {
    fn default() -> Self {
        Self {
            url_pattern: None,
            method: None,
            status_min: None,
            status_max: None,
            lookback_ms: None,
            limit: 50,
            include_request_body: false,
            full_response: false,
            all_tabs: false,
        }
    }
}

pub struct QueryResult {
    pub entries: Vec<NetworkEntry>,
    pub total_matched: usize,
    pub total_captured: usize,
    pub result_truncated: bool,
}

pub fn strip_sensitive_headers(headers: &mut HashMap<String, String>) {
    headers.retain(|k, _| !SENSITIVE_HEADERS.contains(&k.to_lowercase().as_str()));
}

pub fn truncate_body(body: Option<String>, full_response: bool) -> (Option<String>, bool) {
    match body {
        None => (None, false),
        Some(s) if full_response || s.len() <= RESPONSE_BODY_TRUNCATE_BYTES => (Some(s), false),
        Some(s) => {
            let truncated = s[..RESPONSE_BODY_TRUNCATE_BYTES].to_string();
            (Some(truncated), true)
        }
    }
}

pub fn matches_url_pattern(url: &str, pattern: &str) -> bool {
    if pattern.contains('*') || pattern.contains('?') {
        // Two-phase URL matching based on pattern shape:
        //
        // - Path patterns (start with "/") e.g. "/api/v1/*":
        //   Strip scheme + host. Match path portion against pattern.
        //   literal_separator(true): * stays within one path segment; ** crosses /.
        //
        // - Host/full patterns (don't start with "/") e.g. "*.example.com/**":
        //   Strip scheme only. Match host+path against pattern.
        //   * matches within a hostname label; ** crosses / in path.
        let url_portion: &str = if pattern.starts_with('/') {
            let without_scheme = url
                .strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(url);
            if let Some(path_start) = without_scheme.find('/') {
                &without_scheme[path_start..]
            } else {
                "/"
            }
        } else {
            url.strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(url)
        };

        let builder = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .and_then(|g| globset::GlobSet::builder().add(g).build());
        match builder {
            Ok(gs) => gs.is_match(url_portion),
            Err(_) => url.contains(pattern),
        }
    } else {
        url.contains(pattern)
    }
}

/// Ring buffer key for a network log entry:
/// "netlog/{session_id}/{target_id}/{seq:016x}"
fn entry_key(session_id: &str, target_id: &str, seq: u64) -> String {
    format!("{}/{}/{:016x}", session_id, target_id, seq)
}

/// Write an entry to the ring buffer. Evicts oldest entry if buffer is at capacity.
pub fn write_entry(
    db: &Db,
    session_id: &str,
    target_id: &str,
    seq: u64,
    capacity: usize,
    entry: &NetworkEntry,
) -> crate::error::Result<()> {
    // Evict oldest if at capacity
    if seq >= capacity as u64 {
        let evict_seq = seq - capacity as u64;
        let evict_key = entry_key(session_id, target_id, evict_seq);
        db.delete("netlog", &evict_key)?;
    }
    let key = entry_key(session_id, target_id, seq);
    let value = serde_json::to_vec(entry)?;
    db.put("netlog", &key, &value)
}

/// Query entries for one tab (target_id = Some) or all tabs (target_id = None).
pub fn query_entries(
    db: &Db,
    session_id: &str,
    target_id: Option<&str>,
    all_target_ids: &[String],
    query: &NetworkQuery,
    now_ms: u64,
) -> crate::error::Result<QueryResult> {
    let tab_ids: Vec<&str> = if let Some(tid) = target_id {
        vec![tid]
    } else {
        all_target_ids.iter().map(String::as_str).collect()
    };

    let mut all_entries: Vec<NetworkEntry> = Vec::new();
    let mut total_captured: usize = 0;

    for tid in &tab_ids {
        let prefix = format!("{}/{}/", session_id, tid);
        let raw = db.scan_prefix("netlog", &prefix)?;
        total_captured += raw.len();
        for (_, bytes) in raw {
            if let Ok(mut entry) = serde_json::from_slice::<NetworkEntry>(&bytes) {
                // Unconditional 24h TTL (lazy expiry)
                const TTL_MS: u64 = 24 * 60 * 60 * 1000;
                if entry.timestamp_ms < now_ms.saturating_sub(TTL_MS) {
                    continue;
                }
                // User-supplied lookback filter
                if let Some(lookback) = query.lookback_ms {
                    if entry.timestamp_ms < now_ms.saturating_sub(lookback) {
                        continue;
                    }
                }
                // Filter: URL pattern
                if let Some(pattern) = &query.url_pattern {
                    if !matches_url_pattern(&entry.url, pattern) {
                        continue;
                    }
                }
                // Filter: method
                if let Some(method) = &query.method {
                    if !entry.method.eq_ignore_ascii_case(method) {
                        continue;
                    }
                }
                // Filter: status range
                if let Some(min) = query.status_min {
                    if entry.status < min {
                        continue;
                    }
                }
                if let Some(max) = query.status_max {
                    if entry.status > max {
                        continue;
                    }
                }
                // Strip request_body unless requested
                if !query.include_request_body {
                    entry.request_body = None;
                }
                // Truncate response body
                let (body, truncated) = truncate_body(entry.response_body.take(), query.full_response);
                entry.response_body = body;
                entry.response_truncated = truncated;

                all_entries.push(entry);
            }
        }
    }

    let total_matched = all_entries.len();

    // Sort newest-first by timestamp_ms
    all_entries.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));

    let result_truncated = total_matched > query.limit;
    all_entries.truncate(query.limit);

    Ok(QueryResult {
        entries: all_entries,
        total_matched,
        total_captured,
        result_truncated,
    })
}

/// Delete all network log entries for a session (called on close_session).
pub fn delete_session_entries(db: &Db, session_id: &str) -> crate::error::Result<()> {
    db.delete_prefix("netlog", &format!("{}/", session_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_entry(url: &str, method: &str, status: u16, timestamp_ms: u64) -> NetworkEntry {
        NetworkEntry {
            request_id: uuid::Uuid::new_v4().to_string(),
            url: url.into(),
            method: method.into(),
            status,
            duration_ms: 50,
            timestamp_ms,
            request_headers: HashMap::new(),
            request_body: Some("req-body".into()),
            response_body: Some("resp-body".into()),
            response_truncated: false,
            tab_id: "tab1".into(),
        }
    }

    #[test]
    fn test_strip_sensitive_headers() {
        let mut h = HashMap::new();
        h.insert("Authorization".into(), "Bearer token".into());
        h.insert("Content-Type".into(), "application/json".into());
        h.insert("Cookie".into(), "session=abc".into());
        strip_sensitive_headers(&mut h);
        assert!(!h.contains_key("Authorization"));
        assert!(!h.contains_key("Cookie"));
        assert!(h.contains_key("Content-Type"));
    }

    #[test]
    fn test_truncate_body_under_limit() {
        let (body, truncated) = truncate_body(Some("short".into()), false);
        assert_eq!(body, Some("short".into()));
        assert!(!truncated);
    }

    #[test]
    fn test_truncate_body_over_limit() {
        let long = "x".repeat(RESPONSE_BODY_TRUNCATE_BYTES + 100);
        let (body, truncated) = truncate_body(Some(long), false);
        assert_eq!(body.unwrap().len(), RESPONSE_BODY_TRUNCATE_BYTES);
        assert!(truncated);
    }

    #[test]
    fn test_truncate_body_full_response_no_truncation() {
        let long = "x".repeat(RESPONSE_BODY_TRUNCATE_BYTES + 100);
        let len = long.len();
        let (body, truncated) = truncate_body(Some(long), true);
        assert_eq!(body.unwrap().len(), len);
        assert!(!truncated);
    }

    #[test]
    fn test_truncate_body_none() {
        let (body, truncated) = truncate_body(None, false);
        assert!(body.is_none());
        assert!(!truncated);
    }

    #[test]
    fn test_matches_url_pattern_substring() {
        assert!(matches_url_pattern("https://api.example.com/graphql", "graphql"));
        assert!(!matches_url_pattern("https://api.example.com/users", "graphql"));
    }

    #[test]
    fn test_matches_url_pattern_glob_path() {
        assert!(matches_url_pattern("https://api.example.com/api/v1/users", "/api/v1/*"));
        assert!(!matches_url_pattern("https://api.example.com/api/v1/users/123", "/api/v1/*"));
        assert!(matches_url_pattern("https://api.example.com/api/v1/users/123", "/api/v1/**"));
    }

    #[test]
    fn test_matches_url_pattern_glob_host() {
        assert!(matches_url_pattern("https://api.example.com/path", "*.example.com/**"));
        assert!(matches_url_pattern("http://sub.example.com/a/b/c", "*.example.com/**"));
        assert!(!matches_url_pattern("https://other.net/path", "*.example.com/**"));
    }

    #[test]
    fn test_write_and_query_entries() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        let session_id = "sess1";
        let target_id = "tab1";

        for i in 0..3u64 {
            let entry = make_entry(
                &format!("https://api.example.com/item/{}", i),
                "GET",
                200,
                1000 + i * 100,
            );
            write_entry(&db, session_id, target_id, i, 500, &entry).unwrap();
        }

        let query = NetworkQuery::default();
        let result = query_entries(&db, session_id, Some(target_id), &[], &query, 9999999).unwrap();
        assert_eq!(result.entries.len(), 3);
        // newest first
        assert!(result.entries[0].timestamp_ms > result.entries[1].timestamp_ms);
    }

    #[test]
    fn test_query_url_filter() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        write_entry(&db, "s1", "t1", 0, 500, &make_entry("https://api.example.com/users", "GET", 200, 1000)).unwrap();
        write_entry(&db, "s1", "t1", 1, 500, &make_entry("https://api.example.com/graphql", "POST", 200, 1100)).unwrap();

        let query = NetworkQuery { url_pattern: Some("graphql".into()), ..Default::default() };
        let result = query_entries(&db, "s1", Some("t1"), &[], &query, 9999999).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert!(result.entries[0].url.contains("graphql"));
        assert_eq!(result.total_matched, 1);
        assert_eq!(result.total_captured, 2);
    }

    #[test]
    fn test_query_status_filter() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        write_entry(&db, "s1", "t1", 0, 500, &make_entry("https://x.com/a", "GET", 200, 1000)).unwrap();
        write_entry(&db, "s1", "t1", 1, 500, &make_entry("https://x.com/b", "GET", 404, 1100)).unwrap();
        write_entry(&db, "s1", "t1", 2, 500, &make_entry("https://x.com/c", "GET", 500, 1200)).unwrap();

        let query = NetworkQuery { status_min: Some(400), status_max: Some(499), ..Default::default() };
        let result = query_entries(&db, "s1", Some("t1"), &[], &query, 9999999).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].status, 404);
    }

    #[test]
    fn test_query_limit_and_result_truncated() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        for i in 0..10u64 {
            write_entry(&db, "s1", "t1", i, 500, &make_entry("https://x.com/", "GET", 200, 1000 + i)).unwrap();
        }

        let query = NetworkQuery { limit: 3, ..Default::default() };
        let result = query_entries(&db, "s1", Some("t1"), &[], &query, 9999999).unwrap();
        assert_eq!(result.entries.len(), 3);
        assert_eq!(result.total_matched, 10);
        assert!(result.result_truncated);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        // capacity = 3; write 5 entries, oldest 2 should be evicted
        for i in 0..5u64 {
            let entry = make_entry("https://x.com/", "GET", 200, 1000 + i);
            write_entry(&db, "s1", "t1", i, 3, &entry).unwrap();
        }

        let query = NetworkQuery { limit: 500, ..Default::default() };
        let result = query_entries(&db, "s1", Some("t1"), &[], &query, 9999999).unwrap();
        assert_eq!(result.entries.len(), 3, "only last 3 should remain after eviction");
    }

    #[test]
    fn test_delete_session_entries() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        write_entry(&db, "sess_a", "tab1", 0, 500, &make_entry("https://a.com/", "GET", 200, 1000)).unwrap();
        write_entry(&db, "sess_b", "tab1", 0, 500, &make_entry("https://b.com/", "GET", 200, 1000)).unwrap();

        delete_session_entries(&db, "sess_a").unwrap();

        let q = NetworkQuery { limit: 500, ..Default::default() };
        let r_a = query_entries(&db, "sess_a", Some("tab1"), &[], &q, 9999999).unwrap();
        let r_b = query_entries(&db, "sess_b", Some("tab1"), &[], &q, 9999999).unwrap();
        assert_eq!(r_a.entries.len(), 0, "sess_a entries should be deleted");
        assert_eq!(r_b.entries.len(), 1, "sess_b entries should be untouched");
    }

    #[test]
    fn test_request_body_stripped_unless_requested() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        write_entry(&db, "s1", "t1", 0, 500, &make_entry("https://x.com/", "POST", 200, 1000)).unwrap();

        let query_no_body = NetworkQuery { include_request_body: false, ..Default::default() };
        let r = query_entries(&db, "s1", Some("t1"), &[], &query_no_body, 9999999).unwrap();
        assert!(r.entries[0].request_body.is_none());

        let query_with_body = NetworkQuery { include_request_body: true, ..Default::default() };
        let r = query_entries(&db, "s1", Some("t1"), &[], &query_with_body, 9999999).unwrap();
        assert_eq!(r.entries[0].request_body, Some("req-body".into()));
    }

    #[test]
    fn test_lookback_filter() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        write_entry(&db, "s1", "t1", 0, 500, &make_entry("https://x.com/old", "GET", 200, 8000)).unwrap();
        write_entry(&db, "s1", "t1", 1, 500, &make_entry("https://x.com/new", "GET", 200, 9500)).unwrap();

        let query = NetworkQuery { lookback_ms: Some(1000), ..Default::default() };
        let result = query_entries(&db, "s1", Some("t1"), &[], &query, 10000).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert!(result.entries[0].url.contains("new"));
    }

    #[test]
    fn test_24h_ttl_expiry() {
        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();

        let ttl_ms: u64 = 24 * 60 * 60 * 1000;
        let now_ms: u64 = ttl_ms * 2;

        write_entry(&db, "s1", "t1", 0, 500, &make_entry("https://x.com/old", "GET", 200, 0)).unwrap();
        write_entry(&db, "s1", "t1", 1, 500, &make_entry("https://x.com/new", "GET", 200, now_ms - 100)).unwrap();

        let query = NetworkQuery { limit: 500, ..Default::default() };
        let result = query_entries(&db, "s1", Some("t1"), &[], &query, now_ms).unwrap();
        assert_eq!(result.entries.len(), 1, "entry older than 24h should be expired");
        assert!(result.entries[0].url.contains("new"));
    }

    #[test]
    fn test_inflight_request_assembled_correctly() {
        let req = InFlightRequest {
            url: "https://api.example.com/users".into(),
            method: "GET".into(),
            request_headers: {
                let mut h = HashMap::new();
                h.insert("Authorization".into(), "Bearer token".into());
                h.insert("Content-Type".into(), "application/json".into());
                h
            },
            request_body: None,
            start_timestamp_ms: 1000,
            status: Some(200),
        };

        let response_body = Some("{}".to_string());
        let finish_timestamp_ms = 1150u64;
        let tab_id = "tab1".to_string();
        let request_id = "req1".to_string();

        let mut headers = req.request_headers.clone();
        strip_sensitive_headers(&mut headers);

        let (body, truncated) = truncate_body(response_body, false);
        let entry = NetworkEntry {
            request_id: request_id.clone(),
            url: req.url.clone(),
            method: req.method.clone(),
            status: req.status.unwrap(),
            duration_ms: finish_timestamp_ms.saturating_sub(req.start_timestamp_ms),
            timestamp_ms: req.start_timestamp_ms,
            request_headers: headers,
            request_body: req.request_body.clone(),
            response_body: body,
            response_truncated: truncated,
            tab_id: tab_id.clone(),
        };

        assert!(!entry.request_headers.contains_key("Authorization"));
        assert!(entry.request_headers.contains_key("Content-Type"));
        assert_eq!(entry.status, 200);
        assert_eq!(entry.duration_ms, 150);
        assert!(!entry.response_truncated);
    }
}
