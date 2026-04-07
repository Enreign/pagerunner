//! Error types for the LLM provider abstraction.

use thiserror::Error;

/// All errors that can arise from LLM provider operations.
#[derive(Debug, Error)]
pub enum LlmError {
    /// An HTTP-level transport error from reqwest.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// The API returned a non-success status with an error message.
    #[error("API error (status {status}): {message}")]
    Api { status: u16, message: String },

    /// The response body could not be parsed into the expected type.
    #[error("Invalid response from provider: {0}")]
    InvalidResponse(String),

    /// A streaming response error.
    #[error("Stream error: {0}")]
    Stream(String),

    /// The provider is not configured (e.g. missing API key).
    #[error("Provider not configured: {0}")]
    NotConfigured(String),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, LlmError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_display() {
        let e = LlmError::Api {
            status: 429,
            message: "rate limit exceeded".to_string(),
        };
        assert_eq!(e.to_string(), "API error (status 429): rate limit exceeded");
    }

    #[test]
    fn invalid_response_display() {
        let e = LlmError::InvalidResponse("missing field `content`".to_string());
        assert!(e.to_string().contains("missing field"));
    }

    #[test]
    fn stream_error_display() {
        let e = LlmError::Stream("unexpected EOF".to_string());
        assert_eq!(e.to_string(), "Stream error: unexpected EOF");
    }

    #[test]
    fn not_configured_display() {
        let e = LlmError::NotConfigured("ANTHROPIC_API_KEY not set".to_string());
        assert!(e.to_string().contains("ANTHROPIC_API_KEY not set"));
    }
}
