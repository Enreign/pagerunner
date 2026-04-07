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

    /// Tools to expose to the agent. If empty, all tools are exposed.
    /// When non-empty, only these tools are sent to the LLM as schemas.
    /// Recommended default covers browsing-essential tools.
    #[serde(default = "default_core_tools")]
    pub core_tools: Vec<String>,
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

fn default_core_tools() -> Vec<String> {
    vec![
        "navigate",
        "get_content",
        "screenshot",
        "click",
        "fill",
        "scroll",
        "select",
        "type_text",
        "wait_for",
        "evaluate",
        "list_tabs",
        "new_tab",
        "close_tab",
        "list_sessions",
        "open_session",
        "close_session",
        "list_profiles",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_result_chars: default_max_result_chars(),
            max_context_chars: default_max_context_chars(),
            keep_recent: default_keep_recent(),
            core_tools: default_core_tools(),
        }
    }
}

/// Filter tools to only include those in the `core_tools` list.
/// If the list is empty, all tools are returned unchanged.
pub fn filter_tools(
    tools: Vec<pagerunner_llm::ToolSchema>,
    core_tools: &[String],
) -> Vec<pagerunner_llm::ToolSchema> {
    if core_tools.is_empty() {
        return tools;
    }
    tools
        .into_iter()
        .filter(|t| core_tools.contains(&t.name))
        .collect()
}

// ---------------------------------------------------------------------------
// Truncation
// ---------------------------------------------------------------------------

/// Normalize whitespace: collapse runs of 3+ blank lines to 2, trim each line.
fn normalize_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut consecutive_blanks = 0;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            consecutive_blanks += 1;
            if consecutive_blanks <= 2 {
                result.push('\n');
            }
        } else {
            consecutive_blanks = 0;
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(trimmed);
        }
    }

    result
}

/// Truncate a tool result string if it exceeds the configured limit.
///
/// Normalizes excessive whitespace before measuring. The
/// `<<<UNTRUSTED_WEB_CONTENT>>>` wrappers are intentionally preserved —
/// the agent's inner LLM needs to know content is untrusted to resist
/// prompt injection from web pages.
///
/// The full, unmodified result is still emitted via `AgentEvent::ToolResult`.
pub fn truncate_result(result: &str, max_chars: usize) -> String {
    let cleaned = normalize_whitespace(result);

    if max_chars == 0 || cleaned.len() <= max_chars {
        return cleaned;
    }

    // Find a clean break point — prefer line boundary
    let cut = cleaned[..max_chars]
        .rfind('\n')
        .unwrap_or(max_chars);
    let cut = cut.max(max_chars / 2); // don't cut more than half

    let truncated = &cleaned[..cut];
    let remaining = cleaned.len() - cut;
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

/// Summarize tool call arguments for compact display (e.g. "url=https://...")
fn summarize_tool_args(input: &serde_json::Value) -> String {
    match input.as_object() {
        Some(obj) if !obj.is_empty() => {
            obj.iter()
                .map(|(k, v)| {
                    let val_str = match v {
                        serde_json::Value::String(s) => {
                            if s.len() > 60 {
                                format!("{}...", &s[..57])
                            } else {
                                s.clone()
                            }
                        }
                        other => {
                            let s = other.to_string();
                            if s.len() > 60 {
                                format!("{}...", &s[..57])
                            } else {
                                s
                            }
                        }
                    };
                    format!("{k}={val_str}")
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
        _ => String::new(),
    }
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

        // Also compact assistant messages from old turns
        if msg.role == Role::Assistant {
            // Collect ToolUse summaries and replace with a single text block
            let mut summaries: Vec<String> = Vec::new();
            let mut kept_blocks: Vec<ContentBlock> = Vec::new();

            for block in msg.content.drain(..) {
                match block {
                    ContentBlock::ToolUse { name, input, id } => {
                        // Summarize tool call as compact text
                        let args_summary = summarize_tool_args(&input);
                        summaries.push(format!("Called {name}({args_summary})"));
                        // Keep a minimal ToolUse so the API still gets
                        // valid assistant→tool pairing. Replace input with empty obj.
                        kept_blocks.push(ContentBlock::ToolUse {
                            id,
                            name,
                            input: serde_json::json!({}),
                        });
                    }
                    ContentBlock::Text { text } => {
                        if text.len() > 300 {
                            let first_sentence = text
                                .find(". ")
                                .map(|i| &text[..=i])
                                .unwrap_or(&text[..100.min(text.len())]);
                            kept_blocks.push(ContentBlock::Text {
                                text: format!("{first_sentence} [... compacted]"),
                            });
                        } else {
                            kept_blocks.push(ContentBlock::Text { text });
                        }
                    }
                    other => kept_blocks.push(other),
                }
            }

            // If we summarized any tool calls, prepend a single summary text
            if !summaries.is_empty() {
                kept_blocks.insert(
                    0,
                    ContentBlock::Text {
                        text: format!("[{}]", summaries.join("; ")),
                    },
                );
            }

            msg.content = kept_blocks;
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

    // --- Untrusted wrapper stripping tests ---

    #[test]
    fn truncate_result_preserves_untrusted_wrapper() {
        let wrapper = "<<<UNTRUSTED_WEB_CONTENT domain=\"example.com\">>>\nshort content\n<<<END_UNTRUSTED_WEB_CONTENT>>>";
        let r = truncate_result(wrapper, 1000);
        assert!(r.contains("UNTRUSTED_WEB_CONTENT"), "wrapper must be preserved for prompt injection safety");
        assert!(r.contains("short content"));
    }

    // --- Whitespace normalization tests ---

    #[test]
    fn normalize_whitespace_collapses_blank_lines() {
        let input = "line 1\n\n\n\n\nline 2";
        let result = normalize_whitespace(input);
        assert_eq!(result, "line 1\n\nline 2");
    }

    #[test]
    fn normalize_whitespace_trims_lines() {
        let input = "  hello  \n  world  ";
        let result = normalize_whitespace(input);
        assert_eq!(result, "hello\nworld");
    }

    #[test]
    fn truncate_result_normalizes_whitespace() {
        let input = "  line 1  \n\n\n\n\n  line 2  ";
        let r = truncate_result(input, 1000);
        assert_eq!(r, "line 1\n\nline 2");
    }

    #[test]
    fn context_config_defaults() {
        let c = ContextConfig::default();
        assert_eq!(c.max_result_chars, 4000);
        assert_eq!(c.max_context_chars, 24000);
        assert_eq!(c.keep_recent, 4);
        assert!(!c.core_tools.is_empty());
        assert!(c.core_tools.contains(&"navigate".to_string()));
        assert!(c.core_tools.contains(&"get_content".to_string()));
        assert!(c.core_tools.contains(&"screenshot".to_string()));
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

        // First block should be the compacted text (thinking text was long)
        let has_compacted = messages[1].content.iter().any(|b| {
            if let ContentBlock::Text { text } = b {
                text.contains("compacted")
            } else {
                false
            }
        });
        assert!(has_compacted, "old thinking text should be compacted");
    }

    #[test]
    fn compact_messages_compacts_tool_use_blocks() {
        let big = "x".repeat(5000);
        let mut messages = vec![
            Message::user("goal"),
            // Old assistant message with ToolUse
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "tu-1".into(),
                name: "navigate".into(),
                input: serde_json::json!({"url": "https://example.com", "session_id": "s1"}),
            }]),
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

        // The old assistant message should now have a summary text block
        let has_summary = messages[1].content.iter().any(|b| {
            if let ContentBlock::Text { text } = b {
                text.contains("Called navigate") && text.contains("url=")
            } else {
                false
            }
        });
        assert!(has_summary, "should have tool call summary text");

        // ToolUse block should still exist (for API pairing) but with empty input
        let has_empty_tool_use = messages[1].content.iter().any(|b| {
            if let ContentBlock::ToolUse { input, .. } = b {
                input == &serde_json::json!({})
            } else {
                false
            }
        });
        assert!(has_empty_tool_use, "should keep ToolUse with empty input for API pairing");
    }

    #[test]
    fn summarize_tool_args_formats_key_value_pairs() {
        let args = serde_json::json!({"url": "https://example.com", "timeout": 30});
        let summary = summarize_tool_args(&args);
        assert!(summary.contains("url=https://example.com"));
        assert!(summary.contains("timeout=30"));
    }

    #[test]
    fn summarize_tool_args_truncates_long_values() {
        let long_url = "https://example.com/".to_string() + &"x".repeat(100);
        let args = serde_json::json!({"url": long_url});
        let summary = summarize_tool_args(&args);
        assert!(summary.len() < 80);
        assert!(summary.contains("..."));
    }

    #[test]
    fn summarize_tool_args_empty_object() {
        let args = serde_json::json!({});
        let summary = summarize_tool_args(&args);
        assert!(summary.is_empty());
    }

    // --- filter_tools tests ---

    #[test]
    fn filter_tools_empty_list_returns_all() {
        let tools = vec![
            pagerunner_llm::ToolSchema::new("navigate", "nav", serde_json::json!({})),
            pagerunner_llm::ToolSchema::new("kv_set", "set kv", serde_json::json!({})),
        ];
        let filtered = filter_tools(tools.clone(), &[]);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filter_tools_keeps_only_listed() {
        let tools = vec![
            pagerunner_llm::ToolSchema::new("navigate", "nav", serde_json::json!({})),
            pagerunner_llm::ToolSchema::new("kv_set", "set kv", serde_json::json!({})),
            pagerunner_llm::ToolSchema::new("screenshot", "ss", serde_json::json!({})),
        ];
        let core = vec!["navigate".to_string(), "screenshot".to_string()];
        let filtered = filter_tools(tools, &core);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "navigate");
        assert_eq!(filtered[1].name, "screenshot");
    }

    #[test]
    fn filter_tools_nonexistent_names_ignored() {
        let tools = vec![
            pagerunner_llm::ToolSchema::new("navigate", "nav", serde_json::json!({})),
        ];
        let core = vec!["navigate".to_string(), "nonexistent".to_string()];
        let filtered = filter_tools(tools, &core);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "navigate");
    }

    #[test]
    fn default_core_tools_contains_essential_browsing_tools() {
        let defaults = default_core_tools();
        for expected in &[
            "navigate", "get_content", "screenshot", "click", "fill",
            "scroll", "select", "type_text", "wait_for", "evaluate",
            "list_tabs", "new_tab", "close_tab",
        ] {
            assert!(
                defaults.contains(&expected.to_string()),
                "missing expected tool: {expected}"
            );
        }
    }
}
