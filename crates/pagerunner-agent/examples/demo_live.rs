//! Live demo: real LLM (Ollama) + real browser (Pagerunner daemon + Chrome).
//!
//! Prerequisites:
//!   1. pagerunner daemon &          # start the daemon
//!   2. ollama serve                 # if not already running
//!   3. At least one profile in ~/.pagerunner/config.toml
//!
//! Run:
//!   cargo run -p pagerunner-agent --example demo_live
//!
//! Or with a custom goal:
//!   cargo run -p pagerunner-agent --example demo_live -- "Go to github.com and describe what you see"
//!
//! Or with a specific profile:
//!   PAGERUNNER_PROFILE=personal cargo run -p pagerunner-agent --example demo_live

use std::sync::Arc;

use async_trait::async_trait;
use pagerunner_agent::*;
use pagerunner_llm::*;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, watch, Mutex};

// ── Pagerunner Daemon Executor ──────────────────────────────────────
//
// Connects to the real Pagerunner daemon over its Unix socket and
// forwards tool calls. This is the bridge the spec describes in §10.

struct DaemonExecutor {
    stream: Mutex<UnixStream>,
    session_id: Mutex<Option<String>>,
    target_id: Mutex<Option<String>>,
    profile: String,
}

impl DaemonExecutor {
    async fn connect(profile: String) -> std::result::Result<Self, String> {
        let home = dirs::home_dir().ok_or("no home dir")?;
        let path = home.join(".pagerunner/daemon.sock");
        let stream = UnixStream::connect(&path)
            .await
            .map_err(|e| format!("Cannot connect to daemon at {:?}: {}. Is `pagerunner daemon` running?", path, e))?;
        Ok(Self {
            stream: Mutex::new(stream),
            session_id: Mutex::new(None),
            target_id: Mutex::new(None),
            profile,
        })
    }

    async fn call_daemon(&self, tool: &str, args: Value) -> std::result::Result<String, String> {
        let req = json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "tool": tool,
            "args": args,
        });
        let mut msg = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        msg.push('\n');

        let mut stream = self.stream.lock().await;
        let (read_half, mut write_half) = tokio::io::split(&mut *stream);
        write_half
            .write_all(msg.as_bytes())
            .await
            .map_err(|e| format!("write error: {e}"))?;

        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("read error: {e}"))?;

        let resp: Value = serde_json::from_str(line.trim())
            .map_err(|e| format!("parse error: {e}"))?;

        if let Some(err) = resp["error"].as_str() {
            Err(err.to_string())
        } else if let Some(result) = resp.get("result") {
            Ok(result.as_str().unwrap_or(&resp["result"].to_string()).to_string())
        } else {
            Err("empty daemon response".into())
        }
    }

    /// Ensure we have a session open; return (session_id, target_id).
    ///
    /// Strategy: first look for an existing alive session for this profile,
    /// otherwise open a new one.
    async fn ensure_session(&self) -> std::result::Result<(String, String), String> {
        {
            let sid = self.session_id.lock().await;
            let tid = self.target_id.lock().await;
            if let (Some(s), Some(t)) = (sid.as_ref(), tid.as_ref()) {
                return Ok((s.clone(), t.clone()));
            }
        }

        // Try to find an existing alive session for this profile
        let sessions_result = self
            .call_daemon("list_sessions", json!({}))
            .await?;
        let sessions: Value = serde_json::from_str(&sessions_result).unwrap_or(json!({}));
        // Response may be {"data": [...]} or just [...]
        let session_arr = sessions["data"].as_array().or_else(|| sessions.as_array());

        let existing = session_arr
            .and_then(|arr| {
                arr.iter().find(|s| {
                    s["profile"].as_str() == Some(&self.profile)
                        && s["status"].as_str() == Some("alive")
                })
            });

        let sid = if let Some(session) = existing {
            eprintln!(
                "  {DIM}Reusing existing session for profile '{}'{RESET}",
                self.profile
            );
            session["id"].as_str().unwrap().to_string()
        } else {
            // Open a new session
            eprintln!(
                "  {DIM}Opening new Chrome session for profile '{}'{RESET}",
                self.profile
            );
            let result = self
                .call_daemon("open_session", json!({"profile": self.profile}))
                .await?;
            let parsed: Value = serde_json::from_str(&result).unwrap_or(json!({}));
            parsed["session_id"]
                .as_str()
                .ok_or("no session_id in response")?
                .to_string()
        };

        // Get a tab — try list_tabs first, open a new one if empty
        let mut tid = None;
        for attempt in 0..10 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            match self.call_daemon("list_tabs", json!({"session_id": &sid})).await {
                Ok(tabs_result) => {
                    let tabs: Value = serde_json::from_str(&tabs_result).unwrap_or(json!([]));
                    if let Some(first) = tabs.as_array().and_then(|a| a.first()) {
                        if let Some(t) = first["target_id"].as_str() {
                            tid = Some(t.to_string());
                            break;
                        }
                    }
                }
                Err(_) if attempt < 9 => continue,
                Err(_) => {}
            }
        }

        // If no tabs found, open a new tab
        if tid.is_none() {
            eprintln!("  {DIM}No tabs found, opening new tab...{RESET}");
            let tab_result = self
                .call_daemon("new_tab", json!({"session_id": &sid}))
                .await?;
            let parsed: Value = serde_json::from_str(&tab_result).unwrap_or(json!({}));
            tid = parsed["target_id"].as_str().map(|s| s.to_string());

            // Wait for tab to be ready
            if tid.is_none() {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let tabs_result = self
                    .call_daemon("list_tabs", json!({"session_id": &sid}))
                    .await?;
                let tabs: Value = serde_json::from_str(&tabs_result).unwrap_or(json!([]));
                tid = tabs
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|t| t["target_id"].as_str())
                    .map(|s| s.to_string());
            }
        }

        let tid = tid.ok_or("could not get a browser tab")?;

        *self.session_id.lock().await = Some(sid.clone());
        *self.target_id.lock().await = Some(tid.clone());

        Ok((sid, tid))
    }
}

#[async_trait]
impl ToolExecutor for DaemonExecutor {
    async fn execute(&self, name: &str, args: Value) -> std::result::Result<ToolResponse, String> {
        // Auto-inject session_id and target_id for tools that need them
        let (sid, tid) = self.ensure_session().await?;

        let mut full_args = args.clone();
        if let Some(obj) = full_args.as_object_mut() {
            // Inject session_id if not present and tool needs it
            let needs_session = !matches!(name, "list_profiles" | "list_sessions" | "open_session");
            if needs_session && !obj.contains_key("session_id") {
                obj.insert("session_id".into(), json!(sid));
            }
            // Inject target_id if not present and tool needs it
            let needs_target = matches!(
                name,
                "navigate" | "get_content" | "screenshot" | "click" | "fill"
                    | "type_text" | "select" | "scroll" | "evaluate" | "wait_for"
                    | "new_tab" | "close_tab" | "save_snapshot" | "restore_snapshot"
            );
            if needs_target && !obj.contains_key("target_id") {
                obj.insert("target_id".into(), json!(tid));
            }
        }

        match self.call_daemon(name, full_args).await {
            Ok(result) => {
                // Update target_id if navigate or new_tab changes it
                if name == "new_tab" {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&result) {
                        if let Some(new_tid) = parsed["target_id"].as_str() {
                            *self.target_id.lock().await = Some(new_tid.to_string());
                        }
                    }
                }
                Ok(ToolResponse::ok(&result))
            }
            Err(e) => Ok(ToolResponse::error(e)),
        }
    }

    fn available_tools(&self) -> Vec<ToolSchema> {
        // Expose a focused set of tools — the ones an agent actually needs
        vec![
            ToolSchema::new(
                "navigate",
                "Navigate the browser to a URL.",
                json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "URL to navigate to"}
                    },
                    "required": ["url"]
                }),
            ),
            ToolSchema::new(
                "screenshot",
                "Take a screenshot of the current page. Returns a text description of what's visible.",
                json!({"type": "object", "properties": {}}),
            ),
            ToolSchema::new(
                "get_content",
                "Get the text content of the current page.",
                json!({"type": "object", "properties": {}}),
            ),
            ToolSchema::new(
                "click",
                "Click an element by CSS selector.",
                json!({
                    "type": "object",
                    "properties": {
                        "selector": {"type": "string", "description": "CSS selector"}
                    },
                    "required": ["selector"]
                }),
            ),
            ToolSchema::new(
                "fill",
                "Fill a form field with text.",
                json!({
                    "type": "object",
                    "properties": {
                        "selector": {"type": "string", "description": "CSS selector of the input"},
                        "value": {"type": "string", "description": "Text to fill"}
                    },
                    "required": ["selector", "value"]
                }),
            ),
            ToolSchema::new(
                "scroll",
                "Scroll the page up or down.",
                json!({
                    "type": "object",
                    "properties": {
                        "direction": {"type": "string", "enum": ["up", "down"], "description": "Scroll direction"}
                    },
                    "required": ["direction"]
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

const SEP: &str = "───────────────────────────────────────────────────────────";

// ── Main ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!();
    println!("{BOLD}{CYAN}╔═══════════════════════════════════════════════════════════╗{RESET}");
    println!("{BOLD}{CYAN}║  Pagerunner Agent · Live Demo · LLM + Real Chrome         ║{RESET}");
    println!("{BOLD}{CYAN}╚═══════════════════════════════════════════════════════════╝{RESET}");
    println!();

    let goal = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let goal = if goal.is_empty() {
        "Go to https://news.ycombinator.com, get the page content, and summarize the top 3 stories with their point counts.".to_string()
    } else {
        goal
    };

    let profile = std::env::var("PAGERUNNER_PROFILE").unwrap_or_else(|_| {
        // Try to find the first available profile
        "personal".to_string()
    });

    // Provider selection priority: Anthropic → OpenRouter → OpenAI → Ollama
    let (provider, provider_name, model): (Arc<dyn LlmProvider>, String, String) =
        if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
            let model = std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "claude-haiku-4-5-20251001".into());
            let p = pagerunner_llm::anthropic::AnthropicProvider::new(api_key, model.clone());
            (Arc::new(p), "Anthropic".into(), model)
        } else if let Ok(api_key) = std::env::var("OPENROUTER_API_KEY") {
            let model = std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "nvidia/nemotron-nano-9b-v2:free".into());
            let p = pagerunner_llm::openai_compat::OpenAiCompatProvider::new(
                api_key,
                "https://openrouter.ai/api/v1",
                model.clone(),
            );
            (Arc::new(p), "OpenRouter".into(), model)
        } else if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
            let p = pagerunner_llm::openai_compat::OpenAiCompatProvider::new(
                api_key,
                "https://api.openai.com/v1",
                model.clone(),
            );
            (Arc::new(p), "OpenAI".into(), model)
        } else {
            let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "qwen2.5:3b".into());
            let p = pagerunner_llm::ollama::OllamaProvider::new(None::<String>, model.clone());
            (Arc::new(p), "Ollama".into(), model)
        };

    println!("{BOLD}Goal:{RESET} {goal}");
    println!("{DIM}Model: {model} via {provider_name} | Profile: {profile} | Real Chrome{RESET}");
    println!();
    println!("{DIM}{SEP}{RESET}");
    println!();

    // Connect to Pagerunner daemon
    let executor: Arc<dyn ToolExecutor> = match DaemonExecutor::connect(profile).await {
        Ok(e) => {
            println!("{GREEN}Connected to Pagerunner daemon{RESET}");
            Arc::new(e)
        }
        Err(e) => {
            println!("{RED}Failed to connect to daemon: {e}{RESET}");
            println!("{DIM}Make sure `pagerunner daemon` is running.{RESET}");
            std::process::exit(1);
        }
    };
    println!("{GREEN}Using {model} via {provider_name}{RESET}");
    println!();

    let (event_tx, mut event_rx) = broadcast::channel(64);
    let (_interrupt_tx, interrupt_rx) = watch::channel(false);
    let (_approval_tx, approval_rx) = mpsc::channel(16);

    let config = AgentConfig {
        provider: provider_name.to_lowercase(),
        model: model.clone(),
        budget: BudgetConfig {
            max_steps: 15,
            max_tokens_per_step: 4096,
            total_token_budget: 0,
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
                    let preview = if lines.len() > 8 {
                        let mut s = lines[..7].join("\n");
                        s.push_str(&format!(
                            "\n{DIM}  ... ({} more lines){RESET}",
                            lines.len() - 7
                        ));
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
                    // Strip session_id and target_id from display
                    let display: String = if let Ok(mut v) = serde_json::from_str::<Value>(&args_str) {
                        if let Some(obj) = v.as_object_mut() {
                            obj.remove("session_id");
                            obj.remove("target_id");
                        }
                        let s = serde_json::to_string(&v).unwrap_or_default();
                        if s.len() > 80 { format!("{}...", &s[..77]) } else { s }
                    } else {
                        args_str
                    };
                    println!("  {YELLOW}▶{RESET} {BOLD}{name}{RESET} {DIM}{display}{RESET}");
                }
                AgentEvent::ToolResult {
                    name,
                    result,
                    is_error,
                } => {
                    let icon = if is_error {
                        format!("{RED}✗{RESET}")
                    } else {
                        format!("{GREEN}✓{RESET}")
                    };
                    let preview = if result.len() > 120 {
                        format!("{}...", &result[..117])
                    } else {
                        result
                    };
                    println!("  {icon} {BLUE}{name}{RESET}: {DIM}{preview}{RESET}");
                    println!();
                }
                AgentEvent::Done { summary, .. } => {
                    println!("{DIM}{SEP}{RESET}");
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
        "live-demo".into(),
    )
    .await;

    // Give printer a moment to drain
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    drop(printer);

    println!();
    println!("{DIM}{SEP}{RESET}");
    println!(
        "{DIM}Steps: {} | Tokens: {} in + {} out | Outcome: {:?}{RESET}",
        result.total_steps,
        result.usage.input_tokens,
        result.usage.output_tokens,
        result.outcome,
    );
    println!();
}
