//! Core LLM types: messages, content blocks, requests, responses, and streaming chunks.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

/// The role of a participant in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

// ---------------------------------------------------------------------------
// ContentBlock
// ---------------------------------------------------------------------------

/// A single piece of content within a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text { text: String },

    /// A request to call a tool (produced by the model).
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// The result of a tool call (sent back to the model).
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

/// A single message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Create a system message with plain text.
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// Create a user message with plain text.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// Create an assistant message with plain text.
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// Create an assistant message with arbitrary content blocks.
    pub fn assistant(content: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error: None,
            }],
        }
    }
}

// ---------------------------------------------------------------------------
// ToolSchema
// ---------------------------------------------------------------------------

/// JSON-schema description of a tool that can be called by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl ToolSchema {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

// ---------------------------------------------------------------------------
// CompletionRequest
// ---------------------------------------------------------------------------

/// A request to generate a completion from an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub model: String,
}

impl CompletionRequest {
    pub fn new(messages: Vec<Message>, model: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            messages,
            tools: Vec::new(),
            system: None,
            max_tokens,
            temperature: None,
            model: model.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Usage / StopReason
// ---------------------------------------------------------------------------

/// Token usage reported by the model.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Reason the model stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}

// ---------------------------------------------------------------------------
// CompletionResponse
// ---------------------------------------------------------------------------

/// A completed (non-streaming) response from the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: Vec<ContentBlock>,
    pub usage: Usage,
    pub stop_reason: StopReason,
}

impl CompletionResponse {
    /// Extract all text blocks concatenated.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| {
                if let ContentBlock::Text { text } = b {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Return all tool-use blocks.
    pub fn tool_calls(&self) -> Vec<&ContentBlock> {
        self.content
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// StreamChunk
// ---------------------------------------------------------------------------

/// A single event emitted during a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum StreamChunk {
    /// Incremental text from the model.
    TextDelta { text: String },

    /// The model is beginning a tool call.
    ToolUseStart { id: String, name: String },

    /// Incremental JSON input fragment for the current tool call.
    ToolUseInputDelta { input_delta: String },

    /// Token usage for the entire response.
    Usage(Usage),

    /// Stream finished.
    Done,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- Role ---

    #[test]
    fn role_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), r#""system""#);
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), r#""user""#);
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            r#""assistant""#
        );
        assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), r#""tool""#);
    }

    #[test]
    fn role_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let decoded: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, role);
        }
    }

    // --- ContentBlock ---

    #[test]
    fn content_block_text_serialization() {
        let block = ContentBlock::Text {
            text: "hello".to_string(),
        };
        let v: Value = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "text");
        assert_eq!(v["text"], "hello");
    }

    #[test]
    fn content_block_tool_use_serialization() {
        let block = ContentBlock::ToolUse {
            id: "call_1".to_string(),
            name: "navigate".to_string(),
            input: json!({"url": "https://example.com"}),
        };
        let v: Value = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_use");
        assert_eq!(v["id"], "call_1");
        assert_eq!(v["name"], "navigate");
        assert_eq!(v["input"]["url"], "https://example.com");
    }

    #[test]
    fn content_block_tool_result_serialization() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "call_1".to_string(),
            content: "ok".to_string(),
            is_error: None,
        };
        let v: Value = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "tool_result");
        assert_eq!(v["tool_use_id"], "call_1");
        // is_error=None should be omitted
        assert!(v.get("is_error").is_none());
    }

    #[test]
    fn content_block_tool_result_with_error_flag() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "call_1".to_string(),
            content: "boom".to_string(),
            is_error: Some(true),
        };
        let v: Value = serde_json::to_value(&block).unwrap();
        assert_eq!(v["is_error"], true);
    }

    #[test]
    fn content_block_roundtrip() {
        let blocks = vec![
            ContentBlock::Text {
                text: "hi".to_string(),
            },
            ContentBlock::ToolUse {
                id: "x".to_string(),
                name: "click".to_string(),
                input: json!({"selector": "#btn"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "x".to_string(),
                content: "clicked".to_string(),
                is_error: Some(false),
            },
        ];
        for block in blocks {
            let json = serde_json::to_string(&block).unwrap();
            let decoded: ContentBlock = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, block);
        }
    }

    // --- Message constructors ---

    #[test]
    fn message_system_constructor() {
        let m = Message::system("You are helpful.");
        assert_eq!(m.role, Role::System);
        assert_eq!(m.content.len(), 1);
        assert!(matches!(&m.content[0], ContentBlock::Text { text } if text == "You are helpful."));
    }

    #[test]
    fn message_user_constructor() {
        let m = Message::user("Hello");
        assert_eq!(m.role, Role::User);
        assert!(matches!(&m.content[0], ContentBlock::Text { text } if text == "Hello"));
    }

    #[test]
    fn message_assistant_text_constructor() {
        let m = Message::assistant_text("Sure!");
        assert_eq!(m.role, Role::Assistant);
        assert!(matches!(&m.content[0], ContentBlock::Text { text } if text == "Sure!"));
    }

    #[test]
    fn message_assistant_with_tool_use() {
        let blocks = vec![
            ContentBlock::Text {
                text: "Let me check.".to_string(),
            },
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "screenshot".to_string(),
                input: json!({}),
            },
        ];
        let m = Message::assistant(blocks.clone());
        assert_eq!(m.role, Role::Assistant);
        assert_eq!(m.content, blocks);
    }

    #[test]
    fn message_tool_result_constructor() {
        let m = Message::tool_result("t1", "result data");
        assert_eq!(m.role, Role::Tool);
        assert!(
            matches!(&m.content[0], ContentBlock::ToolResult { tool_use_id, content, is_error }
                if tool_use_id == "t1" && content == "result data" && is_error.is_none()
            )
        );
    }

    #[test]
    fn message_roundtrip() {
        let m = Message::user("roundtrip test");
        let json = serde_json::to_string(&m).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, m);
    }

    // --- ToolSchema ---

    #[test]
    fn tool_schema_serialization() {
        let ts = ToolSchema::new(
            "navigate",
            "Navigate to a URL",
            json!({"type": "object", "properties": {"url": {"type": "string"}}}),
        );
        let v: Value = serde_json::to_value(&ts).unwrap();
        assert_eq!(v["name"], "navigate");
        assert_eq!(v["description"], "Navigate to a URL");
        assert_eq!(v["input_schema"]["type"], "object");
    }

    #[test]
    fn tool_schema_roundtrip() {
        let ts = ToolSchema::new("click", "Click an element", json!({}));
        let json = serde_json::to_string(&ts).unwrap();
        let decoded: ToolSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, ts);
    }

    // --- CompletionRequest ---

    #[test]
    fn completion_request_minimal() {
        let req = CompletionRequest::new(
            vec![Message::user("hi")],
            "claude-opus-4-5",
            1024,
        );
        let v: Value = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "claude-opus-4-5");
        assert_eq!(v["max_tokens"], 1024);
        // tools and system should be absent
        assert!(v.get("system").is_none());
        // empty tools vec is skipped
        assert!(v.get("tools").is_none());
    }

    #[test]
    fn completion_request_with_tools_and_system() {
        let mut req = CompletionRequest::new(vec![Message::user("go")], "m", 512);
        req.system = Some("Be concise.".to_string());
        req.tools = vec![ToolSchema::new("click", "Click", json!({}))];
        let v: Value = serde_json::to_value(&req).unwrap();
        assert_eq!(v["system"], "Be concise.");
        assert_eq!(v["tools"][0]["name"], "click");
    }

    // --- Usage ---

    #[test]
    fn usage_default_is_zero() {
        let u = Usage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
    }

    #[test]
    fn usage_roundtrip() {
        let u = Usage {
            input_tokens: 100,
            output_tokens: 200,
        };
        let json = serde_json::to_string(&u).unwrap();
        let decoded: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, u);
    }

    // --- StopReason ---

    #[test]
    fn stop_reason_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&StopReason::EndTurn).unwrap(),
            r#""end_turn""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::ToolUse).unwrap(),
            r#""tool_use""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::MaxTokens).unwrap(),
            r#""max_tokens""#
        );
    }

    #[test]
    fn stop_reason_roundtrip() {
        for sr in [StopReason::EndTurn, StopReason::ToolUse, StopReason::MaxTokens] {
            let json = serde_json::to_string(&sr).unwrap();
            let decoded: StopReason = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, sr);
        }
    }

    // --- CompletionResponse ---

    #[test]
    fn completion_response_text_extraction() {
        let resp = CompletionResponse {
            content: vec![
                ContentBlock::Text {
                    text: "Hello".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "t".to_string(),
                    name: "click".to_string(),
                    input: json!({}),
                },
                ContentBlock::Text {
                    text: " world".to_string(),
                },
            ],
            usage: Usage::default(),
            stop_reason: StopReason::EndTurn,
        };
        assert_eq!(resp.text(), "Hello world");
        assert_eq!(resp.tool_calls().len(), 1);
    }

    #[test]
    fn completion_response_empty_content() {
        let resp = CompletionResponse {
            content: vec![],
            usage: Usage::default(),
            stop_reason: StopReason::MaxTokens,
        };
        assert_eq!(resp.text(), "");
        assert!(resp.tool_calls().is_empty());
    }

    // --- StreamChunk ---

    #[test]
    fn stream_chunk_text_delta() {
        let c = StreamChunk::TextDelta {
            text: "delta".to_string(),
        };
        let v: Value = serde_json::to_value(&c).unwrap();
        assert_eq!(v["type"], "text_delta");
        assert_eq!(v["text"], "delta");
    }

    #[test]
    fn stream_chunk_tool_use_start() {
        let c = StreamChunk::ToolUseStart {
            id: "id1".to_string(),
            name: "navigate".to_string(),
        };
        let v: Value = serde_json::to_value(&c).unwrap();
        assert_eq!(v["type"], "tool_use_start");
        assert_eq!(v["id"], "id1");
    }

    #[test]
    fn stream_chunk_tool_use_input_delta() {
        let c = StreamChunk::ToolUseInputDelta {
            input_delta: r#"{"url":"#.to_string(),
        };
        let v: Value = serde_json::to_value(&c).unwrap();
        assert_eq!(v["type"], "tool_use_input_delta");
    }

    #[test]
    fn stream_chunk_usage() {
        let c = StreamChunk::Usage(Usage {
            input_tokens: 5,
            output_tokens: 10,
        });
        let json = serde_json::to_string(&c).unwrap();
        let decoded: StreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, c);
    }

    #[test]
    fn stream_chunk_done() {
        let c = StreamChunk::Done;
        let v: Value = serde_json::to_value(&c).unwrap();
        assert_eq!(v["type"], "done");
        let decoded: StreamChunk = serde_json::from_value(v).unwrap();
        assert_eq!(decoded, StreamChunk::Done);
    }

    #[test]
    fn stream_chunk_roundtrip_all_variants() {
        let chunks = vec![
            StreamChunk::TextDelta {
                text: "hi".to_string(),
            },
            StreamChunk::ToolUseStart {
                id: "a".to_string(),
                name: "b".to_string(),
            },
            StreamChunk::ToolUseInputDelta {
                input_delta: "{}".to_string(),
            },
            StreamChunk::Usage(Usage {
                input_tokens: 1,
                output_tokens: 2,
            }),
            StreamChunk::Done,
        ];
        for chunk in chunks {
            let json = serde_json::to_string(&chunk).unwrap();
            let decoded: StreamChunk = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, chunk);
        }
    }
}
