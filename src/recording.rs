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
    let sanitized: String = name_or_flow
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    base.join(profile).join(format!("{}_{}", date, sanitized))
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
}
