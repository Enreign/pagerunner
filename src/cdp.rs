use crate::error::{PagerunnerError, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Serialize)]
pub struct CdpMessage {
    pub id: u64,
    pub method: String,
    pub params: Value,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct CdpResponse {
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<Value>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

pub fn frame(msg: &CdpMessage) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(msg)?;
    bytes.push(b'\0');
    Ok(bytes)
}

struct CdpInner {
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>,
    event_tx: broadcast::Sender<Value>,
    next_id: AtomicU64,
    write_tx: mpsc::Sender<Vec<u8>>,
    /// Set to true by the reader task when Chrome's connection closes.
    /// send_on_session checks this to reject new requests immediately.
    closed: AtomicBool,
}

/// Thread-safe, Clone-able handle to a Chrome CDP pipe connection.
/// Background reader and writer tasks handle I/O; callers use async methods.
#[derive(Clone)]
pub struct CdpConn {
    inner: std::sync::Arc<CdpInner>,
}

impl CdpConn {
    /// Create a new CdpConn from write and read file descriptors of the Chrome process.
    /// Returns the connection handle and the reader task JoinHandle.
    pub fn new(write_fd: tokio::fs::File, read_fd: tokio::fs::File) -> (Self, JoinHandle<()>) {
        let (event_tx, _) = broadcast::channel(1024);
        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>(256);

        let inner = std::sync::Arc::new(CdpInner {
            pending: Mutex::new(HashMap::new()),
            event_tx,
            next_id: AtomicU64::new(1),
            write_tx,
            closed: AtomicBool::new(false),
        });

        tokio::spawn(writer_task(write_fd, write_rx));
        let reader_inner = inner.clone();
        let reader_handle = tokio::spawn(reader_task(read_fd, reader_inner));

        (CdpConn { inner }, reader_handle)
    }

    /// Connect to a Chrome instance via WebSocket (for `--remote-debugging-port` instances).
    /// `ws_url` is the `webSocketDebuggerUrl` from `GET http://localhost:<port>/json/version`.
    pub async fn connect_ws(ws_url: &str) -> Result<(Self, JoinHandle<()>)> {
        let (event_tx, _) = broadcast::channel(1024);
        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>(256);

        let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| PagerunnerError::Cdp(format!("WebSocket connect failed: {}", e)))?;
        let (ws_sink, ws_recv) = ws_stream.split();

        let inner = std::sync::Arc::new(CdpInner {
            pending: Mutex::new(HashMap::new()),
            event_tx,
            next_id: AtomicU64::new(1),
            write_tx,
            closed: AtomicBool::new(false),
        });

        tokio::spawn(ws_writer_task(ws_sink, write_rx));
        let reader_inner = inner.clone();
        let reader_handle = tokio::spawn(ws_reader_task(ws_recv, reader_inner));

        Ok((CdpConn { inner }, reader_handle))
    }

    pub async fn send(&self, method: &str, params: Value) -> Result<Value> {
        self.send_on_session(method, params, None).await
    }

    pub async fn send_on_session(
        &self,
        method: &str,
        params: Value,
        session_id: Option<String>,
    ) -> Result<Value> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(PagerunnerError::Cdp("Chrome connection closed".into()));
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self
                .inner
                .pending
                .lock()
                .map_err(|_| PagerunnerError::Cdp("Pending lock poisoned".into()))?;
            // Re-check after acquiring the lock — reader may have closed between the
            // first check and now.
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(PagerunnerError::Cdp("Chrome connection closed".into()));
            }
            pending.insert(id, tx);
        }
        let msg = CdpMessage {
            id,
            method: method.into(),
            params,
            session_id,
        };
        let framed = frame(&msg)?;
        self.inner
            .write_tx
            .send(framed)
            .await
            .map_err(|_| PagerunnerError::Cdp("Write channel closed".into()))?;
        rx.await
            .map_err(|_| PagerunnerError::Cdp("Response channel closed (Chrome exited?)".into()))?
    }

    /// Subscribe to all CDP events. Filter by `event["method"]` on the receiver side.
    pub fn subscribe_events(&self) -> broadcast::Receiver<Value> {
        self.inner.event_tx.subscribe()
    }
}

async fn writer_task(mut write_fd: tokio::fs::File, mut rx: mpsc::Receiver<Vec<u8>>) {
    while let Some(data) = rx.recv().await {
        if write_fd.write_all(&data).await.is_err() {
            break;
        }
    }
}

async fn reader_task(read_fd: tokio::fs::File, inner: std::sync::Arc<CdpInner>) {
    let mut reader = tokio::io::BufReader::new(read_fd);
    loop {
        let mut buf = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            match reader.read_exact(&mut byte).await {
                Err(_) => {
                    // Pipe closed — mark closed, then unblock all pending send() callers.
                    inner.closed.store(true, Ordering::Release);
                    if let Ok(mut pending) = inner.pending.lock() {
                        for (_, tx) in pending.drain() {
                            let _ = tx.send(Err(PagerunnerError::Cdp(
                                "Chrome connection closed".into(),
                            )));
                        }
                    }
                    return;
                }
                Ok(_) => {}
            }
            if byte[0] == b'\0' {
                break;
            }
            buf.push(byte[0]);
        }
        if buf.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_slice(&buf) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(id) = v.get("id").and_then(|id| id.as_u64()) {
            if let Ok(mut pending) = inner.pending.lock() {
                if let Some(tx) = pending.remove(&id) {
                    if let Some(err) = v.get("error") {
                        let _ = tx.send(Err(PagerunnerError::Cdp(err.to_string())));
                    } else {
                        let _ = tx.send(Ok(v["result"].clone()));
                    }
                }
            }
        } else {
            let _ = inner.event_tx.send(v);
        }
    }
}

async fn ws_writer_task(
    mut sink: impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    mut rx: mpsc::Receiver<Vec<u8>>,
) {
    while let Some(mut data) = rx.recv().await {
        // frame() appends a null byte for pipe transport; strip it for WebSocket
        if data.last() == Some(&b'\0') {
            data.pop();
        }
        let text = String::from_utf8_lossy(&data).into_owned();
        if sink.send(Message::Text(text)).await.is_err() {
            break;
        }
    }
}

async fn ws_reader_task(
    mut stream: impl StreamExt<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
    inner: std::sync::Arc<CdpInner>,
) {
    while let Some(msg) = stream.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) | Err(_) => break,
            _ => continue,
        };
        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(id) = v.get("id").and_then(|id| id.as_u64()) {
            if let Ok(mut pending) = inner.pending.lock() {
                if let Some(tx) = pending.remove(&id) {
                    if let Some(err) = v.get("error") {
                        let _ = tx.send(Err(PagerunnerError::Cdp(err.to_string())));
                    } else {
                        let _ = tx.send(Ok(v["result"].clone()));
                    }
                }
            }
        } else {
            let _ = inner.event_tx.send(v);
        }
    }
    // Chrome connection closed — mark closed first, then drain pending.
    // The flag ensures any concurrent send_on_session call that races past
    // the drain will also get an immediate error.
    inner.closed.store(true, Ordering::Release);
    if let Ok(mut pending) = inner.pending.lock() {
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(PagerunnerError::Cdp("Chrome connection closed".into())));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    async fn fake_chrome_response(
        mut write_end: tokio::fs::File,
        id: u64,
        result: serde_json::Value,
    ) {
        let resp = serde_json::json!({ "id": id, "result": result });
        let mut bytes = serde_json::to_vec(&resp).unwrap();
        bytes.push(b'\0');
        write_end.write_all(&bytes).await.unwrap();
    }

    async fn fake_chrome_event(
        mut write_end: tokio::fs::File,
        method: &str,
        params: serde_json::Value,
    ) {
        let evt = serde_json::json!({ "method": method, "params": params });
        let mut bytes = serde_json::to_vec(&evt).unwrap();
        bytes.push(b'\0');
        write_end.write_all(&bytes).await.unwrap();
    }

    fn make_pipe_pair() -> (tokio::fs::File, tokio::fs::File) {
        use nix::unistd::pipe;
        use std::os::unix::io::{FromRawFd, IntoRawFd};
        let (r, w) = pipe().unwrap();
        let read_end = unsafe { tokio::fs::File::from_raw_fd(r.into_raw_fd()) };
        let write_end = unsafe { tokio::fs::File::from_raw_fd(w.into_raw_fd()) };
        (read_end, write_end)
    }

    #[tokio::test]
    async fn test_send_receives_response() {
        let (cmd_read, cmd_write) = make_pipe_pair();
        let (evt_read, evt_write) = make_pipe_pair();

        let (conn, _handle) = CdpConn::new(cmd_write, evt_read);

        tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut reader = tokio::io::BufReader::new(cmd_read);
            use tokio::io::AsyncReadExt;
            loop {
                let mut byte = [0u8; 1];
                reader.read_exact(&mut byte).await.unwrap();
                if byte[0] == b'\0' {
                    break;
                }
                buf.push(byte[0]);
            }
            let msg: serde_json::Value = serde_json::from_slice(&buf).unwrap();
            let id = msg["id"].as_u64().unwrap();
            fake_chrome_response(evt_write, id, serde_json::json!({"ok": true})).await;
        });

        let result = conn
            .send("Target.getTargets", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(result["ok"], true);
    }

    #[tokio::test]
    async fn test_events_are_broadcast() {
        let (cmd_read, cmd_write) = make_pipe_pair();
        let (evt_read, evt_write) = make_pipe_pair();

        let (conn, _handle) = CdpConn::new(cmd_write, evt_read);
        let mut rx = conn.subscribe_events();

        drop(cmd_read);

        tokio::spawn(async move {
            fake_chrome_event(
                evt_write,
                "Network.responseReceived",
                serde_json::json!({"requestId": "abc"}),
            )
            .await;
        });

        let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(event["method"], "Network.responseReceived");
    }

    #[tokio::test]
    async fn test_events_not_confused_with_responses() {
        use nix::unistd::dup;
        use std::os::unix::io::{FromRawFd, IntoRawFd};

        let (cmd_read, cmd_write) = make_pipe_pair();
        let (evt_read, evt_write) = make_pipe_pair();

        let evt_write_raw = evt_write.into_std().await.into_raw_fd();
        let evt_write2_raw = dup(nix::libc::c_int::from(evt_write_raw as i32)).unwrap();
        let evt_write1 = unsafe { tokio::fs::File::from_raw_fd(evt_write_raw) };
        let evt_write2 = unsafe { tokio::fs::File::from_raw_fd(evt_write2_raw) };

        let (conn, _handle) = CdpConn::new(cmd_write, evt_read);
        let mut rx = conn.subscribe_events();

        tokio::spawn(async move {
            drop(cmd_read);
            fake_chrome_event(evt_write1, "Page.loadEventFired", serde_json::json!({})).await;
            drop(evt_write2);
        });

        let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event["method"], "Page.loadEventFired");
    }

    #[test]
    fn test_frame_message() {
        let msg = CdpMessage {
            id: 1,
            method: "Target.getTargets".into(),
            params: serde_json::json!({}),
            session_id: None,
        };
        let framed = frame(&msg).unwrap();
        assert!(framed.ends_with(b"\0"));
        let json: serde_json::Value =
            serde_json::from_slice(&framed[..framed.len() - 1]).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["method"], "Target.getTargets");
    }

    #[test]
    fn test_parse_response() {
        let raw = br#"{"id":1,"result":{"targetInfos":[]}}"#;
        let resp: serde_json::Value = serde_json::from_slice(raw).unwrap();
        assert_eq!(resp["id"], 1);
    }
}
