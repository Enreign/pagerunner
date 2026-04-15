//! Core agent loop — drives the LLM + tool execution cycle.

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{broadcast, mpsc, watch};
use tracing::{debug, info, warn};

use pagerunner_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmProvider, Message, Role, StopReason,
    ToolSchema,
};

use crate::autonomy::ToolDecision;
use crate::budget::{BudgetExceeded, BudgetTracker};
use crate::config::{AgentConfig, SessionContext};
use crate::context::{compact_messages, filter_tools, truncate_result};
use crate::events::{AgentEvent, AgentOutcome, AgentResult};
use crate::executor::{ToolExecutor, ToolResponse};

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Build the system prompt for the agent.
///
/// Includes Pagerunner-specific knowledge, session context if available,
/// and any extra text from the config.
pub fn build_system_prompt(config: &AgentConfig, tool_names: &[String]) -> String {
    let mut prompt = String::from(
        "You are an autonomous browser agent powered by Pagerunner. You drive a real \
         Chrome browser to accomplish goals — navigating pages, clicking elements, \
         filling forms, extracting content, and taking screenshots.\n\n\
         RULES:\n\
         - Be efficient. Minimize tool calls — don't call list_profiles or open_session \
           if you already have a session_id.\n\
         - When you have a session_id and target_id, use them directly in every tool call.\n\
         - Use get_content to read page text. Use screenshot only when you need to see \
           visual layout.\n\
         - Every tool that interacts with a page needs both session_id and target_id.\n\
         - When done, give a clear summary of what you found or did. Do NOT call more \
           tools after you have the answer.\n\
         - If a tool fails, read the error and adapt. Don't retry the same call blindly.",
    );

    if !tool_names.is_empty() {
        prompt.push_str("\n\nAvailable tools: ");
        prompt.push_str(&tool_names.join(", "));
    }

    if let Some(ref ctx) = config.session_context {
        prompt.push_str(&format!(
            "\n\nSESSION CONTEXT (auto-injected — do NOT include session_id or target_id in tool calls):\n\
             - session_id: {}\n\
             - target_id: {}\n\
             You already have a browser session. Use tools directly without session_id/target_id.",
            ctx.session_id, ctx.target_id,
        ));
    }

    if let Some(extra) = &config.system_prompt_extra {
        prompt.push_str("\n\n");
        prompt.push_str(extra);
    }

    if let Some(scope) = &config.scope {
        prompt.push_str(&crate::scope::build_scope_prompt(scope));
    }

    prompt
}

/// Extract concatenated text from a slice of content blocks.
///
/// Returns `None` if there are no text blocks.
pub fn extract_text(content: &[ContentBlock]) -> Option<String> {
    let texts: Vec<&str> = content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();

    if texts.is_empty() {
        None
    } else {
        Some(texts.join(""))
    }
}

// ---------------------------------------------------------------------------
// Session context helpers
// ---------------------------------------------------------------------------

/// Strip `session_id` and `target_id` from tool schemas so the LLM doesn't
/// need to generate them. Only removes them from `properties` and `required`.
pub fn strip_session_params(tools: &mut [ToolSchema]) {
    const STRIP: &[&str] = &["session_id", "target_id"];
    for tool in tools.iter_mut() {
        if tool.name.starts_with('_') {
            // Pseudo-tools (scope, turn summary) keep their params as-is.
            continue;
        }
        if let Some(props) = tool.input_schema.get_mut("properties") {
            if let Some(obj) = props.as_object_mut() {
                for key in STRIP {
                    obj.remove(*key);
                }
            }
        }
        if let Some(req) = tool.input_schema.get_mut("required") {
            if let Some(arr) = req.as_array_mut() {
                arr.retain(|v| v.as_str().is_none_or(|s| !STRIP.contains(&s)));
            }
        }
    }
}

/// Inject `session_id` and `target_id` into tool call args if the tool's
/// schema originally had those parameters (i.e. they were stripped).
/// We inject only when the key is absent — never override an explicit value.
pub fn inject_session_params(args: &mut Value, ctx: &SessionContext) {
    if let Some(obj) = args.as_object_mut() {
        if !obj.contains_key("session_id") {
            obj.insert(
                "session_id".to_string(),
                Value::String(ctx.session_id.clone()),
            );
        }
        if !obj.contains_key("target_id") {
            obj.insert(
                "target_id".to_string(),
                Value::String(ctx.target_id.clone()),
            );
        }
    }
}

/// Inject session params only for tools whose original schema had those params.
pub fn maybe_inject_session_params(
    tool_name: &str,
    args: &mut Value,
    ctx: &SessionContext,
    original_tools: &[ToolSchema],
) {
    if let Some(tool) = original_tools.iter().find(|t| t.name == tool_name) {
        let has_session_id = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.get("session_id"))
            .is_some();
        if has_session_id {
            inject_session_params(args, ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// Main agent loop
// ---------------------------------------------------------------------------

/// Run the agent loop to completion.
///
/// The loop repeatedly calls the LLM, executes tool calls (subject to the
/// autonomy policy), and feeds results back until the model signals it is
/// done, the budget is exhausted, or the run is interrupted.
#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    goal: String,
    config: AgentConfig,
    provider: Arc<dyn LlmProvider>,
    tool_executor: Arc<dyn ToolExecutor>,
    event_tx: broadcast::Sender<AgentEvent>,
    mut interrupt_rx: watch::Receiver<bool>,
    mut approval_rx: mpsc::Receiver<bool>,
    run_id: String,
) -> AgentResult {
    // Keep original tools for session param injection checks.
    let original_tools: Vec<ToolSchema> = tool_executor.available_tools();
    // Filter to core tools if configured (reduces schema tokens sent to LLM).
    let mut tools: Vec<ToolSchema> =
        filter_tools(original_tools.clone(), &config.context.core_tools);

    // If session context is set, strip session_id/target_id from schemas.
    if config.session_context.is_some() {
        strip_session_params(&mut tools);
    }

    // Register pseudo-tool schemas so the LLM knows about them.
    if config.scope.is_some() {
        tools.push(ToolSchema {
            name: "_scope_digest".to_string(),
            description: "Report an observation about a Scope tab. Call after any successful \
                          tool call on a Scope tab. digest must be ≤ 500 chars."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "target_id": {"type": ["string", "null"]},
                    "digest": {"type": "string", "maxLength": 500}
                },
                "required": ["session_id", "digest"]
            }),
        });
        tools.push(ToolSchema {
            name: "_turn_summary".to_string(),
            description: "Summarize this turn for the user's thread log. Call once, \
                          right before emitting `done`."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "summary": {"type": "string"},
                    "touched_tab_ids": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["summary"]
            }),
        });
    }

    let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    let system_prompt = build_system_prompt(&config, &tool_names);

    let mut messages: Vec<Message> = vec![Message::user(&goal)];
    let mut tracker = BudgetTracker::new();

    // Emit helper — ignore send errors (no receivers is fine).
    let emit = |event: AgentEvent| {
        let _ = event_tx.send(event);
    };

    loop {
        // 1. Check interrupt
        if *interrupt_rx.borrow() {
            info!(run_id = %run_id, "agent interrupted");
            emit(AgentEvent::Interrupted);
            return AgentResult {
                outcome: AgentOutcome::Interrupted,
                summary: None,
                total_steps: tracker.steps,
                usage: tracker.usage(),
            };
        }

        // 2. Check budget
        if let Some(exceeded) = tracker.check(&config.budget) {
            let (outcome, reason) = match exceeded {
                BudgetExceeded::MaxSteps { limit, current } => {
                    let reason = format!("max steps reached ({current}/{limit})");
                    (AgentOutcome::MaxStepsReached, reason)
                }
                BudgetExceeded::TokenBudget { limit, used } => {
                    let reason = format!("token budget exceeded ({used}/{limit})");
                    (AgentOutcome::BudgetExceeded, reason)
                }
            };
            warn!(run_id = %run_id, %reason, "budget exceeded");
            emit(AgentEvent::BudgetExceeded {
                reason: reason.clone(),
            });
            return AgentResult {
                outcome,
                summary: None,
                total_steps: tracker.steps,
                usage: tracker.usage(),
            };
        }

        // 3. Build completion request
        let mut request = CompletionRequest::new(
            messages.clone(),
            &config.model,
            config.budget.max_tokens_per_step,
        );
        request.system = Some(system_prompt.clone());
        request.tools = tools.clone();

        // 4. Call provider
        debug!(run_id = %run_id, step = tracker.steps + 1, "calling LLM");
        let response: CompletionResponse = match provider.complete(request).await {
            Ok(resp) => resp,
            Err(e) => {
                let message = format!("LLM error: {e}");
                warn!(run_id = %run_id, %message);
                emit(AgentEvent::Error {
                    message: message.clone(),
                    recoverable: false,
                });
                return AgentResult {
                    outcome: AgentOutcome::Error,
                    summary: Some(message),
                    total_steps: tracker.steps,
                    usage: tracker.usage(),
                };
            }
        };

        // 5. Record usage + step
        tracker.record_usage(&response.usage);
        tracker.record_step();

        // 6. Extract thinking text
        if let Some(text) = extract_text(&response.content) {
            if !text.is_empty() {
                emit(AgentEvent::Thinking { text: text.clone() });
            }
        }

        // 7. EndTurn → done
        if response.stop_reason == StopReason::EndTurn {
            let summary = extract_text(&response.content).unwrap_or_default();
            info!(run_id = %run_id, "agent completed");
            emit(AgentEvent::Done {
                summary: summary.clone(),
                artifacts: vec![],
            });
            return AgentResult {
                outcome: AgentOutcome::Completed,
                summary: Some(summary),
                total_steps: tracker.steps,
                usage: tracker.usage(),
            };
        }

        // 8. MaxTokens → add assistant message + continue
        if response.stop_reason == StopReason::MaxTokens {
            debug!(run_id = %run_id, "max tokens hit, continuing");
            messages.push(Message::assistant(response.content));
            messages.push(Message::user("Continue from where you left off."));
            continue;
        }

        // 9. ToolUse — process tool calls
        let tool_calls: Vec<(String, String, Value)> = response
            .content
            .iter()
            .filter_map(|block| {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    Some((id.clone(), name.clone(), input.clone()))
                } else {
                    None
                }
            })
            .collect();

        // Add the assistant message (contains ToolUse blocks)
        messages.push(Message::assistant(response.content));

        for (tool_use_id, tool_name, tool_args) in &tool_calls {
            // Scope pseudo-tools: the agent uses these to report observations
            // back to iOS. Intercepted here — never hit the real executor.
            if tool_name == "_scope_digest" || tool_name == "_turn_summary" {
                if tool_name == "_scope_digest" {
                    emit_scope_digest(tool_args, &event_tx);
                } else {
                    emit_turn_summary(tool_args, &event_tx);
                }
                // Synthesize a successful tool result so the agent continues.
                let ok_response = ToolResponse::ok("{\"ok\":true}");
                messages.push(make_tool_result_message(tool_use_id, &ok_response));
                continue;
            }

            // Auto-inject session context into tool args if configured.
            let mut tool_args = tool_args.clone();
            if let Some(ref ctx) = config.session_context {
                maybe_inject_session_params(tool_name, &mut tool_args, ctx, &original_tools);
            }

            // Check autonomy policy
            let decision = config.autonomy.decide(tool_name);

            match decision {
                ToolDecision::Block => {
                    debug!(run_id = %run_id, tool = %tool_name, "tool blocked by policy");
                    let blocked_msg =
                        format!("Tool '{tool_name}' is blocked by the autonomy policy.");
                    emit(AgentEvent::ToolResult {
                        name: tool_name.clone(),
                        result: blocked_msg.clone(),
                        is_error: true,
                    });
                    messages.push(Message {
                        role: Role::Tool,
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: blocked_msg,
                            is_error: Some(true),
                        }],
                    });
                }
                ToolDecision::RequireApproval => {
                    emit(AgentEvent::ApprovalRequired {
                        run_id: run_id.clone(),
                        action: tool_name.clone(),
                        description: format!(
                            "Tool '{}' requires approval. Args: {}",
                            tool_name,
                            serde_json::to_string(&tool_args).unwrap_or_default()
                        ),
                    });

                    // Wait for approval or interrupt
                    let approved = tokio::select! {
                        _ = interrupt_rx.changed() => {
                            if *interrupt_rx.borrow() {
                                emit(AgentEvent::Interrupted);
                                return AgentResult {
                                    outcome: AgentOutcome::Interrupted,
                                    summary: None,
                                    total_steps: tracker.steps,
                                    usage: tracker.usage(),
                                };
                            }
                            // Spurious change, treat as denied
                            false
                        }
                        maybe_approval = approval_rx.recv() => {
                            maybe_approval.unwrap_or(false)
                        }
                    };

                    if approved {
                        let (_full, truncated) = execute_tool(
                            &tool_executor,
                            tool_name,
                            &tool_args,
                            config.context.max_result_chars,
                            &emit,
                        )
                        .await;
                        messages.push(make_tool_result_message(tool_use_id, &truncated));
                    } else {
                        let denied_msg = format!("Tool '{tool_name}' was denied by the user.");
                        emit(AgentEvent::ToolResult {
                            name: tool_name.clone(),
                            result: denied_msg.clone(),
                            is_error: true,
                        });
                        messages.push(Message {
                            role: Role::Tool,
                            content: vec![ContentBlock::ToolResult {
                                tool_use_id: tool_use_id.clone(),
                                content: denied_msg,
                                is_error: Some(true),
                            }],
                        });
                    }
                }
                ToolDecision::AutoApprove => {
                    let (_full, truncated) = execute_tool(
                        &tool_executor,
                        tool_name,
                        &tool_args,
                        config.context.max_result_chars,
                        &emit,
                    )
                    .await;
                    messages.push(make_tool_result_message(tool_use_id, &truncated));
                }
            }
        }

        // 10. Compact context if it's grown too large
        compact_messages(&mut messages, &config.context);
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Execute a tool and emit the appropriate events.
///
/// Returns `(full_response, truncated_response)` — the full response goes to
/// the event stream, the truncated one goes into the LLM's message history.
async fn execute_tool(
    executor: &Arc<dyn ToolExecutor>,
    name: &str,
    args: &Value,
    max_result_chars: usize,
    emit: &impl Fn(AgentEvent),
) -> (ToolResponse, ToolResponse) {
    emit(AgentEvent::ToolCall {
        name: name.to_string(),
        args: args.clone(),
    });

    let response = match executor.execute(name, args.clone()).await {
        Ok(r) => r,
        Err(err) => ToolResponse::error(format!("Executor error: {err}")),
    };

    // Emit full result to event stream
    emit(AgentEvent::ToolResult {
        name: name.to_string(),
        result: response.content.clone(),
        is_error: response.is_error,
    });

    // Truncate for LLM context
    let truncated = ToolResponse {
        content: truncate_result(&response.content, max_result_chars),
        is_error: response.is_error,
    };

    (response, truncated)
}

/// Build a tool result message from a ToolResponse.
fn make_tool_result_message(tool_use_id: &str, response: &ToolResponse) -> Message {
    Message {
        role: Role::Tool,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: response.content.clone(),
            is_error: if response.is_error { Some(true) } else { None },
        }],
    }
}

// ---------------------------------------------------------------------------
// Scope pseudo-tool helpers
// ---------------------------------------------------------------------------

fn emit_scope_digest(args: &Value, event_tx: &broadcast::Sender<AgentEvent>) {
    let session_id = args
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let target_id = args
        .get("target_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let raw_digest = args
        .get("digest")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // Truncate to 500 chars to match iOS ScopeTab.digestCap.
    let digest: String = if raw_digest.chars().count() > 500 {
        raw_digest.chars().take(500).collect()
    } else {
        raw_digest.to_string()
    };
    if session_id.is_empty() || digest.is_empty() {
        warn!("_scope_digest called with missing session_id or digest; ignoring");
        return;
    }
    let _ = event_tx.send(AgentEvent::ScopeDigest {
        session_id,
        target_id,
        digest,
    });
}

fn emit_turn_summary(args: &Value, event_tx: &broadcast::Sender<AgentEvent>) {
    let summary = args
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let touched_tab_ids = args
        .get("touched_tab_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|el| el.as_str().map(String::from))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    if summary.is_empty() {
        warn!("_turn_summary called with empty summary; ignoring");
        return;
    }
    let _ = event_tx.send(AgentEvent::TurnSummary {
        summary,
        touched_tab_ids,
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use pagerunner_llm::{CompletionResponse, LlmError, Usage};
    use std::sync::Mutex;

    // --- Mock LLM Provider ---

    /// A mock LLM provider that returns pre-configured responses in sequence.
    struct MockProvider {
        responses: Mutex<Vec<Result<CompletionResponse, LlmError>>>,
    }

    impl MockProvider {
        fn new(responses: Vec<Result<CompletionResponse, LlmError>>) -> Self {
            // Reverse so we can pop from the end.
            let mut responses = responses;
            responses.reverse();
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> pagerunner_llm::Result<CompletionResponse> {
            let mut responses = self.responses.lock().unwrap();
            responses.pop().unwrap_or_else(|| {
                Ok(CompletionResponse {
                    content: vec![ContentBlock::Text {
                        text: "No more responses".to_string(),
                    }],
                    usage: Usage::default(),
                    stop_reason: StopReason::EndTurn,
                })
            })
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> pagerunner_llm::Result<pagerunner_llm::BoxStream> {
            unimplemented!("streaming not used in agent loop tests")
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn supports_tools(&self) -> bool {
            true
        }
    }

    // --- Mock Tool Executor ---

    /// A recording mock executor that logs tool calls and returns a fixed response.
    struct MockExecutor {
        tools: Vec<ToolSchema>,
        calls: Mutex<Vec<(String, Value)>>,
        /// If set, the executor returns this response for every call.
        default_response: ToolResponse,
    }

    impl MockExecutor {
        fn new(tools: Vec<ToolSchema>, default_response: ToolResponse) -> Self {
            Self {
                tools,
                calls: Mutex::new(Vec::new()),
                default_response,
            }
        }

        fn call_log(&self) -> Vec<(String, Value)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ToolExecutor for MockExecutor {
        async fn execute(&self, name: &str, args: Value) -> Result<ToolResponse, String> {
            self.calls.lock().unwrap().push((name.to_string(), args));
            Ok(self.default_response.clone())
        }

        fn available_tools(&self) -> Vec<ToolSchema> {
            self.tools.clone()
        }
    }

    // --- Test Helpers ---

    fn make_tools() -> Vec<ToolSchema> {
        vec![
            ToolSchema::new("navigate", "Navigate to URL", serde_json::json!({})),
            ToolSchema::new("screenshot", "Take a screenshot", serde_json::json!({})),
            ToolSchema::new("click", "Click an element", serde_json::json!({})),
        ]
    }

    fn default_config() -> AgentConfig {
        AgentConfig::default()
    }

    fn text_response(text: &str) -> CompletionResponse {
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
            },
            stop_reason: StopReason::EndTurn,
        }
    }

    fn tool_use_response(tool_id: &str, tool_name: &str, args: Value) -> CompletionResponse {
        CompletionResponse {
            content: vec![ContentBlock::ToolUse {
                id: tool_id.to_string(),
                name: tool_name.to_string(),
                input: args,
            }],
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
            },
            stop_reason: StopReason::ToolUse,
        }
    }

    fn setup_channels() -> (
        broadcast::Sender<AgentEvent>,
        broadcast::Receiver<AgentEvent>,
        watch::Sender<bool>,
        watch::Receiver<bool>,
        mpsc::Sender<bool>,
        mpsc::Receiver<bool>,
    ) {
        let (event_tx, event_rx) = broadcast::channel(64);
        let (interrupt_tx, interrupt_rx) = watch::channel(false);
        let (approval_tx, approval_rx) = mpsc::channel(16);
        (
            event_tx,
            event_rx,
            interrupt_tx,
            interrupt_rx,
            approval_tx,
            approval_rx,
        )
    }

    // --- Tests ---

    #[test]
    fn build_system_prompt_includes_tools() {
        let config = default_config();
        let tool_names = vec!["navigate".to_string(), "screenshot".to_string()];
        let prompt = build_system_prompt(&config, &tool_names);
        assert!(prompt.contains("navigate"));
        assert!(prompt.contains("screenshot"));
        assert!(prompt.contains("Available tools:"));
    }

    #[test]
    fn build_system_prompt_includes_extra() {
        let config = AgentConfig {
            system_prompt_extra: Some("Always respond in JSON.".to_string()),
            ..default_config()
        };
        let prompt = build_system_prompt(&config, &[]);
        assert!(prompt.contains("Always respond in JSON."));
    }

    #[test]
    fn build_system_prompt_no_tools_section_when_empty() {
        let config = default_config();
        let prompt = build_system_prompt(&config, &[]);
        assert!(!prompt.contains("Available tools:"));
    }

    #[test]
    fn extract_text_from_mixed_content() {
        let content = vec![
            ContentBlock::Text {
                text: "Hello ".to_string(),
            },
            ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "click".to_string(),
                input: serde_json::json!({}),
            },
            ContentBlock::Text {
                text: "world".to_string(),
            },
        ];
        assert_eq!(extract_text(&content), Some("Hello world".to_string()));
    }

    #[test]
    fn extract_text_empty_content() {
        let content: Vec<ContentBlock> = vec![];
        assert_eq!(extract_text(&content), None);
    }

    #[test]
    fn extract_text_no_text_blocks() {
        let content = vec![ContentBlock::ToolUse {
            id: "t1".to_string(),
            name: "click".to_string(),
            input: serde_json::json!({}),
        }];
        assert_eq!(extract_text(&content), None);
    }

    #[tokio::test]
    async fn agent_completes_simple_goal() {
        let provider = Arc::new(MockProvider::new(vec![Ok(text_response(
            "Task completed successfully.",
        ))]));
        let executor = Arc::new(MockExecutor::new(make_tools(), ToolResponse::ok("ok")));
        let (event_tx, mut event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        let result = run_agent(
            "Do something simple".to_string(),
            default_config(),
            provider,
            executor,
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-1".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Completed);
        assert_eq!(
            result.summary.as_deref(),
            Some("Task completed successfully.")
        );
        assert_eq!(result.total_steps, 1);
        assert_eq!(result.usage.input_tokens, 10);
        assert_eq!(result.usage.output_tokens, 5);

        // Check events
        let ev = event_rx.try_recv().unwrap();
        assert!(matches!(ev, AgentEvent::Thinking { .. }));
        let ev = event_rx.try_recv().unwrap();
        assert!(matches!(ev, AgentEvent::Done { .. }));
    }

    #[tokio::test]
    async fn agent_executes_tool_then_completes() {
        let provider = Arc::new(MockProvider::new(vec![
            Ok(tool_use_response(
                "call_1",
                "navigate",
                serde_json::json!({"url": "https://example.com"}),
            )),
            Ok(text_response("Done navigating.")),
        ]));
        let executor = Arc::new(MockExecutor::new(
            make_tools(),
            ToolResponse::ok("navigated"),
        ));
        let (event_tx, _event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        let result = run_agent(
            "Navigate to example.com".to_string(),
            default_config(),
            provider,
            executor.clone(),
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-2".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Completed);
        assert_eq!(result.total_steps, 2);

        let calls = executor.call_log();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "navigate");
    }

    #[tokio::test]
    async fn agent_respects_max_steps() {
        // LLM always returns a tool call — should hit max_steps
        let responses: Vec<Result<CompletionResponse, LlmError>> = (0..10)
            .map(|i| {
                Ok(tool_use_response(
                    &format!("call_{i}"),
                    "navigate",
                    serde_json::json!({"url": "https://example.com"}),
                ))
            })
            .collect();

        let provider = Arc::new(MockProvider::new(responses));
        let executor = Arc::new(MockExecutor::new(make_tools(), ToolResponse::ok("ok")));
        let (event_tx, _event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        let mut config = default_config();
        config.budget.max_steps = 3;

        let result = run_agent(
            "Keep navigating".to_string(),
            config,
            provider,
            executor,
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-3".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::MaxStepsReached);
        assert_eq!(result.total_steps, 3);
    }

    #[tokio::test]
    async fn agent_handles_interrupt() {
        let provider = Arc::new(MockProvider::new(vec![Ok(text_response(
            "should not see this",
        ))]));
        let executor = Arc::new(MockExecutor::new(make_tools(), ToolResponse::ok("ok")));
        let (event_tx, _event_rx, interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        // Pre-set interrupt
        interrupt_tx.send(true).unwrap();

        let result = run_agent(
            "Do something".to_string(),
            default_config(),
            provider,
            executor,
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-4".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Interrupted);
        assert_eq!(result.total_steps, 0);
    }

    #[tokio::test]
    async fn agent_blocks_tool_by_policy() {
        let provider = Arc::new(MockProvider::new(vec![
            Ok(tool_use_response(
                "call_1",
                "navigate",
                serde_json::json!({"url": "https://evil.com"}),
            )),
            Ok(text_response("Ok, tool was blocked.")),
        ]));
        let executor = Arc::new(MockExecutor::new(
            make_tools(),
            ToolResponse::ok("should not be called"),
        ));
        let (event_tx, _event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        let mut config = default_config();
        config.autonomy.block = vec!["navigate".to_string()];

        let result = run_agent(
            "Navigate somewhere".to_string(),
            config,
            provider,
            executor.clone(),
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-5".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Completed);
        // The tool should NOT have been executed
        assert!(executor.call_log().is_empty());
    }

    #[tokio::test]
    async fn agent_handles_llm_error() {
        let provider = Arc::new(MockProvider::new(vec![Err(LlmError::Api {
            status: 500,
            message: "Internal Server Error".to_string(),
        })]));
        let executor = Arc::new(MockExecutor::new(make_tools(), ToolResponse::ok("ok")));
        let (event_tx, _event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        let result = run_agent(
            "Do something".to_string(),
            default_config(),
            provider,
            executor,
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-6".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Error);
        assert!(result.summary.as_deref().unwrap().contains("LLM error"));
        assert_eq!(result.total_steps, 0);
    }

    #[tokio::test]
    async fn agent_handles_max_tokens_continuation() {
        // First response hits MaxTokens, second completes.
        let provider = Arc::new(MockProvider::new(vec![
            Ok(CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "Starting to think...".to_string(),
                }],
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                },
                stop_reason: StopReason::MaxTokens,
            }),
            Ok(text_response("All done.")),
        ]));
        let executor = Arc::new(MockExecutor::new(make_tools(), ToolResponse::ok("ok")));
        let (event_tx, _event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        let result = run_agent(
            "Do a big task".to_string(),
            default_config(),
            provider,
            executor,
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-7".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Completed);
        assert_eq!(result.total_steps, 2);
    }

    #[tokio::test]
    async fn agent_approval_flow_approved() {
        let provider = Arc::new(MockProvider::new(vec![
            Ok(tool_use_response(
                "call_1",
                "click",
                serde_json::json!({"selector": "#submit"}),
            )),
            Ok(text_response("Clicked successfully.")),
        ]));
        let executor = Arc::new(MockExecutor::new(make_tools(), ToolResponse::ok("clicked")));
        let (event_tx, _event_rx, _interrupt_tx, interrupt_rx, approval_tx, approval_rx) =
            setup_channels();

        let mut config = default_config();
        config.autonomy.require_approval = vec!["click".to_string()];

        // Pre-send approval
        approval_tx.send(true).await.unwrap();

        let result = run_agent(
            "Click the button".to_string(),
            config,
            provider,
            executor.clone(),
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-8".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Completed);
        let calls = executor.call_log();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "click");
    }

    #[tokio::test]
    async fn agent_approval_flow_denied() {
        let provider = Arc::new(MockProvider::new(vec![
            Ok(tool_use_response(
                "call_1",
                "click",
                serde_json::json!({"selector": "#submit"}),
            )),
            Ok(text_response("Ok, denied.")),
        ]));
        let executor = Arc::new(MockExecutor::new(
            make_tools(),
            ToolResponse::ok("should not be called"),
        ));
        let (event_tx, _event_rx, _interrupt_tx, interrupt_rx, approval_tx, approval_rx) =
            setup_channels();

        let mut config = default_config();
        config.autonomy.require_approval = vec!["click".to_string()];

        // Pre-send denial
        approval_tx.send(false).await.unwrap();

        let result = run_agent(
            "Click the button".to_string(),
            config,
            provider,
            executor.clone(),
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-9".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Completed);
        // Tool should NOT have been executed
        assert!(executor.call_log().is_empty());
    }

    // --- Session context tests ---

    fn make_tools_with_session_params() -> Vec<ToolSchema> {
        vec![
            ToolSchema::new(
                "navigate",
                "Navigate to URL",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "string"},
                        "target_id": {"type": "string"},
                        "url": {"type": "string"}
                    },
                    "required": ["session_id", "target_id", "url"]
                }),
            ),
            ToolSchema::new(
                "list_profiles",
                "List profiles",
                serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            ),
        ]
    }

    #[test]
    fn strip_session_params_removes_from_properties_and_required() {
        let mut tools = make_tools_with_session_params();
        strip_session_params(&mut tools);

        // navigate should have session_id/target_id removed
        let nav = &tools[0];
        let props = nav.input_schema["properties"].as_object().unwrap();
        assert!(!props.contains_key("session_id"));
        assert!(!props.contains_key("target_id"));
        assert!(props.contains_key("url"));

        let required: Vec<&str> = nav.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(!required.contains(&"session_id"));
        assert!(!required.contains(&"target_id"));
        assert!(required.contains(&"url"));

        // list_profiles should be unaffected
        let lp = &tools[1];
        assert!(lp.input_schema["properties"]
            .as_object()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn inject_session_params_adds_missing_keys() {
        let ctx = SessionContext {
            session_id: "sess-1".to_string(),
            target_id: "tab-1".to_string(),
        };
        let mut args = serde_json::json!({"url": "https://example.com"});
        inject_session_params(&mut args, &ctx);

        assert_eq!(args["session_id"], "sess-1");
        assert_eq!(args["target_id"], "tab-1");
        assert_eq!(args["url"], "https://example.com");
    }

    #[test]
    fn inject_session_params_does_not_override_explicit() {
        let ctx = SessionContext {
            session_id: "sess-1".to_string(),
            target_id: "tab-1".to_string(),
        };
        let mut args = serde_json::json!({
            "url": "https://example.com",
            "session_id": "explicit-sess",
            "target_id": "explicit-tab"
        });
        inject_session_params(&mut args, &ctx);

        assert_eq!(args["session_id"], "explicit-sess");
        assert_eq!(args["target_id"], "explicit-tab");
    }

    #[test]
    fn maybe_inject_skips_tools_without_session_params() {
        let ctx = SessionContext {
            session_id: "sess-1".to_string(),
            target_id: "tab-1".to_string(),
        };
        let original_tools = make_tools_with_session_params();
        let mut args = serde_json::json!({});
        maybe_inject_session_params("list_profiles", &mut args, &ctx, &original_tools);

        // list_profiles has no session_id in schema, so nothing should be injected
        assert!(args.as_object().unwrap().is_empty());
    }

    #[test]
    fn maybe_inject_adds_for_tools_with_session_params() {
        let ctx = SessionContext {
            session_id: "sess-1".to_string(),
            target_id: "tab-1".to_string(),
        };
        let original_tools = make_tools_with_session_params();
        let mut args = serde_json::json!({"url": "https://example.com"});
        maybe_inject_session_params("navigate", &mut args, &ctx, &original_tools);

        assert_eq!(args["session_id"], "sess-1");
        assert_eq!(args["target_id"], "tab-1");
    }

    #[test]
    fn build_system_prompt_includes_session_context() {
        let config = AgentConfig {
            session_context: Some(SessionContext {
                session_id: "sess-abc".to_string(),
                target_id: "tab-xyz".to_string(),
            }),
            ..default_config()
        };
        let prompt = build_system_prompt(&config, &[]);
        assert!(prompt.contains("sess-abc"));
        assert!(prompt.contains("tab-xyz"));
        assert!(prompt.contains("auto-injected"));
    }

    #[test]
    fn no_session_context_in_prompt_when_none() {
        let config = default_config();
        let prompt = build_system_prompt(&config, &[]);
        assert!(!prompt.contains("auto-injected"));
    }

    #[tokio::test]
    async fn agent_injects_session_context_into_tool_args() {
        let provider = Arc::new(MockProvider::new(vec![
            Ok(tool_use_response(
                "call_1",
                "navigate",
                serde_json::json!({"url": "https://example.com"}),
            )),
            Ok(text_response("Done.")),
        ]));
        let executor = Arc::new(MockExecutor::new(
            make_tools_with_session_params(),
            ToolResponse::ok("navigated"),
        ));
        let (event_tx, _event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        let mut config = default_config();
        config.session_context = Some(SessionContext {
            session_id: "sess-injected".to_string(),
            target_id: "tab-injected".to_string(),
        });

        let result = run_agent(
            "Navigate somewhere".to_string(),
            config,
            provider,
            executor.clone(),
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-inject".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Completed);
        let calls = executor.call_log();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "navigate");
        // session_id and target_id should have been injected
        assert_eq!(calls[0].1["session_id"], "sess-injected");
        assert_eq!(calls[0].1["target_id"], "tab-injected");
        assert_eq!(calls[0].1["url"], "https://example.com");
    }

    #[test]
    fn build_system_prompt_includes_scope_when_set() {
        use crate::scope::{Scope, ScopeTab};
        let config = AgentConfig {
            scope: Some(Scope {
                tabs: vec![ScopeTab {
                    session_id: "s-1".into(),
                    target_id: Some("t-a".into()),
                    label: "Notion".into(),
                    purpose: Some("source".into()),
                    digest: None,
                }],
                goal: Some("weekly".into()),
                ..Default::default()
            }),
            ..AgentConfig::default()
        };
        let prompt = build_system_prompt(&config, &[]);
        assert!(prompt.contains("SCOPE: weekly"));
        assert!(prompt.contains("@Notion"));
        assert!(prompt.contains("_scope_digest"));
    }

    #[test]
    fn build_system_prompt_no_scope_section_when_empty() {
        let prompt = build_system_prompt(&AgentConfig::default(), &[]);
        assert!(!prompt.contains("SCOPE:"));
        assert!(!prompt.contains("_scope_digest"));
    }

    // --- Scope pseudo-tool tests ---

    #[test]
    fn emit_scope_digest_sends_event() {
        use tokio::sync::broadcast;
        let (tx, mut rx) = broadcast::channel(8);
        let args = serde_json::json!({
            "session_id": "s-1",
            "target_id": "t-a",
            "digest": "rows 1..47"
        });
        emit_scope_digest(&args, &tx);
        let ev = rx.try_recv().expect("event");
        match ev {
            AgentEvent::ScopeDigest {
                session_id,
                target_id,
                digest,
            } => {
                assert_eq!(session_id, "s-1");
                assert_eq!(target_id.as_deref(), Some("t-a"));
                assert_eq!(digest, "rows 1..47");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn emit_scope_digest_truncates_at_500_chars() {
        use tokio::sync::broadcast;
        let (tx, mut rx) = broadcast::channel(8);
        let long = "z".repeat(800);
        let args = serde_json::json!({
            "session_id": "s-1",
            "digest": long
        });
        emit_scope_digest(&args, &tx);
        let ev = rx.try_recv().unwrap();
        if let AgentEvent::ScopeDigest { digest, .. } = ev {
            assert_eq!(digest.len(), 500);
        } else {
            panic!("expected ScopeDigest");
        }
    }

    #[test]
    fn emit_scope_digest_ignores_empty_args() {
        use tokio::sync::broadcast;
        let (tx, mut rx) = broadcast::channel(8);
        emit_scope_digest(&serde_json::json!({}), &tx);
        assert!(rx.try_recv().is_err(), "should not send on empty args");
    }

    #[test]
    fn emit_turn_summary_sends_event() {
        use tokio::sync::broadcast;
        let (tx, mut rx) = broadcast::channel(8);
        let args = serde_json::json!({
            "summary": "done",
            "touched_tab_ids": ["s-1-t-a", "s-1-t-b"]
        });
        emit_turn_summary(&args, &tx);
        let ev = rx.try_recv().unwrap();
        if let AgentEvent::TurnSummary {
            summary,
            touched_tab_ids,
        } = ev
        {
            assert_eq!(summary, "done");
            assert_eq!(touched_tab_ids, vec!["s-1-t-a", "s-1-t-b"]);
        } else {
            panic!("expected TurnSummary");
        }
    }

    #[test]
    fn emit_turn_summary_allows_missing_touched_ids() {
        use tokio::sync::broadcast;
        let (tx, mut rx) = broadcast::channel(8);
        emit_turn_summary(&serde_json::json!({"summary": "s"}), &tx);
        let ev = rx.try_recv().unwrap();
        if let AgentEvent::TurnSummary { touched_tab_ids, .. } = ev {
            assert!(touched_tab_ids.is_empty());
        } else {
            panic!("expected TurnSummary");
        }
    }

    #[tokio::test]
    async fn scope_dispatch_fires_scope_digest_and_turn_summary_events() {
        use crate::scope::{Scope, ScopeTab};

        // Turn 1: LLM calls _scope_digest
        // Turn 2: LLM calls _turn_summary
        // Turn 3: LLM returns text → loop terminates (Done)
        let provider = Arc::new(MockProvider::new(vec![
            Ok(tool_use_response(
                "sd-1",
                "_scope_digest",
                serde_json::json!({
                    "session_id": "s-1",
                    "target_id": "t-a",
                    "digest": "rows observed"
                }),
            )),
            Ok(tool_use_response(
                "ts-1",
                "_turn_summary",
                serde_json::json!({
                    "summary": "done",
                    "touched_tab_ids": ["s-1-t-a"]
                }),
            )),
            Ok(text_response("All done.")),
        ]));
        let executor = Arc::new(MockExecutor::new(make_tools(), ToolResponse::ok("ok")));
        let (event_tx, mut event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
            setup_channels();

        let config = AgentConfig {
            scope: Some(Scope {
                tabs: vec![ScopeTab {
                    session_id: "s-1".into(),
                    target_id: Some("t-a".into()),
                    label: "Test Tab".into(),
                    purpose: None,
                    digest: None,
                }],
                ..Default::default()
            }),
            ..default_config()
        };

        let result = run_agent(
            "Do something with the scope tab".to_string(),
            config,
            provider,
            executor,
            event_tx,
            interrupt_rx,
            approval_rx,
            "run-scope-dispatch".to_string(),
        )
        .await;

        assert_eq!(result.outcome, AgentOutcome::Completed);

        // Drain all events into a vec so we can assert on them.
        let mut events = Vec::new();
        while let Ok(ev) = event_rx.try_recv() {
            events.push(ev);
        }

        // ScopeDigest must be present with correct fields.
        let scope_digest = events.iter().find(|ev| {
            matches!(ev, AgentEvent::ScopeDigest { session_id, target_id, digest }
                if session_id == "s-1"
                && target_id.as_deref() == Some("t-a")
                && digest == "rows observed")
        });
        assert!(scope_digest.is_some(), "expected ScopeDigest event; got: {:?}", events);

        // TurnSummary must be present with correct fields.
        let turn_summary = events.iter().find(|ev| {
            matches!(ev, AgentEvent::TurnSummary { summary, touched_tab_ids }
                if summary == "done" && touched_tab_ids == &["s-1-t-a"])
        });
        assert!(turn_summary.is_some(), "expected TurnSummary event; got: {:?}", events);

        // Neither pseudo-tool should emit a ToolCall or ToolResult event.
        let pseudo_tool_call = events.iter().find(|ev| {
            matches!(ev, AgentEvent::ToolCall { name, .. }
                if name == "_scope_digest" || name == "_turn_summary")
        });
        assert!(pseudo_tool_call.is_none(), "pseudo-tool ToolCall should be suppressed; got: {:?}", events);

        let pseudo_tool_result = events.iter().find(|ev| {
            matches!(ev, AgentEvent::ToolResult { name, .. }
                if name == "_scope_digest" || name == "_turn_summary")
        });
        assert!(pseudo_tool_result.is_none(), "pseudo-tool ToolResult should be suppressed; got: {:?}", events);
    }

    #[test]
    fn strip_session_params_skips_pseudo_tools() {
        let mut tools = vec![
            ToolSchema {
                name: "navigate".to_string(),
                description: "".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"session_id": {"type": "string"}, "url": {"type": "string"}},
                    "required": ["session_id", "url"]
                }),
            },
            ToolSchema {
                name: "_scope_digest".to_string(),
                description: "".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"session_id": {"type": "string"}, "digest": {"type": "string"}},
                    "required": ["session_id", "digest"]
                }),
            },
        ];
        strip_session_params(&mut tools);
        // navigate: session_id stripped
        assert!(tools[0].input_schema["properties"]
            .get("session_id")
            .is_none());
        // _scope_digest: session_id still there
        assert!(tools[1].input_schema["properties"]
            .get("session_id")
            .is_some());
    }
}
