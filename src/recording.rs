use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::error::{PagerunnerError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub ts_ms: u64,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingMetadata {
    pub recording_id: String,
    pub session_id: String,
    pub profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub format: String,
    pub markers: Vec<Marker>,
}

/// Resolve the base recordings directory.
pub fn resolve_recordings_dir(custom: Option<&str>) -> PathBuf {
    if let Some(dir) = custom {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pagerunner/recordings")
}

/// Build the directory path for a specific recording.
/// Format: <base>/<profile>/<YYYY-MM-DD>_<flow_or_id>/
pub fn recording_dir_path(base: &std::path::Path, profile: &str, name_or_flow: &str) -> PathBuf {
    let date = chrono::Utc::now().format("%Y-%m-%d");
    let sanitized: String = name_or_flow
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    base.join(profile).join(format!("{}_{}", date, sanitized))
}

/// Check that ffmpeg is available on PATH.
pub async fn check_ffmpeg() -> Result<()> {
    let output = tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|_| {
            PagerunnerError::Config(
                "ffmpeg not found. Install ffmpeg to use video recording.".into(),
            )
        })?;
    if !output.success() {
        return Err(PagerunnerError::Config(
            "ffmpeg returned non-zero exit code".into(),
        ));
    }
    Ok(())
}

pub struct RecordingState {
    pub metadata: RecordingMetadata,
    pub recording_dir: PathBuf,
}

impl RecordingState {
    pub fn new(
        recording_id: String,
        session_id: String,
        profile: String,
        flow: Option<String>,
        tags: Vec<String>,
        name: Option<String>,
        format: String,
    ) -> Self {
        Self {
            metadata: RecordingMetadata {
                recording_id,
                session_id,
                profile,
                flow,
                tags,
                name,
                started_at: chrono::Utc::now(),
                stopped_at: None,
                duration_ms: None,
                format,
                markers: vec![],
            },
            recording_dir: PathBuf::new(),
        }
    }

    pub fn add_marker(&mut self, label: String, description: Option<String>, ts_ms: u64) {
        self.metadata.markers.push(Marker {
            ts_ms,
            label,
            description,
        });
    }

    /// Elapsed time since recording started, in milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        let now = chrono::Utc::now();
        (now - self.metadata.started_at).num_milliseconds().max(0) as u64
    }

    /// Write the metadata JSON sidecar to disk.
    pub fn save_metadata(&self) -> Result<()> {
        let path = self.recording_dir.join("metadata.json");
        let json = serde_json::to_string_pretty(&self.metadata)
            .map_err(|e| PagerunnerError::Config(format!("Failed to serialize metadata: {}", e)))?;
        std::fs::write(&path, json)
            .map_err(|e| PagerunnerError::Config(format!("Failed to write metadata: {}", e)))?;
        Ok(())
    }
}

/// Handle to a running recording. Owns the ffmpeg child process and frame channel.
pub struct RecordingHandle {
    pub state: RecordingState,
    frame_tx: mpsc::Sender<Vec<u8>>,
    ffmpeg_task: tokio::task::JoinHandle<Result<()>>,
}

impl RecordingHandle {
    /// Start a new recording: spawn ffmpeg and return the handle.
    pub async fn start(
        mut state: RecordingState,
        recording_dir: PathBuf,
        format: &str,
        fps: u8,
    ) -> Result<Self> {
        check_ffmpeg().await?;

        std::fs::create_dir_all(&recording_dir).map_err(|e| {
            PagerunnerError::Config(format!("Failed to create recording dir: {}", e))
        })?;

        state.recording_dir = recording_dir.clone();

        let video_ext = match format {
            "webm" => "webm",
            _ => "mp4",
        };
        let video_path = recording_dir.join(format!("video.{}", video_ext));
        let video_path_str = video_path.to_str().unwrap().to_string();
        let fps_str = fps.to_string();

        let ffmpeg_args: Vec<String> = match format {
            "webm" => vec![
                "-f",
                "image2pipe",
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "libvpx-vp9",
                "-pix_fmt",
                "yuva420p",
                "-y",
                &video_path_str,
            ],
            _ => vec![
                "-f",
                "image2pipe",
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                "-y",
                &video_path_str,
            ],
        }
        .into_iter()
        .map(|s| s.to_string())
        .collect();

        let mut child = tokio::process::Command::new("ffmpeg")
            .args(&ffmpeg_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PagerunnerError::Config(format!("Failed to spawn ffmpeg: {}", e)))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| PagerunnerError::Config("Failed to open ffmpeg stdin".into()))?;

        let (frame_tx, mut frame_rx) = mpsc::channel::<Vec<u8>>(64);

        let ffmpeg_task = tokio::spawn(async move {
            while let Some(frame_data) = frame_rx.recv().await {
                if stdin.write_all(&frame_data).await.is_err() {
                    break;
                }
            }
            drop(stdin);
            let status = child
                .wait()
                .await
                .map_err(|e| PagerunnerError::Config(format!("ffmpeg wait error: {}", e)))?;
            if !status.success() {
                return Err(PagerunnerError::Config(format!(
                    "ffmpeg exited with status: {}",
                    status
                )));
            }
            Ok(())
        });

        Ok(Self {
            state,
            frame_tx,
            ffmpeg_task,
        })
    }

    /// Get a clone of the frame sender for the frame processor task.
    pub fn frame_tx_clone(&self) -> mpsc::Sender<Vec<u8>> {
        self.frame_tx.clone()
    }

    /// Send a JPEG frame to the ffmpeg encoder.
    pub async fn send_frame(&self, jpeg_data: Vec<u8>) -> Result<()> {
        self.frame_tx
            .send(jpeg_data)
            .await
            .map_err(|_| PagerunnerError::Config("Recording frame channel closed".into()))
    }

    /// Stop the recording: close the frame channel, wait for ffmpeg, save metadata.
    pub async fn stop(self) -> Result<RecordingMetadata> {
        let mut state = self.state;
        let now = chrono::Utc::now();
        state.metadata.stopped_at = Some(now);
        state.metadata.duration_ms =
            Some((now - state.metadata.started_at).num_milliseconds().max(0) as u64);

        drop(self.frame_tx);

        match self.ffmpeg_task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "ffmpeg encoding error during stop");
            }
            Err(e) => {
                tracing::warn!(error = %e, "ffmpeg task panicked");
            }
        }

        state.save_metadata()?;

        Ok(state.metadata)
    }
}

/// Spawn a background task that listens for CDP Page.screencastFrame events
/// and feeds decoded JPEG frames to the recording handle.
pub fn spawn_frame_processor(
    cdp: crate::cdp::CdpConn,
    cdp_session_id: String,
    frame_tx: mpsc::Sender<Vec<u8>>,
) -> tokio::task::JoinHandle<()> {
    let mut events = cdp.subscribe_events();
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            let method = event.get("method").and_then(|m| m.as_str());
            if method != Some("Page.screencastFrame") {
                continue;
            }
            let event_session = event.get("sessionId").and_then(|s| s.as_str());
            if event_session != Some(&cdp_session_id) {
                continue;
            }

            let params = match event.get("params") {
                Some(p) => p,
                None => continue,
            };

            let data_b64 = match params.get("data").and_then(|d| d.as_str()) {
                Some(d) => d,
                None => continue,
            };
            let frame_bytes = match base64::engine::general_purpose::STANDARD.decode(data_b64) {
                Ok(b) => b,
                Err(_) => continue,
            };

            // Ack the frame so Chrome sends the next one
            let session_id_val = params
                .get("sessionId")
                .and_then(|n| n.as_u64())
                .unwrap_or(0);
            let _ = cdp
                .send_on_session(
                    "Page.screencastFrameAck",
                    serde_json::json!({"sessionId": session_id_val}),
                    Some(cdp_session_id.clone()),
                )
                .await;

            if frame_tx.send(frame_bytes).await.is_err() {
                break;
            }
        }
    })
}

// --- DB index for recordings ---

const RECORDING_TABLE: &str = "recordings";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingIndexEntry {
    pub recording_id: String,
    pub session_id: String,
    pub profile: String,
    pub flow: Option<String>,
    pub tags: Vec<String>,
    pub name: Option<String>,
    pub started_at: DateTime<Utc>,
    pub duration_ms: Option<u64>,
    pub format: String,
    pub dir_path: String,
}

pub fn save_recording_index(db: &crate::db::Db, entry: &RecordingIndexEntry) -> Result<()> {
    let json = serde_json::to_vec(entry)
        .map_err(|e| PagerunnerError::Config(format!("serialize recording index: {}", e)))?;
    db.put(RECORDING_TABLE, &entry.recording_id, &json)
}

pub fn get_recording_index(
    db: &crate::db::Db,
    recording_id: &str,
) -> Result<Option<RecordingIndexEntry>> {
    match db.get(RECORDING_TABLE, recording_id)? {
        None => Ok(None),
        Some(bytes) => {
            let entry: RecordingIndexEntry = serde_json::from_slice(&bytes)
                .map_err(|e| PagerunnerError::Config(format!("deserialize recording: {}", e)))?;
            Ok(Some(entry))
        }
    }
}

pub fn list_recordings(
    db: &crate::db::Db,
    profile: Option<&str>,
    flow: Option<&str>,
    tag: Option<&str>,
) -> Result<Vec<RecordingIndexEntry>> {
    let all = db.scan_prefix(RECORDING_TABLE, "")?;
    let mut entries = Vec::new();
    for (_key, bytes) in all {
        let entry: RecordingIndexEntry = match serde_json::from_slice(&bytes) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if let Some(p) = profile {
            if entry.profile != p {
                continue;
            }
        }
        if let Some(f) = flow {
            if entry.flow.as_deref() != Some(f) {
                continue;
            }
        }
        if let Some(t) = tag {
            if !entry.tags.iter().any(|et| et == t) {
                continue;
            }
        }
        entries.push(entry);
    }
    entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(entries)
}

pub fn delete_recording_index(db: &crate::db::Db, recording_id: &str) -> Result<()> {
    db.delete(RECORDING_TABLE, recording_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recording_metadata_roundtrip() {
        let meta = RecordingMetadata {
            recording_id: "rec_abc123".to_string(),
            session_id: "ses_xyz".to_string(),
            profile: "personal".to_string(),
            flow: Some("people-hub-demo".to_string()),
            tags: vec!["feature-demo".to_string()],
            name: Some("People Hub Demo".to_string()),
            started_at: chrono::Utc::now(),
            stopped_at: None,
            duration_ms: None,
            format: "mp4".to_string(),
            markers: vec![],
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: RecordingMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.recording_id, "rec_abc123");
        assert_eq!(parsed.profile, "personal");
        assert_eq!(parsed.flow.as_deref(), Some("people-hub-demo"));
    }

    #[test]
    fn test_marker_serialization() {
        let marker = Marker {
            ts_ms: 3000,
            label: "Open People hub".to_string(),
            description: Some("Click sidebar People icon".to_string()),
        };
        let json = serde_json::to_string(&marker).unwrap();
        assert!(json.contains("3000"));
        assert!(json.contains("Open People hub"));
    }

    #[test]
    fn test_recordings_dir_default() {
        let dir = resolve_recordings_dir(None);
        assert!(dir.to_str().unwrap().contains(".pagerunner/recordings"));
    }

    #[test]
    fn test_recordings_dir_custom() {
        let dir = resolve_recordings_dir(Some("/tmp/my-recordings"));
        assert_eq!(dir, std::path::PathBuf::from("/tmp/my-recordings"));
    }

    #[test]
    fn test_recording_dir_path() {
        let base = std::path::PathBuf::from("/tmp/recordings");
        let path = recording_dir_path(&base, "personal", "people-hub-demo");
        assert!(path.starts_with("/tmp/recordings/personal"));
        assert!(path.to_str().unwrap().contains("people-hub-demo"));
    }

    #[test]
    fn test_recording_dir_sanitizes_special_chars() {
        let base = std::path::PathBuf::from("/tmp/recordings");
        let path = recording_dir_path(&base, "personal", "deploy/v2.3.1@prod");
        let dir_name = path.file_name().unwrap().to_str().unwrap();
        assert!(!dir_name.contains('/'));
        assert!(!dir_name.contains('@'));
    }

    #[test]
    fn test_recording_state_new() {
        let state = RecordingState::new(
            "rec_123".to_string(),
            "ses_456".to_string(),
            "personal".to_string(),
            Some("demo".to_string()),
            vec!["tag1".to_string()],
            Some("My Demo".to_string()),
            "mp4".to_string(),
        );
        assert_eq!(state.metadata.recording_id, "rec_123");
        assert!(state.metadata.markers.is_empty());
        assert!(state.metadata.stopped_at.is_none());
    }

    #[test]
    fn test_add_marker() {
        let mut state = RecordingState::new(
            "rec_123".to_string(),
            "ses_456".to_string(),
            "personal".to_string(),
            None,
            vec![],
            None,
            "mp4".to_string(),
        );
        state.add_marker("Step 1".to_string(), Some("Description".to_string()), 3000);
        assert_eq!(state.metadata.markers.len(), 1);
        assert_eq!(state.metadata.markers[0].ts_ms, 3000);
        assert_eq!(state.metadata.markers[0].label, "Step 1");
    }

    #[test]
    fn test_elapsed_ms() {
        let state = RecordingState::new(
            "rec_1".into(),
            "ses_1".into(),
            "p".into(),
            None,
            vec![],
            None,
            "mp4".into(),
        );
        assert!(state.elapsed_ms() < 1000);
    }

    #[test]
    fn test_metadata_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = RecordingState::new(
            "rec_save".into(),
            "ses_1".into(),
            "p".into(),
            Some("flow1".into()),
            vec!["t1".into()],
            Some("Test".into()),
            "mp4".into(),
        );
        state.recording_dir = dir.path().to_path_buf();
        state.add_marker("m1".into(), None, 1000);
        state.save_metadata().unwrap();

        let path = dir.path().join("metadata.json");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: RecordingMetadata = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.recording_id, "rec_save");
        assert_eq!(loaded.markers.len(), 1);
    }

    #[test]
    fn test_recording_index_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = crate::db::Db::open_with_key(
            dir.path().join("test.db").to_str().unwrap(),
            key,
        )
        .unwrap();

        let entry = RecordingIndexEntry {
            recording_id: "rec_123".to_string(),
            session_id: "ses_456".to_string(),
            profile: "personal".to_string(),
            flow: Some("demo".to_string()),
            tags: vec!["feature".to_string()],
            name: Some("Demo".to_string()),
            started_at: chrono::Utc::now(),
            duration_ms: Some(5000),
            format: "mp4".to_string(),
            dir_path: "/tmp/recordings/personal/2026-04-06_demo".to_string(),
        };

        save_recording_index(&db, &entry).unwrap();
        let entries = list_recordings(&db, None, None, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].recording_id, "rec_123");

        let entries = list_recordings(&db, Some("personal"), None, None).unwrap();
        assert_eq!(entries.len(), 1);
        let entries = list_recordings(&db, Some("other"), None, None).unwrap();
        assert!(entries.is_empty());

        let entries = list_recordings(&db, None, Some("demo"), None).unwrap();
        assert_eq!(entries.len(), 1);

        delete_recording_index(&db, "rec_123").unwrap();
        let entries = list_recordings(&db, None, None, None).unwrap();
        assert!(entries.is_empty());
    }
}
