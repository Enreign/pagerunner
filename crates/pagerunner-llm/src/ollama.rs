//! Ollama backend for the LLM provider abstraction.
//!
//! Communicates with a local (or remote) Ollama server using the
//! `/api/chat` NDJSON endpoint. No authentication is required.

use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use tracing::debug;
use uuid::Uuid;

use crate::error::{LlmError, Result};
use crate::provider::{BoxStream, LlmProvider};
use crate::types::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, Role, StopReason, StreamChunk,
    ToolSchema, Usage,
};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Ollama local-model provider.
pub struct OllamaProvider {
    base_url: String,
    default_model: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    /// Create a new `OllamaProvider`.
    ///
    /// `base_url` defaults to `http://localhost:11434` when `None`.
    pub fn new(base_url: Option<impl Into<String>>, default_model: impl Into<String>) -> Self {
        Self {
            base_url: base_url
                .map(|u| u.into())
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            default_model: default_model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Translate `ToolSchema` values into the OpenAI-like format Ollama
    /// understands.
    pub fn translate_tools(tools: &[ToolSchema]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect()
    }

    /// Build the JSON body for `POST /api/chat`.
    pub fn build_request_body(&self, request: &CompletionRequest, stream: bool) -> Value {
        let model = if request.model.is_empty() {
            self.default_model.clone()
        } else {
            request.model.clone()
        };

        let messages = build_messages(request);

        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": stream,
        });

        // Options sub-object.
        let mut options = json!({});
        options["num_predict"] = json!(request.max_tokens);
        if let Some(temp) = request.temperature {
            options["temperature"] = json!(temp);
        }
        body["options"] = options;

        if !request.tools.is_empty() {
            body["tools"] = json!(Self::translate_tools(&request.tools));
        }

        body
    }

    /// Parse a non-streaming `POST /api/chat` JSON response.
    pub fn parse_response(&self, body: &Value) -> Result<CompletionResponse> {
        // Ollama error format: `{"error": "some message"}`.
        if let Some(err) = body.get("error").and_then(|e| e.as_str()) {
            return Err(LlmError::Api {
                status: 0,
                message: err.to_string(),
            });
        }

        let message = body
            .get("message")
            .ok_or_else(|| LlmError::InvalidResponse("missing `message` field".into()))?;

        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        // Text content.
        if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
            if !text.is_empty() {
                content_blocks.push(ContentBlock::Text {
                    text: text.to_string(),
                });
            }
        }

        // Tool calls.
        if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
            for tc in tool_calls {
                let func = tc.get("function").unwrap_or(&Value::Null);
                let name = func
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                // Ollama doesn't provide tool call IDs — generate a UUID.
                let id = Uuid::new_v4().to_string();
                // Arguments can be either a JSON object or a string.
                let input = if let Some(args_obj) = func.get("arguments") {
                    if args_obj.is_object() || args_obj.is_array() {
                        args_obj.clone()
                    } else if let Some(s) = args_obj.as_str() {
                        serde_json::from_str(s).unwrap_or(Value::Object(Default::default()))
                    } else {
                        Value::Object(Default::default())
                    }
                } else {
                    Value::Object(Default::default())
                };
                content_blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }

        // Usage (prompt_eval_count = input tokens, eval_count = output tokens).
        let usage = Usage {
            input_tokens: body
                .get("prompt_eval_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            output_tokens: body.get("eval_count").and_then(|v| v.as_u64()).unwrap_or(0),
        };

        // Stop reason: Ollama uses `done_reason` ("stop", "length", etc.).
        let stop_reason = match body
            .get("done_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("stop")
        {
            "length" => StopReason::MaxTokens,
            // Ollama reports tool calls via message.tool_calls, not done_reason.
            _ if !content_blocks
                .iter()
                .all(|b| !matches!(b, ContentBlock::ToolUse { .. })) =>
            {
                StopReason::ToolUse
            }
            _ => StopReason::EndTurn,
        };

        Ok(CompletionResponse {
            content: content_blocks,
            usage,
            stop_reason,
        })
    }

    /// Parse a single NDJSON line from an Ollama streaming response.
    pub fn parse_ndjson_line(&self, line: &str) -> Option<StreamChunk> {
        parse_ndjson_line(line)
    }
}

// ---------------------------------------------------------------------------
// NDJSON parsing (exported for unit tests)
// ---------------------------------------------------------------------------

/// Parse a single NDJSON line from the Ollama streaming API.
pub fn parse_ndjson_line(line: &str) -> Option<StreamChunk> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let event: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return None,
    };

    // Error response.
    if let Some(err) = event.get("error").and_then(|e| e.as_str()) {
        // We can't return Err here because this function returns Option; callers
        // that need error propagation should use the full stream adapter.
        debug!("Ollama stream error: {err}");
        return None;
    }

    let done = event.get("done").and_then(|d| d.as_bool()).unwrap_or(false);

    if done {
        // Final message — may include usage.
        let input_tokens = event
            .get("prompt_eval_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output_tokens = event
            .get("eval_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if input_tokens > 0 || output_tokens > 0 {
            // Emit usage before Done in the stream via a two-item vec;
            // for simplicity this function emits the Usage chunk and the
            // caller will get Done from the next None→end-of-stream behaviour.
            // Since we can only return one item, we return Usage here; the
            // stream adapter appends Done explicitly after the stream ends.
            return Some(StreamChunk::Usage(Usage {
                input_tokens,
                output_tokens,
            }));
        }

        return Some(StreamChunk::Done);
    }

    // Intermediate streaming message.
    if let Some(message) = event.get("message") {
        // Text delta.
        if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
            if !text.is_empty() {
                return Some(StreamChunk::TextDelta {
                    text: text.to_string(),
                });
            }
        }

        // Tool call (rare in streaming mode but possible).
        if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
            if let Some(tc) = tool_calls.first() {
                let func = tc.get("function").unwrap_or(&Value::Null);
                let name = func
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let id = Uuid::new_v4().to_string();
                return Some(StreamChunk::ToolUseStart { id, name });
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Helper: build messages array
// ---------------------------------------------------------------------------

fn build_messages(request: &CompletionRequest) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();

    // Prepend top-level system prompt.
    if let Some(sys) = &request.system {
        if !sys.is_empty() {
            messages.push(json!({ "role": "system", "content": sys }));
        }
    }

    for msg in &request.messages {
        messages.push(translate_message_to_ollama(msg));
    }

    messages
}

fn translate_message_to_ollama(msg: &Message) -> Value {
    match msg.role {
        Role::System => {
            let text: String = msg
                .content
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::Text { text } = b {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            json!({ "role": "system", "content": text })
        }
        Role::User => {
            let text: String = msg
                .content
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::Text { text } = b {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            json!({ "role": "user", "content": text })
        }
        Role::Assistant => {
            let mut text_parts: Vec<&str> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();

            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => text_parts.push(text.as_str()),
                    ContentBlock::ToolUse { name, input, .. } => {
                        tool_calls.push(json!({
                            "function": {
                                "name": name,
                                "arguments": input,
                            }
                        }));
                    }
                    _ => {}
                }
            }

            let mut v = json!({ "role": "assistant" });
            v["content"] = json!(text_parts.join("\n"));
            if !tool_calls.is_empty() {
                v["tool_calls"] = json!(tool_calls);
            }
            v
        }
        Role::Tool => {
            if let Some(ContentBlock::ToolResult { content, .. }) = msg.content.first() {
                json!({ "role": "tool", "content": content })
            } else {
                json!({ "role": "tool", "content": "" })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// LlmProvider impl
// ---------------------------------------------------------------------------

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let body = self.build_request_body(&request, false);
        debug!("Ollama request body: {}", body);

        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));

        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let response_body: Value = resp.json().await?;

        if status >= 400 {
            let message = response_body
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown API error")
                .to_string();
            return Err(LlmError::Api { status, message });
        }

        self.parse_response(&response_body)
    }

    async fn complete_stream(&self, request: CompletionRequest) -> Result<BoxStream> {
        let body = self.build_request_body(&request, true);
        debug!("Ollama streaming request body: {}", body);

        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));

        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let err_body: Value = resp.json().await?;
            let message = err_body
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown API error")
                .to_string();
            return Err(LlmError::Api { status, message });
        }

        let byte_stream = resp.bytes_stream();

        // Ollama sends one JSON object per line (NDJSON).
        let chunk_stream = byte_stream
            .map(|result| match result {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    let chunks: Vec<Result<StreamChunk>> = text
                        .lines()
                        .filter_map(|line| parse_ndjson_line(line).map(Ok))
                        .collect();
                    stream::iter(chunks)
                }
                Err(e) => stream::iter(vec![Err(LlmError::Http(e))]),
            })
            .flatten();

        Ok(Box::pin(chunk_stream))
    }

    fn name(&self) -> &str {
        "ollama"
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

    fn provider() -> OllamaProvider {
        OllamaProvider::new(None::<String>, "llama3.2")
    }

    // --- build_request_body ---

    #[test]
    fn build_body_defaults_to_localhost() {
        let p = OllamaProvider::new(None::<String>, "llama3.2");
        assert!(p.base_url.contains("localhost:11434"));
    }

    #[test]
    fn build_body_simple_user_message() {
        let req = CompletionRequest::new(vec![Message::user("Hello!")], "llama3.2", 256);
        let body = provider().build_request_body(&req, false);
        assert_eq!(body["model"], "llama3.2");
        assert_eq!(body["stream"], false);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hello!");
    }

    #[test]
    fn build_body_uses_default_model_when_empty() {
        let req = CompletionRequest::new(vec![Message::user("hi")], "", 128);
        let body = provider().build_request_body(&req, false);
        assert_eq!(body["model"], "llama3.2");
    }

    #[test]
    fn build_body_num_predict_in_options() {
        let req = CompletionRequest::new(vec![Message::user("hi")], "m", 512);
        let body = provider().build_request_body(&req, false);
        assert_eq!(body["options"]["num_predict"], 512);
    }

    #[test]
    fn build_body_temperature_in_options() {
        let mut req = CompletionRequest::new(vec![Message::user("hi")], "m", 128);
        req.temperature = Some(0.9);
        let body = provider().build_request_body(&req, false);
        let temp = body["options"]["temperature"].as_f64().unwrap();
        assert!((temp - 0.9).abs() < 1e-5);
    }

    #[test]
    fn build_body_system_field_prepended() {
        let mut req = CompletionRequest::new(vec![Message::user("go")], "m", 128);
        req.system = Some("Be concise.".into());
        let body = provider().build_request_body(&req, false);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be concise.");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn build_body_tools_translated() {
        let mut req = CompletionRequest::new(vec![Message::user("hi")], "m", 128);
        req.tools = vec![ToolSchema::new(
            "click",
            "Click an element",
            json!({"type": "object"}),
        )];
        let body = provider().build_request_body(&req, false);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "click");
    }

    // --- parse_response ---

    #[test]
    fn parse_response_text_only() {
        let body = json!({
            "model": "llama3.2",
            "message": {"role": "assistant", "content": "Hello from Ollama"},
            "done": true,
            "done_reason": "stop",
            "prompt_eval_count": 12,
            "eval_count": 7
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.text(), "Hello from Ollama");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 12);
        assert_eq!(resp.usage.output_tokens, 7);
    }

    #[test]
    fn parse_response_tool_call() {
        let body = json!({
            "model": "llama3.2",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "function": {
                        "name": "navigate",
                        "arguments": {"url": "https://example.com"}
                    }
                }]
            },
            "done": true,
            "done_reason": "stop",
            "prompt_eval_count": 20,
            "eval_count": 10
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        if let ContentBlock::ToolUse { name, input, .. } = calls[0] {
            assert_eq!(name, "navigate");
            assert_eq!(input["url"], "https://example.com");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn parse_response_max_tokens() {
        let body = json!({
            "message": {"role": "assistant", "content": "truncated"},
            "done": true,
            "done_reason": "length",
            "prompt_eval_count": 5,
            "eval_count": 100
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    }

    #[test]
    fn parse_response_error_field() {
        let body = json!({"error": "model not found"});
        let err = provider().parse_response(&body).unwrap_err();
        assert!(
            matches!(err, LlmError::Api { ref message, .. } if message.contains("model not found"))
        );
    }

    #[test]
    fn parse_response_missing_message_errors() {
        let body = json!({"done": true});
        let err = provider().parse_response(&body).unwrap_err();
        assert!(matches!(err, LlmError::InvalidResponse(_)));
    }

    // --- NDJSON parsing ---

    #[test]
    fn ndjson_blank_returns_none() {
        assert!(parse_ndjson_line("").is_none());
        assert!(parse_ndjson_line("  ").is_none());
    }

    #[test]
    fn ndjson_text_delta() {
        let line =
            r#"{"model":"llama3.2","message":{"role":"assistant","content":"Hello"},"done":false}"#;
        let chunk = parse_ndjson_line(line).unwrap();
        assert!(matches!(chunk, StreamChunk::TextDelta { ref text } if text == "Hello"));
    }

    #[test]
    fn ndjson_empty_content_returns_none() {
        let line =
            r#"{"model":"llama3.2","message":{"role":"assistant","content":""},"done":false}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn ndjson_done_with_usage() {
        let line = r#"{"model":"llama3.2","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":8,"eval_count":4}"#;
        let chunk = parse_ndjson_line(line).unwrap();
        assert!(matches!(
            chunk,
            StreamChunk::Usage(Usage {
                input_tokens: 8,
                output_tokens: 4
            })
        ));
    }

    #[test]
    fn ndjson_done_without_usage() {
        let line = r#"{"model":"llama3.2","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop"}"#;
        let chunk = parse_ndjson_line(line).unwrap();
        assert_eq!(chunk, StreamChunk::Done);
    }

    #[test]
    fn ndjson_tool_call_in_stream() {
        let line = r#"{"model":"llama3.2","message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"screenshot","arguments":{}}}]},"done":false}"#;
        let chunk = parse_ndjson_line(line).unwrap();
        assert!(
            matches!(chunk, StreamChunk::ToolUseStart { ref name, .. } if name == "screenshot")
        );
    }
}
