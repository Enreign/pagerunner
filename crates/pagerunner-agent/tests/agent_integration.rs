//! Integration tests for the agent loop.
//!
//! These tests use a scripted LLM provider and a recording executor to verify
//! multi-step agent behavior, approval flows, and event emission.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, watch};

use pagerunner_agent::{
    run_agent, AgentConfig, AgentEvent, AgentOutcome, ToolExecutor, ToolResponse,
};
use pagerunner_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError, LlmProvider, StopReason,
    ToolSchema, Usage,
};

// ---------------------------------------------------------------------------
// Scripted LLM provider
// ---------------------------------------------------------------------------

/// A mock LLM that returns pre-configured responses in order.
struct ScriptedProvider {
    responses: Mutex<Vec<Result<CompletionResponse, LlmError>>>,
}

impl ScriptedProvider {
    fn new(mut responses: Vec<Result<CompletionResponse, LlmError>>) -> Self {
        responses.reverse(); // so we can pop
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> pagerunner_llm::Result<CompletionResponse> {
        self.responses
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| {
                Ok(CompletionResponse {
                    content: vec![ContentBlock::Text {
                        text: "Fallback: no more scripted responses.".to_string(),
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
        unimplemented!("not used in integration tests")
    }

    fn name(&self) -> &str {
        "scripted"
    }

    fn supports_tools(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Recording executor
// ---------------------------------------------------------------------------

/// A tool executor that records all calls and returns scripted responses.
struct RecordingExecutor {
    tools: Vec<ToolSchema>,
    calls: Mutex<Vec<(String, Value)>>,
    responses: Mutex<Vec<ToolResponse>>,
}

impl RecordingExecutor {
    fn new(tools: Vec<ToolSchema>, mut responses: Vec<ToolResponse>) -> Self {
        responses.reverse(); // so we can pop
        Self {
            tools,
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(responses),
        }
    }

    fn call_log(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl ToolExecutor for RecordingExecutor {
    async fn execute(&self, name: &str, args: Value) -> Result<ToolResponse, String> {
        self.calls.lock().unwrap().push((name.to_string(), args));
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| ToolResponse::ok("default response"));
        Ok(response)
    }

    fn available_tools(&self) -> Vec<ToolSchema> {
        self.tools.clone()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn browser_tools() -> Vec<ToolSchema> {
    vec![
        ToolSchema::new("navigate", "Navigate to a URL", json!({})),
        ToolSchema::new("screenshot", "Take a screenshot", json!({})),
        ToolSchema::new("get_content", "Get page content", json!({})),
        ToolSchema::new("click", "Click an element", json!({})),
    ]
}

fn tool_use_response(id: &str, name: &str, args: Value) -> CompletionResponse {
    CompletionResponse {
        content: vec![ContentBlock::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input: args,
        }],
        usage: Usage {
            input_tokens: 50,
            output_tokens: 20,
        },
        stop_reason: StopReason::ToolUse,
    }
}

fn text_response(text: &str) -> CompletionResponse {
    CompletionResponse {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        usage: Usage {
            input_tokens: 50,
            output_tokens: 30,
        },
        stop_reason: StopReason::EndTurn,
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
    let (event_tx, event_rx) = broadcast::channel(128);
    let (interrupt_tx, interrupt_rx) = watch::channel(false);
    let (approval_tx, approval_rx) = mpsc::channel(16);
    (event_tx, event_rx, interrupt_tx, interrupt_rx, approval_tx, approval_rx)
}

/// Collect all events from a broadcast receiver (non-blocking).
fn drain_events(rx: &mut broadcast::Receiver<AgentEvent>) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(ev) => events.push(ev),
            Err(_) => break,
        }
    }
    events
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

/// Multi-step browse-and-report: navigate → screenshot → get_content → report.
///
/// Verifies:
/// - 4 LLM steps (3 tool calls + 1 final report)
/// - Correct tool calls in order
/// - Events emitted for each step
#[tokio::test]
async fn multi_step_browse_and_report() {
    let provider = Arc::new(ScriptedProvider::new(vec![
        // Step 1: navigate
        Ok(tool_use_response(
            "call_1",
            "navigate",
            json!({"url": "https://example.com"}),
        )),
        // Step 2: screenshot
        Ok(tool_use_response("call_2", "screenshot", json!({}))),
        // Step 3: get_content
        Ok(tool_use_response("call_3", "get_content", json!({}))),
        // Step 4: final report
        Ok(text_response(
            "The page at example.com shows a simple landing page with a heading 'Example Domain'.",
        )),
    ]));

    let executor = Arc::new(RecordingExecutor::new(
        browser_tools(),
        vec![
            ToolResponse::ok(r#"{"status":"navigated","url":"https://example.com"}"#),
            ToolResponse::ok(r#"{"screenshot":"base64data..."}"#),
            ToolResponse::ok(r#"<html><body><h1>Example Domain</h1></body></html>"#),
        ],
    ));

    let (event_tx, mut event_rx, _interrupt_tx, interrupt_rx, _approval_tx, approval_rx) =
        setup_channels();

    let result = run_agent(
        "Go to example.com, take a screenshot, and tell me what you see.".to_string(),
        AgentConfig::default(),
        provider,
        executor.clone(),
        event_tx,
        interrupt_rx,
        approval_rx,
        "integration-1".to_string(),
    )
    .await;

    // Outcome
    assert_eq!(result.outcome, AgentOutcome::Completed);
    assert_eq!(result.total_steps, 4);
    assert!(result.summary.as_deref().unwrap().contains("Example Domain"));

    // Tool calls in correct order
    let calls = executor.call_log();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].0, "navigate");
    assert_eq!(calls[1].0, "screenshot");
    assert_eq!(calls[2].0, "get_content");

    // Token usage accumulated across all 4 steps
    // Each step: input=50, output=20 (tool) or 30 (text)
    assert!(result.usage.input_tokens > 0);
    assert!(result.usage.output_tokens > 0);

    // Events: we should see ToolCall/ToolResult pairs for each tool + Thinking + Done
    let events = drain_events(&mut event_rx);
    let tool_call_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolCall { .. }))
        .collect();
    let tool_result_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolResult { .. }))
        .collect();
    let done_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Done { .. }))
        .collect();

    assert_eq!(tool_call_events.len(), 3, "expected 3 ToolCall events");
    assert_eq!(tool_result_events.len(), 3, "expected 3 ToolResult events");
    assert_eq!(done_events.len(), 1, "expected 1 Done event");
}

/// Approval flow: tool requires approval, user approves → tool executes.
#[tokio::test]
async fn approval_flow_approved() {
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(tool_use_response(
            "call_1",
            "click",
            json!({"selector": "#delete-btn"}),
        )),
        Ok(text_response("Item deleted.")),
    ]));

    let executor = Arc::new(RecordingExecutor::new(
        browser_tools(),
        vec![ToolResponse::ok("deleted")],
    ));

    let (event_tx, mut event_rx, _interrupt_tx, interrupt_rx, approval_tx, approval_rx) =
        setup_channels();

    let mut config = AgentConfig::default();
    config.autonomy.require_approval = vec!["click".to_string()];

    // Pre-send approval before the agent runs
    approval_tx.send(true).await.unwrap();

    let result = run_agent(
        "Delete the item.".to_string(),
        config,
        provider,
        executor.clone(),
        event_tx,
        interrupt_rx,
        approval_rx,
        "integration-2".to_string(),
    )
    .await;

    assert_eq!(result.outcome, AgentOutcome::Completed);
    assert_eq!(result.total_steps, 2);

    // Tool was executed
    let calls = executor.call_log();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "click");

    // Events include ApprovalRequired
    let events = drain_events(&mut event_rx);
    let approval_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ApprovalRequired { .. }))
        .collect();
    assert_eq!(approval_events.len(), 1);

    // Tool was also called and returned
    let tool_call_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolCall { .. }))
        .collect();
    assert_eq!(tool_call_events.len(), 1);
}

/// Approval flow: tool requires approval, user denies → tool NOT executed.
#[tokio::test]
async fn approval_flow_denied() {
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(tool_use_response(
            "call_1",
            "click",
            json!({"selector": "#delete-btn"}),
        )),
        Ok(text_response("Ok, I won't delete it.")),
    ]));

    let executor = Arc::new(RecordingExecutor::new(
        browser_tools(),
        vec![ToolResponse::ok("should not be called")],
    ));

    let (event_tx, mut event_rx, _interrupt_tx, interrupt_rx, approval_tx, approval_rx) =
        setup_channels();

    let mut config = AgentConfig::default();
    config.autonomy.require_approval = vec!["click".to_string()];

    // Pre-send denial
    approval_tx.send(false).await.unwrap();

    let result = run_agent(
        "Delete the item.".to_string(),
        config,
        provider,
        executor.clone(),
        event_tx,
        interrupt_rx,
        approval_rx,
        "integration-3".to_string(),
    )
    .await;

    assert_eq!(result.outcome, AgentOutcome::Completed);
    assert_eq!(result.total_steps, 2);

    // Tool was NOT executed
    let calls = executor.call_log();
    assert!(calls.is_empty(), "tool should not have been called");

    // Events include ApprovalRequired but NO ToolCall
    let events = drain_events(&mut event_rx);
    let approval_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ApprovalRequired { .. }))
        .collect();
    assert_eq!(approval_events.len(), 1);

    let tool_call_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolCall { .. }))
        .collect();
    assert_eq!(
        tool_call_events.len(),
        0,
        "no ToolCall should be emitted for denied tool"
    );

    // But there should be a ToolResult with is_error=true (the denial message)
    let denied_results: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. }))
        .collect();
    assert_eq!(
        denied_results.len(),
        1,
        "should emit a denied ToolResult event"
    );
}
