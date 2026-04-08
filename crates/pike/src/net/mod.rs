//! HTTP client for fetching pages.
//!
//! Wraps reqwest with cookie jar support, redirect following,
//! and response body capture for the network log.

use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use std::sync::Arc;
use std::time::Instant;

/// A fetched page response.
#[derive(Debug)]
pub struct FetchResult {
    pub url: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub duration_ms: u64,
    pub content_type: Option<String>,
}

/// Simple HTTP client with cookie persistence.
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub fn new() -> Self {
        let cookie_jar = Arc::new(reqwest::cookie::Jar::default());
        let mut default_headers = HeaderMap::new();
        default_headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) pike/0.1.0 Safari/537.36",
            ),
        );

        let client = reqwest::Client::builder()
            .cookie_provider(cookie_jar)
            .default_headers(default_headers)
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("failed to build HTTP client");

        HttpClient { client }
    }

    /// Fetch a URL and return the response.
    pub async fn fetch(&self, url: &str) -> Result<FetchResult, String> {
        let start = Instant::now();

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("fetch error: {}", e))?;

        let status = response.status().as_u16();
        let final_url = response.url().to_string();

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response
            .text()
            .await
            .map_err(|e| format!("body read error: {}", e))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(FetchResult {
            url: final_url,
            status,
            headers,
            body,
            duration_ms,
            content_type,
        })
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_creates() {
        let _client = HttpClient::new();
    }
}
