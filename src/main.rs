mod adapter_generator;
mod adapters;
mod anonymizer;
mod audit;
mod auth_token_detector;
mod browser;
mod cdp;
mod checkpoint;
mod chrome;
mod chrome_detect;
mod cli_tools;
mod config;
mod console_log;
mod daemon;
mod daemon_client;
mod db;
mod endpoint_mapper;
mod error;
mod init;
mod ipc;
mod mcp_server;
mod network_guard;
mod network_log;
pub mod notification;
mod sanitizer;
mod schema_inference;
mod security;
mod session;
pub mod session_health;
pub mod session_registry;
mod site_knowledge;
mod snapshot;
mod stealth;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "pagerunner", about = "Browser automation MCP server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start as MCP server (stdio transport)
    Mcp,
    /// List configured profiles
    Profiles,
    /// Print example config
    ExampleConfig,
    /// Run as persistent background daemon (Unix socket server)
    Daemon,
    /// Detect Chrome profiles and write ~/.pagerunner/config.toml
    Init {
        /// Overwrite existing config
        #[arg(long)]
        force: bool,
        /// Output JSON result instead of interactive prompts
        #[arg(long)]
        json: bool,
    },
    /// Show config, daemon, and database status
    Status,
    /// View audit log entries
    Audit {
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Show last N events (default: 50)
        #[arg(long, default_value = "50")]
        tail: usize,
        /// Show events since this RFC 3339 datetime (e.g. 2026-03-20T14:00:00Z)
        #[arg(long)]
        since: Option<String>,
    },
    /// List Chrome profiles (JSON output for agent use; see also `profiles` for human-readable)
    ListProfiles,
    /// Launch Chrome for a named profile
    OpenSession {
        profile: String,
        #[arg(long)]
        stealth: bool,
        /// Comma-separated domain allowlist (e.g. github.com,docs.rs)
        #[arg(long, value_delimiter = ',')]
        allowed_domains: Option<Vec<String>>,
        #[arg(long)]
        max_navigations: Option<u64>,
        #[arg(long)]
        sanitize_content: Option<bool>,
        #[arg(long)]
        scan_injections: Option<bool>,
        /// Comma-separated tool allowlist
        #[arg(long, value_delimiter = ',')]
        allowed_tools: Option<Vec<String>>,
        /// Comma-separated tool blocklist
        #[arg(long, value_delimiter = ',')]
        blocked_tools: Option<Vec<String>>,
        #[arg(long)]
        anonymize: bool,
        #[arg(long)]
        anonymization_profile: Option<String>,
        /// Comma-separated entity types: EMAIL,PHONE,CREDIT_CARD,IBAN,SSN,IP
        #[arg(long, value_delimiter = ',')]
        anonymization_entities: Option<Vec<String>>,
        /// tokenize or redact
        #[arg(long)]
        anonymization_mode: Option<String>,
    },
    /// Attach to an already-running Chrome (must be launched with --remote-debugging-port)
    #[command(name = "attach-session")]
    AttachSession {
        /// Port Chrome was launched with (e.g. 9222)
        #[arg(long)]
        debug_port: Option<u16>,
        /// Full base URL if Chrome is non-local (e.g. http://localhost:9222)
        #[arg(long)]
        debug_url: Option<String>,
        /// Optional display label for this session
        #[arg(long)]
        profile: Option<String>,
    },
    /// Close a Chrome session
    CloseSession { session_id: String },
    /// List open sessions
    ListSessions,
    /// List open tabs in a session
    ListTabs { session_id: String },
    /// Open a new tab in a session
    NewTab {
        session_id: String,
        #[arg(long)]
        url: Option<String>,
    },
    /// Close a specific tab (fails if it's the last tab in the session)
    #[command(name = "close-tab")]
    CloseTab {
        session_id: String,
        target_id: String,
    },
    /// Navigate to a URL
    Navigate {
        session_id: String,
        target_id: String,
        url: String,
    },
    /// Wait for a CSS selector, URL pattern, or fixed delay
    WaitFor {
        session_id: String,
        target_id: String,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        ms: Option<u64>,
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// Get page text content
    GetContent {
        session_id: String,
        target_id: String,
    },
    /// Take a screenshot
    Screenshot {
        session_id: String,
        target_id: String,
        /// Return base64 inline instead of writing a file
        #[arg(long)]
        base64: bool,
    },
    /// Evaluate a JavaScript expression
    Evaluate {
        session_id: String,
        target_id: String,
        expression: String,
    },
    /// Click an element by CSS selector
    Click {
        session_id: String,
        target_id: String,
        selector: String,
    },
    /// Type text (optionally focus a selector first)
    TypeText {
        session_id: String,
        target_id: String,
        text: String,
        #[arg(long)]
        selector: Option<String>,
    },
    /// Set an input field's value (works with React/Vue/Angular)
    Fill {
        session_id: String,
        target_id: String,
        selector: String,
        value: String,
    },
    /// Choose an option in a <select> dropdown
    Select {
        session_id: String,
        target_id: String,
        selector: String,
        value: String,
    },
    /// Scroll the page or scroll an element into view
    Scroll {
        session_id: String,
        target_id: String,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        x: Option<i64>,
        #[arg(long)]
        y: Option<i64>,
    },
    /// Save cookies and localStorage for an origin
    SaveSnapshot {
        session_id: String,
        target_id: String,
        /// Origin URL to capture (omit for all origins in session)
        #[arg(long)]
        origin: Option<String>,
    },
    /// Restore cookies and localStorage from a saved snapshot
    RestoreSnapshot {
        session_id: String,
        target_id: String,
        origin: String,
        /// Restore from this profile instead of current session's profile
        #[arg(long)]
        from_profile: Option<String>,
    },
    /// List saved browser state snapshots
    ListSnapshots {
        /// Filter to this profile only
        #[arg(long)]
        profile: Option<String>,
        /// Show all versions, not just latest per origin
        #[arg(long)]
        all: bool,
    },
    /// Delete saved snapshots for a profile+origin
    DeleteSnapshot {
        profile: String,
        origin: String,
        /// Unix microsecond timestamp of specific version to delete (omit for all)
        #[arg(long)]
        saved_at: Option<i64>,
    },
    /// Save a session checkpoint (tabs + auth state)
    #[command(name = "save-session-checkpoint")]
    SaveSessionCheckpoint {
        session_id: String,
        /// Optional name (auto-named if omitted)
        #[arg(long)]
        name: Option<String>,
    },
    /// Restore a session from a saved checkpoint
    #[command(name = "restore-session-checkpoint")]
    RestoreSessionCheckpoint {
        session_id: String,
        checkpoint_id: String,
    },
    /// List saved session checkpoints for a profile
    #[command(name = "list-session-checkpoints")]
    ListSessionCheckpoints {
        #[arg(long)]
        profile: String,
    },
    /// Delete a saved session checkpoint
    #[command(name = "delete-session-checkpoint")]
    DeleteSessionCheckpoint {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        checkpoint_id: String,
    },
    /// Save tab URLs and titles for later restoration
    SaveTabState { session_id: String },
    /// Reopen tabs from the most recently saved tab state
    RestoreTabState { session_id: String },
    /// Store a value in the encrypted KV store
    KvSet {
        namespace: String,
        key: String,
        value: String,
    },
    /// Retrieve a value from the encrypted KV store
    KvGet { namespace: String, key: String },
    /// Delete a key from the encrypted KV store
    KvDelete { namespace: String, key: String },
    /// List keys in a namespace
    KvList {
        namespace: String,
        /// Filter by key prefix
        #[arg(long)]
        prefix: Option<String>,
        /// Return only key names, no values
        #[arg(long)]
        keys_only: bool,
    },
    /// Delete all keys in a namespace
    KvClear { namespace: String },
    /// Run a command with a sealed secret injected via stdin.
    /// The secret value is NEVER printed — it flows directly to the command's stdin.
    /// Example: pagerunner use-secret npm_token -- gh secret set NPM_TOKEN --repos owner/repo
    #[command(name = "use-secret")]
    UseSecret {
        /// Name of the secret (as stored via extract_secret or store_as_secret)
        name: String,
        /// Command and arguments to run. The secret value is piped to its stdin.
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// List secret names stored in the sealed store (names only — values are never shown).
    #[command(name = "list-secrets")]
    ListSecrets,
    /// Delete a named secret from the sealed store.
    #[command(name = "delete-secret")]
    DeleteSecret { name: String },
    /// Query network requests captured during a session
    #[command(name = "get-network-log")]
    GetNetworkLog {
        session_id: String,
        #[arg(long)]
        target_id: Option<String>,
        #[arg(long)]
        url_pattern: Option<String>,
        #[arg(long)]
        method: Option<String>,
        #[arg(long)]
        status_min: Option<u16>,
        #[arg(long)]
        status_max: Option<u16>,
        #[arg(long)]
        lookback_ms: Option<u64>,
        #[arg(long, default_value = "50")]
        limit: usize,
        #[arg(long)]
        include_request_body: bool,
        #[arg(long)]
        full_response: bool,
        #[arg(long)]
        all_tabs: bool,
    },
    /// Query captured browser console messages and JS exceptions for a tab
    #[command(name = "get-console-log")]
    GetConsoleLog {
        session_id: String,
        #[arg(long)]
        target_id: String,
        #[arg(long, default_value = "10")]
        limit: u64,
    },
    /// Get what pagerunner knows about a site
    #[command(name = "get-site-knowledge")]
    GetSiteKnowledge {
        /// Site origin, e.g. 'https://linear.app'
        origin: String,
    },
    /// Register a JS adapter for direct API calls to a site
    #[command(name = "register-adapter")]
    RegisterAdapter {
        /// Site origin
        origin: String,
        /// Unique adapter name
        name: String,
        /// Description of what this adapter does
        description: String,
        /// JS function body (receives 'params' and 'session' args)
        js_code: String,
    },
    /// Call a registered site adapter
    #[command(name = "call-site-api")]
    CallSiteApi {
        /// Session ID
        session_id: String,
        /// Target (tab) ID
        target_id: String,
        /// Site origin
        origin: String,
        /// Adapter name
        name: String,
        /// JSON params to pass to adapter
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// Generate a JS adapter for a site using the Claude API
    #[command(name = "generate-adapter")]
    GenerateAdapter {
        /// Site origin, e.g. https://linear.app
        origin: String,
        /// Adapter name
        name: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
    /// Evaluate a JavaScript expression in a tab and store the result as a named secret
    /// in the sealed store. The value never appears in stdout or logs.
    /// Example: pagerunner extract-secret <session> <target> "document.querySelector('.token').textContent.trim()" npm_token
    #[command(name = "extract-secret")]
    ExtractSecret {
        session_id: String,
        target_id: String,
        /// JavaScript expression whose result is the secret value
        expression: String,
        /// Name to store the secret under (e.g. npm_token, stripe_key)
        name: String,
    },
    /// Send a macOS notification via the Pagerunner menu bar.
    #[command(name = "notify")]
    Notify {
        /// Notification title
        title: String,
        /// Optional body text
        #[arg(long)]
        body: Option<String>,
        /// Severity level: info (default), warning, or error
        #[arg(long, default_value = "info")]
        level: String,
        /// Associate with a session ID (used for menu bar deep-link)
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Download the NER model for PERSON/ORG name detection (requires --features ner build)
    DownloadModel,
}

/// Resolve the state DB path: PAGERUNNER_DB_PATH env var takes precedence,
/// otherwise defaults to ~/.pagerunner/state.db.
/// All CLI commands that open the DB directly must use this so that test
/// isolation (PAGERUNNER_DB_PATH=/tmp/...) works correctly.
fn resolve_db_path() -> crate::error::Result<std::path::PathBuf> {
    if let Ok(p) = std::env::var("PAGERUNNER_DB_PATH") {
        return Ok(std::path::PathBuf::from(p));
    }
    let home = dirs::home_dir()
        .ok_or_else(|| crate::error::PagerunnerError::Config("Cannot find home dir".into()))?;
    Ok(home.join(".pagerunner/state.db"))
}

#[cfg(feature = "ner")]
fn download_ner_model() -> anyhow::Result<()> {
    use crate::anonymizer::ner::{verify_model_hash, MODEL_SHA256, MODEL_URL, TOKENIZER_URL};

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    let model_dir = home.join(".pagerunner/models");
    let model_path = model_dir.join("ner.onnx");
    let tok_path = model_dir.join("tokenizer.json");

    std::fs::create_dir_all(&model_dir)?;

    // Model file — idempotent
    if model_path.exists() && verify_model_hash(&model_path).is_ok() {
        println!("NER model already up to date: {}", model_path.display());
    } else {
        if model_path.exists() {
            eprintln!("Existing model has wrong hash — re-downloading.");
            std::fs::remove_file(&model_path)?;
        }
        download_file(MODEL_URL, &model_path, Some(MODEL_SHA256))?;
        println!("NER model downloaded: {}", model_path.display());
    }

    // Tokenizer — no hash check (small file, updated together with model)
    if tok_path.exists() {
        println!("Tokenizer already present: {}", tok_path.display());
    } else {
        download_file(TOKENIZER_URL, &tok_path, None)?;
        println!("Tokenizer downloaded: {}", tok_path.display());
    }

    Ok(())
}

#[cfg(feature = "ner")]
fn download_file(
    url: &str,
    dest: &std::path::Path,
    expected_sha256: Option<&str>,
) -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};

    eprintln!("Downloading {} -> {}", url, dest.display());

    let client = reqwest::blocking::Client::new();
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| anyhow::anyhow!("Download failed: {}", e))?;

    let total = resp.content_length();
    let mut downloaded = 0u64;
    let mut hasher = Sha256::new();
    let tmp_path = dest.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp_path)?;
    let mut buf = vec![0u8; 65536];

    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        if let Some(t) = total {
            eprint!(
                "\r  {:.1} MB / {:.1} MB",
                downloaded as f64 / 1e6,
                t as f64 / 1e6
            );
        } else {
            eprint!("\r  {:.1} MB", downloaded as f64 / 1e6);
        }
    }
    eprintln!();
    drop(file);

    if let Some(expected) = expected_sha256 {
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected {
            std::fs::remove_file(&tmp_path).ok();
            return Err(anyhow::anyhow!(
                "Hash mismatch!\nExpected: {}\nActual:   {}\nFile deleted.",
                expected,
                actual
            ));
        }
    }

    std::fs::rename(&tmp_path, dest)?;
    Ok(())
}

fn format_audit_event(event: &crate::audit::AuditEvent) -> String {
    let ts = event.timestamp.format("%Y-%m-%d %H:%M:%S UTC");
    match &event.kind {
        crate::audit::AuditEventKind::SessionOpened {
            session_id,
            profile,
            stealth,
            ..
        } => {
            let sid = if session_id.len() >= 8 {
                &session_id[..8]
            } else {
                session_id
            };
            format!(
                "[{}] SESSION_OPENED  session={} profile={} stealth={}",
                ts, sid, profile, stealth
            )
        }
        crate::audit::AuditEventKind::SessionClosed { session_id } => {
            let sid = if session_id.len() >= 8 {
                &session_id[..8]
            } else {
                session_id
            };
            format!("[{}] SESSION_CLOSED  session={}", ts, sid)
        }
        crate::audit::AuditEventKind::ToolCall {
            session_id,
            tool,
            args_summary,
            outcome,
            security_violation,
        } => {
            let sid = session_id.as_deref().unwrap_or("-");
            let sid = if sid.len() >= 8 { &sid[..8] } else { sid };
            let ok = matches!(outcome, crate::audit::ToolOutcome::Success);
            let sv = if *security_violation {
                " [SECURITY]"
            } else {
                ""
            };
            let status = if ok { "OK" } else { "ERR" };
            format!(
                "[{}] TOOL_CALL {}{} session={} tool={} args={}",
                ts, status, sv, sid, tool, args_summary
            )
        }
        crate::audit::AuditEventKind::SecurityEvent {
            session_id,
            kind,
            detail,
        } => {
            let sid = session_id.as_deref().unwrap_or("-");
            let sid = if sid.len() >= 8 { &sid[..8] } else { sid };
            format!(
                "[{}] SECURITY {:?} session={} detail={}",
                ts, kind, sid, detail
            )
        }
        crate::audit::AuditEventKind::ContentAnonymized {
            session_id,
            target_id,
            mode,
            entity_counts,
        } => {
            let sid = if session_id.len() >= 8 {
                &session_id[..8]
            } else {
                session_id
            };
            let counts: Vec<String> = entity_counts
                .iter()
                .map(|(k, v)| format!("{}:{}", k, v))
                .collect();
            format!(
                "[{}] CONTENT_ANONYMIZED session={} target={} mode={} counts={}",
                ts,
                sid,
                target_id,
                mode,
                counts.join(",")
            )
        }
        crate::audit::AuditEventKind::AdapterRegistered {
            origin,
            name,
            trusted,
        } => {
            format!(
                "[{}] ADAPTER_REGISTERED origin={} name={} trusted={}",
                ts, origin, name, trusted
            )
        }
        crate::audit::AuditEventKind::AuthTokenDetected { origin, kind } => {
            format!(
                "[{}] AUTH_TOKEN_DETECTED origin={} kind={}",
                ts, origin, kind
            )
        }
        crate::audit::AuditEventKind::SiteApiCalled {
            origin,
            adapter_name,
        } => {
            format!(
                "[{}] SITE_API_CALLED origin={} adapter={}",
                ts, origin, adapter_name
            )
        }
        crate::audit::AuditEventKind::SecretScrubbed {
            session_id,
            target_id,
            count,
        } => {
            let sid = if session_id.len() >= 8 {
                &session_id[..8]
            } else {
                session_id
            };
            format!(
                "[{}] SECRET_SCRUBBED  session={} target={} count={}",
                ts, sid, target_id, count
            )
        }
        crate::audit::AuditEventKind::SecretStored { name, source } => {
            format!("[{}] SECRET_STORED   name={} source={}", ts, name, source)
        }
        crate::audit::AuditEventKind::SecretUsed { name, command } => {
            format!("[{}] SECRET_USED     name={} command={}", ts, name, command)
        }
        crate::audit::AuditEventKind::AnonymizationGap {
            session_id,
            target_id,
            entity_counts,
            source,
        } => {
            let sid = if session_id.len() >= 8 {
                &session_id[..8]
            } else {
                session_id
            };
            let entities: Vec<String> = entity_counts
                .iter()
                .map(|(k, v)| format!("{}:{}", k, v))
                .collect();
            format!(
                "[{}] ANONYMIZATION_GAP  session={} target={} source={} entities=[{}]",
                ts,
                sid,
                target_id,
                source,
                entities.join(", ")
            )
        }
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        if let Some(pe) = e.downcast_ref::<crate::error::PagerunnerError>() {
            eprintln!("Error: {}", pe);
            eprintln!("error_type: {}", pe.error_type());
            eprintln!("recovery_hint: {}", pe.recovery_hint());
        } else {
            eprintln!("Error: {}", e);
        }
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Mcp => mcp_server::run().await?,
        Commands::Profiles => {
            let config = config::PagerunnerConfig::load()?;
            for p in &config.profiles {
                println!(
                    "{}: {}",
                    p.name,
                    p.user_data_dir.as_deref().unwrap_or("(attached)")
                );
            }
        }
        Commands::ExampleConfig => {
            println!("{}", include_str!("../config.example.toml"));
        }
        Commands::Daemon => daemon::run().await?,
        Commands::Init { force, json } => {
            if let Err(e) = crate::init::run(force, json) {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({ "ok": false, "error": e.to_string() })
                    );
                } else {
                    eprintln!("Error: {}", e);
                }
                std::process::exit(1);
            }
        }
        Commands::Status => {
            let home =
                dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
            let config_path = home.join(".pagerunner/config.toml");
            let socket_path = home.join(ipc::SOCKET_SUBPATH);
            let db_path = home.join(".pagerunner/state.db");

            // Config
            if config_path.exists() {
                match config::PagerunnerConfig::load() {
                    Ok(cfg) if cfg.profiles.is_empty() => {
                        println!(
                            "Config:  {} ⚠️  (no profiles — run `pagerunner init`)",
                            config_path.display()
                        );
                    }
                    Ok(cfg) => {
                        println!(
                            "Config:  {} ✓ ({} profile{})",
                            config_path.display(),
                            cfg.profiles.len(),
                            if cfg.profiles.len() == 1 { "" } else { "s" }
                        );
                        for p in &cfg.profiles {
                            println!("  • {}  ({})", p.name, p.display_name);
                        }
                    }
                    Err(e) => println!("Config:  {} ✗ ({})", config_path.display(), e),
                }
            } else {
                println!(
                    "Config:  {} ✗ (not found — run `pagerunner init`)",
                    config_path.display()
                );
            }

            // Daemon
            let daemon_ok = daemon_client::DaemonClient::connect().await.is_ok();
            if daemon_ok {
                println!("Daemon:  {} ✓ running", socket_path.display());
            } else {
                println!(
                    "Daemon:  {} ✗ not running  (standalone mode — OK for single session; run `pagerunner daemon &` to share state across sessions)",
                    socket_path.display()
                );
            }

            // DB
            if db_path.exists() {
                println!("DB:      {} ✓", db_path.display());
            } else {
                println!("DB:      {} (created on first use)", db_path.display());
            }

            // NER model
            #[cfg(feature = "ner")]
            {
                let model_path = home.join(".pagerunner/models/ner.onnx");
                let ner_config_disabled = crate::config::PagerunnerConfig::load()
                    .map(|c| c.ner.enabled == Some(false))
                    .unwrap_or(false);
                let ner_line = if ner_config_disabled {
                    "disabled in config".to_string()
                } else if !model_path.exists() {
                    "missing  (run `pagerunner download-model` to install)".to_string()
                } else {
                    match crate::anonymizer::ner::verify_model_hash(&model_path) {
                        Ok(()) => format!("present, hash ok  ({})", model_path.display()),
                        Err(e) => format!("CORRUPT: {}  (run `pagerunner download-model`)", e),
                    }
                };
                println!("NER:     {}", ner_line);
            }
            #[cfg(not(feature = "ner"))]
            println!("NER:     not compiled  (rebuild with --features ner to enable)");
        }
        Commands::ListProfiles => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "list_profiles",
                serde_json::json!({}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::OpenSession {
            profile,
            stealth,
            allowed_domains,
            max_navigations,
            sanitize_content,
            scan_injections,
            allowed_tools,
            blocked_tools,
            anonymize,
            anonymization_profile,
            anonymization_entities,
            anonymization_mode,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"profile": profile});
            if stealth {
                args["stealth"] = serde_json::json!(true);
            }
            if let Some(v) = allowed_domains {
                args["allowed_domains"] = serde_json::json!(v);
            }
            if let Some(v) = max_navigations {
                args["max_navigations"] = serde_json::json!(v);
            }
            if let Some(v) = sanitize_content {
                args["sanitize_content"] = serde_json::json!(v);
            }
            if let Some(v) = scan_injections {
                args["scan_injections"] = serde_json::json!(v);
            }
            if let Some(v) = allowed_tools {
                args["allowed_tools"] = serde_json::json!(v);
            }
            if let Some(v) = blocked_tools {
                args["blocked_tools"] = serde_json::json!(v);
            }
            if anonymize {
                args["anonymize"] = serde_json::json!(true);
            }
            if let Some(v) = anonymization_profile {
                args["anonymization_profile"] = serde_json::json!(v);
            }
            if let Some(v) = anonymization_entities {
                args["anonymization_entities"] = serde_json::json!(v);
            }
            if let Some(v) = anonymization_mode {
                args["anonymization_mode"] = serde_json::json!(v);
            }
            crate::cli_tools::run_tool(
                "open_session",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::AttachSession {
            debug_port,
            debug_url,
            profile,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({});
            if let Some(port) = debug_port {
                args["debug_port"] = serde_json::json!(port);
            } else if let Some(url) = debug_url {
                args["debug_url"] = serde_json::json!(url);
            } else {
                eprintln!(
                    "error: attach-session requires --debug-port <PORT> or --debug-url <URL>"
                );
                std::process::exit(1);
            }
            if let Some(p) = profile {
                args["profile"] = serde_json::json!(p);
            }
            crate::cli_tools::run_tool(
                "attach_session",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::CloseSession { session_id } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "close_session",
                serde_json::json!({"session_id": session_id}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::ListSessions => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "list_sessions",
                serde_json::json!({}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::ListTabs { session_id } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "list_tabs",
                serde_json::json!({"session_id": session_id}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::NewTab { session_id, url } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"session_id": session_id});
            if let Some(u) = url {
                args["url"] = serde_json::json!(u);
            }
            crate::cli_tools::run_tool(
                "new_tab",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::CloseTab {
            session_id,
            target_id,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "close_tab",
                serde_json::json!({"session_id": session_id, "target_id": target_id}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::Navigate {
            session_id,
            target_id,
            url,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "navigate",
                serde_json::json!({"session_id": session_id, "target_id": target_id, "url": url}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::WaitFor {
            session_id,
            target_id,
            selector,
            url,
            ms,
            timeout_ms,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"session_id": session_id, "target_id": target_id});
            if let Some(v) = selector {
                args["selector"] = serde_json::json!(v);
            }
            if let Some(v) = url {
                args["url"] = serde_json::json!(v);
            }
            if let Some(v) = ms {
                args["ms"] = serde_json::json!(v);
            }
            if let Some(v) = timeout_ms {
                args["timeout_ms"] = serde_json::json!(v);
            }
            crate::cli_tools::run_tool(
                "wait_for",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::GetContent {
            session_id,
            target_id,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "get_content",
                serde_json::json!({"session_id": session_id, "target_id": target_id}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::Screenshot {
            session_id,
            target_id,
            base64,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mode = if base64 {
                crate::cli_tools::ScreenshotMode::Base64
            } else {
                crate::cli_tools::ScreenshotMode::File
            };
            crate::cli_tools::run_tool(
                "screenshot",
                serde_json::json!({"session_id": session_id, "target_id": target_id}),
                mode,
                &config,
            )
            .await?;
        }
        Commands::Evaluate {
            session_id,
            target_id,
            expression,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool("evaluate",
                serde_json::json!({"session_id": session_id, "target_id": target_id, "expression": expression}),
                crate::cli_tools::ScreenshotMode::File, &config).await?;
        }
        Commands::Click {
            session_id,
            target_id,
            selector,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool("click",
                serde_json::json!({"session_id": session_id, "target_id": target_id, "selector": selector}),
                crate::cli_tools::ScreenshotMode::File, &config).await?;
        }
        Commands::TypeText {
            session_id,
            target_id,
            text,
            selector,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args =
                serde_json::json!({"session_id": session_id, "target_id": target_id, "text": text});
            if let Some(v) = selector {
                args["selector"] = serde_json::json!(v);
            }
            crate::cli_tools::run_tool(
                "type_text",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::Fill {
            session_id,
            target_id,
            selector,
            value,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool("fill",
                serde_json::json!({"session_id": session_id, "target_id": target_id, "selector": selector, "value": value}),
                crate::cli_tools::ScreenshotMode::File, &config).await?;
        }
        Commands::Select {
            session_id,
            target_id,
            selector,
            value,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool("select",
                serde_json::json!({"session_id": session_id, "target_id": target_id, "selector": selector, "value": value}),
                crate::cli_tools::ScreenshotMode::File, &config).await?;
        }
        Commands::Scroll {
            session_id,
            target_id,
            selector,
            x,
            y,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"session_id": session_id, "target_id": target_id});
            if let Some(v) = selector {
                args["selector"] = serde_json::json!(v);
            }
            if let Some(v) = x {
                args["x"] = serde_json::json!(v);
            }
            if let Some(v) = y {
                args["y"] = serde_json::json!(v);
            }
            crate::cli_tools::run_tool(
                "scroll",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::SaveSnapshot {
            session_id,
            target_id,
            origin,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"session_id": session_id, "target_id": target_id});
            if let Some(v) = origin {
                args["origin"] = serde_json::json!(v);
            }
            crate::cli_tools::run_tool(
                "save_snapshot",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::RestoreSnapshot {
            session_id,
            target_id,
            origin,
            from_profile,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"session_id": session_id, "target_id": target_id, "origin": origin});
            if let Some(v) = from_profile {
                args["from_profile"] = serde_json::json!(v);
            }
            crate::cli_tools::run_tool(
                "restore_snapshot",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::ListSnapshots { profile, all } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({});
            if let Some(v) = profile {
                args["profile"] = serde_json::json!(v);
            }
            if all {
                args["latest_only"] = serde_json::json!(false);
            }
            crate::cli_tools::run_tool(
                "list_snapshots",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::DeleteSnapshot {
            profile,
            origin,
            saved_at,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"profile": profile, "origin": origin});
            if let Some(v) = saved_at {
                args["saved_at"] = serde_json::json!(v);
            }
            crate::cli_tools::run_tool(
                "delete_snapshot",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::SaveSessionCheckpoint { session_id, name } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"session_id": session_id});
            if let Some(n) = name {
                args["name"] = serde_json::json!(n);
            }
            crate::cli_tools::run_tool(
                "save_session_checkpoint",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::RestoreSessionCheckpoint {
            session_id,
            checkpoint_id,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "restore_session_checkpoint",
                serde_json::json!({"session_id": session_id, "checkpoint_id": checkpoint_id}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::ListSessionCheckpoints { profile } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "list_session_checkpoints",
                serde_json::json!({"profile": profile}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::DeleteSessionCheckpoint {
            profile,
            checkpoint_id,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "delete_session_checkpoint",
                serde_json::json!({"profile": profile, "checkpoint_id": checkpoint_id}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::SaveTabState { session_id } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "save_tab_state",
                serde_json::json!({"session_id": session_id}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::RestoreTabState { session_id } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "restore_tab_state",
                serde_json::json!({"session_id": session_id}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::KvSet {
            namespace,
            key,
            value,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "kv_set",
                serde_json::json!({"namespace": namespace, "key": key, "value": value}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::KvGet { namespace, key } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "kv_get",
                serde_json::json!({"namespace": namespace, "key": key}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::KvDelete { namespace, key } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "kv_delete",
                serde_json::json!({"namespace": namespace, "key": key}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::KvList {
            namespace,
            prefix,
            keys_only,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"namespace": namespace});
            if let Some(v) = prefix {
                args["prefix"] = serde_json::json!(v);
            }
            if keys_only {
                args["include_values"] = serde_json::json!(false);
            }
            crate::cli_tools::run_tool(
                "kv_list",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::KvClear { namespace } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "kv_clear",
                serde_json::json!({"namespace": namespace}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }

        Commands::UseSecret { name, command } => {
            if command.is_empty() {
                eprintln!("Usage: pagerunner use-secret <name> -- <command> [args...]");
                eprintln!("Example: pagerunner use-secret npm_token -- gh secret set NPM_TOKEN --repos owner/repo");
                std::process::exit(1);
            }
            let db_path = resolve_db_path()?;
            if !db_path.exists() {
                eprintln!("No secrets found (database not yet created).");
                std::process::exit(1);
            }
            let db_path_str = db_path
                .to_str()
                .ok_or_else(|| crate::error::PagerunnerError::Config("Non-UTF-8 db path".into()))?;
            let db = crate::db::Db::open(db_path_str)?;
            let secret_bytes = db
                .get(crate::mcp_server::SEALED_SECRETS_TABLE, &name)?
                .ok_or_else(|| {
                    crate::error::PagerunnerError::Config(format!(
                        "Secret '{}' not found. List available secrets with: pagerunner list-secrets",
                        name
                    ))
                })?;
            let secret_value = String::from_utf8(secret_bytes).map_err(|e| {
                crate::error::PagerunnerError::Config(format!("Secret is not valid UTF-8: {}", e))
            })?;

            // Emit audit event — command binary only, never the full args
            let audit_path = dirs::home_dir()
                .expect("No home dir")
                .join(".pagerunner/audit.log");
            let audit = crate::audit::AuditLog::new(audit_path, std::sync::Arc::new(db));
            audit
                .record(crate::audit::AuditEvent::new(
                    crate::audit::AuditEventKind::SecretUsed {
                        name: name.clone(),
                        command: command[0].clone(),
                    },
                ))
                .await;

            // Pipe secret to command stdin — value never touches stdout/stderr
            use std::io::Write;
            use std::process::{Command, Stdio};
            let mut child = Command::new(&command[0])
                .args(&command[1..])
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    crate::error::PagerunnerError::Config(format!(
                        "Failed to spawn '{}': {}",
                        command[0], e
                    ))
                })?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(secret_value.as_bytes()).ok();
            }
            let status = child.wait().map_err(|e| {
                crate::error::PagerunnerError::Config(format!("Command wait failed: {}", e))
            })?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }

        Commands::ListSecrets => {
            let db_path = resolve_db_path()?;
            if !db_path.exists() {
                println!("{{\"secrets\":[]}}");
                return Ok(());
            }
            let db_path_str = db_path
                .to_str()
                .ok_or_else(|| crate::error::PagerunnerError::Config("Non-UTF-8 db path".into()))?;
            let db = crate::db::Db::open(db_path_str)?;
            let entries = db.scan_prefix(crate::mcp_server::SEALED_SECRETS_TABLE, "")?;
            let names: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
            println!("{}", serde_json::json!({"secrets": names}));
        }

        Commands::DeleteSecret { name } => {
            let db_path = resolve_db_path()?;
            let db_path_str = db_path
                .to_str()
                .ok_or_else(|| crate::error::PagerunnerError::Config("Non-UTF-8 db path".into()))?;
            let db = crate::db::Db::open(db_path_str)?;
            db.delete(crate::mcp_server::SEALED_SECRETS_TABLE, &name)?;
            println!("{}", serde_json::json!({"ok": true, "deleted": name}));
        }

        Commands::ExtractSecret {
            session_id,
            target_id,
            expression,
            name,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "extract_secret",
                serde_json::json!({
                    "session_id": session_id,
                    "target_id": target_id,
                    "expression": expression,
                    "name": name,
                }),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }

        Commands::Notify {
            title,
            body,
            level,
            session_id,
        } => {
            let config = config::PagerunnerConfig::load()?;
            let mut args = serde_json::json!({"title": title, "level": level});
            if let Some(b) = body {
                args["body"] = serde_json::json!(b);
            }
            if let Some(sid) = session_id {
                args["session_id"] = serde_json::json!(sid);
            }
            crate::cli_tools::run_tool(
                "notify",
                args,
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }

        Commands::GetNetworkLog {
            session_id,
            target_id,
            url_pattern,
            method,
            status_min,
            status_max,
            lookback_ms,
            limit,
            include_request_body,
            full_response,
            all_tabs,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "get_network_log",
                serde_json::json!({
                    "session_id": session_id,
                    "target_id": target_id,
                    "url_pattern": url_pattern,
                    "method": method,
                    "status_min": status_min,
                    "status_max": status_max,
                    "lookback_ms": lookback_ms,
                    "limit": limit,
                    "include_request_body": include_request_body,
                    "full_response": full_response,
                    "all_tabs": all_tabs
                }),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::GetConsoleLog {
            session_id,
            target_id,
            limit,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "get_console_log",
                serde_json::json!({
                    "session_id": session_id,
                    "target_id": target_id,
                    "limit": limit,
                }),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::GetSiteKnowledge { origin } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "get_site_knowledge",
                serde_json::json!({"origin": origin}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::RegisterAdapter {
            origin,
            name,
            description,
            js_code,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "register_adapter",
                serde_json::json!({"origin": origin, "name": name, "description": description, "js_code": js_code}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::CallSiteApi {
            session_id,
            target_id,
            origin,
            name,
            params,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_tool(
                "call_site_api",
                serde_json::json!({"session_id": session_id, "target_id": target_id, "origin": origin, "name": name, "params": serde_json::from_str::<serde_json::Value>(&params).unwrap_or(serde_json::json!({}))}),
                crate::cli_tools::ScreenshotMode::File,
                &config,
            )
            .await?;
        }
        Commands::GenerateAdapter {
            origin,
            name,
            description,
        } => {
            let config = config::PagerunnerConfig::load()?;
            crate::cli_tools::run_generate_adapter(&origin, &name, description.as_deref(), &config)
                .await?;
        }
        Commands::DownloadModel => {
            #[cfg(not(feature = "ner"))]
            {
                eprintln!("Error: this binary was not compiled with --features ner.");
                eprintln!("Rebuild with: cargo build --release --features ner");
                std::process::exit(1);
            }
            #[cfg(feature = "ner")]
            download_ner_model()?;
        }
        Commands::Audit {
            session,
            tail,
            since,
        } => {
            let since_dt: Option<DateTime<Utc>> = if let Some(s) = &since {
                match DateTime::parse_from_rfc3339(s) {
                    Ok(dt) => Some(dt.with_timezone(&Utc)),
                    Err(_) => {
                        eprintln!(
                            "Error: --since must be RFC 3339 format (e.g. 2026-03-20T14:00:00Z)"
                        );
                        std::process::exit(1);
                    }
                }
            } else {
                None
            };

            let db_path = resolve_db_path()?;
            if !db_path.exists() {
                eprintln!("No audit records found (database not yet created).");
                return Ok(());
            }
            let db_path_str = db_path
                .to_str()
                .ok_or_else(|| crate::error::PagerunnerError::Config("Non-UTF-8 db path".into()))?;
            let db = crate::db::Db::open(db_path_str)?;
            let entries = db.scan_prefix("audit", "")?;

            let mut events: Vec<crate::audit::AuditEvent> = entries
                .iter()
                .filter_map(|(_, v)| serde_json::from_slice(v).ok())
                .collect();

            if let Some(ref sid) = session {
                events.retain(|e| {
                    let event_sid: Option<&str> = match &e.kind {
                        crate::audit::AuditEventKind::SessionOpened { session_id, .. } => {
                            Some(session_id)
                        }
                        crate::audit::AuditEventKind::SessionClosed { session_id } => {
                            Some(session_id)
                        }
                        crate::audit::AuditEventKind::ToolCall { session_id, .. } => {
                            session_id.as_deref()
                        }
                        crate::audit::AuditEventKind::SecurityEvent { session_id, .. } => {
                            session_id.as_deref()
                        }
                        crate::audit::AuditEventKind::ContentAnonymized { session_id, .. } => {
                            Some(session_id)
                        }
                        crate::audit::AuditEventKind::AdapterRegistered { .. }
                        | crate::audit::AuditEventKind::AuthTokenDetected { .. }
                        | crate::audit::AuditEventKind::SiteApiCalled { .. }
                        | crate::audit::AuditEventKind::SecretStored { .. }
                        | crate::audit::AuditEventKind::SecretUsed { .. } => None,
                        crate::audit::AuditEventKind::SecretScrubbed { session_id, .. } => {
                            Some(session_id.as_str())
                        }
                        crate::audit::AuditEventKind::AnonymizationGap { session_id, .. } => {
                            Some(session_id.as_str())
                        }
                    };
                    event_sid == Some(sid.as_str())
                });
            }

            if let Some(dt) = since_dt {
                events.retain(|e| e.timestamp >= dt);
            }

            let start = events.len().saturating_sub(tail);
            for event in &events[start..] {
                println!("{}", format_audit_event(event));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod download_tests {
    #[test]
    #[cfg(feature = "ner")]
    fn test_verify_model_hash_rejects_wrong_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ner.onnx");
        std::fs::write(&path, b"not a real model").unwrap();
        let result = crate::anonymizer::ner::verify_model_hash(&path);
        assert!(result.is_err(), "wrong file must not match pinned hash");
    }
}
