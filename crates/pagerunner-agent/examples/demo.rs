//! Demo: run the Pagerunner agent loop with a real LLM via Ollama.
//!
//! Uses qwen2.5:7b through the Ollama backend to drive a fake browser,
//! showing the full event stream as the agent reasons and calls tools.
//!
//! Prerequisites:
//!   ollama pull qwen2.5:7b
//!   ollama serve              # if not already running
//!
//! Run:
//!   cargo run -p pagerunner-agent --example demo

use std::sync::Arc;

use async_trait::async_trait;
use pagerunner_agent::*;
use pagerunner_llm::*;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, watch};

// ── Fake Browser ────────────────────────────────────────────────────
//
// Simulates Pagerunner tool responses so we don't need a real Chrome.

struct FakeBrowser;

#[async_trait]
impl ToolExecutor for FakeBrowser {
    async fn execute(&self, name: &str, args: Value) -> std::result::Result<ToolResponse, String> {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        match name {
            "navigate" => {
                let url = args["url"].as_str().unwrap_or("unknown");
                Ok(ToolResponse::ok(&format!(
                    "Navigated to {url}. Page loaded successfully (1.2s)."
                )))
            }
            "screenshot" => Ok(ToolResponse::ok(
                "[Screenshot description: The page shows the Hacker News front page. \
                 Orange header bar with 'Hacker News' title. Below it, a numbered list \
                 of 30 story links. Each has a title, domain, points count, and comment \
                 count. The top story is 'Show HN: Pagerunner — browser automation for \
                 AI agents' with 342 points.]",
            )),
            "get_content" => Ok(ToolResponse::ok(
                "Hacker News - news.ycombinator.com\n\n\
                 1. Show HN: Pagerunner — browser automation for AI agents (github.com/enreign)\n\
                    342 points by enreign 3 hours ago | 128 comments\n\n\
                 2. Rust is eating the world, one rewrite at a time (blog.rust-lang.org)\n\
                    287 points by rustfan 5 hours ago | 94 comments\n\n\
                 3. Why SQLite is the most deployed database engine (sqlite.org)\n\
                    195 points by drh 7 hours ago | 67 comments\n\n\
                 4. The future of local AI models on consumer hardware (arxiv.org)\n\
                    178 points by mlresearcher 4 hours ago | 52 comments\n\n\
                 5. Building autonomous agents that actually work (anthropic.com)\n\
                    156 points by dario 6 hours ago | 89 comments",
            )),
            "click" => {
                let sel = args["selector"].as_str().unwrap_or("?");
                Ok(ToolResponse::ok(&format!(
                    "Clicked element matching selector '{sel}'. Page updated."
                )))
            }
            _ => Ok(ToolResponse::ok(&format!("Executed tool '{name}' successfully."))),
        }
    }

    fn available_tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema::new(
                "navigate",
                "Navigate the browser to a URL. Use this to open web pages.",
                json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "The URL to navigate to"}
                    },
                    "required": ["url"]
                }),
            ),
            ToolSchema::new(
                "screenshot",
                "Take a screenshot and get a text description of the current page.",
                json!({"type": "object", "properties": {}}),
            ),
            ToolSchema::new(
                "get_content",
                "Get the full text content of the current page.",
                json!({"type": "object", "properties": {}}),
            ),
            ToolSchema::new(
                "click",
                "Click an element on the page by CSS selector.",
                json!({
                    "type": "object",
                    "properties": {
                        "selector": {"type": "string", "description": "CSS selector of the element to click"}
                    },
                    "required": ["selector"]
                }),
            ),
        ]
    }
}

// ── ANSI colors ─────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const MAGENTA: &str = "\x1b[35m";
const BLUE: &str = "\x1b[34m";

// ── Main ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!();
    println!("{BOLD}{CYAN}╔═══════════════════════════════════════════════════════════╗{RESET}");
    println!("{BOLD}{CYAN}║     Pagerunner Agent Demo  ·  Ollama + qwen2.5:7b         ║{RESET}");
    println!("{BOLD}{CYAN}╚═══════════════════════════════════════════════════════════╝{RESET}");
    println!();

    let goal = std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ");
    let goal = if goal.is_empty() {
        "Go to Hacker News, get the page content, and give me a summary of the top 3 stories.".to_string()
    } else {
        goal
    };

    println!("{BOLD}Goal:{RESET} {goal}");
    println!("{DIM}Model: qwen2.5:7b via Ollama (localhost:11434){RESET}");
    println!("{DIM}Browser: simulated (fake tool responses){RESET}");
    println!();
    println!("{DIM}───────────────────────────────────────────────────────────{RESET}");
    println!();

    // Real Ollama provider
    let provider: Arc<dyn LlmProvider> = Arc::new(
        pagerunner_llm::ollama::OllamaProvider::new(None::<String>, "qwen2.5:7b".to_string()),
    );

    let executor: Arc<dyn ToolExecutor> = Arc::new(FakeBrowser);

    let (event_tx, mut event_rx) = broadcast::channel(64);
    let (_interrupt_tx, interrupt_rx) = watch::channel(false);
    let (_approval_tx, approval_rx) = mpsc::channel(16);

    let config = AgentConfig {
        provider: "ollama".into(),
        model: "qwen2.5:7b".into(),
        budget: BudgetConfig {
            max_steps: 10,
            max_tokens_per_step: 4096,
            total_token_budget: 0, // unlimited for demo
        },
        ..Default::default()
    };

    // Spawn event printer
    let printer = tokio::spawn(async move {
        let mut step = 0u32;
        while let Ok(event) = event_rx.recv().await {
            match event {
                AgentEvent::Thinking { text } => {
                    step += 1;
                    let lines: Vec<&str> = text.lines().collect();
                    let preview = if lines.len() > 6 {
                        let mut s = lines[..5].join("\n");
                        s.push_str(&format!("\n{DIM}  ... ({} more lines){RESET}", lines.len() - 5));
                        s
                    } else {
                        text
                    };
                    println!("{DIM}[step {step}]{RESET} {MAGENTA}Thinking:{RESET}");
                    for line in preview.lines() {
                        println!("  {line}");
                    }
                    println!();
                }
                AgentEvent::ToolCall { name, args } => {
                    let args_str = serde_json::to_string(&args).unwrap_or_default();
                    let display = if args_str.len() > 70 {
                        format!("{}…", &args_str[..67])
                    } else {
                        args_str
                    };
                    println!("  {YELLOW}▶{RESET} {BOLD}{name}{RESET} {DIM}{display}{RESET}");
                }
                AgentEvent::ToolResult { name, result, is_error } => {
                    let icon = if is_error {
                        format!("{RED}✗{RESET}")
                    } else {
                        format!("{GREEN}✓{RESET}")
                    };
                    let preview = if result.len() > 100 {
                        format!("{}…", &result[..97])
                    } else {
                        result
                    };
                    println!("  {icon} {BLUE}{name}{RESET}: {DIM}{preview}{RESET}");
                    println!();
                }
                AgentEvent::Done { summary, .. } => {
                    println!("{DIM}───────────────────────────────────────────────────────────{RESET}");
                    println!();
                    println!("{BOLD}{GREEN}✓ Agent completed{RESET}");
                    println!();
                    println!("{summary}");
                }
                AgentEvent::Error { message, .. } => {
                    println!("{RED}✗ Error: {message}{RESET}");
                }
                AgentEvent::BudgetExceeded { reason } => {
                    println!("{YELLOW}⚠ Budget exceeded: {reason}{RESET}");
                }
                AgentEvent::ApprovalRequired { action, .. } => {
                    println!("{YELLOW}? Approval needed for: {action}{RESET}");
                }
                _ => {}
            }
        }
    });

    // Run the agent
    let result = run_agent(
        goal,
        config,
        provider,
        executor,
        event_tx,
        interrupt_rx,
        approval_rx,
        "demo-1".into(),
    )
    .await;

    // Give printer a moment to drain
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    drop(printer);

    // Print stats
    println!();
    println!("{DIM}───────────────────────────────────────────────────────────{RESET}");
    println!(
        "{DIM}Steps: {} | Tokens: {} in + {} out | Outcome: {:?}{RESET}",
        result.total_steps,
        result.usage.input_tokens,
        result.usage.output_tokens,
        result.outcome,
    );
    println!();
}
