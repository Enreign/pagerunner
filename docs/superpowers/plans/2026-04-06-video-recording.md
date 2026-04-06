# Video Recording Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add video recording to Pagerunner using CDP screencast, with marker annotations, local storage organized by profile+flow, and optional annotated rendering via ffmpeg.

**Architecture:** CDP `Page.startScreencast` streams JPEG frames to a background task that pipes them into ffmpeg for real-time MP4/WebM encoding. Recording state lives on `Session` (one active recording per session). Metadata (markers, timestamps, tags) is stored as a JSON sidecar alongside the video file. A DB index in ReDB enables querying recordings by profile, flow, or tag. Phase 2 adds `render_recording` which re-encodes with ffmpeg text overlay filters from the marker sidecar.

**Tech Stack:** Rust, CDP (Page.startScreencast/stopScreencast), ffmpeg (spawned as child process), ReDB, serde_json, tokio

---

## File Structure

| File | Responsibility |
|---|---|
| `src/recording.rs` (create) | Core recording module: `RecordingState`, frame handler, ffmpeg pipe, marker management, storage, DB index CRUD |
| `src/recording_render.rs` (create) | Phase 2: ffmpeg overlay rendering from marker sidecar |
| `src/config.rs` (modify) | Add `RecordingConfig` struct |
| `src/session.rs` (modify) | Add `recording_state: Option<RecordingHandle>` field to `Session` |
| `src/mcp_server.rs` (modify) | Register 6 new tools, add dispatch arms |
| `src/main.rs` (modify) | Add 6 CLI subcommands + `mod recording` |
| `src/browser.rs` (modify) | Add `start_screencast` / `stop_screencast` CDP helpers |
| `src/audit.rs` (modify) | Add `RecordingStarted` / `RecordingStopped` audit event kinds |

---

## Phase 1: Record + Organize Locally

### Task 1: RecordingConfig in config.rs

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn test_recording_config_defaults() {
    let config = PagerunnerConfig::default();
    assert!(config.recording.storage_dir.is_none());
    assert_eq!(config.recording.retention_days, 0);
    assert_eq!(config.recording.max_size_mb, 0);
    assert_eq!(config.recording.format, RecordingFormat::Mp4);
    assert!(!config.recording.auto_record);
    assert_eq!(config.recording.fps, 2);
}

#[test]
fn test_recording_config_from_toml() {
    let toml = r#"
[recording]
storage_dir = "/tmp/recordings"
retention_days = 30
max_size_mb = 500
format = "webm"
auto_record = true
fps = 5
"#;
    let config: PagerunnerConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.recording.storage_dir.as_deref(), Some("/tmp/recordings"));
    assert_eq!(config.recording.retention_days, 30);
    assert_eq!(config.recording.max_size_mb, 500);
    assert_eq!(config.recording.format, RecordingFormat::Webm);
    assert!(config.recording.auto_record);
    assert_eq!(config.recording.fps, 5);
}

#[test]
fn test_recording_config_absent_gives_defaults() {
    let toml = r#"
[[profiles]]
name = "test"
display_name = "Test"
user_data_dir = "/tmp/t"
"#;
    let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
    assert!(!cfg.recording.auto_record);
    assert_eq!(cfg.recording.fps, 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_recording_config`
Expected: FAIL — `RecordingConfig` and `RecordingFormat` do not exist

- [ ] **Step 3: Write minimal implementation**

Add to `src/config.rs` before `PagerunnerConfig`:

```rust
fn default_recording_fps() -> u8 {
    2
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingFormat {
    #[default]
    Mp4,
    Webm,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecordingConfig {
    /// Override default storage dir (~/.pagerunner/recordings)
    #[serde(default)]
    pub storage_dir: Option<String>,
    /// Auto-delete recordings older than N days. 0 = never.
    #[serde(default)]
    pub retention_days: u64,
    /// Max recording file size in MB. 0 = unlimited.
    #[serde(default)]
    pub max_size_mb: u64,
    /// Output format: mp4 or webm
    #[serde(default)]
    pub format: RecordingFormat,
    /// Automatically record every session (dashcam mode)
    #[serde(default)]
    pub auto_record: bool,
    /// Frames per second for screencast capture (1-10, default 2)
    #[serde(default = "default_recording_fps")]
    pub fps: u8,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            storage_dir: None,
            retention_days: 0,
            max_size_mb: 0,
            format: RecordingFormat::default(),
            auto_record: false,
            fps: default_recording_fps(),
        }
    }
}
```

Add field to `PagerunnerConfig`:

```rust
#[serde(default)]
pub recording: RecordingConfig,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_recording_config`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(recording): add RecordingConfig to config.rs"
```

---

### Task 2: Recording module — types and storage helpers

**Files:**
- Create: `src/recording.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test recording::tests`
Expected: FAIL — module does not exist

- [ ] **Step 3: Write minimal implementation**

Create `src/recording.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    let sanitized = name_or_flow
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>();
    base.join(profile).join(format!("{}_{}", date, sanitized))
}

#[cfg(test)]
mod tests {
    // ... tests from step 1 ...
}
```

Add `mod recording;` to `src/main.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test recording::tests`
Expected: PASS (5 tests)

- [ ] **Step 5: Commit**

```bash
git add src/recording.rs src/main.rs
git commit -m "feat(recording): add types, metadata, and storage helpers"
```

---

### Task 3: CDP screencast helpers in browser.rs

**Files:**
- Modify: `src/browser.rs`

- [ ] **Step 1: Write the unit test**

These functions wrap CDP calls and cannot be unit-tested without a live Chrome. We'll add integration tests in Task 8. For now, write the implementation and verify compilation.

- [ ] **Step 2: Implement start_screencast and stop_screencast**

Add to `src/browser.rs`:

```rust
/// Start CDP screencast — Chrome will push frames as events.
/// Returns Ok(()) on success.
pub async fn start_screencast(
    session: &mut Session,
    target_id: &str,
    format: &str,
    quality: u8,
    max_width: u32,
    max_height: u32,
    every_nth_frame: u32,
) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    session
        .cdp
        .send_on_session(
            "Page.startScreencast",
            json!({
                "format": format,
                "quality": quality,
                "maxWidth": max_width,
                "maxHeight": max_height,
                "everyNthFrame": every_nth_frame,
            }),
            Some(session_id),
        )
        .await?;
    Ok(())
}

/// Stop CDP screencast.
pub async fn stop_screencast(session: &mut Session, target_id: &str) -> Result<()> {
    let session_id = attach_to_target(session, target_id).await?;
    session
        .cdp
        .send_on_session("Page.stopScreencast", json!({}), Some(session_id))
        .await?;
    Ok(())
}

/// Acknowledge a screencast frame so Chrome sends the next one.
pub async fn ack_screencast_frame(cdp: &CdpConn, cdp_session_id: &str, frame_number: u64) -> Result<()> {
    cdp.send_on_session(
        "Page.screencastFrameAck",
        json!({"sessionId": frame_number}),
        Some(cdp_session_id.to_string()),
    )
    .await?;
    Ok(())
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 4: Commit**

```bash
git add src/browser.rs
git commit -m "feat(recording): add CDP screencast helpers to browser.rs"
```

---

### Task 4: Recording engine — ffmpeg pipe and frame handler

**Files:**
- Modify: `src/recording.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn test_ffmpeg_availability() {
    let result = check_ffmpeg().await;
    // This test passes if ffmpeg is installed, fails with a clear message if not
    assert!(result.is_ok(), "ffmpeg not found: {:?}", result.err());
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
    // Simulate 3 seconds elapsed
    state.add_marker("Step 1".to_string(), Some("Description".to_string()), 3000);
    assert_eq!(state.metadata.markers.len(), 1);
    assert_eq!(state.metadata.markers[0].ts_ms, 3000);
    assert_eq!(state.metadata.markers[0].label, "Step 1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test recording::tests`
Expected: FAIL — `RecordingState`, `check_ffmpeg` don't exist

- [ ] **Step 3: Write implementation**

Add to `src/recording.rs`:

```rust
use crate::error::{PagerunnerError, Result};
use std::process::Stdio;
use tokio::process::{Child, Command};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

/// Check that ffmpeg is available on PATH.
pub async fn check_ffmpeg() -> Result<()> {
    let output = Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|_| PagerunnerError::Config(
            "ffmpeg not found. Install ffmpeg to use video recording.".into()
        ))?;
    if !output.success() {
        return Err(PagerunnerError::Config("ffmpeg returned non-zero exit code".into()));
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
                profile: profile.clone(),
                flow: flow.clone(),
                tags,
                name: name.clone(),
                started_at: chrono::Utc::now(),
                stopped_at: None,
                duration_ms: None,
                format,
                markers: vec![],
            },
            recording_dir: PathBuf::new(), // set by caller before starting
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

        std::fs::create_dir_all(&recording_dir)
            .map_err(|e| PagerunnerError::Config(format!("Failed to create recording dir: {}", e)))?;

        state.recording_dir = recording_dir.clone();

        let video_ext = match format {
            "webm" => "webm",
            _ => "mp4",
        };
        let video_path = recording_dir.join(format!("video.{}", video_ext));

        // ffmpeg args: read JPEG frames from stdin pipe, encode to video
        let ffmpeg_args = match format {
            "webm" => vec![
                "-f", "image2pipe", "-framerate", &fps.to_string(),
                "-i", "pipe:0",
                "-c:v", "libvpx-vp9", "-pix_fmt", "yuva420p",
                "-y", video_path.to_str().unwrap(),
            ],
            _ => vec![
                "-f", "image2pipe", "-framerate", &fps.to_string(),
                "-i", "pipe:0",
                "-c:v", "libx264", "-pix_fmt", "yuv420p",
                "-movflags", "+faststart",
                "-y", video_path.to_str().unwrap(),
            ],
        };

        let fps_str = fps.to_string();
        let video_path_str = video_path.to_str().unwrap().to_string();
        let format_owned = format.to_string();

        let mut child = Command::new("ffmpeg")
            .args(match format_owned.as_str() {
                "webm" => vec![
                    "-f", "image2pipe", "-framerate", &fps_str,
                    "-i", "pipe:0",
                    "-c:v", "libvpx-vp9", "-pix_fmt", "yuva420p",
                    "-y", &video_path_str,
                ],
                _ => vec![
                    "-f", "image2pipe", "-framerate", &fps_str,
                    "-i", "pipe:0",
                    "-c:v", "libx264", "-pix_fmt", "yuv420p",
                    "-movflags", "+faststart",
                    "-y", &video_path_str,
                ],
            })
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PagerunnerError::Config(format!("Failed to spawn ffmpeg: {}", e)))?;

        let mut stdin = child.stdin.take()
            .ok_or_else(|| PagerunnerError::Config("Failed to open ffmpeg stdin".into()))?;

        let (frame_tx, mut frame_rx) = mpsc::channel::<Vec<u8>>(64);

        let ffmpeg_task = tokio::spawn(async move {
            while let Some(frame_data) = frame_rx.recv().await {
                if stdin.write_all(&frame_data).await.is_err() {
                    break;
                }
            }
            // Close stdin to signal ffmpeg to finalize
            drop(stdin);
            let status = child.wait().await
                .map_err(|e| PagerunnerError::Config(format!("ffmpeg wait error: {}", e)))?;
            if !status.success() {
                return Err(PagerunnerError::Config(
                    format!("ffmpeg exited with status: {}", status)
                ));
            }
            Ok(())
        });

        Ok(Self {
            state,
            frame_tx,
            ffmpeg_task,
        })
    }

    /// Send a JPEG frame to the ffmpeg encoder.
    pub async fn send_frame(&self, jpeg_data: Vec<u8>) -> Result<()> {
        self.frame_tx.send(jpeg_data).await
            .map_err(|_| PagerunnerError::Config("Recording frame channel closed".into()))
    }

    /// Stop the recording: close the frame channel, wait for ffmpeg, save metadata.
    pub async fn stop(self) -> Result<RecordingMetadata> {
        let mut state = self.state;
        let now = chrono::Utc::now();
        state.metadata.stopped_at = Some(now);
        state.metadata.duration_ms = Some(
            (now - state.metadata.started_at).num_milliseconds().max(0) as u64
        );

        // Drop the sender to close the channel, which makes ffmpeg finalize
        drop(self.frame_tx);

        // Wait for ffmpeg to finish
        match self.ffmpeg_task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "ffmpeg encoding error during stop");
            }
            Err(e) => {
                tracing::warn!(error = %e, "ffmpeg task panicked");
            }
        }

        // Save metadata sidecar
        state.save_metadata()?;

        Ok(state.metadata)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test recording::tests`
Expected: PASS (all tests including new ones)

- [ ] **Step 5: Commit**

```bash
git add src/recording.rs
git commit -m "feat(recording): add RecordingState, RecordingHandle, ffmpeg pipe engine"
```

---

### Task 5: Recording state on Session + frame processor

**Files:**
- Modify: `src/session.rs`
- Modify: `src/recording.rs`

- [ ] **Step 1: Add recording handle field to Session**

Add to `Session` struct in `src/session.rs`:

```rust
/// Active recording handle — None when not recording.
pub recording: Option<crate::recording::RecordingHandle>,
```

Initialize it as `None` in the `open()` and `attach()` constructors.

- [ ] **Step 2: Add screencast frame processor to recording.rs**

```rust
/// Spawn a background task that listens for CDP Page.screencastFrame events
/// and feeds them to the recording handle.
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
            // Only process events for our CDP session
            let event_session = event.get("sessionId").and_then(|s| s.as_str());
            if event_session != Some(&cdp_session_id) {
                continue;
            }

            let params = match event.get("params") {
                Some(p) => p,
                None => continue,
            };

            // Decode base64 frame data
            let data_b64 = match params.get("data").and_then(|d| d.as_str()) {
                Some(d) => d,
                None => continue,
            };
            let frame_bytes = match base64::engine::general_purpose::STANDARD.decode(data_b64) {
                Ok(b) => b,
                Err(_) => continue,
            };

            // Ack the frame so Chrome sends the next one
            let frame_number = params
                .get("sessionId")  // CDP's "sessionId" in screencastFrame params is the frame number
                .and_then(|n| n.as_u64())
                .unwrap_or(0);
            let _ = cdp.send_on_session(
                "Page.screencastFrameAck",
                serde_json::json!({"sessionId": frame_number}),
                Some(cdp_session_id.clone()),
            ).await;

            // Send frame to ffmpeg
            if frame_tx.send(frame_bytes).await.is_err() {
                break; // Recording stopped
            }
        }
    })
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 4: Commit**

```bash
git add src/session.rs src/recording.rs
git commit -m "feat(recording): add recording field to Session, frame processor"
```

---

### Task 6: DB index for recordings

**Files:**
- Modify: `src/recording.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn test_recording_index_roundtrip() {
    let dir = tempdir().unwrap();
    let key = crate::db::Db::generate_key();
    let db = crate::db::Db::open_with_key(dir.path().join("test.db").to_str().unwrap(), key).unwrap();
    let db = std::sync::Arc::new(db);

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

    // Filter by profile
    let entries = list_recordings(&db, Some("personal"), None, None).unwrap();
    assert_eq!(entries.len(), 1);
    let entries = list_recordings(&db, Some("other"), None, None).unwrap();
    assert!(entries.is_empty());

    // Filter by flow
    let entries = list_recordings(&db, None, Some("demo"), None).unwrap();
    assert_eq!(entries.len(), 1);

    // Delete
    delete_recording_index(&db, "rec_123").unwrap();
    let entries = list_recordings(&db, None, None, None).unwrap();
    assert!(entries.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_recording_index`
Expected: FAIL — types and functions don't exist

- [ ] **Step 3: Write implementation**

Add to `src/recording.rs`:

```rust
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

pub fn save_recording_index(
    db: &crate::db::Db,
    entry: &RecordingIndexEntry,
) -> Result<()> {
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
            if entry.profile != p { continue; }
        }
        if let Some(f) = flow {
            if entry.flow.as_deref() != Some(f) { continue; }
        }
        if let Some(t) = tag {
            if !entry.tags.iter().any(|et| et == t) { continue; }
        }
        entries.push(entry);
    }
    // Sort by started_at descending (newest first)
    entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(entries)
}

pub fn delete_recording_index(db: &crate::db::Db, recording_id: &str) -> Result<()> {
    db.delete(RECORDING_TABLE, recording_id)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_recording_index`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/recording.rs
git commit -m "feat(recording): add DB index for recording metadata"
```

---

### Task 7: MCP tools — register and dispatch

**Files:**
- Modify: `src/mcp_server.rs`

- [ ] **Step 1: Add tool definitions to all_tools()**

Add these 6 tools to the `all_tools()` vec in `src/mcp_server.rs`:

```rust
json!({
    "name": "start_recording",
    "description": "Start recording the current tab as video. Uses CDP screencast to capture frames and ffmpeg to encode. Requires ffmpeg on PATH.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "session_id": { "type": "string", "description": "Session to record" },
            "target_id": { "type": "string", "description": "Tab to record" },
            "name": { "type": "string", "description": "Optional recording name" },
            "tags": {
                "type": "array", "items": { "type": "string" },
                "description": "Optional tags for organization"
            },
            "flow": { "type": "string", "description": "Optional flow label (e.g. 'deploy-v2.3.1', 'people-hub-demo')" }
        },
        "required": ["session_id", "target_id"]
    }
}),
json!({
    "name": "stop_recording",
    "description": "Stop the active recording on a session. Finalizes the video file and saves metadata.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "session_id": { "type": "string", "description": "Session with active recording" }
        },
        "required": ["session_id"]
    }
}),
json!({
    "name": "add_marker",
    "description": "Add a timestamped marker/annotation to the active recording. Markers are saved in the metadata sidecar and can be rendered as text overlays.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "session_id": { "type": "string", "description": "Session with active recording" },
            "label": { "type": "string", "description": "Short label for this marker" },
            "description": { "type": "string", "description": "Optional longer description" }
        },
        "required": ["session_id", "label"]
    }
}),
json!({
    "name": "list_recordings",
    "description": "List saved recordings, optionally filtered by profile, flow, or tag.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "profile": { "type": "string", "description": "Filter by profile name" },
            "flow": { "type": "string", "description": "Filter by flow label" },
            "tag": { "type": "string", "description": "Filter by tag" }
        }
    }
}),
json!({
    "name": "get_recording",
    "description": "Get details about a specific recording including path, duration, and markers.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "recording_id": { "type": "string", "description": "Recording ID" }
        },
        "required": ["recording_id"]
    }
}),
json!({
    "name": "delete_recording",
    "description": "Delete a recording and its files from disk.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "recording_id": { "type": "string", "description": "Recording ID to delete" }
        },
        "required": ["recording_id"]
    }
}),
```

- [ ] **Step 2: Add dispatch arms to dispatch_tool_inner()**

Add match arms in the `dispatch_tool_inner` function:

```rust
"start_recording" => {
    let session_id_str = args["session_id"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Missing session_id".into()))?
        .to_string();
    let target_id = args["target_id"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Missing target_id".into()))?
        .to_string();
    let name = args["name"].as_str().map(|s| s.to_string());
    let flow = args["flow"].as_str().map(|s| s.to_string());
    let tags: Vec<String> = args["tags"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let mut mgr = sessions.lock().await;
    let session = mgr
        .get_mut(&session_id_str)
        .ok_or_else(|| PagerunnerError::Config(format!("Session {} not found", session_id_str)))?;

    if session.recording.is_some() {
        return Err(PagerunnerError::Config("Recording already active on this session".into()));
    }

    // Block recording in anonymization mode (like screenshots)
    if session.anon_config.is_some() {
        return Err(PagerunnerError::Config(
            "Recording is blocked when anonymization is active (would capture PII)".into()
        ));
    }

    let recording_id = format!("rec_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string());
    let profile = session.profile_name.clone();
    let flow_label = flow.clone().or_else(|| name.clone()).unwrap_or_else(|| recording_id.clone());

    let base_dir = crate::recording::resolve_recordings_dir(
        config.recording.storage_dir.as_deref()
    );
    let rec_dir = crate::recording::recording_dir_path(&base_dir, &profile, &flow_label);

    let format_str = match config.recording.format {
        crate::config::RecordingFormat::Webm => "webm",
        crate::config::RecordingFormat::Mp4 => "mp4",
    };

    let state = crate::recording::RecordingState::new(
        recording_id.clone(),
        session_id_str.clone(),
        profile.clone(),
        flow,
        tags,
        name,
        format_str.to_string(),
    );

    let fps = config.recording.fps.max(1).min(10);
    let handle = crate::recording::RecordingHandle::start(state, rec_dir, format_str, fps).await?;

    // Start CDP screencast
    let cdp_session_id = crate::browser::attach_to_target(session, &target_id).await?;

    // Spawn frame processor
    let frame_tx = handle.frame_tx_clone();
    let processor = crate::recording::spawn_frame_processor(
        session.cdp.clone(),
        cdp_session_id.clone(),
        frame_tx,
    );

    // Start screencast
    crate::browser::start_screencast(session, &target_id, "jpeg", 80, 1280, 720, 1).await?;

    session.recording = Some(handle);

    Ok(serde_json::json!({
        "ok": true,
        "recording_id": recording_id,
        "message": format!("Recording started for session {}", session_id_str)
    }).to_string())
}

"stop_recording" => {
    let session_id_str = args["session_id"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Missing session_id".into()))?
        .to_string();

    let mut mgr = sessions.lock().await;
    let session = mgr
        .get_mut(&session_id_str)
        .ok_or_else(|| PagerunnerError::Config(format!("Session {} not found", session_id_str)))?;

    let handle = session.recording.take()
        .ok_or_else(|| PagerunnerError::Config("No active recording on this session".into()))?;

    // Stop screencast (best-effort — session may have switched tabs)
    // We don't have the target_id stored, so we stop for all targets
    let _ = session.cdp.send("Page.stopScreencast", serde_json::json!({})).await;

    let metadata = handle.stop().await?;

    // Save to DB index
    let entry = crate::recording::RecordingIndexEntry {
        recording_id: metadata.recording_id.clone(),
        session_id: metadata.session_id.clone(),
        profile: metadata.profile.clone(),
        flow: metadata.flow.clone(),
        tags: metadata.tags.clone(),
        name: metadata.name.clone(),
        started_at: metadata.started_at,
        duration_ms: metadata.duration_ms,
        format: metadata.format.clone(),
        dir_path: metadata.recording_id.clone(), // will be set properly
    };
    let _ = crate::recording::save_recording_index(&db, &entry);

    Ok(serde_json::json!({
        "ok": true,
        "recording_id": metadata.recording_id,
        "duration_ms": metadata.duration_ms,
        "markers": metadata.markers.len(),
    }).to_string())
}

"add_marker" => {
    let session_id_str = args["session_id"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Missing session_id".into()))?
        .to_string();
    let label = args["label"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Missing label".into()))?
        .to_string();
    let description = args["description"].as_str().map(|s| s.to_string());

    let mut mgr = sessions.lock().await;
    let session = mgr
        .get_mut(&session_id_str)
        .ok_or_else(|| PagerunnerError::Config(format!("Session {} not found", session_id_str)))?;

    let handle = session.recording.as_mut()
        .ok_or_else(|| PagerunnerError::Config("No active recording on this session".into()))?;

    let ts_ms = handle.state.elapsed_ms();
    handle.state.add_marker(label.clone(), description, ts_ms);

    Ok(serde_json::json!({
        "ok": true,
        "ts_ms": ts_ms,
        "label": label,
        "total_markers": handle.state.metadata.markers.len(),
    }).to_string())
}

"list_recordings" => {
    let profile = args["profile"].as_str();
    let flow = args["flow"].as_str();
    let tag = args["tag"].as_str();

    let entries = crate::recording::list_recordings(&db, profile, flow, tag)?;
    let items: Vec<serde_json::Value> = entries.iter().map(|e| {
        serde_json::json!({
            "recording_id": e.recording_id,
            "profile": e.profile,
            "flow": e.flow,
            "name": e.name,
            "tags": e.tags,
            "started_at": e.started_at.to_rfc3339(),
            "duration_ms": e.duration_ms,
            "format": e.format,
        })
    }).collect();

    Ok(serde_json::json!({
        "ok": true,
        "recordings": items,
        "count": items.len(),
    }).to_string())
}

"get_recording" => {
    let recording_id = args["recording_id"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Missing recording_id".into()))?;

    let entry = crate::recording::get_recording_index(&db, recording_id)?
        .ok_or_else(|| PagerunnerError::Config(format!("Recording {} not found", recording_id)))?;

    // Try to read the metadata sidecar for full marker details
    let metadata_path = std::path::PathBuf::from(&entry.dir_path).join("metadata.json");
    let markers: Vec<serde_json::Value> = if metadata_path.exists() {
        match std::fs::read_to_string(&metadata_path) {
            Ok(json) => {
                if let Ok(meta) = serde_json::from_str::<crate::recording::RecordingMetadata>(&json) {
                    meta.markers.iter().map(|m| serde_json::json!({
                        "ts_ms": m.ts_ms,
                        "label": m.label,
                        "description": m.description,
                    })).collect()
                } else { vec![] }
            }
            Err(_) => vec![],
        }
    } else { vec![] };

    let video_ext = &entry.format;
    let video_path = std::path::PathBuf::from(&entry.dir_path).join(format!("video.{}", video_ext));

    Ok(serde_json::json!({
        "ok": true,
        "recording_id": entry.recording_id,
        "profile": entry.profile,
        "flow": entry.flow,
        "name": entry.name,
        "tags": entry.tags,
        "started_at": entry.started_at.to_rfc3339(),
        "duration_ms": entry.duration_ms,
        "format": entry.format,
        "video_path": video_path.to_str(),
        "metadata_path": metadata_path.to_str(),
        "dir_path": entry.dir_path,
        "markers": markers,
    }).to_string())
}

"delete_recording" => {
    let recording_id = args["recording_id"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Missing recording_id".into()))?;

    let entry = crate::recording::get_recording_index(&db, recording_id)?
        .ok_or_else(|| PagerunnerError::Config(format!("Recording {} not found", recording_id)))?;

    // Delete files from disk
    let dir = std::path::PathBuf::from(&entry.dir_path);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| PagerunnerError::Config(format!("Failed to delete recording dir: {}", e)))?;
    }

    // Remove from DB index
    crate::recording::delete_recording_index(&db, recording_id)?;

    Ok(serde_json::json!({
        "ok": true,
        "deleted": recording_id,
    }).to_string())
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 4: Commit**

```bash
git add src/mcp_server.rs
git commit -m "feat(recording): register 6 MCP tools and dispatch handlers"
```

---

### Task 8: CLI subcommands

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add CLI subcommand variants**

Add to the `Commands` enum:

```rust
/// Start recording a tab as video
#[command(name = "start-recording")]
StartRecording {
    session_id: String,
    target_id: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    flow: Option<String>,
    #[arg(long, value_delimiter = ',')]
    tags: Option<Vec<String>>,
},
/// Stop the active recording on a session
#[command(name = "stop-recording")]
StopRecording {
    session_id: String,
},
/// Add a timestamped marker to the active recording
#[command(name = "add-marker")]
AddMarker {
    session_id: String,
    label: String,
    #[arg(long)]
    description: Option<String>,
},
/// List saved recordings
#[command(name = "list-recordings")]
ListRecordings {
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    flow: Option<String>,
    #[arg(long)]
    tag: Option<String>,
},
/// Get details about a specific recording
#[command(name = "get-recording")]
GetRecording {
    recording_id: String,
},
/// Delete a recording and its files
#[command(name = "delete-recording")]
DeleteRecording {
    recording_id: String,
},
```

- [ ] **Step 2: Add match arms in run()**

```rust
Commands::StartRecording { session_id, target_id, name, flow, tags } => {
    let config = config::PagerunnerConfig::load()?;
    let mut args = serde_json::json!({"session_id": session_id, "target_id": target_id});
    if let Some(v) = name { args["name"] = serde_json::json!(v); }
    if let Some(v) = flow { args["flow"] = serde_json::json!(v); }
    if let Some(v) = tags { args["tags"] = serde_json::json!(v); }
    crate::cli_tools::run_tool("start_recording", args, crate::cli_tools::ScreenshotMode::File, &config).await?;
}
Commands::StopRecording { session_id } => {
    let config = config::PagerunnerConfig::load()?;
    crate::cli_tools::run_tool("stop_recording", serde_json::json!({"session_id": session_id}), crate::cli_tools::ScreenshotMode::File, &config).await?;
}
Commands::AddMarker { session_id, label, description } => {
    let config = config::PagerunnerConfig::load()?;
    let mut args = serde_json::json!({"session_id": session_id, "label": label});
    if let Some(v) = description { args["description"] = serde_json::json!(v); }
    crate::cli_tools::run_tool("add_marker", args, crate::cli_tools::ScreenshotMode::File, &config).await?;
}
Commands::ListRecordings { profile, flow, tag } => {
    let config = config::PagerunnerConfig::load()?;
    let mut args = serde_json::json!({});
    if let Some(v) = profile { args["profile"] = serde_json::json!(v); }
    if let Some(v) = flow { args["flow"] = serde_json::json!(v); }
    if let Some(v) = tag { args["tag"] = serde_json::json!(v); }
    crate::cli_tools::run_tool("list_recordings", args, crate::cli_tools::ScreenshotMode::File, &config).await?;
}
Commands::GetRecording { recording_id } => {
    let config = config::PagerunnerConfig::load()?;
    crate::cli_tools::run_tool("get_recording", serde_json::json!({"recording_id": recording_id}), crate::cli_tools::ScreenshotMode::File, &config).await?;
}
Commands::DeleteRecording { recording_id } => {
    let config = config::PagerunnerConfig::load()?;
    crate::cli_tools::run_tool("delete_recording", serde_json::json!({"recording_id": recording_id}), crate::cli_tools::ScreenshotMode::File, &config).await?;
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(recording): add 6 CLI subcommands for video recording"
```

---

### Task 9: Audit events for recording

**Files:**
- Modify: `src/audit.rs`
- Modify: `src/main.rs` (format_audit_event)

- [ ] **Step 1: Add audit event kinds**

Add to `AuditEventKind` enum in `src/audit.rs`:

```rust
RecordingStarted {
    session_id: String,
    recording_id: String,
    profile: String,
},
RecordingStopped {
    session_id: String,
    recording_id: String,
    duration_ms: Option<u64>,
    markers_count: usize,
},
```

- [ ] **Step 2: Add format_audit_event arm in main.rs**

```rust
crate::audit::AuditEventKind::RecordingStarted { session_id, recording_id, profile } => {
    let sid = if session_id.len() >= 8 { &session_id[..8] } else { session_id };
    format!("[{}] RECORDING_STARTED session={} recording={} profile={}", ts, sid, recording_id, profile)
}
crate::audit::AuditEventKind::RecordingStopped { session_id, recording_id, duration_ms, markers_count } => {
    let sid = if session_id.len() >= 8 { &session_id[..8] } else { session_id };
    let dur = duration_ms.map(|d| format!("{}ms", d)).unwrap_or_else(|| "?".to_string());
    format!("[{}] RECORDING_STOPPED session={} recording={} duration={} markers={}", ts, sid, recording_id, dur, markers_count)
}
```

- [ ] **Step 3: Wire audit events into start_recording and stop_recording dispatch**

In `dispatch_tool_inner`, after successful start/stop, add:

```rust
// In start_recording handler, after session.recording = Some(handle):
if let Some(audit) = audit {
    audit.record(crate::audit::AuditEvent::new(
        crate::audit::AuditEventKind::RecordingStarted {
            session_id: session_id_str.clone(),
            recording_id: recording_id.clone(),
            profile: profile.clone(),
        }
    )).await;
}

// In stop_recording handler, after metadata = handle.stop():
if let Some(audit) = audit {
    audit.record(crate::audit::AuditEvent::new(
        crate::audit::AuditEventKind::RecordingStopped {
            session_id: session_id_str.clone(),
            recording_id: metadata.recording_id.clone(),
            duration_ms: metadata.duration_ms,
            markers_count: metadata.markers.len(),
        }
    )).await;
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 5: Commit**

```bash
git add src/audit.rs src/main.rs src/mcp_server.rs
git commit -m "feat(recording): add audit events for recording start/stop"
```

---

### Task 10: Unit tests for recording module

**Files:**
- Modify: `src/recording.rs`

All tests from Tasks 2, 4, 6 should already exist. This task adds any remaining edge case tests.

- [ ] **Step 1: Add edge case tests**

```rust
#[test]
fn test_recording_dir_sanitizes_special_chars() {
    let base = std::path::PathBuf::from("/tmp/recordings");
    let path = recording_dir_path(&base, "personal", "deploy/v2.3.1@prod");
    let dir_name = path.file_name().unwrap().to_str().unwrap();
    assert!(!dir_name.contains('/'));
    assert!(!dir_name.contains('@'));
}

#[test]
fn test_elapsed_ms() {
    let state = RecordingState::new(
        "rec_1".into(), "ses_1".into(), "p".into(),
        None, vec![], None, "mp4".into(),
    );
    // elapsed should be >= 0
    assert!(state.elapsed_ms() < 1000);
}

#[test]
fn test_metadata_save_and_load() {
    let dir = tempdir().unwrap();
    let mut state = RecordingState::new(
        "rec_save".into(), "ses_1".into(), "p".into(),
        Some("flow1".into()), vec!["t1".into()], Some("Test".into()), "mp4".into(),
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
```

- [ ] **Step 2: Run all recording tests**

Run: `cargo test recording`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add src/recording.rs
git commit -m "test(recording): add edge case unit tests"
```

---

## Phase 2: Render Annotated Video

### Task 11: Render recording module

**Files:**
- Create: `src/recording_render.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_ffmpeg_filter_no_markers() {
        let markers: Vec<crate::recording::Marker> = vec![];
        let filter = build_subtitle_filter(&markers, 30000);
        assert!(filter.is_none());
    }

    #[test]
    fn test_build_ffmpeg_filter_single_marker() {
        let markers = vec![crate::recording::Marker {
            ts_ms: 3000,
            label: "Step 1".to_string(),
            description: Some("Click the button".to_string()),
        }];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
        // Should produce a drawtext filter with enable='between(t,3,8)'
        assert!(filter.contains("drawtext"));
        assert!(filter.contains("Step 1"));
    }

    #[test]
    fn test_build_ffmpeg_filter_multiple_markers() {
        let markers = vec![
            crate::recording::Marker { ts_ms: 1000, label: "A".to_string(), description: None },
            crate::recording::Marker { ts_ms: 5000, label: "B".to_string(), description: None },
            crate::recording::Marker { ts_ms: 10000, label: "C".to_string(), description: None },
        ];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
        // Each marker should appear until the next one starts (or +5s for last)
        assert!(filter.contains("A"));
        assert!(filter.contains("B"));
        assert!(filter.contains("C"));
    }

    #[test]
    fn test_escape_ffmpeg_text() {
        assert_eq!(escape_ffmpeg_text("it's a test"), "it\\'s a test");
        assert_eq!(escape_ffmpeg_text("line1:line2"), "line1\\:line2");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test recording_render::tests`
Expected: FAIL — module does not exist

- [ ] **Step 3: Write implementation**

Create `src/recording_render.rs`:

```rust
use crate::error::{PagerunnerError, Result};
use crate::recording::{Marker, RecordingMetadata, RecordingIndexEntry, get_recording_index};
use std::path::PathBuf;
use tokio::process::Command;

/// Escape special characters for ffmpeg drawtext filter.
pub fn escape_ffmpeg_text(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace(':', "\\:")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace(';', "\\;")
}

/// Build an ffmpeg drawtext filter chain from markers.
/// Each marker is displayed from its timestamp until the next marker (or +5s for the last one).
/// Returns None if there are no markers.
pub fn build_subtitle_filter(markers: &[Marker], total_duration_ms: u64) -> Option<String> {
    if markers.is_empty() {
        return None;
    }

    let mut filters = Vec::new();

    for (i, marker) in markers.iter().enumerate() {
        let start_s = marker.ts_ms as f64 / 1000.0;
        let end_s = if i + 1 < markers.len() {
            markers[i + 1].ts_ms as f64 / 1000.0
        } else {
            // Last marker: show for 5 seconds or until end of video
            let end = start_s + 5.0;
            let total = total_duration_ms as f64 / 1000.0;
            if end > total { total } else { end }
        };

        let text = if let Some(desc) = &marker.description {
            format!("{}: {}", marker.label, desc)
        } else {
            marker.label.clone()
        };

        let escaped = escape_ffmpeg_text(&text);

        filters.push(format!(
            "drawtext=text='{}':fontsize=24:fontcolor=white:borderw=2:bordercolor=black:x=20:y=h-60:enable='between(t,{:.2},{:.2})'",
            escaped, start_s, end_s
        ));
    }

    Some(filters.join(","))
}

/// Render an annotated version of a recording by compositing marker overlays.
pub async fn render_annotated(
    db: &crate::db::Db,
    recording_id: &str,
    output_format: Option<&str>,
) -> Result<String> {
    let entry = get_recording_index(db, recording_id)?
        .ok_or_else(|| PagerunnerError::Config(format!("Recording {} not found", recording_id)))?;

    let dir = PathBuf::from(&entry.dir_path);
    let video_path = dir.join(format!("video.{}", entry.format));
    if !video_path.exists() {
        return Err(PagerunnerError::Config(format!(
            "Video file not found: {}", video_path.display()
        )));
    }

    // Read metadata for markers
    let metadata_path = dir.join("metadata.json");
    let metadata: RecordingMetadata = if metadata_path.exists() {
        let json = std::fs::read_to_string(&metadata_path)
            .map_err(|e| PagerunnerError::Config(format!("Failed to read metadata: {}", e)))?;
        serde_json::from_str(&json)
            .map_err(|e| PagerunnerError::Config(format!("Failed to parse metadata: {}", e)))?
    } else {
        return Err(PagerunnerError::Config("No metadata.json found for recording".into()));
    };

    let total_ms = metadata.duration_ms.unwrap_or(0);
    let filter = build_subtitle_filter(&metadata.markers, total_ms);

    let out_format = output_format.unwrap_or(&entry.format);
    let output_path = dir.join(format!("annotated.{}", out_format));

    let mut args = vec![
        "-i".to_string(),
        video_path.to_str().unwrap().to_string(),
    ];

    if let Some(f) = &filter {
        args.push("-vf".to_string());
        args.push(f.clone());
    }

    // Encoding settings based on format
    match out_format {
        "webm" => {
            args.extend_from_slice(&[
                "-c:v".to_string(), "libvpx-vp9".to_string(),
                "-pix_fmt".to_string(), "yuva420p".to_string(),
            ]);
        }
        _ => {
            args.extend_from_slice(&[
                "-c:v".to_string(), "libx264".to_string(),
                "-pix_fmt".to_string(), "yuv420p".to_string(),
                "-movflags".to_string(), "+faststart".to_string(),
            ]);
        }
    }

    args.push("-y".to_string());
    args.push(output_path.to_str().unwrap().to_string());

    let status = Command::new("ffmpeg")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| PagerunnerError::Config(format!("Failed to spawn ffmpeg: {}", e)))?;

    if !status.success() {
        return Err(PagerunnerError::Config(
            format!("ffmpeg render failed with status: {}", status)
        ));
    }

    Ok(output_path.to_str().unwrap().to_string())
}
```

Add `mod recording_render;` to `src/main.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test recording_render::tests`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add src/recording_render.rs src/main.rs
git commit -m "feat(recording): add render module with ffmpeg subtitle overlay"
```

---

### Task 12: render_recording MCP tool + CLI

**Files:**
- Modify: `src/mcp_server.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add tool definition to all_tools()**

```rust
json!({
    "name": "render_recording",
    "description": "Render an annotated version of a recording with marker text overlays composited onto the video. Requires ffmpeg on PATH.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "recording_id": { "type": "string", "description": "Recording to render" },
            "format": {
                "type": "string",
                "enum": ["mp4", "webm"],
                "description": "Output format (defaults to recording's original format)"
            }
        },
        "required": ["recording_id"]
    }
}),
```

- [ ] **Step 2: Add dispatch arm**

```rust
"render_recording" => {
    let recording_id = args["recording_id"]
        .as_str()
        .ok_or_else(|| PagerunnerError::Config("Missing recording_id".into()))?;
    let format = args["format"].as_str();

    let output_path = crate::recording_render::render_annotated(&db, recording_id, format).await?;

    Ok(serde_json::json!({
        "ok": true,
        "recording_id": recording_id,
        "annotated_path": output_path,
    }).to_string())
}
```

- [ ] **Step 3: Add CLI subcommand**

Add to `Commands` enum:

```rust
/// Render an annotated version of a recording with marker overlays
#[command(name = "render-recording")]
RenderRecording {
    recording_id: String,
    #[arg(long)]
    format: Option<String>,
},
```

Add match arm:

```rust
Commands::RenderRecording { recording_id, format } => {
    let config = config::PagerunnerConfig::load()?;
    let mut args = serde_json::json!({"recording_id": recording_id});
    if let Some(v) = format { args["format"] = serde_json::json!(v); }
    crate::cli_tools::run_tool("render_recording", args, crate::cli_tools::ScreenshotMode::File, &config).await?;
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles without errors

- [ ] **Step 5: Commit**

```bash
git add src/mcp_server.rs src/main.rs
git commit -m "feat(recording): add render_recording MCP tool and CLI subcommand"
```

---

### Task 13: Final integration — verify full build and run all unit tests

- [ ] **Step 1: Run cargo build**

Run: `cargo build`
Expected: Compiles clean

- [ ] **Step 2: Run all recording-related tests**

Run: `cargo test recording`
Expected: All tests PASS

- [ ] **Step 3: Run config tests**

Run: `cargo test config::tests`
Expected: All tests PASS

- [ ] **Step 4: Verify CLI help shows new commands**

Run: `cargo run -- --help`
Expected: Shows `start-recording`, `stop-recording`, `add-marker`, `list-recordings`, `get-recording`, `delete-recording`, `render-recording`

- [ ] **Step 5: Final commit if any fixups needed**

```bash
git add -A
git commit -m "chore(recording): final integration fixups"
```
