//! Context management — truncate tool results and compact message history
//! to keep token usage efficient across multi-step agent runs.

use pagerunner_llm::{ContentBlock, Message, Role};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Controls how the agent manages conversation context size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Max characters for a single tool result before truncation (0 = no limit).
    /// Default: 4000 (~1000 tokens). The full result is still emitted via
    /// AgentEvent::ToolResult — only the LLM's copy is truncated.
    #[serde(default = "default_max_result_chars")]
    pub max_result_chars: usize,

    /// Max total characters across all messages before compaction (0 = no limit).
    /// When exceeded, older tool results are replaced with summaries.
    /// Default: 24000 (~6000 tokens).
    #[serde(default = "default_max_context_chars")]
    pub max_context_chars: usize,

    /// Number of recent message pairs to always keep uncompacted.
    /// Default: 4 (keeps the last 4 assistant+tool exchanges intact).
    #[serde(default = "default_keep_recent")]
    pub keep_recent: usize,
}

fn default_max_result_chars() -> usize {
    4000
}

fn default_max_context_chars() -> usize {
    24000
}

fn default_keep_recent() -> usize {
    4
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_result_chars: default_max_result_chars(),
            max_context_chars: default_max_context_chars(),
            keep_recent: default_keep_recent(),
        }
    }
}

// ---------------------------------------------------------------------------
// Truncation
// ---------------------------------------------------------------------------

/// Truncate a tool result string if it exceeds the configured limit.
/// Appends a note about the truncation so the LLM knows content was cut.
pub fn truncate_result(result: &str, max_chars: usize) -> String {
    if max_chars == 0 || result.len() <= max_chars {
        return result.to_string();
    }

    // Find a clean break point — prefer line boundary
    let cut = result[..max_chars]
        .rfind('\n')
        .unwrap_or(max_chars);
    let cut = cut.max(max_chars / 2); // don't cut more than half

    let truncated = &result[..cut];
    let remaining = result.len() - cut;
    format!(
        "{truncated}\n\n[... truncated — {remaining} more characters. \
         Use get_content or scroll for more detail.]"
    )
}

// ---------------------------------------------------------------------------
// Context compaction
// ---------------------------------------------------------------------------

/// Estimate character count across all messages.
fn estimate_context_size(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| {
            m.content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => text.len(),
                    ContentBlock::ToolUse { input, .. } => {
                        input.to_string().len() + 50 // name + id overhead
                    }
                    ContentBlock::ToolResult { content, .. } => content.len() + 30,
                })
                .sum::<usize>()
        })
        .sum()
}

/// Compact message history by replacing old tool results with short summaries.
///
/// Keeps the first message (user goal) and the last `keep_recent` message
/// pairs intact. Everything in between has its tool results replaced with
/// "[Previous: tool_name returned N chars — see above for context]".
pub fn compact_messages(messages: &mut Vec<Message>, config: &ContextConfig) {
    if config.max_context_chars == 0 {
        return;
    }

    let size = estimate_context_size(messages);
    if size <= config.max_context_chars {
        return;
    }

    let len = messages.len();
    if len <= 1 + config.keep_recent * 2 {
        return; // not enough messages to compact
    }

    // Protected range: first message + last keep_recent*2 messages
    let compact_end = len.saturating_sub(config.keep_recent * 2);

    for msg in messages[1..compact_end].iter_mut() {
        if msg.role == Role::Tool {
            for block in msg.content.iter_mut() {
                if let ContentBlock::ToolResult {
                    content, is_error, ..
                } = block
                {
                    if content.len() > 200 {
                        let orig_len = content.len();
                        let status = if *is_error == Some(true) {
                            "error"
                        } else {
                            "ok"
                        };
                        // Keep the first line as a hint
                        let first_line = content.lines().next().unwrap_or("");
                        let preview = if first_line.len() > 100 {
                            &first_line[..100]
                        } else {
                            first_line
                        };
                        *content = format!(
                            "[Compacted: {status}, {orig_len} chars. Preview: {preview}...]"
                        );
                    }
                }
            }
        }

        // Also compact long assistant thinking text from old turns
        if msg.role == Role::Assistant {
            for block in msg.content.iter_mut() {
                if let ContentBlock::Text { text } = block {
                    if text.len() > 300 {
                        let first_sentence = text
                            .find(". ")
                            .map(|i| &text[..=i])
                            .unwrap_or(&text[..100.min(text.len())]);
                        *text = format!("{first_sentence} [... compacted]");
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use pagerunner_llm::ContentBlock;

    #[test]
    fn truncate_result_short_passes_through() {
        let r = truncate_result("hello world", 100);
        assert_eq!(r, "hello world");
    }

    #[test]
    fn truncate_result_disabled_when_zero() {
        let long = "x".repeat(10000);
        let r = truncate_result(&long, 0);
        assert_eq!(r.len(), 10000);
    }

    #[test]
    fn truncate_result_cuts_at_line_boundary() {
        let text = "line one\nline two\nline three\nline four\nline five";
        let r = truncate_result(text, 30);
        assert!(r.contains("line one"));
        assert!(r.contains("truncated"));
        assert!(r.contains("more characters"));
    }

    #[test]
    fn truncate_result_preserves_at_least_half() {
        let text = "a".repeat(100);
        let r = truncate_result(&text, 60);
        // Should keep at least 30 chars (half of 60)
        let content_before_note = r.split("\n\n[...").next().unwrap();
        assert!(content_before_note.len() >= 30);
    }

    #[test]
    fn truncate_result_exact_limit_passes_through() {
        let text = "x".repeat(100);
        let r = truncate_result(&text, 100);
        assert_eq!(r, text);
    }

    #[test]
    fn context_config_defaults() {
        let c = ContextConfig::default();
        assert_eq!(c.max_result_chars, 4000);
        assert_eq!(c.max_context_chars, 24000);
        assert_eq!(c.keep_recent, 4);
    }

    #[test]
    fn context_config_from_toml() {
        let toml = r#"
max_result_chars = 2000
max_context_chars = 12000
keep_recent = 2
"#;
        let c: ContextConfig = toml::from_str(toml).unwrap();
        assert_eq!(c.max_result_chars, 2000);
        assert_eq!(c.max_context_chars, 12000);
        assert_eq!(c.keep_recent, 2);
    }

    #[test]
    fn estimate_context_size_counts_content() {
        let messages = vec![
            Message::user("hello"),
            Message::tool_result("tu-1", "x".repeat(500).as_str()),
        ];
        let size = estimate_context_size(&messages);
        assert!(size >= 505); // "hello" + 500 + overhead
    }

    #[test]
    fn compact_messages_noop_when_small() {
        let mut messages = vec![
            Message::user("goal"),
            Message::tool_result("tu-1", "small result"),
        ];
        let config = ContextConfig::default();
        let before = messages.len();
        compact_messages(&mut messages, &config);
        assert_eq!(messages.len(), before); // unchanged
    }

    #[test]
    fn compact_messages_noop_when_disabled() {
        let big_result = "x".repeat(50000);
        let mut messages = vec![
            Message::user("goal"),
            Message::tool_result("tu-1", &big_result),
            Message::tool_result("tu-2", &big_result),
        ];
        let config = ContextConfig {
            max_context_chars: 0,
            ..Default::default()
        };
        compact_messages(&mut messages, &config);
        // Content should be unchanged
        if let ContentBlock::ToolResult { content, .. } = &messages[1].content[0] {
            assert_eq!(content.len(), 50000);
        }
    }

    #[test]
    fn compact_messages_replaces_old_tool_results() {
        let big = "x".repeat(5000);
        let mut messages = vec![
            Message::user("goal"),
            // Old messages (should be compacted)
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "tu-1".into(),
                name: "get_content".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_result("tu-1", &big),
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "tu-2".into(),
                name: "navigate".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_result("tu-2", &big),
            // Recent messages (should be kept)
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "tu-3".into(),
                name: "screenshot".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_result("tu-3", &big),
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "tu-4".into(),
                name: "click".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_result("tu-4", &big),
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "tu-5".into(),
                name: "get_content".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_result("tu-5", &big),
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "tu-6".into(),
                name: "screenshot".into(),
                input: serde_json::json!({}),
            }]),
            Message::tool_result("tu-6", &big),
        ];

        let config = ContextConfig {
            max_context_chars: 10000, // force compaction
            keep_recent: 4,
            ..Default::default()
        };

        compact_messages(&mut messages, &config);

        // Old tool results (index 2, 4) should be compacted
        if let ContentBlock::ToolResult { content, .. } = &messages[2].content[0] {
            assert!(content.contains("Compacted"), "old result should be compacted: {content}");
            assert!(content.len() < 200);
        }

        // Recent tool results (last 8 messages = 4 pairs) should be preserved
        if let ContentBlock::ToolResult { content, .. } = &messages[messages.len() - 1].content[0] {
            assert_eq!(content.len(), 5000, "recent result should be preserved");
        }
    }

    #[test]
    fn compact_messages_compacts_old_thinking_text() {
        let long_thinking = "I need to think about this carefully. ".repeat(20);
        let big = "x".repeat(5000);
        let mut messages = vec![
            Message::user("goal"),
            Message::assistant(vec![ContentBlock::Text { text: long_thinking.clone() }]),
            Message::tool_result("tu-1", &big),
            // Recent (protected)
            Message::assistant(vec![ContentBlock::Text { text: "recent".into() }]),
            Message::tool_result("tu-2", "recent result"),
        ];

        let config = ContextConfig {
            max_context_chars: 1000,
            keep_recent: 1,
            ..Default::default()
        };

        compact_messages(&mut messages, &config);

        if let ContentBlock::Text { text } = &messages[1].content[0] {
            assert!(text.contains("compacted"));
            assert!(text.len() < long_thinking.len());
        }
    }
}
