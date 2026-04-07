//! Anthropic Claude backend for the LLM provider abstraction.
//!
//! Translates `CompletionRequest` into Anthropic Messages API format and parses
//! responses back into `CompletionResponse` / `StreamChunk` values.

use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::error::{LlmError, Result};
use crate::provider::{BoxStream, LlmProvider};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, Role, StopReason, StreamChunk,
    ToolSchema, Usage,
};

const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Claude provider.
pub struct AnthropicProvider {
    api_key: String,
    default_model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    /// Create a new `AnthropicProvider`.
    pub fn new(api_key: impl Into<String>, default_model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            default_model: default_model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Translate a `CompletionRequest` into the JSON body expected by the
    /// Anthropic Messages API.
    pub fn build_request_body(&self, request: &CompletionRequest) -> Value {
        // Collect system messages from the messages array.
        let mut system_parts: Vec<String> = Vec::new();

        // Top-level system field in the request has priority; insert it first.
        if let Some(s) = &request.system {
            if !s.is_empty() {
                system_parts.push(s.clone());
            }
        }

        // Extract system-role messages (Anthropic requires them at the top level).
        for msg in &request.messages {
            if msg.role == Role::System {
                for block in &msg.content {
                    if let ContentBlock::Text { text } = block {
                        system_parts.push(text.clone());
                    }
                }
            }
        }

        // Build the messages array — skip system-role messages.
        let messages: Vec<Value> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| translate_message_to_anthropic(m))
            .collect();

        let model = if request.model.is_empty() {
            self.default_model.clone()
        } else {
            request.model.clone()
        };

        let mut body = json!({
            "model": model,
            "max_tokens": request.max_tokens,
            "messages": messages,
        });

        if !system_parts.is_empty() {
            body["system"] = json!(system_parts.join("\n\n"));
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }

        if !request.tools.is_empty() {
            body["tools"] = json!(translate_tools(&request.tools));
        }

        body
    }

    /// Parse the Anthropic Messages API response JSON into a `CompletionResponse`.
    pub fn parse_response(&self, body: &Value) -> Result<CompletionResponse> {
        // Check for top-level error object.
        if let Some(err_type) = body.get("type").and_then(|t| t.as_str()) {
            if err_type == "error" {
                let message = body
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                return Err(LlmError::Api { status: 0, message });
            }
        }

        let content_arr = body
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| LlmError::InvalidResponse("missing `content` array".into()))?;

        let mut content_blocks: Vec<ContentBlock> = Vec::new();
        for block in content_arr {
            let block_type = block
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or_default();

            match block_type {
                "text" => {
                    let text = block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                        .to_string();
                    content_blocks.push(ContentBlock::Text { text });
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let input = block
                        .get("input")
                        .cloned()
                        .unwrap_or(Value::Object(Default::default()));
                    content_blocks.push(ContentBlock::ToolUse { id, name, input });
                }
                other => {
                    warn!("Anthropic: unknown content block type `{other}`, skipping");
                }
            }
        }

        // Usage.
        let usage = body.get("usage").map_or(Usage::default(), |u| Usage {
            input_tokens: u
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            output_tokens: u
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        });

        // Stop reason.
        let stop_reason = match body
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("end_turn")
        {
            "tool_use" => StopReason::ToolUse,
            "max_tokens" => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };

        Ok(CompletionResponse {
            content: content_blocks,
            usage,
            stop_reason,
        })
    }

    /// Parse a single SSE line and return a `StreamChunk` if the line carries
    /// meaningful data. Returns `None` for blank lines, comments, or
    /// non-data lines.
    pub fn parse_sse_line(&self, line: &str) -> Option<StreamChunk> {
        parse_sse_line(line)
    }
}

// ---------------------------------------------------------------------------
// SSE parsing (exported for unit tests)
// ---------------------------------------------------------------------------

/// Parse a single raw SSE line into a `StreamChunk`. Returns `None` for lines
/// that do not produce a chunk (comments, blanks, non-data events).
pub fn parse_sse_line(line: &str) -> Option<StreamChunk> {
    if !line.starts_with("data: ") {
        return None;
    }

    let data = &line["data: ".len()..];
    let event: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match event_type {
        "content_block_start" => {
            // A new content block is beginning. If it's a tool_use block, emit
            // ToolUseStart; if it's a text block, nothing to emit yet.
            let block = event.get("content_block")?;
            let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if btype == "tool_use" {
                let id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                Some(StreamChunk::ToolUseStart { id, name })
            } else {
                None
            }
        }
        "content_block_delta" => {
            let delta = event.get("delta")?;
            let dtype = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match dtype {
                "text_delta" => {
                    let text = delta
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                        .to_string();
                    Some(StreamChunk::TextDelta { text })
                }
                "input_json_delta" => {
                    let partial = delta
                        .get("partial_json")
                        .and_then(|p| p.as_str())
                        .unwrap_or_default()
                        .to_string();
                    Some(StreamChunk::ToolUseInputDelta {
                        input_delta: partial,
                    })
                }
                _ => None,
            }
        }
        "message_start" => {
            // Extract initial usage (input tokens).
            let msg = event.get("message")?;
            let usage = msg.get("usage")?;
            let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            if input_tokens > 0 {
                Some(StreamChunk::Usage(Usage {
                    input_tokens,
                    output_tokens: 0,
                }))
            } else {
                None
            }
        }
        "message_delta" => {
            // Contains output token count and stop reason.
            let usage = event.get("usage")?;
            let output_tokens = usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            Some(StreamChunk::Usage(Usage {
                input_tokens: 0,
                output_tokens,
            }))
        }
        "message_stop" => Some(StreamChunk::Done),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helper: translate our message types → Anthropic format
// ---------------------------------------------------------------------------

fn translate_message_to_anthropic(msg: &Message) -> Value {
    let role = match msg.role {
        Role::User | Role::Tool => "user",
        Role::Assistant => "assistant",
        Role::System => "user", // should have been filtered out before calling this
    };

    // Build Anthropic content array.
    let content: Vec<Value> = msg
        .content
        .iter()
        .map(|block| translate_content_block_to_anthropic(block))
        .collect();

    if content.len() == 1 {
        // If there's a single text block, Anthropic also accepts a plain string.
        // We still use the array form for consistency.
    }

    json!({ "role": role, "content": content })
}

fn translate_content_block_to_anthropic(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::ToolUse { id, name, input } => {
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let mut v = json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
            });
            if let Some(is_err) = is_error {
                v["is_error"] = json!(is_err);
            }
            v
        }
    }
}

fn translate_tools(tools: &[ToolSchema]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// LlmProvider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let body = self.build_request_body(&request);
        debug!("Anthropic request body: {}", body);

        let resp = self
            .client
            .post(format!("{ANTHROPIC_API_BASE}/messages"))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let response_body: Value = resp.json().await?;

        if status >= 400 {
            let message = response_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown API error")
                .to_string();
            return Err(LlmError::Api { status, message });
        }

        self.parse_response(&response_body)
    }

    async fn complete_stream(&self, request: CompletionRequest) -> Result<BoxStream> {
        let mut body = self.build_request_body(&request);
        body["stream"] = json!(true);

        debug!("Anthropic streaming request body: {}", body);

        let resp = self
            .client
            .post(format!("{ANTHROPIC_API_BASE}/messages"))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let err_body: Value = resp.json().await?;
            let message = err_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown API error")
                .to_string();
            return Err(LlmError::Api { status, message });
        }

        let byte_stream = resp.bytes_stream();

        let chunk_stream = byte_stream
            .map(|result| match result {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    // Each chunk may contain multiple SSE lines.
                    let chunks: Vec<Result<StreamChunk>> = text
                        .lines()
                        .filter_map(|line| parse_sse_line(line).map(Ok))
                        .collect();
                    stream::iter(chunks)
                }
                Err(e) => stream::iter(vec![Err(LlmError::Http(e))]),
            })
            .flatten();

        Ok(Box::pin(chunk_stream))
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn supports_tools(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider() -> AnthropicProvider {
        AnthropicProvider::new("test-key", "claude-opus-4-5")
    }

    // --- build_request_body ---

    #[test]
    fn build_body_simple_user_message() {
        let req = CompletionRequest::new(vec![Message::user("Hello!")], "claude-opus-4-5", 1024);
        let body = provider().build_request_body(&req);
        assert_eq!(body["model"], "claude-opus-4-5");
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["text"], "Hello!");
        assert!(body.get("system").is_none());
    }

    #[test]
    fn build_body_uses_default_model_when_empty() {
        let req = CompletionRequest::new(vec![Message::user("hi")], "", 512);
        let body = provider().build_request_body(&req);
        assert_eq!(body["model"], "claude-opus-4-5");
    }

    #[test]
    fn build_body_system_field_extracted() {
        let mut req = CompletionRequest::new(vec![Message::user("go")], "m", 512);
        req.system = Some("You are helpful.".into());
        let body = provider().build_request_body(&req);
        assert_eq!(body["system"], "You are helpful.");
        // No system-role message should appear in messages array.
        let msgs = body["messages"].as_array().unwrap();
        assert!(msgs.iter().all(|m| m["role"] != "system"));
    }

    #[test]
    fn build_body_system_message_in_array_extracted() {
        let req = CompletionRequest::new(
            vec![
                Message::system("Be concise."),
                Message::user("What time is it?"),
            ],
            "m",
            512,
        );
        let body = provider().build_request_body(&req);
        assert_eq!(body["system"], "Be concise.");
        // Only the user message should be in messages.
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn build_body_system_field_and_system_message_merged() {
        let mut req = CompletionRequest::new(
            vec![Message::system("Second part."), Message::user("ok")],
            "m",
            512,
        );
        req.system = Some("First part.".into());
        let body = provider().build_request_body(&req);
        let system = body["system"].as_str().unwrap();
        assert!(system.contains("First part."));
        assert!(system.contains("Second part."));
    }

    #[test]
    fn build_body_tool_result_becomes_user_role() {
        let req = CompletionRequest::new(
            vec![Message::tool_result("call-1", "tool output")],
            "m",
            512,
        );
        let body = provider().build_request_body(&req);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"][0]["type"], "tool_result");
        assert_eq!(msgs[0]["content"][0]["tool_use_id"], "call-1");
    }

    #[test]
    fn build_body_assistant_tool_use_block() {
        let req = CompletionRequest::new(
            vec![Message::assistant(vec![ContentBlock::ToolUse {
                id: "t1".into(),
                name: "navigate".into(),
                input: json!({"url": "https://example.com"}),
            }])],
            "m",
            512,
        );
        let body = provider().build_request_body(&req);
        let block = &body["messages"][0]["content"][0];
        assert_eq!(block["type"], "tool_use");
        assert_eq!(block["id"], "t1");
        assert_eq!(block["name"], "navigate");
        assert_eq!(block["input"]["url"], "https://example.com");
    }

    #[test]
    fn build_body_tools_translated() {
        let mut req = CompletionRequest::new(vec![Message::user("hi")], "m", 512);
        req.tools = vec![ToolSchema::new(
            "screenshot",
            "Take a screenshot",
            json!({"type": "object", "properties": {}}),
        )];
        let body = provider().build_request_body(&req);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "screenshot");
        assert_eq!(tools[0]["description"], "Take a screenshot");
        assert!(tools[0].get("input_schema").is_some());
    }

    #[test]
    fn build_body_temperature_included_when_set() {
        let mut req = CompletionRequest::new(vec![Message::user("hi")], "m", 512);
        req.temperature = Some(0.7);
        let body = provider().build_request_body(&req);
        let temp = body["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 1e-5);
    }

    #[test]
    fn build_body_no_tools_field_when_empty() {
        let req = CompletionRequest::new(vec![Message::user("hi")], "m", 512);
        let body = provider().build_request_body(&req);
        assert!(body.get("tools").is_none());
    }

    // --- parse_response ---

    #[test]
    fn parse_response_text_only() {
        let body = json!({
            "type": "message",
            "content": [{"type": "text", "text": "Hello from Claude"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.text(), "Hello from Claude");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[test]
    fn parse_response_tool_use() {
        let body = json!({
            "type": "message",
            "content": [
                {"type": "tool_use", "id": "tu1", "name": "click", "input": {"selector": "#btn"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 8}
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        if let ContentBlock::ToolUse { id, name, input } = calls[0] {
            assert_eq!(id, "tu1");
            assert_eq!(name, "click");
            assert_eq!(input["selector"], "#btn");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn parse_response_max_tokens_stop() {
        let body = json!({
            "type": "message",
            "content": [{"type": "text", "text": "truncated"}],
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 5, "output_tokens": 100}
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    }

    #[test]
    fn parse_response_missing_content_errors() {
        let body = json!({"type": "message", "stop_reason": "end_turn"});
        let err = provider().parse_response(&body).unwrap_err();
        assert!(matches!(err, LlmError::InvalidResponse(_)));
    }

    #[test]
    fn parse_response_api_error_body() {
        let body = json!({
            "type": "error",
            "error": {"type": "authentication_error", "message": "Invalid API key"}
        });
        let err = provider().parse_response(&body).unwrap_err();
        assert!(matches!(err, LlmError::Api { message, .. } if message.contains("Invalid API key")));
    }

    #[test]
    fn parse_response_empty_content_array() {
        let body = json!({
            "type": "message",
            "content": [],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        let resp = provider().parse_response(&body).unwrap();
        assert!(resp.content.is_empty());
        assert_eq!(resp.text(), "");
    }

    #[test]
    fn parse_response_missing_usage_defaults_to_zero() {
        let body = json!({
            "type": "message",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn"
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.usage, Usage::default());
    }

    // --- SSE line parsing ---

    #[test]
    fn sse_blank_line_returns_none() {
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line("   ").is_none());
        assert!(parse_sse_line(": comment").is_none());
        assert!(parse_sse_line("event: content_block_start").is_none());
    }

    #[test]
    fn sse_message_start_with_usage() {
        let line = r#"data: {"type":"message_start","message":{"usage":{"input_tokens":15,"output_tokens":0}}}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(matches!(chunk, StreamChunk::Usage(Usage { input_tokens: 15, .. })));
    }

    #[test]
    fn sse_message_start_zero_tokens_returns_none() {
        let line = r#"data: {"type":"message_start","message":{"usage":{"input_tokens":0,"output_tokens":0}}}"#;
        assert!(parse_sse_line(line).is_none());
    }

    #[test]
    fn sse_content_block_start_text_returns_none() {
        let line = r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
        assert!(parse_sse_line(line).is_none());
    }

    #[test]
    fn sse_content_block_start_tool_use() {
        let line = r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01","name":"click"}}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(
            matches!(chunk, StreamChunk::ToolUseStart { ref id, ref name } if id == "toolu_01" && name == "click")
        );
    }

    #[test]
    fn sse_content_block_delta_text() {
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(matches!(chunk, StreamChunk::TextDelta { ref text } if text == "Hello"));
    }

    #[test]
    fn sse_content_block_delta_input_json() {
        let line = r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"url\":"}}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(
            matches!(chunk, StreamChunk::ToolUseInputDelta { ref input_delta } if input_delta.contains("url"))
        );
    }

    #[test]
    fn sse_message_delta_output_tokens() {
        let line = r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(
            matches!(chunk, StreamChunk::Usage(Usage { output_tokens: 42, .. }))
        );
    }

    #[test]
    fn sse_message_stop() {
        let line = r#"data: {"type":"message_stop"}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }
}
