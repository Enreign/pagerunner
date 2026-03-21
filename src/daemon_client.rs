use crate::error::{PagerunnerError, Result};
use crate::ipc::{DaemonRequest, DaemonResponse, SOCKET_SUBPATH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use uuid::Uuid;

#[derive(Debug)]
pub struct DaemonClient {
    stream: UnixStream,
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
        Ok(Self { stream })
    }

    pub async fn call(&mut self, tool: &str, args: serde_json::Value) -> Result<String> {
        let req = DaemonRequest {
            id: Uuid::new_v4().to_string(),
            tool: tool.into(),
            args,
        };
        let mut msg = serde_json::to_string(&req)?;
        msg.push('\n');
        let (read_half, mut write_half) = tokio::io::split(&mut self.stream);
        write_half.write_all(msg.as_bytes()).await?;
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let resp: DaemonResponse = serde_json::from_str(line.trim())
            .map_err(|e| PagerunnerError::Config(e.to_string()))?;
        match (resp.result, resp.error) {
            (Some(r), _) => Ok(r),
            (_, Some(e)) => Err(PagerunnerError::Config(e)),
            _ => Err(PagerunnerError::Config("Empty daemon response".into())),
        }
    }
}
