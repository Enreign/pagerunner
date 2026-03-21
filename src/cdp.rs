use crate::error::{PagerunnerError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

pub struct CdpConn {
    writer: tokio::fs::File,
    reader: tokio::io::BufReader<tokio::fs::File>,
    next_id: u64,
}

impl CdpConn {
    pub fn new(write_fd: tokio::fs::File, read_fd: tokio::fs::File) -> Self {
        Self {
            writer: write_fd,
            reader: tokio::io::BufReader::new(read_fd),
            next_id: 1,
        }
    }

    pub async fn send(&mut self, method: &str, params: Value) -> Result<Value> {
        self.send_on_session(method, params, None).await
    }

    pub async fn send_on_session(
        &mut self,
        method: &str,
        params: Value,
        session_id: Option<String>,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let msg = CdpMessage {
            id,
            method: method.into(),
            params,
            session_id,
        };
        let framed = frame(&msg)?;
        self.writer.write_all(&framed).await?;

        // Read responses until we get the one matching our id
        loop {
            let raw = self.read_message().await?;
            let v: Value = serde_json::from_slice(&raw)?;

            if v.get("id") == Some(&Value::Number(id.into())) {
                if let Some(err) = v.get("error") {
                    return Err(PagerunnerError::Cdp(err.to_string()));
                }
                return Ok(v["result"].clone());
            }
            // Events (no "id") are dropped for now
        }
    }

    async fn read_message(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            self.reader
                .read_exact(&mut byte)
                .await
                .map_err(|e| PagerunnerError::Cdp(format!("Pipe read error: {}", e)))?;
            if byte[0] == b'\0' {
                break;
            }
            buf.push(byte[0]);
        }
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let json: serde_json::Value = serde_json::from_slice(&framed[..framed.len() - 1]).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["method"], "Target.getTargets");
    }

    #[test]
    fn test_parse_response() {
        let raw = br#"{"id":1,"result":{"targetInfos":[]}}"#;
        let resp: CdpResponse = serde_json::from_slice(raw).unwrap();
        assert_eq!(resp.id, 1);
        assert!(resp.error.is_none());
    }
}
