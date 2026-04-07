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
use crate::config::AgentConfig;
use crate::context::{compact_messages, truncate_result};
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

    if let Some(extra) = &config.system_prompt_extra {
        prompt.push_str("\n\n");
        prompt.push_str(extra);
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
    let tools: Vec<ToolSchema> = tool_executor.available_tools();
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
        let mut request =
            CompletionRequest::new(messages.clone(), &config.model, config.budget.max_tokens_per_step);
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
            messages.push(Message::user(
                "Continue from where you left off.",
            ));
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
            // Check autonomy policy
            let decision = config.autonomy.decide(tool_name);

            match decision {
                ToolDecision::Block => {
                    debug!(run_id = %run_id, tool = %tool_name, "tool blocked by policy");
                    let blocked_msg = format!("Tool '{tool_name}' is blocked by the autonomy policy.");
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
                            serde_json::to_string(tool_args).unwrap_or_default()
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
                        let (_full, truncated) =
                            execute_tool(&tool_executor, tool_name, tool_args, config.context.max_result_chars, &emit).await;
                        messages.push(make_tool_result_message(
                            tool_use_id,
                            &truncated,
                        ));
                    } else {
                        let denied_msg =
                            format!("Tool '{tool_name}' was denied by the user.");
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
                    let (_full, truncated) =
                        execute_tool(&tool_executor, tool_name, tool_args, config.context.max_result_chars, &emit).await;
                    messages.push(make_tool_result_message(
                        tool_use_id,
                        &truncated,
                    ));
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
            responses
                .pop()
                .unwrap_or_else(|| {
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
            self.calls
                .lock()
                .unwrap()
                .push((name.to_string(), args));
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
        (event_tx, event_rx, interrupt_tx, interrupt_rx, approval_tx, approval_rx)
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
        let executor = Arc::new(MockExecutor::new(
            make_tools(),
            ToolResponse::ok("ok"),
        ));
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
        let provider = Arc::new(MockProvider::new(vec![Ok(text_response("should not see this"))]));
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
        let executor = Arc::new(MockExecutor::new(
            make_tools(),
            ToolResponse::ok("clicked"),
        ));
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
}
