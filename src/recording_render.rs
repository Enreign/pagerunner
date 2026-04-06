use crate::error::{PagerunnerError, Result};
use crate::recording::{get_recording_index, Marker, RecordingMetadata};
use std::path::PathBuf;

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
            let end = start_s + 5.0;
            let total = total_duration_ms as f64 / 1000.0;
            if end > total {
                total
            } else {
                end
            }
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
            "Video file not found: {}",
            video_path.display()
        )));
    }

    let metadata_path = dir.join("metadata.json");
    let metadata: RecordingMetadata = if metadata_path.exists() {
        let json = std::fs::read_to_string(&metadata_path)
            .map_err(|e| PagerunnerError::Config(format!("Failed to read metadata: {}", e)))?;
        serde_json::from_str(&json)
            .map_err(|e| PagerunnerError::Config(format!("Failed to parse metadata: {}", e)))?
    } else {
        return Err(PagerunnerError::Config(
            "No metadata.json found for recording".into(),
        ));
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

    match out_format {
        "webm" => {
            args.extend_from_slice(&[
                "-c:v".to_string(),
                "libvpx-vp9".to_string(),
                "-pix_fmt".to_string(),
                "yuva420p".to_string(),
            ]);
        }
        _ => {
            args.extend_from_slice(&[
                "-c:v".to_string(),
                "libx264".to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
                "-movflags".to_string(),
                "+faststart".to_string(),
            ]);
        }
    }

    args.push("-y".to_string());
    args.push(output_path.to_str().unwrap().to_string());

    let status = tokio::process::Command::new("ffmpeg")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| PagerunnerError::Config(format!("Failed to spawn ffmpeg: {}", e)))?;

    if !status.success() {
        return Err(PagerunnerError::Config(format!(
            "ffmpeg render failed with status: {}",
            status
        )));
    }

    Ok(output_path.to_str().unwrap().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_ffmpeg_filter_no_markers() {
        let markers: Vec<Marker> = vec![];
        let filter = build_subtitle_filter(&markers, 30000);
        assert!(filter.is_none());
    }

    #[test]
    fn test_build_ffmpeg_filter_single_marker() {
        let markers = vec![Marker {
            ts_ms: 3000,
            label: "Step 1".to_string(),
            description: Some("Click the button".to_string()),
        }];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
        assert!(filter.contains("drawtext"));
        assert!(filter.contains("Step 1"));
    }

    #[test]
    fn test_build_ffmpeg_filter_multiple_markers() {
        let markers = vec![
            Marker {
                ts_ms: 1000,
                label: "A".to_string(),
                description: None,
            },
            Marker {
                ts_ms: 5000,
                label: "B".to_string(),
                description: None,
            },
            Marker {
                ts_ms: 10000,
                label: "C".to_string(),
                description: None,
            },
        ];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
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
