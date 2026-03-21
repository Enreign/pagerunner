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

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|e| crate::error::PagerunnerError::Config(e.to_string()))?;
        let sessions = Arc::clone(&sessions);
        let db = Arc::clone(&db);
        let config = config.clone();
        tokio::spawn(async move {
            let _ = handle_connection(stream, sessions, db, config).await;
        });
    }
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
            req.args,
            &config,
            Arc::clone(&sessions),
            Arc::clone(&db),
            None,
        )
        .await;
        let resp = match outcome {
            Ok(text) => DaemonResponse {
                id,
                result: Some(text),
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
