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

/// Format a millisecond timestamp as SRT time format (HH:MM:SS,mmm).
fn ms_to_srt_time(ms: u64) -> String {
    let total_s = ms / 1000;
    let h = total_s / 3600;
    let m = (total_s % 3600) / 60;
    let s = total_s % 60;
    let remainder_ms = ms % 1000;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, remainder_ms)
}

/// Generate an SRT subtitle file from markers.
pub fn generate_srt(markers: &[Marker], total_duration_ms: u64) -> String {
    let mut srt = String::new();
    for (i, marker) in markers.iter().enumerate() {
        let start_ms = marker.ts_ms;
        let end_ms = if i + 1 < markers.len() {
            markers[i + 1].ts_ms
        } else {
            let end = start_ms + 5000;
            if end > total_duration_ms { total_duration_ms } else { end }
        };

        let text = if let Some(desc) = &marker.description {
            format!("{}\n{}", marker.label, desc)
        } else {
            marker.label.clone()
        };

        srt.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            ms_to_srt_time(start_ms),
            ms_to_srt_time(end_ms),
            text
        ));
    }
    srt
}

/// Render an annotated recording. Generates an SRT subtitle file alongside the video.
/// If ffmpeg has drawtext support, also renders an annotated video with burned-in overlays.
pub async fn render_annotated(
    db: &crate::db::Db,
    recording_id: &str,
    _output_format: Option<&str>,
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

    // Always generate SRT subtitle file
    let srt_path = dir.join("markers.srt");
    let srt_content = generate_srt(&metadata.markers, total_ms);
    std::fs::write(&srt_path, &srt_content)
        .map_err(|e| PagerunnerError::Config(format!("Failed to write SRT: {}", e)))?;

    // Mux subtitles into the video container so they show automatically in players
    let annotated_path = dir.join(format!("annotated.{}", entry.format));
    let mux_status = tokio::process::Command::new("ffmpeg")
        .args(&[
            "-i", video_path.to_str().unwrap(),
            "-i", srt_path.to_str().unwrap(),
            "-c", "copy",
            "-c:s", "mov_text",
            "-metadata:s:s:0", "language=eng",
            "-y", annotated_path.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .ok();

    let annotated = match mux_status {
        Some(s) if s.success() => Some(annotated_path.to_str().unwrap().to_string()),
        _ => None,
    };

    Ok(serde_json::json!({
        "srt_path": srt_path.to_str().unwrap(),
        "annotated_video": annotated,
        "video_path": video_path.to_str().unwrap(),
    }).to_string())
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

    #[test]
    fn test_escape_ffmpeg_text_all_special() {
        assert_eq!(escape_ffmpeg_text("a\\b"), "a\\\\b");
        assert_eq!(escape_ffmpeg_text("[x]"), "\\[x\\]");
        assert_eq!(escape_ffmpeg_text("a;b"), "a\\;b");
    }

    #[test]
    fn test_escape_ffmpeg_text_no_special() {
        assert_eq!(escape_ffmpeg_text("hello world 123"), "hello world 123");
    }

    #[test]
    fn test_filter_marker_timing_boundaries() {
        // Marker A at 1s, B at 5s. A should show from 1-5, B from 5-10 (or end)
        let markers = vec![
            Marker { ts_ms: 1000, label: "A".into(), description: None },
            Marker { ts_ms: 5000, label: "B".into(), description: None },
        ];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
        // A: between(t,1.00,5.00)
        assert!(filter.contains("between(t,1.00,5.00)"));
        // B: between(t,5.00,10.00) (last marker: +5s)
        assert!(filter.contains("between(t,5.00,10.00)"));
    }

    #[test]
    fn test_filter_last_marker_clamps_to_video_duration() {
        // Marker at 28s, video is 30s — last marker should show until 30s, not 33s
        let markers = vec![
            Marker { ts_ms: 28000, label: "End".into(), description: None },
        ];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
        // Should clamp to 30.00, not 33.00
        assert!(filter.contains("between(t,28.00,30.00)"));
    }

    #[test]
    fn test_filter_last_marker_within_5s() {
        // Marker at 10s, video is 30s — last marker shows for 5s
        let markers = vec![
            Marker { ts_ms: 10000, label: "Mid".into(), description: None },
        ];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
        assert!(filter.contains("between(t,10.00,15.00)"));
    }

    #[test]
    fn test_filter_with_description() {
        let markers = vec![
            Marker {
                ts_ms: 0,
                label: "Start".into(),
                description: Some("Beginning of recording".into()),
            },
        ];
        let filter = build_subtitle_filter(&markers, 10000).unwrap();
        assert!(filter.contains("Start\\: Beginning of recording"));
    }

    #[test]
    fn test_filter_without_description() {
        let markers = vec![
            Marker { ts_ms: 0, label: "Just a label".into(), description: None },
        ];
        let filter = build_subtitle_filter(&markers, 10000).unwrap();
        assert!(filter.contains("Just a label"));
        // Without description, text is just the label — no "label: desc" colon separator
        assert!(!filter.contains("Just a label\\:"));
    }

    #[test]
    fn test_filter_special_chars_in_label() {
        let markers = vec![
            Marker {
                ts_ms: 0,
                label: "Step [1]: it's a test".into(),
                description: None,
            },
        ];
        let filter = build_subtitle_filter(&markers, 10000).unwrap();
        // Special chars should be escaped
        assert!(filter.contains("\\[1\\]"));
        assert!(filter.contains("\\'"));
    }

    #[test]
    fn test_filter_zero_duration_video() {
        let markers = vec![
            Marker { ts_ms: 0, label: "X".into(), description: None },
        ];
        // Video duration 0ms — marker should still appear briefly
        let filter = build_subtitle_filter(&markers, 0).unwrap();
        assert!(filter.contains("drawtext"));
    }

    #[test]
    fn test_filter_many_markers() {
        let markers: Vec<Marker> = (0..20)
            .map(|i| Marker {
                ts_ms: i * 1000,
                label: format!("Step {}", i),
                description: None,
            })
            .collect();
        let filter = build_subtitle_filter(&markers, 25000).unwrap();
        // Should have 20 drawtext filters joined by commas
        let count = filter.matches("drawtext=").count();
        assert_eq!(count, 20);
    }

    #[test]
    fn test_filter_consecutive_markers_same_timestamp() {
        // Edge case: two markers at the same time
        let markers = vec![
            Marker { ts_ms: 5000, label: "A".into(), description: None },
            Marker { ts_ms: 5000, label: "B".into(), description: None },
        ];
        let filter = build_subtitle_filter(&markers, 10000).unwrap();
        // Both should appear; A shows from 5-5 (zero duration), B from 5-10
        assert!(filter.contains("A"));
        assert!(filter.contains("B"));
    }
}
