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
        &db,
        &sessions,
        &config,
        None,  // site_store: not needed for basic reattach
    ).await;
    if !reattached.is_empty() {
        tracing::info!("Daemon: reattached {} surviving Chrome session(s)", reattached.len());
    }

    // Accept loop with graceful shutdown on SIGTERM/SIGINT.
    // With TCP-only Chrome, we MUST kill owned Chrome processes on exit
    // to prevent orphans that hold profile directory locks.
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
                tracing::info!("Daemon received SIGTERM, killing Chrome processes");
                sessions.lock().await.kill_all_chrome().await;
                break;
            }
            _ = sigint.recv() => {
                tracing::info!("Daemon received SIGINT, killing Chrome processes");
                sessions.lock().await.kill_all_chrome().await;
                break;
            }
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
