// src/adapter_generator.rs

use crate::error::{PagerunnerError, Result};
use crate::site_knowledge::{ApiKind, CrudOp, SiteKnowledgeEntry};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_MODEL: &str = "claude-sonnet-4-6";

/// Generate a JS adapter for the given origin and adapter name by calling the Anthropic API.
/// Requires ANTHROPIC_API_KEY env var. Returns the JS function body string on success.
pub async fn generate(
    origin: &str,
    entry: &SiteKnowledgeEntry,
    adapter_name: &str,
) -> Result<String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        PagerunnerError::Config(
            "ANTHROPIC_API_KEY not set — cannot generate adapter".into(),
        )
    })?;

    let prompt = build_prompt(origin, entry, adapter_name);

    let client = reqwest::Client::new();
    let response = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": ANTHROPIC_MODEL,
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": prompt }]
        }))
        .send()
        .await
        .map_err(|e| PagerunnerError::Config(format!("Adapter generation HTTP error: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(PagerunnerError::Config(format!(
            "Anthropic API error {status}: {body}"
        )));
    }

    let resp_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| PagerunnerError::Config(format!("Adapter generation parse error: {e}")))?;

    let js_code = resp_json["content"][0]["text"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Empty response from Anthropic API".into()))?
        .trim()
        .to_string();

    Ok(js_code)
}

pub fn build_prompt(
    origin: &str,
    entry: &SiteKnowledgeEntry,
    adapter_name: &str,
) -> String {
    let endpoints: Vec<String> = entry
        .endpoints
        .iter()
        .map(|(key, ep)| {
            let kind = match ep.api_kind {
                ApiKind::Rest => "REST",
                ApiKind::GraphQL => "GraphQL",
                ApiKind::Trpc => "tRPC",
            };
            let crud = ep
                .crud_op
                .as_ref()
                .map(|c| match c {
                    CrudOp::GetList => " [list]",
                    CrudOp::GetItem => " [get]",
                    CrudOp::Create => " [create]",
                    CrudOp::Update => " [update]",
                    CrudOp::Delete => " [delete]",
                })
                .unwrap_or("");
            let schema_note = if ep.schema.is_some() {
                " (schema known)"
            } else {
                ""
            };
            format!("  {key} ({kind}){crud}{schema_note}")
        })
        .collect();

    let auth_kinds: Vec<&str> = entry.auth_tokens.keys().map(String::as_str).collect();
    let existing: Vec<&str> = entry.adapters.keys().map(String::as_str).collect();

    format!(
        r#"You are generating a JavaScript adapter for the pagerunner browser automation tool.

Origin: {origin}
Known API endpoints:
{endpoints}
Auth patterns detected: {auth}
Existing adapters: {existing}

Generate an adapter named "{name}".

Requirements:
- Pure JavaScript function body only (no function keyword, no declaration)
- Receives `params` (input object) and `session` ({{origin: "..."}})
- Access auth automatically via fetch() — cookies and headers are inherited from the browser session
- Must return a value using `return` or `await`
- Throw on error: throw new Error("description")
- Single operation, minimal, no comments

Return ONLY the function body code. No markdown, no code fences, no explanation."#,
        origin = origin,
        endpoints = if endpoints.is_empty() {
            "  (none observed yet)".to_string()
        } else {
            endpoints.join("\n")
        },
        auth = if auth_kinds.is_empty() {
            "none".to_string()
        } else {
            auth_kinds.join(", ")
        },
        existing = if existing.is_empty() {
            "none".to_string()
        } else {
            existing.join(", ")
        },
        name = adapter_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site_knowledge::{ApiKind, CrudOp, EndpointEntry, SiteKnowledgeEntry};

    fn entry_with_endpoints() -> SiteKnowledgeEntry {
        let mut e = SiteKnowledgeEntry::default();
        e.endpoints.insert(
            "GET /api/issues/{id}".into(),
            EndpointEntry {
                method: "GET".into(),
                path_pattern: "/api/issues/{id}".into(),
                api_kind: ApiKind::Rest,
                crud_op: Some(CrudOp::GetItem),
                observation_count: 5,
                last_seen: 0,
                schema: None,
            },
        );
        e
    }

    #[test]
    fn build_prompt_includes_origin() {
        let entry = entry_with_endpoints();
        let prompt = build_prompt("https://example.com", &entry, "get_issue");
        assert!(prompt.contains("https://example.com"), "prompt: {}", prompt);
    }

    #[test]
    fn build_prompt_includes_adapter_name() {
        let entry = entry_with_endpoints();
        let prompt = build_prompt("https://example.com", &entry, "get_issue");
        assert!(prompt.contains("get_issue"), "prompt: {}", prompt);
    }

    #[test]
    fn build_prompt_includes_endpoint_info() {
        let entry = entry_with_endpoints();
        let prompt = build_prompt("https://example.com", &entry, "get_issue");
        assert!(
            prompt.contains("/api/issues/{id}") || prompt.contains("GET"),
            "prompt: {}",
            prompt
        );
    }

    #[test]
    fn build_prompt_mentions_function_body_requirement() {
        let entry = SiteKnowledgeEntry::default();
        let prompt = build_prompt("https://example.com", &entry, "test");
        // Must instruct Claude to return only function body
        assert!(
            prompt.contains("function body")
                || prompt.contains("ONLY")
                || prompt.contains("no markdown"),
            "prompt: {}",
            prompt
        );
    }
}
