//! OpenAI-compatible backend for the LLM provider abstraction.
//!
//! Works with any API that speaks the OpenAI Chat Completions format, including
//! OpenAI itself, Azure OpenAI, local servers (LM Studio, vLLM, etc.).

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

/// OpenAI-compatible provider (supports any Chat Completions endpoint).
pub struct OpenAiCompatProvider {
    api_key: String,
    base_url: String,
    default_model: String,
    client: reqwest::Client,
}

impl OpenAiCompatProvider {
    /// Create a new `OpenAiCompatProvider`.
    ///
    /// `base_url` is the root of the API (e.g. `"https://api.openai.com/v1"`).
    pub fn new(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: base_url.into(),
            default_model: default_model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Translate `ToolSchema` values into OpenAI function-calling format.
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

    /// Translate a `CompletionRequest` into the JSON body expected by the
    /// OpenAI Chat Completions API.
    pub fn build_request_body(&self, request: &CompletionRequest) -> Value {
        let model = if request.model.is_empty() {
            self.default_model.clone()
        } else {
            request.model.clone()
        };

        let mut messages: Vec<Value> = Vec::new();

        // Prepend top-level system field as a system message.
        if let Some(sys) = &request.system {
            if !sys.is_empty() {
                messages.push(json!({ "role": "system", "content": sys }));
            }
        }

        for msg in &request.messages {
            messages.push(translate_message_to_openai(msg));
        }

        let mut body = json!({
            "model": model,
            "max_tokens": request.max_tokens,
            "messages": messages,
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }

        if !request.tools.is_empty() {
            body["tools"] = json!(Self::translate_tools(&request.tools));
            body["tool_choice"] = json!("auto");
        }

        body
    }

    /// Parse the OpenAI Chat Completions response JSON into a
    /// `CompletionResponse`.
    pub fn parse_response(&self, body: &Value) -> Result<CompletionResponse> {
        // Error response (OpenAI wraps errors in `{"error": {...}}`).
        if let Some(err) = body.get("error") {
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
                .to_string();
            return Err(LlmError::Api { status: 0, message });
        }

        let choices = body
            .get("choices")
            .and_then(|c| c.as_array())
            .ok_or_else(|| LlmError::InvalidResponse("missing `choices` array".into()))?;

        let choice = choices
            .first()
            .ok_or_else(|| LlmError::InvalidResponse("empty `choices` array".into()))?;

        let message = choice
            .get("message")
            .ok_or_else(|| LlmError::InvalidResponse("missing `message` in choice".into()))?;

        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        // Plain text content.
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
                let id = tc
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let func = tc.get("function").unwrap_or(&Value::Null);
                let name = func
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let args_str = func
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let input: Value =
                    serde_json::from_str(args_str).unwrap_or(Value::Object(Default::default()));
                content_blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }

        // Usage.
        let usage = body.get("usage").map_or(Usage::default(), |u| Usage {
            input_tokens: u
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            output_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        });

        // Stop reason.
        let stop_reason = match choice
            .get("finish_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("stop")
        {
            "tool_calls" => StopReason::ToolUse,
            "length" => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };

        Ok(CompletionResponse {
            content: content_blocks,
            usage,
            stop_reason,
        })
    }

    /// Parse a single SSE data line in OpenAI streaming format.
    ///
    /// Returns `None` for lines that should be ignored (blank, `[DONE]`, etc.).
    pub fn parse_sse_line(&self, line: &str) -> Option<StreamChunk> {
        parse_sse_line(line)
    }
}

// ---------------------------------------------------------------------------
// SSE parsing (exported for unit tests)
// ---------------------------------------------------------------------------

/// Parse a single raw SSE data line in OpenAI format.
pub fn parse_sse_line(line: &str) -> Option<StreamChunk> {
    if !line.starts_with("data: ") {
        return None;
    }

    let data = &line["data: ".len()..];

    if data.trim() == "[DONE]" {
        return Some(StreamChunk::Done);
    }

    let event: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return None,
    };

    // Check for error event.
    if let Some(err) = event.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown streaming error")
            .to_string();
        warn!("OpenAI stream error: {msg}");
        return None;
    }

    // Usage (sometimes sent in the final chunk, which may have empty choices).
    if let Some(usage) = event.get("usage") {
        let input_tokens = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output_tokens = usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if input_tokens > 0 || output_tokens > 0 {
            return Some(StreamChunk::Usage(Usage {
                input_tokens,
                output_tokens,
            }));
        }
    }

    let choices = event.get("choices").and_then(|c| c.as_array())?;
    let choice = choices.first()?;
    let delta = choice.get("delta")?;

    // Text delta.
    if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
        if !text.is_empty() {
            return Some(StreamChunk::TextDelta {
                text: text.to_string(),
            });
        }
    }

    // Tool call delta.
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
        if let Some(tc) = tool_calls.first() {
            let func = tc.get("function").unwrap_or(&Value::Null);

            // Tool use start — carries id and name.
            if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                let name = func
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                return Some(StreamChunk::ToolUseStart {
                    id: id.to_string(),
                    name,
                });
            }

            // Subsequent deltas carry argument fragments.
            if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                if !args.is_empty() {
                    return Some(StreamChunk::ToolUseInputDelta {
                        input_delta: args.to_string(),
                    });
                }
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Helper: translate our message types → OpenAI format
// ---------------------------------------------------------------------------

fn translate_message_to_openai(msg: &Message) -> Value {
    match msg.role {
        Role::System => {
            // Collect text from system messages.
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
            // User messages should only have text (tool results are handled below).
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
            // May contain text and/or tool_calls.
            let mut text_parts: Vec<&str> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();

            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => text_parts.push(text.as_str()),
                    ContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(input).unwrap_or_default(),
                            }
                        }));
                    }
                    _ => {}
                }
            }

            let mut v = json!({ "role": "assistant" });
            if !text_parts.is_empty() {
                v["content"] = json!(text_parts.join("\n"));
            } else {
                v["content"] = json!(null);
            }
            if !tool_calls.is_empty() {
                v["tool_calls"] = json!(tool_calls);
            }
            v
        }

        Role::Tool => {
            // Each ToolResult block becomes a separate "tool" role message.
            // If there are multiple blocks, we emit the first one here; callers
            // should split multi-result messages before calling this function if
            // they need per-call granularity. In practice messages contain one.
            if let Some(ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            }) = msg.content.first()
            {
                json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": content,
                })
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
impl LlmProvider for OpenAiCompatProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let body = self.build_request_body(&request);
        debug!("OpenAI request body: {}", body);

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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
        // Request usage to be included in the last chunk.
        body["stream_options"] = json!({ "include_usage": true });

        debug!("OpenAI streaming request body: {}", body);

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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
        "openai-compat"
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

    fn provider() -> OpenAiCompatProvider {
        OpenAiCompatProvider::new(
            "test-key",
            "https://api.openai.com/v1",
            "gpt-4o",
        )
    }

    // --- build_request_body ---

    #[test]
    fn build_body_simple_user_message() {
        let req = CompletionRequest::new(vec![Message::user("Hello!")], "gpt-4o", 1024);
        let body = provider().build_request_body(&req);
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["max_tokens"], 1024);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hello!");
    }

    #[test]
    fn build_body_uses_default_model_when_empty() {
        let req = CompletionRequest::new(vec![Message::user("hi")], "", 512);
        let body = provider().build_request_body(&req);
        assert_eq!(body["model"], "gpt-4o");
    }

    #[test]
    fn build_body_system_field_prepended_as_message() {
        let mut req = CompletionRequest::new(vec![Message::user("go")], "m", 512);
        req.system = Some("You are helpful.".into());
        let body = provider().build_request_body(&req);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful.");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn build_body_system_role_message_kept_as_system() {
        let req = CompletionRequest::new(
            vec![Message::system("Be brief."), Message::user("hi")],
            "m",
            512,
        );
        let body = provider().build_request_body(&req);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be brief.");
    }

    #[test]
    fn build_body_tool_result_becomes_tool_role() {
        let req = CompletionRequest::new(
            vec![Message::tool_result("call-1", "tool output")],
            "m",
            512,
        );
        let body = provider().build_request_body(&req);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["tool_call_id"], "call-1");
        assert_eq!(msgs[0]["content"], "tool output");
    }

    #[test]
    fn build_body_assistant_with_tool_calls() {
        let req = CompletionRequest::new(
            vec![Message::assistant(vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "click".into(),
                input: json!({"selector": "#btn"}),
            }])],
            "m",
            512,
        );
        let body = provider().build_request_body(&req);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "assistant");
        let tool_calls = msgs[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls[0]["id"], "c1");
        assert_eq!(tool_calls[0]["type"], "function");
        assert_eq!(tool_calls[0]["function"]["name"], "click");
    }

    #[test]
    fn build_body_tools_translated() {
        let mut req = CompletionRequest::new(vec![Message::user("hi")], "m", 512);
        req.tools = vec![ToolSchema::new(
            "navigate",
            "Navigate to URL",
            json!({"type": "object", "properties": {"url": {"type": "string"}}}),
        )];
        let body = provider().build_request_body(&req);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "navigate");
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn build_body_temperature_included() {
        let mut req = CompletionRequest::new(vec![Message::user("hi")], "m", 512);
        req.temperature = Some(0.5);
        let body = provider().build_request_body(&req);
        let temp = body["temperature"].as_f64().unwrap();
        assert!((temp - 0.5).abs() < 1e-5);
    }

    // --- parse_response ---

    #[test]
    fn parse_response_text_only() {
        let body = json!({
            "choices": [{
                "message": {"role": "assistant", "content": "Hi there!"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.text(), "Hi there!");
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[test]
    fn parse_response_tool_calls() {
        let body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "screenshot",
                            "arguments": "{\"full_page\": true}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 10, "total_tokens": 30}
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        if let ContentBlock::ToolUse { id, name, input } = calls[0] {
            assert_eq!(id, "call_abc");
            assert_eq!(name, "screenshot");
            assert_eq!(input["full_page"], true);
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn parse_response_max_tokens() {
        let body = json!({
            "choices": [{
                "message": {"role": "assistant", "content": "cut off"},
                "finish_reason": "length"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 50, "total_tokens": 55}
        });
        let resp = provider().parse_response(&body).unwrap();
        assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    }

    #[test]
    fn parse_response_missing_choices_errors() {
        let body = json!({"model": "gpt-4o"});
        let err = provider().parse_response(&body).unwrap_err();
        assert!(matches!(err, LlmError::InvalidResponse(_)));
    }

    #[test]
    fn parse_response_api_error_body() {
        let body = json!({
            "error": {"message": "model not found", "type": "invalid_request_error"}
        });
        let err = provider().parse_response(&body).unwrap_err();
        assert!(matches!(err, LlmError::Api { message, .. } if message.contains("model not found")));
    }

    // --- SSE parsing ---

    #[test]
    fn sse_done_signal() {
        assert_eq!(parse_sse_line("data: [DONE]"), Some(StreamChunk::Done));
    }

    #[test]
    fn sse_blank_and_comment_return_none() {
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line(": ping").is_none());
        assert!(parse_sse_line("event: something").is_none());
    }

    #[test]
    fn sse_text_delta() {
        let line = r#"data: {"id":"chatcmpl-1","choices":[{"delta":{"content":"Hello"},"finish_reason":null,"index":0}]}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(matches!(chunk, StreamChunk::TextDelta { ref text } if text == "Hello"));
    }

    #[test]
    fn sse_empty_content_delta_returns_none() {
        let line = r#"data: {"id":"x","choices":[{"delta":{"content":""},"finish_reason":null,"index":0}]}"#;
        assert!(parse_sse_line(line).is_none());
    }

    #[test]
    fn sse_tool_use_start() {
        let line = r#"data: {"id":"x","choices":[{"delta":{"tool_calls":[{"id":"call_1","type":"function","function":{"name":"click","arguments":""}}]},"finish_reason":null,"index":0}]}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(
            matches!(chunk, StreamChunk::ToolUseStart { ref id, ref name } if id == "call_1" && name == "click")
        );
    }

    #[test]
    fn sse_tool_input_delta() {
        let line = r#"data: {"id":"x","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"url\":"}}]},"finish_reason":null,"index":0}]}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(
            matches!(chunk, StreamChunk::ToolUseInputDelta { ref input_delta } if input_delta.contains("url"))
        );
    }

    #[test]
    fn sse_usage_in_final_chunk() {
        let line = r#"data: {"id":"x","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20}}"#;
        let chunk = parse_sse_line(line).unwrap();
        assert!(
            matches!(chunk, StreamChunk::Usage(Usage { input_tokens: 10, output_tokens: 20 }))
        );
    }
}
