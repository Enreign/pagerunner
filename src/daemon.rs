use crate::config::PagerunnerConfig;
use crate::db::Db;
use crate::error::Result;
use crate::ipc::{DaemonEvent, DaemonMessage, DaemonRequest, DaemonResponse, SOCKET_SUBPATH};
use crate::session::SessionManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, mpsc, watch, Mutex};

pub async fn run() -> Result<()> {
    let config = PagerunnerConfig::load()?;
    let sessions = Arc::new(Mutex::new(SessionManager::new()));

    let home = dirs::home_dir().ok_or_else(|| {
        crate::error::PagerunnerError::Config("home directory not accessible".into())
    })?;

    // Load API keys from ~/.pagerunner/.env if present (for agent LLM providers).
    let env_path = home.join(".pagerunner/.env");
    if env_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&env_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();
                    if !key.is_empty() && !value.is_empty() && std::env::var(key).is_err() {
                        std::env::set_var(key, value);
                    }
                }
            }
            tracing::debug!("Loaded env from {:?}", env_path);
        }
    }

    let db_path_str = std::env::var("PAGERUNNER_DB_PATH").unwrap_or_else(|_| {
        home.join(".pagerunner/state.db")
            .to_str()
            .unwrap()
            .to_string()
    });
    let db_path = std::path::Path::new(&db_path_str);
    std::fs::create_dir_all(db_path.parent().unwrap())?;
    let db = Arc::new(Db::open(&db_path_str)?);

    let socket_path = home.join(SOCKET_SUBPATH);
    // Remove stale socket
    let _ = std::fs::remove_file(&socket_path);
    std::fs::create_dir_all(socket_path.parent().unwrap())?;

    let listener = UnixListener::bind(&socket_path)
        .map_err(|e| crate::error::PagerunnerError::Config(format!("Cannot bind socket: {}", e)))?;

    // chmod 0600 — owner only
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
    }

    tracing::info!("Pagerunner daemon listening on {:?}", socket_path);

    // Start HTTP API server if enabled
    let event_tx: broadcast::Sender<crate::ipc::DaemonEvent> = broadcast::channel(256).0;
    if config.http_api.enabled {
        if config.http_api.token.is_empty() {
            tracing::warn!("HTTP API enabled but no token configured — refusing to start without auth");
        } else {
            let http_config = config.http_api.clone();
            let http_sessions = Arc::clone(&sessions);
            let http_db = Arc::clone(&db);
            let http_app_config = config.clone();
            let http_event_tx = event_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::http_api::start_http_server(
                    &http_config,
                    http_app_config,
                    http_sessions,
                    http_db,
                    http_event_tx,
                )
                .await
                {
                    tracing::error!("HTTP API server failed: {}", e);
                }
            });
        }
    }

    // Reattach surviving Chrome sessions. Passing site_store: None here is intentional —
    // the daemon constructs site knowledge per-request in dispatch_tool_inner, not at startup.
    let reattached = crate::session_registry::reconcile_sessions(
        &db, &sessions, &config, None, // site_store: not needed for basic reattach
    )
    .await;
    if !reattached.is_empty() {
        tracing::info!(
            "Daemon: reattached {} surviving Chrome session(s)",
            reattached.len()
        );
    }

    // Cleanup old recordings based on retention policy
    if config.recording.retention_days > 0 {
        match crate::recording::cleanup_old_recordings(&db, config.recording.retention_days) {
            Ok(n) if n > 0 => tracing::info!(deleted = n, "Recording retention cleanup"),
            Err(e) => tracing::warn!(error = %e, "Recording retention cleanup failed"),
            _ => {}
        }
    }

    // Background reconnection task: periodically checks for sessions in Reconnecting
    // state and attempts to reconnect them. Acquires and releases the lock around
    // each step to avoid blocking tool calls.
    let reconnect_sessions = Arc::clone(&sessions);
    let db_for_reconnect = Arc::clone(&db);
    let config_for_reconnect = config.clone();
    let _reconnect_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;

            // Step 1: lock, call list() to detect disconnections, collect reconnecting IDs
            let reconnecting_ids: Vec<String> = {
                let mut mgr = reconnect_sessions.lock().await;
                let sessions = mgr.list();
                sessions
                    .into_iter()
                    .filter(|s| s.status == "reconnecting")
                    .map(|s| s.id)
                    .collect()
            };

            // Step 2: for each reconnecting session, extract info under lock,
            // do the actual reconnection outside the lock, then update state under lock.
            for id in reconnecting_ids {
                // Extract what we need under lock
                let reconnect_info = {
                    let mgr = reconnect_sessions.lock().await;
                    mgr.get(&id).and_then(|s| {
                        if s.health != crate::session_health::SessionHealth::Reconnecting {
                            return None;
                        }
                        if s.debug_port == 0 {
                            return None; // secondary session, skip
                        }
                        Some((s.debug_port, s.cdp.clone()))
                    })
                };
                // Lock released here

                if let Some((debug_port, cdp)) = reconnect_info {
                    // Do the actual reconnection WITHOUT holding the lock.
                    // This can take up to 30s but won't block tool calls.
                    let result = crate::cdp_reconnect::reconnect_cdp(debug_port, &cdp).await;

                    // Re-acquire lock to update session state
                    let mut mgr = reconnect_sessions.lock().await;
                    if let Some(session) = mgr.get_mut(&id) {
                        if session.health != crate::session_health::SessionHealth::Reconnecting {
                            // State changed while we were reconnecting; skip
                            continue;
                        }
                        match result {
                            Ok(new_reader_handle) => {
                                session._reader_task.abort();
                                session._reader_task = new_reader_handle;
                                session.health = crate::session_health::SessionHealth::Alive;
                                session.cdp_sessions.clear();
                                tracing::info!(
                                    session_id = id,
                                    "Session reconnected successfully (daemon)"
                                );
                            }
                            Err(e) => {
                                session.health = crate::session_health::SessionHealth::Dead;
                                session._reader_task.abort();
                                if let Some(ref h) = session._network_processor {
                                    h.abort();
                                }
                                if let Some(ref h) = session._console_processor {
                                    h.abort();
                                }
                                if let Some(ref h) = session._frame_nav_processor {
                                    h.abort();
                                }
                                tracing::warn!(session_id = id, error = %e, "Session reconnection failed (daemon), marked dead");
                            }
                        }
                    }
                } else {
                    // No port or state changed — skip; next tick will retry
                    tracing::debug!(
                        session_id = id,
                        "Skipping reconnect: no port or state changed"
                    );
                }
            }

            // Step 3: handle Recovering sessions (Chrome crashed, need new process)
            let recovering_ids: Vec<String> = {
                let mut mgr = reconnect_sessions.lock().await;
                let sessions = mgr.list();
                sessions
                    .into_iter()
                    .filter(|s| s.status == "recovering")
                    .map(|s| s.id)
                    .collect()
            };

            for id in recovering_ids {
                let should_recover = {
                    let mgr = reconnect_sessions.lock().await;
                    mgr.get(&id)
                        .map(|s| {
                            s.health == crate::session_health::SessionHealth::Recovering
                                && s.owns_process
                        })
                        .unwrap_or(false)
                };

                if should_recover {
                    let mut mgr = reconnect_sessions.lock().await;
                    let result = mgr
                        .recover_session(
                            &id,
                            Arc::clone(&db_for_reconnect),
                            &config_for_reconnect,
                            &config_for_reconnect.network,
                            None, // site_store: not needed for basic recovery
                        )
                        .await;

                    match result {
                        Ok(()) => {
                            // Try to restore latest checkpoint
                            let restore_info = mgr.get(&id).map(|s| s.profile_name.clone());
                            if let Some(profile) = restore_info {
                                let checkpoints = crate::checkpoint::list_checkpoints(
                                    &db_for_reconnect,
                                    &profile,
                                )
                                .unwrap_or_default();
                                if let Some(latest) = checkpoints.first() {
                                    if let Ok(session) = mgr.get_live(&id) {
                                        let _ = crate::checkpoint::restore_session_checkpoint(
                                            session,
                                            &latest.checkpoint_id,
                                            &db_for_reconnect,
                                        )
                                        .await;
                                    }
                                    tracing::info!(
                                        session_id = %id,
                                        profile = %profile,
                                        "Session recovered from Chrome crash"
                                    );
                                } else {
                                    tracing::info!(
                                        session_id = %id,
                                        profile = %profile,
                                        "Session recovered (no checkpoint to restore)"
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                session_id = %id,
                                error = %e,
                                "Session recovery failed, marked dead"
                            );
                        }
                    }
                }
            }
        }
    });

    // Sleep/wake handler: checkpoint before sleep, trigger reconnection after wake.
    let mut power_rx = crate::sleep_watcher::start();
    let sm_power = Arc::clone(&sessions);
    let db_power = Arc::clone(&db);
    let config_power = config.clone();
    let _power_task = tokio::spawn(async move {
        while let Some(event) = power_rx.recv().await {
            match event {
                crate::sleep_watcher::PowerEvent::WillSleep { done } => {
                    tracing::info!("System going to sleep — checkpointing all sessions");
                    let session_ids: Vec<String> = {
                        let mut sm = sm_power.lock().await;
                        sm.list()
                            .into_iter()
                            .filter(|s| s.alive)
                            .map(|s| s.id)
                            .collect()
                    };
                    for sid in &session_ids {
                        let mut sm = sm_power.lock().await;
                        if let Ok(session) = sm.get_live(sid) {
                            let _ = crate::checkpoint::save_session_checkpoint(
                                session,
                                Some("Autosave · pre-sleep"),
                                &db_power,
                                config_power.retention.max_snapshot_versions,
                            )
                            .await;
                        }
                    }
                    // Signal sleep_watcher to call IOAllowPowerChange
                    let _ = done.send(());
                }
                crate::sleep_watcher::PowerEvent::DidWake => {
                    tracing::info!("System woke — triggering session reconnection");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let mut sm = sm_power.lock().await;
                    let _ = sm.list(); // triggers Alive → Reconnecting transitions
                }
            }
        }
    });

    // Accept loop with graceful shutdown on SIGTERM/SIGINT.
    // On shutdown, Chrome processes are intentionally LEFT ALIVE — that's the whole
    // point of TCP-only transport. The daemon will reattach to them on next startup
    // via reconcile_sessions(). We just save checkpoints and clean up the socket.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| crate::error::PagerunnerError::Config(format!("signal handler: {}", e)))?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|e| crate::error::PagerunnerError::Config(format!("signal handler: {}", e)))?;

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                let (stream, _) = accept_result
                    .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
                let sessions = Arc::clone(&sessions);
                let db = Arc::clone(&db);
                let config = config.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(stream, sessions, db, config).await;
                });
            }
            _ = sigterm.recv() => {
                tracing::info!("Daemon shutting down (SIGTERM) — Chrome processes left alive for reattach");
                break;
            }
            _ = sigint.recv() => {
                tracing::info!("Daemon shutting down (SIGINT) — Chrome processes left alive for reattach");
                break;
            }
        }
    }

    // Checkpoint all alive sessions before shutdown
    {
        let session_ids: Vec<String> = {
            let mut sm = sessions.lock().await;
            sm.list()
                .into_iter()
                .filter(|s| s.alive)
                .map(|s| s.id)
                .collect()
        };
        let count = session_ids.len();
        for sid in &session_ids {
            let mut sm = sessions.lock().await;
            if let Ok(session) = sm.get_live(sid) {
                let _ = crate::checkpoint::save_session_checkpoint(
                    session,
                    Some("Autosave · shutdown"),
                    &db,
                    config.retention.max_snapshot_versions,
                )
                .await;
            }
        }
        if count > 0 {
            tracing::info!("Saved shutdown checkpoints for {} session(s)", count);
        }
    }

    // Clean up socket
    let _ = std::fs::remove_file(&socket_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// DaemonToolExecutor — adapts dispatch_tool for the agent's ToolExecutor trait
// ---------------------------------------------------------------------------

pub struct DaemonToolExecutor {
    pub config: PagerunnerConfig,
    pub sessions: Arc<Mutex<SessionManager>>,
    pub db: Arc<Db>,
    pub audit: Option<Arc<crate::audit::AuditLog>>,
}

#[async_trait::async_trait]
impl pagerunner_agent::ToolExecutor for DaemonToolExecutor {
    async fn execute(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> std::result::Result<pagerunner_agent::ToolResponse, String> {
        let outcome = crate::mcp_server::dispatch_tool(
            name,
            &args,
            &self.config,
            Arc::clone(&self.sessions),
            Arc::clone(&self.db),
            self.audit.clone(),
        )
        .await;
        match outcome {
            Ok(tool_response) => Ok(pagerunner_agent::ToolResponse::ok(tool_response.result)),
            Err(e) => Ok(pagerunner_agent::ToolResponse::error(e.to_string())),
        }
    }

    fn available_tools(&self) -> Vec<pagerunner_llm::ToolSchema> {
        crate::mcp_server::all_tools()
            .into_iter()
            .filter_map(|v| {
                let name = v.get("name")?.as_str()?.to_string();
                // Exclude agent_run to prevent recursive agent invocation
                if name == "agent_run" {
                    return None;
                }
                let description = v.get("description")?.as_str()?.to_string();
                let input_schema = v.get("inputSchema").cloned().unwrap_or_else(|| {
                    serde_json::json!({"type": "object"})
                });
                Some(pagerunner_llm::ToolSchema::new(name, description, input_schema))
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Session context preparation — opens/reuses a session for the agent
// ---------------------------------------------------------------------------

pub struct SessionContext {
    pub session_id: String,
    pub target_id: String,
    pub profile: String,
    pub current_url: Option<String>,
}

/// Open or reuse a session for the given profile, return session_id + target_id.
pub async fn prepare_session_context(
    profile_name: &str,
    config: &PagerunnerConfig,
    sessions: Arc<Mutex<SessionManager>>,
    db: Arc<Db>,
) -> std::result::Result<SessionContext, String> {
    // Check for an existing alive session for this profile
    let existing = {
        let mut mgr = sessions.lock().await;
        let list = mgr.list();
        list.into_iter().find(|s| s.profile_name == profile_name && s.alive)
    };

    let session_id = if let Some(s) = existing {
        tracing::debug!(profile = %profile_name, session = %s.id, "Reusing existing session for agent");
        s.id
    } else {
        // Open a new session
        let result = crate::mcp_server::dispatch_tool(
            "open_session",
            &serde_json::json!({"profile": profile_name}),
            config,
            Arc::clone(&sessions),
            Arc::clone(&db),
            None,
        ).await.map_err(|e| format!("open_session failed: {e}"))?;

        let parsed: serde_json::Value = serde_json::from_str(&result.result)
            .map_err(|e| format!("parse open_session response: {e}"))?;
        parsed["session_id"]
            .as_str()
            .ok_or("no session_id in open_session response")?
            .to_string()
    };

    // Get a tab — poll briefly if needed (Chrome might still be starting)
    let mut target_id = None;
    let mut current_url = None;
    for attempt in 0..10 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        let tabs_result = crate::mcp_server::dispatch_tool(
            "list_tabs",
            &serde_json::json!({"session_id": &session_id}),
            config,
            Arc::clone(&sessions),
            Arc::clone(&db),
            None,
        ).await;

        if let Ok(tr) = tabs_result {
            if let Ok(tabs) = serde_json::from_str::<serde_json::Value>(&tr.result) {
                if let Some(first) = tabs["data"].as_array().or(tabs.as_array()).and_then(|a| a.first()) {
                    target_id = first["target_id"].as_str().map(|s| s.to_string());
                    current_url = first["url"].as_str().map(|s| s.to_string());
                    if target_id.is_some() {
                        break;
                    }
                }
            }
        }
    }

    // If still no tab, open one
    if target_id.is_none() {
        let new_tab = crate::mcp_server::dispatch_tool(
            "new_tab",
            &serde_json::json!({"session_id": &session_id}),
            config,
            Arc::clone(&sessions),
            Arc::clone(&db),
            None,
        ).await.map_err(|e| format!("new_tab failed: {e}"))?;

        let parsed: serde_json::Value = serde_json::from_str(&new_tab.result)
            .unwrap_or_default();
        target_id = parsed["target_id"].as_str().map(|s| s.to_string());
    }

    let target_id = target_id.ok_or("could not get a browser tab")?;

    Ok(SessionContext {
        session_id,
        target_id,
        profile: profile_name.to_string(),
        current_url,
    })
}

// ---------------------------------------------------------------------------
// Active agent run tracking
// ---------------------------------------------------------------------------

struct ActiveRun {
    interrupt_tx: watch::Sender<bool>,
    approval_tx: mpsc::Sender<bool>,
}

type ActiveRuns = Arc<Mutex<HashMap<String, ActiveRun>>>;

// ---------------------------------------------------------------------------
// handle_connection — now supports DaemonMessage + legacy DaemonRequest
// ---------------------------------------------------------------------------

async fn handle_connection(
    stream: tokio::net::UnixStream,
    sessions: Arc<Mutex<SessionManager>>,
    db: Arc<Db>,
    config: PagerunnerConfig,
) -> Result<()> {
    let active_runs: ActiveRuns = Arc::new(Mutex::new(HashMap::new()));
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();

        // Try to parse as DaemonMessage first, fall back to legacy DaemonRequest
        let parsed: std::result::Result<DaemonMessage, _> = serde_json::from_str(trimmed);
        let msg = match parsed {
            Ok(m) => m,
            Err(_) => {
                // Fall back to legacy DaemonRequest
                match serde_json::from_str::<DaemonRequest>(trimmed) {
                    Ok(req) => DaemonMessage::ToolCall(req),
                    Err(e) => {
                        tracing::warn!("Invalid JSON request: {}", e);
                        continue;
                    }
                }
            }
        };

        match msg {
            DaemonMessage::ToolCall(req) => {
                let id = req.id.clone();
                let outcome = crate::mcp_server::dispatch_tool(
                    &req.tool,
                    &req.args,
                    &config,
                    Arc::clone(&sessions),
                    Arc::clone(&db),
                    None,
                )
                .await;
                let resp = match outcome {
                    Ok(tool_response) => DaemonResponse {
                        id,
                        result: Some(tool_response.result),
                        error: None,
                    },
                    Err(e) => DaemonResponse {
                        id,
                        result: None,
                        error: Some(e.to_string()),
                    },
                };
                let mut out = serde_json::to_string(&resp)?;
                out.push('\n');
                write_half.write_all(out.as_bytes()).await?;
            }

            DaemonMessage::AgentRun { id, goal, config: agent_config_override } => {
                let run_id = uuid::Uuid::new_v4().to_string();
                let mut agent_config = agent_config_override.unwrap_or_else(|| config.agent.default_config.clone());

                // Create LLM provider
                let llm_config = {
                    let mut c = config.agent.llm.clone();
                    if agent_config.model != "claude-haiku-4-5-20251001" {
                        c.default_model = agent_config.model.clone();
                    }
                    c
                };
                let provider = match pagerunner_llm::create_default_provider(&llm_config) {
                    Ok(p) => p,
                    Err(e) => {
                        let resp = DaemonResponse {
                            id,
                            result: None,
                            error: Some(format!("Failed to create LLM provider: {}", e)),
                        };
                        let mut out = serde_json::to_string(&resp)?;
                        out.push('\n');
                        write_half.write_all(out.as_bytes()).await?;
                        continue;
                    }
                };

                // Create audit log for agent tool calls
                let audit: Option<Arc<crate::audit::AuditLog>> = {
                    let home = dirs::home_dir();
                    home.map(|h| {
                        let audit_path = h.join(".pagerunner/audit.log");
                        Arc::new(crate::audit::AuditLog::new(audit_path, Arc::clone(&db)))
                    })
                };

                let tool_executor = Arc::new(DaemonToolExecutor {
                    config: config.clone(),
                    sessions: Arc::clone(&sessions),
                    db: Arc::clone(&db),
                    audit,
                });

                // Pre-inject session context: if a profile is specified, open/reuse
                // a session and get a tab so the agent doesn't waste steps bootstrapping.
                let session_context = if let Some(profile_name) = &agent_config.session_profile {
                    match prepare_session_context(
                        profile_name,
                        &config,
                        Arc::clone(&sessions),
                        Arc::clone(&db),
                    ).await {
                        Ok(ctx) => Some(ctx),
                        Err(e) => {
                            tracing::warn!(profile = %profile_name, error = %e, "Failed to prepare session context, agent will bootstrap itself");
                            None
                        }
                    }
                } else {
                    None
                };

                // Build the enriched goal with session context
                let enriched_goal = if let Some(ref ctx) = session_context {
                    // Set session context for auto-injection (Optimization 4).
                    // This strips session_id/target_id from tool schemas and
                    // auto-injects them into tool args — the LLM never sees them.
                    agent_config.session_context = Some(pagerunner_agent::SessionContext {
                        session_id: ctx.session_id.clone(),
                        target_id: ctx.target_id.clone(),
                    });

                    // Add current URL info if available
                    if let Some(url) = &ctx.current_url {
                        let url_extra = format!("Current page: {}", url);
                        agent_config.system_prompt_extra = Some(
                            agent_config.system_prompt_extra
                                .map(|s| format!("{}\n\n{}", url_extra, s))
                                .unwrap_or(url_extra),
                        );
                    }

                    format!(
                        "You already have a browser session open on profile '{}'. \
                         session_id and target_id are auto-injected — do NOT include them in tool calls.\n\n{}",
                        ctx.profile, goal
                    )
                } else {
                    goal
                };

                let (event_tx, mut event_rx) = broadcast::channel::<pagerunner_agent::AgentEvent>(256);
                let (interrupt_tx, interrupt_rx) = watch::channel(false);
                let (approval_tx, approval_rx) = mpsc::channel(16);

                // Track this run
                {
                    let mut runs = active_runs.lock().await;
                    runs.insert(run_id.clone(), ActiveRun {
                        interrupt_tx,
                        approval_tx,
                    });
                }

                let run_id_clone = run_id.clone();
                let agent_config_clone = agent_config.clone();

                // Spawn the agent loop
                let agent_handle = tokio::spawn(async move {
                    pagerunner_agent::run_agent(
                        enriched_goal,
                        agent_config_clone,
                        provider,
                        tool_executor,
                        event_tx,
                        interrupt_rx,
                        approval_rx,
                        run_id_clone,
                    )
                    .await
                });

                // Stream events back to client
                let active_runs_cleanup = Arc::clone(&active_runs);
                let run_id_stream = run_id.clone();
                loop {
                    match event_rx.recv().await {
                        Ok(event) => {
                            let daemon_event = DaemonEvent {
                                run_id: run_id_stream.clone(),
                                event,
                            };
                            let mut out = serde_json::to_string(&daemon_event)
                                .unwrap_or_else(|_| "{}".to_string());
                            out.push('\n');
                            if write_half.write_all(out.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(run_id = %run_id_stream, lagged = n, "Event stream lagged");
                        }
                    }
                }

                // Wait for agent to finish and send final result
                let agent_result = match agent_handle.await {
                    Ok(result) => result,
                    Err(e) => pagerunner_agent::AgentResult {
                        outcome: pagerunner_agent::AgentOutcome::Error,
                        summary: Some(format!("Agent task panicked: {}", e)),
                        total_steps: 0,
                        usage: pagerunner_llm::Usage {
                            input_tokens: 0,
                            output_tokens: 0,
                        },
                    },
                };

                // Cleanup
                {
                    let mut runs = active_runs_cleanup.lock().await;
                    runs.remove(&run_id);
                }

                let resp = DaemonResponse {
                    id,
                    result: Some(serde_json::to_string(&agent_result).unwrap_or_default()),
                    error: None,
                };
                let mut out = serde_json::to_string(&resp)?;
                out.push('\n');
                let _ = write_half.write_all(out.as_bytes()).await;
            }

            DaemonMessage::AgentApprove { id, run_id, approved } => {
                let sent = {
                    let runs = active_runs.lock().await;
                    if let Some(run) = runs.get(&run_id) {
                        run.approval_tx.send(approved).await.is_ok()
                    } else {
                        false
                    }
                };
                let resp = if sent {
                    DaemonResponse { id, result: Some("ok".into()), error: None }
                } else {
                    DaemonResponse { id, result: None, error: Some(format!("No active run: {}", run_id)) }
                };
                let mut out = serde_json::to_string(&resp)?;
                out.push('\n');
                write_half.write_all(out.as_bytes()).await?;
            }

            DaemonMessage::AgentInterrupt { id, run_id } => {
                let sent = {
                    let runs = active_runs.lock().await;
                    if let Some(run) = runs.get(&run_id) {
                        run.interrupt_tx.send(true).is_ok()
                    } else {
                        false
                    }
                };
                let resp = if sent {
                    DaemonResponse { id, result: Some("ok".into()), error: None }
                } else {
                    DaemonResponse { id, result: None, error: Some(format!("No active run: {}", run_id)) }
                };
                let mut out = serde_json::to_string(&resp)?;
                out.push('\n');
                write_half.write_all(out.as_bytes()).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{DaemonRequest, DaemonResponse};

    #[test]
    fn test_daemon_request_roundtrip() {
        let req = DaemonRequest {
            id: "test-id".into(),
            tool: "list_sessions".into(),
            args: serde_json::json!({}),
        };
        let s = serde_json::to_string(&req).unwrap();
        let decoded: DaemonRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(decoded.id, "test-id");
        assert_eq!(decoded.tool, "list_sessions");
    }

    #[test]
    fn test_daemon_response_error_roundtrip() {
        let resp = DaemonResponse {
            id: "test-id".into(),
            result: None,
            error: Some("oops".into()),
        };
        let s = serde_json::to_string(&resp).unwrap();
        let decoded: DaemonResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(decoded.error, Some("oops".into()));
        assert!(decoded.result.is_none());
    }
}
