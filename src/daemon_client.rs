use crate::error::{PagerunnerError, Result};
use crate::ipc::{DaemonRequest, DaemonResponse, SOCKET_SUBPATH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use uuid::Uuid;

pub struct DaemonClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl DaemonClient {
    pub async fn connect() -> Result<Self> {
        let path = dirs::home_dir()
            .ok_or_else(|| PagerunnerError::Config("No home dir".into()))?
            .join(SOCKET_SUBPATH);
        let stream = UnixStream::connect(&path).await.map_err(|e| {
            PagerunnerError::Config(format!(
                "Cannot connect to daemon at {:?}: {}. Is `pagerunner daemon` running?",
                path, e
            ))
        })?;
        let (read_half, write_half) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(read_half),
            writer: write_half,
        })
    }

    pub async fn call(&mut self, tool: &str, args: serde_json::Value) -> Result<String> {
        let req = DaemonRequest {
            id: Uuid::new_v4().to_string(),
            tool: tool.into(),
            args,
        };
        let mut msg = serde_json::to_string(&req)?;
        msg.push('\n');
        self.writer.write_all(msg.as_bytes()).await?;
        let mut line = String::new();
        self.reader.read_line(&mut line).await?;
        let resp: DaemonResponse = serde_json::from_str(line.trim())
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        match (resp.result, resp.error) {
            (Some(r), _) => Ok(r),
            (_, Some(e)) => Err(PagerunnerError::Config(e)),
            _ => Err(PagerunnerError::Config("Empty daemon response".into())),
        }
    }

    /// Send a raw string (must include trailing newline).
    pub async fn send_raw(&mut self, data: &str) -> Result<()> {
        self.writer
            .write_all(data.as_bytes())
            .await
            .map_err(|e| PagerunnerError::Config(format!("Write error: {}", e)))
    }

    /// Read a single line from the daemon.
    pub async fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(PagerunnerError::Config("Daemon connection closed".into()));
        }
        Ok(line)
    }
}
