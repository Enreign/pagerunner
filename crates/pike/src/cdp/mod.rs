//! CDP-compatible server.
//!
//! Implements the subset of Chrome DevTools Protocol that pagerunner uses:
//! - HTTP endpoints: /json/version, /json/list
//! - WebSocket transport for CDP messages
//! - Target, Runtime, Page, Network, Input domain handlers
//!
//! This lets pagerunner connect to pike exactly as it connects to Chrome.

pub mod handler;

use crate::cdp::handler::CdpHandler;
use futures_util::{SinkExt, StreamExt};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use http_body_util::Full;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

/// Configuration for the CDP server.
pub struct CdpServerConfig {
    pub port: u16,
    pub headless: bool,
}

/// The CDP server — listens for HTTP and WebSocket connections.
pub struct CdpServer {
    config: CdpServerConfig,
    handler: Arc<Mutex<CdpHandler>>,
}

impl CdpServer {
    pub fn new(config: CdpServerConfig) -> Self {
        CdpServer {
            handler: Arc::new(Mutex::new(CdpHandler::new(config.headless))),
            config,
        }
    }

    /// Start the CDP server. Returns the actual bound port.
    pub async fn start(self) -> std::io::Result<u16> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.config.port));
        let listener = TcpListener::bind(addr).await?;
        let port = listener.local_addr()?.port();
        let handler = self.handler.clone();

        tracing::info!("pike CDP server listening on 127.0.0.1:{}", port);

        tokio::spawn(async move {
            loop {
                let (stream, _addr) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(e) => {
                        tracing::error!("accept error: {}", e);
                        continue;
                    }
                };

                let handler = handler.clone();
                let port_copy = port;

                tokio::spawn(async move {
                    // Peek at the first bytes to determine if this is an HTTP
                    // request or a direct WebSocket upgrade.
                    let io = TokioIo::new(stream);

                    let service = service_fn(move |req: Request<Incoming>| {
                        let handler = handler.clone();
                        async move {
                            handle_request(req, handler, port_copy).await
                        }
                    });

                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .with_upgrades()
                        .await
                    {
                        if !e.to_string().contains("early eof")
                            && !e.to_string().contains("connection closed")
                        {
                            tracing::debug!("connection error: {}", e);
                        }
                    }
                });
            }
        });

        Ok(port)
    }
}

type BoxBody = Full<hyper::body::Bytes>;

fn json_response(body: Value) -> Response<BoxBody> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(hyper::body::Bytes::from(body.to_string())))
        .unwrap()
}

async fn handle_request(
    req: Request<Incoming>,
    handler: Arc<Mutex<CdpHandler>>,
    port: u16,
) -> Result<Response<BoxBody>, hyper::Error> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    // Check for WebSocket upgrade.
    if is_websocket_upgrade(&req) {
        return handle_ws_upgrade(req, handler).await;
    }

    match (method, path.as_str()) {
        (Method::GET, "/json/version") => {
            Ok(json_response(json!({
                "Browser": "pike/0.1.0",
                "Protocol-Version": "1.3",
                "User-Agent": "pike/0.1.0",
                "V8-Version": "0.0.0",
                "WebKit-Version": "0.0.0",
                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{}/devtools/browser", port),
            })))
        }
        (Method::GET, "/json/list") | (Method::GET, "/json") => {
            let handler = handler.lock().await;
            let targets = handler.list_targets();
            Ok(json_response(json!(targets)))
        }
        (Method::PUT, path) if path.starts_with("/json/new") => {
            let url = req.uri().query()
                .and_then(|q| q.strip_prefix("url="))
                .unwrap_or("about:blank")
                .to_string();
            let mut handler = handler.lock().await;
            let target = handler.create_target(&url);
            Ok(json_response(json!(target)))
        }
        _ => {
            let body = Full::new(hyper::body::Bytes::from("404 Not Found"));
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(body)
                .unwrap())
        }
    }
}

fn is_websocket_upgrade(req: &Request<Incoming>) -> bool {
    req.headers()
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

async fn handle_ws_upgrade(
    req: Request<Incoming>,
    handler: Arc<Mutex<CdpHandler>>,
) -> Result<Response<BoxBody>, hyper::Error> {
    // Use tokio-tungstenite for WebSocket handling.
    // Since we're behind hyper, we need to do the upgrade manually.
    let (response, ws_fut) = match hyper_tungstenite_upgrade(req) {
        Ok(pair) => pair,
        Err(_) => {
            let body = Full::new(hyper::body::Bytes::from("WebSocket upgrade failed"));
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(body)
                .unwrap());
        }
    };

    tokio::spawn(async move {
        match ws_fut.await {
            Ok(ws_stream) => {
                handle_ws_connection(ws_stream, handler).await;
            }
            Err(e) => {
                tracing::error!("websocket upgrade error: {}", e);
            }
        }
    });

    Ok(response)
}

/// Minimal WebSocket upgrade using hyper's upgrade mechanism.
fn hyper_tungstenite_upgrade(
    req: Request<Incoming>,
) -> Result<
    (
        Response<BoxBody>,
        impl std::future::Future<
            Output = Result<
                tokio_tungstenite::WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>,
                tokio_tungstenite::tungstenite::Error,
            >,
        >,
    ),
    String,
> {
    let key = req
        .headers()
        .get("sec-websocket-key")
        .ok_or("missing sec-websocket-key")?
        .to_str()
        .map_err(|e| e.to_string())?
        .to_string();

    let accept = compute_ws_accept(&key);

    let response = Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Accept", accept)
        .body(Full::new(hyper::body::Bytes::new()))
        .map_err(|e| e.to_string())?;

    let upgraded_fut = async move {
        let upgraded = hyper::upgrade::on(req)
            .await
            .map_err(|e| tokio_tungstenite::tungstenite::Error::Io(
                std::io::Error::other(e.to_string()),
            ))?;
        let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
            TokioIo::new(upgraded),
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            None,
        )
        .await;
        Ok(ws)
    };

    Ok((response, upgraded_fut))
}

fn compute_ws_accept(key: &str) -> String {
    use std::io::Write;
    // WebSocket accept = base64(SHA-1(key + magic GUID))
    let magic = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut hasher = Sha1::new();
    write!(hasher, "{}{}", key, magic).unwrap();
    let hash = hasher.finalize();
    base64_encode(&hash)
}

// Minimal SHA-1 implementation (just for WebSocket accept header).
struct Sha1 {
    data: Vec<u8>,
}

impl Sha1 {
    fn new() -> Self {
        Sha1 { data: Vec::new() }
    }

    fn finalize(self) -> [u8; 20] {
        sha1_hash(&self.data)
    }
}

impl std::io::Write for Sha1 {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.data.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn sha1_hash(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut result = [0u8; 20];
    result[0..4].copy_from_slice(&h0.to_be_bytes());
    result[4..8].copy_from_slice(&h1.to_be_bytes());
    result[8..12].copy_from_slice(&h2.to_be_bytes());
    result[12..16].copy_from_slice(&h3.to_be_bytes());
    result[16..20].copy_from_slice(&h4.to_be_bytes());
    result
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

async fn handle_ws_connection(
    ws_stream: tokio_tungstenite::WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>,
    handler: Arc<Mutex<CdpHandler>>,
) {
    let (mut ws_sink, mut ws_recv) = ws_stream.split();

    while let Some(msg) = ws_recv.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("websocket recv error: {}", e);
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                let request: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("invalid CDP message: {}", e);
                        continue;
                    }
                };

                let id = request["id"].as_u64().unwrap_or(0);
                let method = request["method"].as_str().unwrap_or("");
                let params = request.get("params").cloned().unwrap_or(json!({}));
                let session_id = request["sessionId"].as_str().map(|s| s.to_string());

                let mut h = handler.lock().await;
                let result = h.handle(method, params, session_id.as_deref());

                let response = if let Some(sid) = session_id {
                    json!({
                        "id": id,
                        "result": result,
                        "sessionId": sid,
                    })
                } else {
                    json!({
                        "id": id,
                        "result": result,
                    })
                };

                if let Err(e) = ws_sink
                    .send(Message::Text(response.to_string().into()))
                    .await
                {
                    tracing::debug!("websocket send error: {}", e);
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_known_vector() {
        // SHA-1("abc") = a9993e36 4706816a ba3e2571 7850c26c 9cd0d89d
        let hash = sha1_hash(b"abc");
        assert_eq!(
            hash,
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e,
                0x25, 0x71, 0x78, 0x50, 0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
            ]
        );
    }

    #[test]
    fn ws_accept_computation() {
        // RFC 6455 example: key "dGhlIHNhbXBsZSBub25jZQ=="
        // Expected accept: "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        let accept = compute_ws_accept("dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn base64_encode_known() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }

    #[tokio::test]
    async fn cdp_server_starts() {
        let server = CdpServer::new(CdpServerConfig {
            port: 0, // OS-assigned
            headless: true,
        });
        let port = server.start().await.unwrap();
        assert!(port > 0);

        // Test /json/version endpoint.
        let resp = reqwest::get(format!("http://127.0.0.1:{}/json/version", port))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["Browser"], "pike/0.1.0");
        assert!(body["webSocketDebuggerUrl"].as_str().unwrap().contains(&port.to_string()));
    }

    #[tokio::test]
    async fn cdp_server_json_list() {
        let server = CdpServer::new(CdpServerConfig {
            port: 0,
            headless: true,
        });
        let port = server.start().await.unwrap();

        let resp = reqwest::get(format!("http://127.0.0.1:{}/json/list", port))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        let targets = body.as_array().unwrap();
        // Should have one default target.
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0]["type"], "page");
    }

    #[tokio::test]
    async fn cdp_websocket_basic() {
        let server = CdpServer::new(CdpServerConfig {
            port: 0,
            headless: true,
        });
        let port = server.start().await.unwrap();

        let ws_url = format!("ws://127.0.0.1:{}/devtools/browser", port);
        let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("ws connect");

        // Send Target.getTargets.
        let msg = json!({
            "id": 1,
            "method": "Target.getTargets",
            "params": {}
        });
        ws.send(Message::Text(msg.to_string().into())).await.unwrap();

        // Read response.
        let resp = ws.next().await.unwrap().unwrap();
        let text = match resp {
            Message::Text(t) => t,
            other => panic!("expected text, got {:?}", other),
        };
        let resp: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(resp["id"], 1);
        let targets = resp["result"]["targetInfos"].as_array().unwrap();
        assert!(!targets.is_empty());
    }
}
