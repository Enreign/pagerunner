//! ToolExecutor trait — the interface agents use to invoke tools.

use async_trait::async_trait;
use serde_json::Value;

use pagerunner_llm::ToolSchema;

// ---------------------------------------------------------------------------
// ToolResponse
// ---------------------------------------------------------------------------

/// Result of executing a single tool call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResponse {
    /// The textual content returned by the tool.
    pub content: String,
    /// Whether the tool execution resulted in an error.
    pub is_error: bool,
}

impl ToolResponse {
    /// Create a successful response.
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    /// Create an error response.
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

// ---------------------------------------------------------------------------
// ToolExecutor
// ---------------------------------------------------------------------------

/// An executor that runs tool calls on behalf of the agent.
#[async_trait]
pub trait ToolExecutor: Send + Sync + 'static {
    /// Execute a tool by name with the given JSON arguments.
    ///
    /// Returns `Ok(ToolResponse)` on success (note: `is_error` inside the
    /// response indicates a tool-level error rather than a transport error).
    /// Returns `Err(String)` for unexpected / fatal executor failures.
    async fn execute(
        &self,
        name: &str,
        args: Value,
    ) -> Result<ToolResponse, String>;

    /// Return the list of tools available through this executor.
    fn available_tools(&self) -> Vec<ToolSchema>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- ToolResponse helpers ---

    #[test]
    fn tool_response_ok() {
        let r = ToolResponse::ok("all good");
        assert_eq!(r.content, "all good");
        assert!(!r.is_error);
    }

    #[test]
    fn tool_response_error() {
        let r = ToolResponse::error("something broke");
        assert_eq!(r.content, "something broke");
        assert!(r.is_error);
    }

    // --- Mock executor ---

    struct EchoExecutor;

    #[async_trait]
    impl ToolExecutor for EchoExecutor {
        async fn execute(&self, name: &str, args: Value) -> Result<ToolResponse, String> {
            Ok(ToolResponse::ok(format!("{name}: {args}")))
        }

        fn available_tools(&self) -> Vec<ToolSchema> {
            vec![
                ToolSchema::new("echo", "Echo the args back", json!({"type": "object"})),
                ToolSchema::new("fail", "Always fails", json!({"type": "object"})),
            ]
        }
    }

    #[tokio::test]
    async fn mock_executor_execute() {
        let exec = EchoExecutor;
        let response = exec
            .execute("echo", json!({"message": "hello"}))
            .await
            .unwrap();
        assert!(!response.is_error);
        assert!(response.content.contains("echo"));
        assert!(response.content.contains("hello"));
    }

    #[tokio::test]
    async fn mock_executor_available_tools() {
        let exec = EchoExecutor;
        let tools = exec.available_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[1].name, "fail");
    }

    #[tokio::test]
    async fn executor_behind_arc() {
        let exec: std::sync::Arc<dyn ToolExecutor> = std::sync::Arc::new(EchoExecutor);
        let tools = exec.available_tools();
        assert!(!tools.is_empty());

        let response = exec.execute("echo", json!({})).await.unwrap();
        assert!(!response.is_error);
    }

    struct ErroringExecutor;

    #[async_trait]
    impl ToolExecutor for ErroringExecutor {
        async fn execute(&self, _name: &str, _args: Value) -> Result<ToolResponse, String> {
            Err("fatal transport error".to_string())
        }

        fn available_tools(&self) -> Vec<ToolSchema> {
            vec![]
        }
    }

    #[tokio::test]
    async fn executor_fatal_error_returns_err() {
        let exec = ErroringExecutor;
        let result = exec.execute("anything", json!({})).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "fatal transport error");
    }

    #[tokio::test]
    async fn tool_level_error_is_ok_with_is_error_true() {
        // A tool may succeed at the transport level but report an error in content.
        struct ToolErrorExecutor;

        #[async_trait]
        impl ToolExecutor for ToolErrorExecutor {
            async fn execute(&self, _name: &str, _args: Value) -> Result<ToolResponse, String> {
                Ok(ToolResponse::error("selector not found"))
            }

            fn available_tools(&self) -> Vec<ToolSchema> {
                vec![]
            }
        }

        let exec = ToolErrorExecutor;
        let response = exec.execute("click", json!({"selector": "#missing"})).await.unwrap();
        assert!(response.is_error);
        assert_eq!(response.content, "selector not found");
    }
}
