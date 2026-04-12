//! HTTP/WebSocket API gateway for the Pagerunner daemon.
//!
//! Runs alongside the Unix socket listener, exposing the same `dispatch_tool`
//! surface over REST + WebSocket. Designed for the iOS companion app connecting
//! over a secure network (e.g. Tailscale).
//!
//! ## Authentication
//! All requests must carry `Authorization: Bearer <token>` where `<token>`
//! matches `[http_api] token` in `~/.pagerunner/config.toml`.
//!
//! ## Endpoints
//! - `GET  /health`              → daemon health check (no auth required)
//! - `POST /api/tool`            → execute any tool: `{ "tool": "…", "args": {…} }`
//! - `GET  /api/sessions`        → shorthand for list_sessions
//! - `GET  /api/profiles`        → shorthand for list_profiles
//! - `GET  /api/sessions/:id/tabs`             → list_tabs
//! - `GET  /api/sessions/:id/network-log`      → get_network_log
//! - `GET  /api/sessions/:id/console-log`      → get_console_log
//! - `POST /api/sessions/:id/screenshot/:tid`  → screenshot (base64 PNG)
//! - `GET  /api/notifications`   → drain pending notifications
//! - `GET  /api/checkpoints/:profile`          → list_session_checkpoints
//! - `GET  /api/recordings`      → list_recordings
//! - `WS   /ws/events`           → real-time event stream (agent events, notifications)

use crate::config::{AuthMode, HttpApiConfig, PagerunnerConfig};
use crate::db::Db;
use crate::ipc::DaemonEvent;
use crate::session::SessionManager;
use crate::tailscale;
use axum::{
    extract::{
        connect_info::ConnectInfo,
        ws::{Message, WebSocket},
        Path, Query, Request, State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tower_http::cors::{Any, CorsLayer};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ApiState {
    pub config: PagerunnerConfig,
    pub sessions: Arc<Mutex<SessionManager>>,
    pub db: Arc<Db>,
    pub bearer_token: String,
    pub auth_mode: AuthMode,
    pub tailscale_allowed_users: Vec<String>,
    pub tailscale_allowed_tags: Vec<String>,
    pub event_tx: broadcast::Sender<DaemonEvent>,
    /// Broadcast channel for notification push (profile, title, body).
    pub notification_tx: broadcast::Sender<NotificationPush>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPush {
    pub profile: Option<String>,
    pub title: String,
    pub body: String,
    pub level: String,
}

// ---------------------------------------------------------------------------
// Auth middleware helper
// ---------------------------------------------------------------------------

/// Token-mode auth check — exported for unit tests.
fn check_token(headers: &HeaderMap, expected: &str) -> Result<(), StatusCode> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if let Some(token) = auth.strip_prefix("Bearer ") {
        if token == expected {
            return Ok(());
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

/// Dispatch auth based on the configured mode. Used by every authenticated
/// handler; returns `Ok(())` when the caller is authorised, or a status code
/// to reply with.
async fn authorize(
    state: &ApiState,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
) -> Result<(), StatusCode> {
    match state.auth_mode {
        AuthMode::Token => check_token(headers, &state.bearer_token),
        AuthMode::Tailscale => check_tailscale(state, peer).await,
    }
}

async fn check_tailscale(
    state: &ApiState,
    peer: Option<SocketAddr>,
) -> Result<(), StatusCode> {
    let Some(addr) = peer else {
        tracing::warn!("tailscale auth: no peer addr available");
        return Err(StatusCode::UNAUTHORIZED);
    };

    let whois = match tailscale::whois(addr).await {
        Ok(Some(w)) => w,
        Ok(None) => {
            tracing::debug!("tailscale auth: {} is not in tailnet", addr);
            return Err(StatusCode::UNAUTHORIZED);
        }
        Err(e) => {
            tracing::error!("tailscale auth: {}", e);
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    };

    if !state.tailscale_allowed_users.is_empty()
        && !state
            .tailscale_allowed_users
            .iter()
            .any(|u| u == &whois.user_profile.login_name)
    {
        tracing::debug!(
            "tailscale auth: user {:?} not in allow list",
            whois.user_profile.login_name
        );
        return Err(StatusCode::FORBIDDEN);
    }
    if !state.tailscale_allowed_tags.is_empty()
        && !state
            .tailscale_allowed_tags
            .iter()
            .any(|t| whois.node.tags.iter().any(|nt| nt == t))
    {
        tracing::debug!(
            "tailscale auth: node tags {:?} do not match allow list",
            whois.node.tags
        );
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ToolCallRequest {
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Serialize)]
pub struct ToolCallResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub limit: Option<usize>,
    pub target_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Unauthenticated — reachable without any credentials. `auth-info` lets
    // clients discover which auth mode the daemon expects before connecting.
    let public = Router::new()
        .route("/health", get(health))
        .route("/auth-info", get(auth_info))
        .with_state(state.clone());

    // Authenticated — every request passes through `auth_middleware`.
    let protected = Router::new()
        .route("/api/tool", post(tool_call))
        .route("/api/profiles", get(list_profiles))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/{id}/tabs", get(list_tabs))
        .route("/api/sessions/{id}/network-log", get(network_log))
        .route("/api/sessions/{id}/console-log", get(console_log))
        .route("/api/sessions/{id}/screenshot/{tid}", post(screenshot))
        .route("/api/notifications", get(notifications))
        .route("/api/checkpoints/{profile}", get(checkpoints))
        .route("/api/recordings", get(recordings))
        .route("/ws/events", get(ws_events))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .with_state(state);

    public.merge(protected).layer(cors)
}

/// Tower middleware: runs before every protected route and rejects callers
/// that can't satisfy the configured auth mode.
async fn auth_middleware(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Response {
    let peer_addr = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    let headers = request.headers().clone();
    if let Err(status) = authorize(&state, &headers, peer_addr).await {
        let body = json!({ "ok": false, "error": "unauthorized" });
        return (status, Json(body)).into_response();
    }
    next.run(request).await
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// Publishes the auth mode so clients can decide what credentials to present
/// before making a real request. Always unauthenticated.
async fn auth_info(State(state): State<ApiState>) -> impl IntoResponse {
    let mode = match state.auth_mode {
        AuthMode::Token => "token",
        AuthMode::Tailscale => "tailscale",
    };
    Json(json!({ "mode": mode }))
}

async fn tool_call(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Json(req): Json<ToolCallRequest>,
) -> impl IntoResponse {
    match dispatch(&state, &req.tool, &req.args).await {
        Ok(result) => (
            StatusCode::OK,
            Json(ToolCallResponse {
                ok: true,
                result: Some(parse_result(&result)),
                error: None,
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ToolCallResponse {
                ok: false,
                result: None,
                error: Some(e),
            }),
        ),
    }
}

async fn list_profiles(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> impl IntoResponse {
    match dispatch(&state, "list_profiles", &json!({})).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

async fn list_sessions(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> impl IntoResponse {
    match dispatch(&state, "list_sessions", &json!({})).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

async fn list_tabs(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match dispatch(&state, "list_tabs", &json!({"session_id": session_id})).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

async fn network_log(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(q): Query<LogQuery>,
) -> impl IntoResponse {
    let mut args = json!({"session_id": session_id});
    if let Some(limit) = q.limit {
        args["limit"] = json!(limit);
    }
    if let Some(ref tid) = q.target_id {
        args["target_id"] = json!(tid);
    }
    match dispatch(&state, "get_network_log", &args).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

async fn console_log(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(q): Query<LogQuery>,
) -> impl IntoResponse {
    let mut args = json!({"session_id": session_id});
    if let Some(ref tid) = q.target_id {
        args["target_id"] = json!(tid);
    }
    match dispatch(&state, "get_console_log", &args).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

async fn screenshot(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path((session_id, target_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let args = json!({
        "session_id": session_id,
        "target_id": target_id,
        "base64": true
    });
    match dispatch(&state, "screenshot", &args).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

async fn notifications(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> impl IntoResponse {
    match dispatch(&state, "list_notifications", &json!({})).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

async fn checkpoints(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(profile): Path<String>,
) -> impl IntoResponse {
    match dispatch(&state, "list_session_checkpoints", &json!({"profile": profile})).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

async fn recordings(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> impl IntoResponse {
    match dispatch(&state, "list_recordings", &json!({})).await {
        Ok(r) => (StatusCode::OK, Json(parse_result(&r))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))),
    }
}

// ---------------------------------------------------------------------------
// WebSocket — real-time event stream
// ---------------------------------------------------------------------------

async fn ws_events(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
        .into_response()
}

async fn handle_ws(mut socket: WebSocket, state: ApiState) {
    let mut event_rx = state.event_tx.subscribe();
    let mut notif_rx = state.notification_tx.subscribe();

    // Merge channel: all background forwarders send here, main loop reads from it.
    let (merge_tx, mut merge_rx) = tokio::sync::mpsc::channel::<String>(256);

    // Agent event forwarder
    let tx1 = merge_tx.clone();
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(ev) => {
                    if let Ok(json) = serde_json::to_string(&json!({
                        "type": "agent_event",
                        "data": ev
                    })) {
                        if tx1.send(json).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // Notification forwarder
    let tx2 = merge_tx.clone();
    tokio::spawn(async move {
        loop {
            match notif_rx.recv().await {
                Ok(notif) => {
                    if let Ok(json) = serde_json::to_string(&json!({
                        "type": "notification",
                        "data": notif
                    })) {
                        if tx2.send(json).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // Periodic heartbeat + status push
    let state_for_poll = state.clone();
    let tx3 = merge_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let status = match dispatch(&state_for_poll, "list_sessions", &json!({})).await {
                Ok(r) => parse_result(&r),
                Err(_) => json!({"data": []}),
            };
            if let Ok(json) = serde_json::to_string(&json!({
                "type": "session_status",
                "data": status
            })) {
                if tx3.send(json).await.is_err() {
                    break;
                }
            }
        }
    });
    drop(merge_tx); // Drop our handle so merge_rx closes when all senders are gone

    // Main send loop: merge channel + WebSocket recv
    loop {
        tokio::select! {
            Some(msg) = merge_rx.recv() => {
                if socket.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        // Handle incoming commands (tool calls over WS)
                        if let Ok(req) = serde_json::from_str::<ToolCallRequest>(&text) {
                            let result = match dispatch(&state, &req.tool, &req.args).await {
                                Ok(r) => json!({"type": "tool_result", "ok": true, "result": parse_result(&r)}),
                                Err(e) => json!({"type": "tool_result", "ok": false, "error": e}),
                            };
                            if let Ok(json) = serde_json::to_string(&result) {
                                let _ = socket.send(Message::Text(json.into())).await;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn dispatch(state: &ApiState, tool: &str, args: &Value) -> Result<String, String> {
    crate::mcp_server::dispatch_tool(
        tool,
        args,
        &state.config,
        Arc::clone(&state.sessions),
        Arc::clone(&state.db),
        None,
    )
    .await
    .map(|r| r.result)
    .map_err(|e| e.to_string())
}

/// Parse a tool result string into a JSON Value. Tool results are JSON strings,
/// so we try to parse them; if that fails, wrap in a text object.
fn parse_result(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!({"text": s}))
}

// ---------------------------------------------------------------------------
// Server startup — called from daemon.rs
// ---------------------------------------------------------------------------

pub async fn start_http_server(
    config: &HttpApiConfig,
    app_config: PagerunnerConfig,
    sessions: Arc<Mutex<SessionManager>>,
    db: Arc<Db>,
    event_tx: broadcast::Sender<DaemonEvent>,
) -> Result<(), String> {
    let state = ApiState {
        config: app_config,
        sessions,
        db,
        bearer_token: config.token.clone(),
        auth_mode: config.auth,
        tailscale_allowed_users: config.tailscale_allowed_users.clone(),
        tailscale_allowed_tags: config.tailscale_allowed_tags.clone(),
        event_tx,
        notification_tx: broadcast::channel(256).0,
    };

    let app = router(state);
    let addr = format!("{}:{}", config.bind_address, config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("HTTP API bind {}: {}", addr, e))?;

    tracing::info!(
        "HTTP API listening on {} (auth: {:?})",
        addr, config.auth
    );

    // `into_make_service_with_connect_info` exposes the TCP peer address to
    // handlers via `ConnectInfo<SocketAddr>`. Tailscale auth needs this to
    // identify the caller.
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(|e| format!("HTTP API server error: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_result_valid_json() {
        let input = r#"{"data": [{"id": "s1"}]}"#;
        let result = parse_result(input);
        assert!(result["data"].is_array());
    }

    #[test]
    fn test_parse_result_plain_text() {
        let input = "some plain text";
        let result = parse_result(input);
        assert_eq!(result["text"], "some plain text");
    }

    #[test]
    fn test_check_auth_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer test-token".parse().unwrap());
        assert!(check_token(&headers, "test-token").is_ok());
    }

    #[test]
    fn test_check_auth_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        assert!(check_token(&headers, "test-token").is_err());
    }

    #[test]
    fn test_check_auth_missing() {
        let headers = HeaderMap::new();
        assert!(check_token(&headers, "test-token").is_err());
    }

    #[test]
    fn test_tool_call_request_deserialize() {
        let json = r#"{"tool": "list_sessions", "args": {}}"#;
        let req: ToolCallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tool, "list_sessions");
    }

    #[test]
    fn test_tool_call_request_without_args() {
        let json = r#"{"tool": "list_profiles"}"#;
        let req: ToolCallRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tool, "list_profiles");
        assert!(req.args.is_null());
    }

    #[test]
    fn test_notification_push_serialize() {
        let n = NotificationPush {
            profile: Some("personal".into()),
            title: "Session crashed".into(),
            body: "Chrome exited unexpectedly".into(),
            level: "error".into(),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("Session crashed"));
        assert!(s.contains("personal"));
    }
}
