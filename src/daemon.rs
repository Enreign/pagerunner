use crate::config::PagerunnerConfig;
use crate::db::Db;
use crate::error::Result;
use crate::ipc::{DaemonRequest, DaemonResponse, SOCKET_SUBPATH};
use crate::session::SessionManager;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

pub async fn run() -> Result<()> {
    let config = PagerunnerConfig::load()?;
    let sessions = Arc::new(Mutex::new(SessionManager::new()));

    let home = dirs::home_dir().ok_or_else(|| {
        crate::error::PagerunnerError::Config("home directory not accessible".into())
    })?;

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

    // Background reconnection task: periodically checks for sessions in Reconnecting
    // state and attempts to reconnect them. Acquires and releases the lock around
    // each step to avoid blocking tool calls.
    let reconnect_sessions = Arc::clone(&sessions);
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
                                tracing::info!(session_id = id, "Session reconnected successfully (daemon)");
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
                    tracing::debug!(session_id = id, "Skipping reconnect: no port or state changed");
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

async fn handle_connection(
    stream: tokio::net::UnixStream,
    sessions: Arc<Mutex<SessionManager>>,
    db: Arc<Db>,
    config: PagerunnerConfig,
) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let req: DaemonRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Invalid JSON request: {}", e);
                continue;
            }
        };
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
