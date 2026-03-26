// src/endpoint_mapper.rs

use crate::site_knowledge::{ApiKind, CrudOp, EndpointEntry, SiteKnowledgeStore};

const SKIP_EXTENSIONS: &[&str] = &[
    ".js", ".css", ".png", ".jpg", ".jpeg", ".gif", ".svg",
    ".ico", ".woff", ".woff2", ".ttf", ".map", ".xml",
];

const SKIP_PREFIXES: &[&str] = &[
    "/static/", "/assets/", "/public/", "/_next/", "/favicon",
];

pub fn should_skip(path: &str) -> bool {
    for ext in SKIP_EXTENSIONS {
        if path.ends_with(ext) {
            return true;
        }
    }
    for prefix in SKIP_PREFIXES {
        if path.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Replace variable path segments with `{id}` placeholders.
pub fn parameterize_path(path: &str) -> String {
    path.split('/')
        .map(|seg| if looks_like_id(seg) { "{id}".to_string() } else { seg.to_string() })
        .collect::<Vec<_>>()
        .join("/")
}

fn looks_like_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Pure numeric: "42"
    if s.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // UUID: "550e8400-e29b-41d4-a716-446655440000" (36 chars, 4 dashes)
    if s.len() == 36 && s.chars().filter(|&c| c == '-').count() == 4 {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() == 5 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit())) {
            return true;
        }
    }
    // Short hex ID: 8-32 hex chars. Known limitation: English words that are
    // valid hex (e.g. "deadbeef") will be parameterized. Lower bound of 8
    // makes common short words safe in practice.
    if s.len() >= 8 && s.len() <= 32 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // Project-style ID: "ABC-123" (alpha prefix, numeric suffix)
    if s.contains('-') {
        let parts: Vec<&str> = s.splitn(2, '-').collect();
        if parts.len() == 2
            && !parts[0].is_empty()
            && parts[0].chars().all(|c| c.is_ascii_alphabetic())
            && !parts[1].is_empty()
            && parts[1].chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

/// Classify request as REST, GraphQL, or tRPC.
pub fn classify_api(url: &str, body: Option<&str>) -> ApiKind {
    if url.contains("/trpc/") {
        return ApiKind::Trpc;
    }
    if let Some(b) = body {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(b) {
            if val.get("query").is_some() || val.get("mutation").is_some() {
                return ApiKind::GraphQL;
            }
        }
    }
    ApiKind::Rest
}

/// Classify HTTP method + parameterized path as a CRUD operation.
pub fn detect_crud(method: &str, path_pattern: &str) -> Option<CrudOp> {
    let has_id = path_pattern.contains("{id}");
    match method.to_uppercase().as_str() {
        "GET" if !has_id => Some(CrudOp::GetList),
        "GET" => Some(CrudOp::GetItem),
        "POST" => Some(CrudOp::Create),
        "PUT" | "PATCH" => Some(CrudOp::Update),
        "DELETE" => Some(CrudOp::Delete),
        _ => None,
    }
}

/// Process a completed network request and update endpoint knowledge.
/// Non-API requests (static assets, navigation, etc.) are silently skipped.
/// Best-effort: any storage error is logged and ignored.
pub fn ingest(
    entry: &crate::network_log::NetworkEntry,
    store: &SiteKnowledgeStore,
) {
    let origin = match crate::network_log::url_to_origin(&entry.url) {
        Some(o) => o,
        None => return,
    };
    let parsed = match url::Url::parse(&entry.url) {
        Ok(u) => u,
        Err(_) => return,
    };
    let path = parsed.path();
    if should_skip(path) {
        return;
    }

    let path_pattern = parameterize_path(path);
    let api_kind = classify_api(&entry.url, entry.request_body.as_deref());
    let crud_op = detect_crud(&entry.method, &path_pattern);
    let key = format!("{} {}", entry.method.to_uppercase(), path_pattern);

    let mut sk_entry = store.get(&origin).unwrap_or_default().unwrap_or_default();
    let now = crate::site_knowledge::now_micros();

    let ep = sk_entry.endpoints.entry(key).or_insert_with(|| EndpointEntry {
        method: entry.method.to_uppercase(),
        path_pattern: path_pattern.clone(),
        api_kind: api_kind.clone(),
        crud_op: crud_op.clone(),
        observation_count: 0,
        last_seen: now,
        schema: None,
    });
    ep.observation_count += 1;
    ep.last_seen = now;
    // Update schema if response body is parseable JSON
    if let Some(ref body) = entry.response_body {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
            crate::schema_inference::update_endpoint_schema(ep, entry.request_body.as_deref(), &val);
        }
    }

    sk_entry.last_updated = now;
    if let Err(e) = store.put(&origin, &sk_entry) {
        tracing::warn!("endpoint_mapper: failed to persist site_knowledge for {}: {}", origin, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameterize_numeric_id() {
        assert_eq!(parameterize_path("/users/42"), "/users/{id}");
    }

    #[test]
    fn parameterize_uuid() {
        assert_eq!(
            parameterize_path("/items/550e8400-e29b-41d4-a716-446655440000"),
            "/items/{id}"
        );
    }

    #[test]
    fn parameterize_project_id() {
        assert_eq!(parameterize_path("/issues/ABC-123"), "/issues/{id}");
    }

    #[test]
    fn parameterize_no_change_for_words() {
        assert_eq!(parameterize_path("/api/v1/search"), "/api/v1/search");
    }

    #[test]
    fn parameterize_mixed_path() {
        assert_eq!(parameterize_path("/api/users/42/comments"), "/api/users/{id}/comments");
    }

    #[test]
    fn classify_graphql_by_body() {
        use crate::site_knowledge::ApiKind;
        assert_eq!(
            classify_api("https://example.com/graphql", Some(r#"{"query":"{ users { id } }"}"#)),
            ApiKind::GraphQL
        );
    }

    #[test]
    fn classify_trpc_by_path() {
        use crate::site_knowledge::ApiKind;
        assert_eq!(
            classify_api("https://example.com/trpc/user.getById", None),
            ApiKind::Trpc
        );
    }

    #[test]
    fn classify_rest_by_default() {
        use crate::site_knowledge::ApiKind;
        assert_eq!(
            classify_api("https://example.com/api/users", None),
            ApiKind::Rest
        );
    }

    #[test]
    fn detect_crud_get_list() {
        use crate::site_knowledge::CrudOp;
        assert_eq!(detect_crud("GET", "/users"), Some(CrudOp::GetList));
    }

    #[test]
    fn detect_crud_get_item() {
        use crate::site_knowledge::CrudOp;
        assert_eq!(detect_crud("GET", "/users/{id}"), Some(CrudOp::GetItem));
    }

    #[test]
    fn detect_crud_post_create() {
        use crate::site_knowledge::CrudOp;
        assert_eq!(detect_crud("POST", "/users"), Some(CrudOp::Create));
    }

    #[test]
    fn detect_crud_patch_update() {
        use crate::site_knowledge::CrudOp;
        assert_eq!(detect_crud("PATCH", "/users/{id}"), Some(CrudOp::Update));
    }

    #[test]
    fn detect_crud_delete() {
        use crate::site_knowledge::CrudOp;
        assert_eq!(detect_crud("DELETE", "/users/{id}"), Some(CrudOp::Delete));
    }

    #[test]
    fn skip_static_js_file() {
        assert!(should_skip("/app.bundle.js"));
    }

    #[test]
    fn skip_assets_path() {
        assert!(should_skip("/assets/logo.png"));
    }

    #[test]
    fn do_not_skip_api_path() {
        assert!(!should_skip("/api/users"));
    }

    #[test]
    fn ingest_stores_endpoint_in_site_knowledge() {
        use std::sync::Arc;
        use tempfile::tempdir;
        use crate::db::Db;
        use crate::site_knowledge::SiteKnowledgeStore;
        use crate::network_log::NetworkEntry;
        use std::collections::HashMap;

        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Arc::new(Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap());
        let store = SiteKnowledgeStore::new(db, key);

        let entry = NetworkEntry {
            request_id: "1".into(),
            url: "https://api.example.com/users/42".into(),
            method: "GET".into(),
            status: 200,
            duration_ms: 50,
            timestamp_ms: 0,
            request_headers: HashMap::new(),
            request_body: None,
            response_body: None,
            response_truncated: false,
            tab_id: "t1".into(),
        };

        ingest(&entry, &store);

        let sk = store.get("https://api.example.com").unwrap().unwrap();
        assert!(sk.endpoints.contains_key("GET /users/{id}"),
            "endpoints: {:?}", sk.endpoints.keys().collect::<Vec<_>>());
    }

    #[test]
    fn ingest_increments_observation_count() {
        use std::sync::Arc;
        use tempfile::tempdir;
        use crate::db::Db;
        use crate::site_knowledge::SiteKnowledgeStore;
        use crate::network_log::NetworkEntry;
        use std::collections::HashMap;

        let dir = tempdir().unwrap();
        let key = Db::generate_key();
        let db = Arc::new(Db::open_with_key(dir.path().join("t.db").to_str().unwrap(), key).unwrap());
        let store = SiteKnowledgeStore::new(db, key);

        let make_entry = || NetworkEntry {
            request_id: "1".into(),
            url: "https://api.example.com/users/42".into(),
            method: "GET".into(),
            status: 200,
            duration_ms: 50,
            timestamp_ms: 0,
            request_headers: HashMap::new(),
            request_body: None,
            response_body: None,
            response_truncated: false,
            tab_id: "t1".into(),
        };

        ingest(&make_entry(), &store);
        ingest(&make_entry(), &store);

        let sk = store.get("https://api.example.com").unwrap().unwrap();
        let ep = sk.endpoints.get("GET /users/{id}").unwrap();
        assert_eq!(ep.observation_count, 2);
    }
}
