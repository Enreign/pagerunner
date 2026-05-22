use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::error::{PagerunnerError, Result};

// ── Auto-zoom state ──

/// Zoom state shared between MCP dispatch (writer) and frame capture (reader).
#[derive(Debug, Clone)]
pub struct ZoomState {
    pub target_scale: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub current_scale: f64,
    pub current_x: f64,
    pub current_y: f64,
    pub viewport_w: f64,
    pub viewport_h: f64,
}

impl Default for ZoomState {
    fn default() -> Self {
        Self {
            target_scale: 1.0,
            target_x: 0.0,
            target_y: 0.0,
            current_scale: 1.0,
            current_x: 0.0,
            current_y: 0.0,
            viewport_w: 0.0,
            viewport_h: 0.0,
        }
    }
}

impl ZoomState {
    /// Set zoom target — capture loop smoothly interpolates toward this.
    pub fn zoom_to(&mut self, x: f64, y: f64, scale: f64) {
        self.target_scale = scale.clamp(1.0, 3.0);
        // Pre-clamp target position so we don't interpolate toward unreachable coords
        if self.viewport_w > 0.0 {
            let s = self.target_scale;
            let clip_w = self.viewport_w / s;
            let clip_h = self.viewport_h / s;
            self.target_x = x.max(clip_w / 2.0).min(self.viewport_w - clip_w / 2.0);
            self.target_y = y.max(clip_h / 2.0).min(self.viewport_h - clip_h / 2.0);
        } else {
            self.target_x = x;
            self.target_y = y;
        }
    }

    /// Reset to full viewport.
    pub fn zoom_out(&mut self) {
        self.target_scale = 1.0;
        // Move target to viewport center for smooth zoom-out
        if self.viewport_w > 0.0 {
            self.target_x = self.viewport_w / 2.0;
            self.target_y = self.viewport_h / 2.0;
        }
    }

    /// Interpolate current values toward target (called each frame).
    /// Uses aggressive lerp (0.35) so zoom settles in ~4 frames at 10fps.
    pub fn step(&mut self) {
        let lerp = 0.35;
        self.current_scale += (self.target_scale - self.current_scale) * lerp;
        self.current_x += (self.target_x - self.current_x) * lerp;
        self.current_y += (self.target_y - self.current_y) * lerp;
        // Snap when close — prevents lingering micro-movements
        if (self.target_scale - self.current_scale).abs() < 0.02 {
            self.current_scale = self.target_scale;
        }
        if (self.target_x - self.current_x).abs() < 1.0 {
            self.current_x = self.target_x;
        }
        if (self.target_y - self.current_y).abs() < 1.0 {
            self.current_y = self.target_y;
        }
    }

    /// Build CDP clip parameter for Page.captureScreenshot.
    /// Returns None when not zoomed (captures full viewport).
    pub fn clip_params(&self) -> Option<serde_json::Value> {
        if self.current_scale <= 1.01 || self.viewport_w == 0.0 {
            return None;
        }
        let s = self.current_scale;
        let clip_w = self.viewport_w / s;
        let clip_h = self.viewport_h / s;
        let clip_x = (self.current_x - clip_w / 2.0)
            .max(0.0)
            .min(self.viewport_w - clip_w);
        let clip_y = (self.current_y - clip_h / 2.0)
            .max(0.0)
            .min(self.viewport_h - clip_h);
        Some(serde_json::json!({
            "x": clip_x, "y": clip_y,
            "width": clip_w, "height": clip_h,
            "scale": s
        }))
    }
}

pub type SharedZoomState = Arc<RwLock<ZoomState>>;

// ── Recording types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub ts_ms: u64,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A zoom keyframe — stored in metadata, applied during render.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoomKeyframe {
    pub ts_ms: u64,
    pub x: f64,
    pub y: f64,
    pub scale: f64, // 1.0 = no zoom, 1.8 = zoomed in
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zoom_keyframes: Vec<ZoomKeyframe>,
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
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
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
                zoom_keyframes: vec![],
            },
            recording_dir: PathBuf::new(),
        }
    }

    /// Add a zoom keyframe at the current elapsed time.
    pub fn add_zoom(&mut self, x: f64, y: f64, scale: f64) {
        let ts_ms = self.elapsed_ms();
        self.metadata.zoom_keyframes.push(ZoomKeyframe {
            ts_ms,
            x,
            y,
            scale: scale.clamp(1.0, 3.0),
        });
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
    /// Handle to the frame capture task — aborted on stop to close the channel.
    capture_task: Option<tokio::task::JoinHandle<()>>,
    /// Shared zoom state for auto-zoom during recording.
    pub zoom: SharedZoomState,
}

impl RecordingHandle {
    /// Start a new recording: spawn ffmpeg and return the handle.
    /// `fps` is the capture rate, `output_fps` is the final video frame rate
    /// (ffmpeg minterpolate fills in the gap for smooth motion).
    pub async fn start(
        mut state: RecordingState,
        recording_dir: PathBuf,
        format: &str,
        fps: u8,
        output_fps: u8,
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
        let _out_fps_str = output_fps.to_string();

        // Encode at capture fps — motion interpolation applied during render_recording
        let ffmpeg_args: Vec<String> = match format {
            "webm" => vec![
                "-f",
                "image2pipe",
                "-c:v",
                "mjpeg",
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "libvpx-vp9",
                "-pix_fmt",
                "yuv420p",
                "-y",
                &video_path_str,
            ],
            _ => vec![
                "-f",
                "image2pipe",
                "-c:v",
                "mjpeg",
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "fast",
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
            let mut bytes_written: u64 = 0;
            let mut frames_written: u64 = 0;
            while let Some(frame_data) = frame_rx.recv().await {
                let len = frame_data.len();
                match stdin.write_all(&frame_data).await {
                    Ok(()) => {
                        if let Err(e) = stdin.flush().await {
                            tracing::warn!(error = %e, "ffmpeg stdin flush failed");
                            break;
                        }
                        bytes_written += len as u64;
                        frames_written += 1;
                        if frames_written <= 3 || frames_written.is_multiple_of(10) {
                            tracing::info!(
                                frame = frames_written,
                                frame_bytes = len,
                                total_bytes = bytes_written,
                                "Wrote frame to ffmpeg"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            bytes_written,
                            frames_written,
                            "ffmpeg stdin write failed"
                        );
                        break;
                    }
                }
            }
            tracing::info!(
                bytes_written,
                "Closing ffmpeg stdin, waiting for finalization"
            );
            drop(stdin);
            let status = child
                .wait()
                .await
                .map_err(|e| PagerunnerError::Config(format!("ffmpeg wait error: {}", e)))?;
            if !status.success() {
                tracing::warn!(status = %status, "ffmpeg encoding failed");
                return Err(PagerunnerError::Config(format!(
                    "ffmpeg exited with status: {}",
                    status
                )));
            }
            tracing::info!("ffmpeg encoding completed successfully");
            Ok(())
        });

        Ok(Self {
            state,
            frame_tx,
            ffmpeg_task,
            capture_task: None,
            zoom: Arc::new(RwLock::new(ZoomState::default())),
        })
    }

    /// Get a clone of the frame sender for the capture task.
    pub fn frame_tx_clone(&self) -> mpsc::Sender<Vec<u8>> {
        self.frame_tx.clone()
    }

    /// Set the capture task handle so it can be aborted on stop.
    pub fn set_capture_task(&mut self, task: tokio::task::JoinHandle<()>) {
        self.capture_task = Some(task);
    }

    /// Send a JPEG frame to the ffmpeg encoder.
    pub async fn send_frame(&self, jpeg_data: Vec<u8>) -> Result<()> {
        self.frame_tx
            .send(jpeg_data)
            .await
            .map_err(|_| PagerunnerError::Config("Recording frame channel closed".into()))
    }

    /// Stop the recording: abort capture, close the frame channel, wait for ffmpeg, save metadata.
    pub async fn stop(self) -> Result<RecordingMetadata> {
        let mut state = self.state;
        let now = chrono::Utc::now();
        state.metadata.stopped_at = Some(now);
        state.metadata.duration_ms =
            Some((now - state.metadata.started_at).num_milliseconds().max(0) as u64);

        // Reset zoom before stopping
        if let Ok(mut z) = self.zoom.write() {
            z.zoom_out();
        }

        // Abort the capture task first — it holds a clone of frame_tx.
        // Without this, dropping our frame_tx doesn't close the channel.
        if let Some(task) = self.capture_task {
            task.abort();
            let _ = task.await; // wait for abort to take effect
        }

        // Now drop the sender to close the channel — ffmpeg task will drop stdin
        drop(self.frame_tx);

        // Wait for ffmpeg to finalize — encoding + container finalization
        let timeout = std::time::Duration::from_secs(30);
        match tokio::time::timeout(timeout, self.ffmpeg_task).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => {
                tracing::warn!(error = %e, "ffmpeg encoding error during stop");
            }
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "ffmpeg task panicked");
            }
            Err(_) => {
                tracing::warn!("ffmpeg finalization timed out after 30s — video may be incomplete");
            }
        }

        state.save_metadata()?;

        Ok(state.metadata)
    }
}

/// Spawn a background task that periodically captures screenshots (JPEG)
/// and feeds them to the ffmpeg encoder.
///
/// Uses `Page.captureScreenshot` on a timer rather than `Page.startScreencast`
/// because screencast only emits frames when page content changes visually,
/// which misses static pages entirely.
pub fn spawn_frame_capture(
    cdp: crate::cdp::CdpConn,
    cdp_session_id: String,
    frame_tx: mpsc::Sender<Vec<u8>>,
    fps: u8,
    zoom: SharedZoomState,
) -> tokio::task::JoinHandle<()> {
    let interval = std::time::Duration::from_millis(1000 / fps.max(1) as u64);
    let mut frame_count: u64 = 0;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;

            if frame_tx.is_closed() {
                break;
            }

            let result = cdp
                .send_on_session_with_timeout(
                    "Page.captureScreenshot",
                    serde_json::json!({
                        "format": "jpeg",
                        "quality": 80,
                    }),
                    Some(cdp_session_id.clone()),
                    std::time::Duration::from_secs(2),
                )
                .await;

            let data_b64 = match result {
                Ok(val) => match val.get("data").and_then(|d| d.as_str()) {
                    Some(d) => d.to_string(),
                    None => continue,
                },
                Err(e) => {
                    tracing::warn!(error = %e, "Screenshot capture failed — skipping frame");
                    continue;
                }
            };

            let frame_bytes = match base64::engine::general_purpose::STANDARD.decode(&data_b64) {
                Ok(b) => b,
                Err(_) => continue,
            };

            // On first frame, detect viewport dimensions for zoom calculations
            if frame_count == 0 {
                // Get viewport size
                if let Ok(r) = cdp
                    .send_on_session(
                        "Runtime.evaluate",
                        serde_json::json!({
                            "expression": "({w:window.innerWidth,h:window.innerHeight})",
                            "returnByValue": true
                        }),
                        Some(cdp_session_id.clone()),
                    )
                    .await
                {
                    let v = &r["result"]["value"];
                    if let (Some(w), Some(h)) = (v["w"].as_f64(), v["h"].as_f64()) {
                        let mut z = zoom.write().unwrap_or_else(|e| e.into_inner());
                        z.viewport_w = w;
                        z.viewport_h = h;
                    }
                }
            }

            frame_count += 1;
            if frame_count == 1 {
                tracing::info!(
                    session = %cdp_session_id,
                    frame_size = frame_bytes.len(),
                    "First capture frame received"
                );
            }

            if frame_tx.send(frame_bytes).await.is_err() {
                tracing::info!("Frame channel closed — stopping capture");
                break;
            }
        }
        // Reset CSS zoom before exiting
        let _ = cdp.send_on_session(
            "Runtime.evaluate",
            serde_json::json!({"expression": "document.documentElement.style.transform='';document.documentElement.style.transformOrigin='';", "returnByValue": true}),
            Some(cdp_session_id.clone()),
        ).await;
        tracing::info!(frames = frame_count, "Frame capture exiting");
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
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.started_at));
    Ok(entries)
}

pub fn delete_recording_index(db: &crate::db::Db, recording_id: &str) -> Result<()> {
    db.delete(RECORDING_TABLE, recording_id)
}

/// Delete recordings older than `retention_days`. Returns count of deleted recordings.
pub fn cleanup_old_recordings(db: &crate::db::Db, retention_days: u64) -> Result<usize> {
    if retention_days == 0 {
        return Ok(0);
    }
    let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
    let all = db.scan_prefix(RECORDING_TABLE, "")?;
    let mut deleted = 0;
    for (_key, bytes) in all {
        let entry: RecordingIndexEntry = match serde_json::from_slice(&bytes) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.started_at < cutoff {
            // Delete files from disk
            let dir = std::path::PathBuf::from(&entry.dir_path);
            if dir.exists() {
                let _ = std::fs::remove_dir_all(&dir);
            }
            let _ = db.delete(RECORDING_TABLE, &entry.recording_id);
            deleted += 1;
        }
    }
    if deleted > 0 {
        tracing::info!(deleted, retention_days, "Cleaned up old recordings");
    }
    Ok(deleted)
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
            zoom_keyframes: vec![],
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
    fn test_recording_state_new_minimal() {
        let state = RecordingState::new(
            "rec_min".into(),
            "ses_min".into(),
            "p".into(),
            None,
            vec![],
            None,
            "webm".into(),
        );
        assert_eq!(state.metadata.format, "webm");
        assert!(state.metadata.flow.is_none());
        assert!(state.metadata.name.is_none());
        assert!(state.metadata.tags.is_empty());
        assert_eq!(state.recording_dir, PathBuf::new());
    }

    #[test]
    fn test_add_multiple_markers() {
        let mut state = RecordingState::new(
            "rec_1".into(),
            "ses_1".into(),
            "p".into(),
            None,
            vec![],
            None,
            "mp4".into(),
        );
        state.add_marker("A".into(), None, 1000);
        state.add_marker("B".into(), Some("desc B".into()), 2000);
        state.add_marker("C".into(), None, 5000);
        assert_eq!(state.metadata.markers.len(), 3);
        assert_eq!(state.metadata.markers[0].label, "A");
        assert!(state.metadata.markers[0].description.is_none());
        assert_eq!(
            state.metadata.markers[1].description.as_deref(),
            Some("desc B")
        );
        assert_eq!(state.metadata.markers[2].ts_ms, 5000);
    }

    #[test]
    fn test_metadata_skip_serializing_none_fields() {
        let meta = RecordingMetadata {
            recording_id: "r1".into(),
            session_id: "s1".into(),
            profile: "p".into(),
            flow: None,
            tags: vec![],
            name: None,
            started_at: chrono::Utc::now(),
            stopped_at: None,
            duration_ms: None,
            format: "mp4".into(),
            markers: vec![],
            zoom_keyframes: vec![],
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("\"flow\""));
        assert!(!json.contains("\"name\""));
        assert!(!json.contains("\"stopped_at\""));
        assert!(!json.contains("\"duration_ms\""));
    }

    #[test]
    fn test_metadata_includes_set_fields() {
        let meta = RecordingMetadata {
            recording_id: "r1".into(),
            session_id: "s1".into(),
            profile: "p".into(),
            flow: Some("deploy".into()),
            tags: vec!["prod".into()],
            name: Some("Deploy v1".into()),
            started_at: chrono::Utc::now(),
            stopped_at: Some(chrono::Utc::now()),
            duration_ms: Some(5000),
            format: "webm".into(),
            markers: vec![Marker {
                ts_ms: 100,
                label: "start".into(),
                description: None,
            }],
            zoom_keyframes: vec![],
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"flow\""));
        assert!(json.contains("deploy"));
        assert!(json.contains("\"stopped_at\""));
        assert!(json.contains("5000"));
        assert!(json.contains("\"webm\""));
    }

    #[test]
    fn test_marker_description_skip_none() {
        let m = Marker {
            ts_ms: 0,
            label: "x".into(),
            description: None,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("description"));
    }

    #[test]
    fn test_recording_dir_empty_flow() {
        let base = PathBuf::from("/tmp/rec");
        let path = recording_dir_path(&base, "p", "");
        // Should still produce a valid path
        assert!(path.starts_with("/tmp/rec/p"));
    }

    #[test]
    fn test_recording_dir_unicode_flow() {
        let base = PathBuf::from("/tmp/rec");
        let path = recording_dir_path(&base, "p", "демо-тест");
        let name = path.file_name().unwrap().to_str().unwrap();
        // Cyrillic chars are alphanumeric, should be kept
        assert!(name.contains("демо-тест"));
    }

    #[test]
    fn test_metadata_save_creates_pretty_json() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = RecordingState::new(
            "rec_pretty".into(),
            "ses_1".into(),
            "p".into(),
            None,
            vec![],
            None,
            "mp4".into(),
        );
        state.recording_dir = dir.path().to_path_buf();
        state.save_metadata().unwrap();
        let content = std::fs::read_to_string(dir.path().join("metadata.json")).unwrap();
        // Pretty JSON has newlines
        assert!(content.contains('\n'));
        assert!(content.contains("  "));
    }

    #[test]
    fn test_metadata_save_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = RecordingState::new(
            "rec_ow".into(),
            "ses_1".into(),
            "p".into(),
            None,
            vec![],
            None,
            "mp4".into(),
        );
        state.recording_dir = dir.path().to_path_buf();
        state.save_metadata().unwrap();
        state.add_marker("m1".into(), None, 500);
        state.save_metadata().unwrap();
        let content = std::fs::read_to_string(dir.path().join("metadata.json")).unwrap();
        let loaded: RecordingMetadata = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.markers.len(), 1);
    }

    #[tokio::test]
    async fn test_check_ffmpeg_available() {
        // This test verifies ffmpeg is available in the dev environment.
        // If ffmpeg is not installed, this test will fail — which is correct,
        // since video recording requires ffmpeg.
        let result = check_ffmpeg().await;
        assert!(
            result.is_ok(),
            "ffmpeg not found on PATH — install ffmpeg to run recording tests"
        );
    }

    #[tokio::test]
    async fn test_recording_handle_start_stop() {
        // End-to-end test: start a recording, send a dummy JPEG frame, stop it.
        let dir = tempfile::tempdir().unwrap();
        let rec_dir = dir.path().join("test_recording");

        let state = RecordingState::new(
            "rec_e2e".into(),
            "ses_1".into(),
            "test".into(),
            Some("e2e-flow".into()),
            vec!["integration".into()],
            Some("E2E Test".into()),
            "mp4".into(),
        );

        let handle = RecordingHandle::start(state, rec_dir.clone(), "mp4", 2, 2).await;
        if handle.is_err() {
            // ffmpeg not available — skip
            eprintln!("Skipping: ffmpeg not available");
            return;
        }
        let handle = handle.unwrap();

        // Send a minimal valid JPEG (smallest valid JPEG is the SOI+EOI markers)
        // ffmpeg needs real frames; send a tiny 1x1 JPEG
        let tiny_jpeg = create_tiny_jpeg();
        handle.send_frame(tiny_jpeg.clone()).await.unwrap();
        handle.send_frame(tiny_jpeg).await.unwrap();

        let metadata = handle.stop().await.unwrap();
        assert_eq!(metadata.recording_id, "rec_e2e");
        assert_eq!(metadata.profile, "test");
        assert_eq!(metadata.flow.as_deref(), Some("e2e-flow"));
        assert!(metadata.stopped_at.is_some());
        assert!(metadata.duration_ms.is_some());

        // metadata.json should exist
        assert!(rec_dir.join("metadata.json").exists());

        // video.mp4 should exist — but ffmpeg may fail to encode the minimal
        // JPEG frames, in which case the file may be empty or absent.
        // The important thing is that stop() didn't panic.
    }

    #[tokio::test]
    async fn test_recording_handle_stop_without_frames() {
        let dir = tempfile::tempdir().unwrap();
        let rec_dir = dir.path().join("empty_recording");

        let state = RecordingState::new(
            "rec_empty".into(),
            "ses_1".into(),
            "test".into(),
            None,
            vec![],
            None,
            "mp4".into(),
        );

        let handle = RecordingHandle::start(state, rec_dir.clone(), "mp4", 2, 2).await;
        if handle.is_err() {
            return; // ffmpeg not available
        }
        let handle = handle.unwrap();

        // Stop immediately without sending any frames
        let metadata = handle.stop().await.unwrap();
        assert_eq!(metadata.recording_id, "rec_empty");
        assert!(metadata.stopped_at.is_some());
        assert!(rec_dir.join("metadata.json").exists());
    }

    #[tokio::test]
    async fn test_recording_handle_with_markers() {
        let dir = tempfile::tempdir().unwrap();
        let rec_dir = dir.path().join("marker_recording");

        let state = RecordingState::new(
            "rec_markers".into(),
            "ses_1".into(),
            "test".into(),
            None,
            vec![],
            None,
            "mp4".into(),
        );

        let Ok(mut handle) = RecordingHandle::start(state, rec_dir.clone(), "mp4", 2, 2).await
        else {
            return;
        };

        // Add markers
        handle
            .state
            .add_marker("Step 1".into(), Some("First step".into()), 0);
        handle.state.add_marker("Step 2".into(), None, 1000);

        let metadata = handle.stop().await.unwrap();
        assert_eq!(metadata.markers.len(), 2);
        assert_eq!(metadata.markers[0].label, "Step 1");
        assert_eq!(metadata.markers[1].ts_ms, 1000);

        // Verify metadata file has markers
        let content = std::fs::read_to_string(rec_dir.join("metadata.json")).unwrap();
        let loaded: RecordingMetadata = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.markers.len(), 2);
    }

    #[tokio::test]
    async fn test_recording_handle_webm_format() {
        let dir = tempfile::tempdir().unwrap();
        let rec_dir = dir.path().join("webm_recording");

        let state = RecordingState::new(
            "rec_webm".into(),
            "ses_1".into(),
            "test".into(),
            None,
            vec![],
            None,
            "webm".into(),
        );

        let handle = RecordingHandle::start(state, rec_dir.clone(), "webm", 2, 2).await;
        if handle.is_err() {
            return;
        }
        let handle = handle.unwrap();

        let metadata = handle.stop().await.unwrap();
        assert_eq!(metadata.format, "webm");
        // video.webm may not exist if ffmpeg couldn't encode without frames.
        // The important thing is stop() completed and metadata was saved.
        assert!(rec_dir.join("metadata.json").exists());
    }

    #[test]
    fn test_recording_index_get() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = crate::db::Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key)
            .unwrap();

        // get nonexistent
        assert!(get_recording_index(&db, "nonexistent").unwrap().is_none());

        let entry = RecordingIndexEntry {
            recording_id: "rec_get".into(),
            session_id: "ses_1".into(),
            profile: "p".into(),
            flow: Some("f".into()),
            tags: vec!["t".into()],
            name: Some("n".into()),
            started_at: chrono::Utc::now(),
            duration_ms: Some(1000),
            format: "mp4".into(),
            dir_path: "/tmp/r".into(),
        };

        save_recording_index(&db, &entry).unwrap();
        let got = get_recording_index(&db, "rec_get").unwrap().unwrap();
        assert_eq!(got.recording_id, "rec_get");
        assert_eq!(got.dir_path, "/tmp/r");
        assert_eq!(got.format, "mp4");
    }

    #[test]
    fn test_recording_index_tag_filter() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = crate::db::Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key)
            .unwrap();

        let e1 = RecordingIndexEntry {
            recording_id: "rec_1".into(),
            session_id: "s".into(),
            profile: "p".into(),
            flow: None,
            tags: vec!["deploy".into(), "prod".into()],
            name: None,
            started_at: chrono::Utc::now(),
            duration_ms: None,
            format: "mp4".into(),
            dir_path: "/tmp/1".into(),
        };
        let e2 = RecordingIndexEntry {
            recording_id: "rec_2".into(),
            session_id: "s".into(),
            profile: "p".into(),
            flow: None,
            tags: vec!["demo".into()],
            name: None,
            started_at: chrono::Utc::now(),
            duration_ms: None,
            format: "mp4".into(),
            dir_path: "/tmp/2".into(),
        };

        save_recording_index(&db, &e1).unwrap();
        save_recording_index(&db, &e2).unwrap();

        let by_deploy = list_recordings(&db, None, None, Some("deploy")).unwrap();
        assert_eq!(by_deploy.len(), 1);
        assert_eq!(by_deploy[0].recording_id, "rec_1");

        let by_demo = list_recordings(&db, None, None, Some("demo")).unwrap();
        assert_eq!(by_demo.len(), 1);

        let by_missing = list_recordings(&db, None, None, Some("missing")).unwrap();
        assert!(by_missing.is_empty());
    }

    #[test]
    fn test_recording_index_combined_filters() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = crate::db::Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key)
            .unwrap();

        let e1 = RecordingIndexEntry {
            recording_id: "rec_a".into(),
            session_id: "s".into(),
            profile: "personal".into(),
            flow: Some("deploy".into()),
            tags: vec!["prod".into()],
            name: None,
            started_at: chrono::Utc::now(),
            duration_ms: None,
            format: "mp4".into(),
            dir_path: "/a".into(),
        };
        let e2 = RecordingIndexEntry {
            recording_id: "rec_b".into(),
            session_id: "s".into(),
            profile: "work".into(),
            flow: Some("deploy".into()),
            tags: vec!["prod".into()],
            name: None,
            started_at: chrono::Utc::now(),
            duration_ms: None,
            format: "mp4".into(),
            dir_path: "/b".into(),
        };

        save_recording_index(&db, &e1).unwrap();
        save_recording_index(&db, &e2).unwrap();

        // profile + flow
        let results = list_recordings(&db, Some("personal"), Some("deploy"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].recording_id, "rec_a");

        // profile + flow + tag
        let results = list_recordings(&db, Some("work"), Some("deploy"), Some("prod")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].recording_id, "rec_b");
    }

    #[test]
    fn test_recording_index_ordering() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = crate::db::Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key)
            .unwrap();

        let now = chrono::Utc::now();
        let earlier = now - chrono::Duration::hours(1);

        let e_old = RecordingIndexEntry {
            recording_id: "rec_old".into(),
            session_id: "s".into(),
            profile: "p".into(),
            flow: None,
            tags: vec![],
            name: None,
            started_at: earlier,
            duration_ms: None,
            format: "mp4".into(),
            dir_path: "/old".into(),
        };
        let e_new = RecordingIndexEntry {
            recording_id: "rec_new".into(),
            session_id: "s".into(),
            profile: "p".into(),
            flow: None,
            tags: vec![],
            name: None,
            started_at: now,
            duration_ms: None,
            format: "mp4".into(),
            dir_path: "/new".into(),
        };

        // Insert old first, new second
        save_recording_index(&db, &e_old).unwrap();
        save_recording_index(&db, &e_new).unwrap();

        let results = list_recordings(&db, None, None, None).unwrap();
        assert_eq!(results.len(), 2);
        // Newest first
        assert_eq!(results[0].recording_id, "rec_new");
        assert_eq!(results[1].recording_id, "rec_old");
    }

    #[test]
    fn test_delete_recording_index_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = crate::db::Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key)
            .unwrap();

        // Should not error
        delete_recording_index(&db, "nonexistent").unwrap();
    }

    #[test]
    fn test_recording_index_update() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = crate::db::Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key)
            .unwrap();

        let entry = RecordingIndexEntry {
            recording_id: "rec_upd".into(),
            session_id: "s".into(),
            profile: "p".into(),
            flow: None,
            tags: vec![],
            name: None,
            started_at: chrono::Utc::now(),
            duration_ms: None,
            format: "mp4".into(),
            dir_path: "/a".into(),
        };
        save_recording_index(&db, &entry).unwrap();

        // Update with duration
        let updated = RecordingIndexEntry {
            duration_ms: Some(5000),
            dir_path: "/b".into(),
            ..entry
        };
        save_recording_index(&db, &updated).unwrap();

        let got = get_recording_index(&db, "rec_upd").unwrap().unwrap();
        assert_eq!(got.duration_ms, Some(5000));
        assert_eq!(got.dir_path, "/b");

        // Should still be just 1 entry
        let all = list_recordings(&db, None, None, None).unwrap();
        assert_eq!(all.len(), 1);
    }

    /// Create a minimal valid 1x1 white JPEG for testing.
    fn create_tiny_jpeg() -> Vec<u8> {
        // Minimal JFIF JPEG: 1x1 pixel, white
        vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06,
            0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D,
            0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D,
            0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28,
            0x37, 0x29, 0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32,
            0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01,
            0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
            0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10,
            0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00,
            0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06,
            0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42,
            0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16,
            0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
            0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55,
            0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73,
            0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
            0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
            0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA,
            0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6,
            0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
            0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08,
            0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x7B, 0x94, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xD9,
        ]
    }

    #[test]
    fn test_recording_index_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::Db::generate_key();
        let db = crate::db::Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key)
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
