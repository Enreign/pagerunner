use crate::error::{PagerunnerError, Result};
use crate::recording::{get_recording_index, Marker, RecordingMetadata};
use std::path::{Path, PathBuf};

/// Build an ffmpeg filter for smooth zoom from keyframes.
/// Uses simple `between(t,...)` expressions with linear interpolation.
/// Returns None if there are no zoom keyframes.
pub fn build_zoom_filter(
    keyframes: &[crate::recording::ZoomKeyframe],
    _total_duration_ms: u64,
    video_w: u32,
    video_h: u32,
) -> Option<String> {
    if keyframes.is_empty() {
        return None;
    }

    let transition = 0.8_f64;

    // Build a scale expression: chain of if(between(t,a,b), lerp, ...) ending in 1
    // Each zoom_in/zoom_out pair produces two segments
    let mut parts: Vec<String> = Vec::new();

    for (i, kf) in keyframes.iter().enumerate() {
        let t0 = kf.ts_ms as f64 / 1000.0;
        let t1 = t0 + transition;

        let from_scale = if i > 0 { keyframes[i - 1].scale } else { 1.0 };
        let to_scale = kf.scale;

        if (to_scale - from_scale).abs() < 0.01 {
            continue;
        }

        // Linear interp: from_scale + (to_scale - from_scale) * (t - t0) / dur
        // Held after transition completes until next keyframe
        let next_t = if i + 1 < keyframes.len() {
            keyframes[i + 1].ts_ms as f64 / 1000.0
        } else {
            t1 + 10.0 // hold for a while
        };

        // During transition: interpolate
        parts.push(format!(
            "if(between(t,{t0:.2},{t1:.2}),{from:.3}+{delta:.3}*(t-{t0:.2})/{dur:.2},0)",
            t0 = t0,
            t1 = t1,
            from = from_scale,
            delta = to_scale - from_scale,
            dur = transition,
        ));
        // After transition until next keyframe: hold
        parts.push(format!(
            "if(between(t,{t1:.2},{next:.2}),{val:.3},0)",
            t1 = t1,
            next = next_t,
            val = to_scale
        ));
    }

    if parts.is_empty() {
        return None;
    }

    // Sum all parts + base of 1 for times outside any zoom
    // (parts are non-overlapping, only one is non-zero at any time)
    // We add a base and subtract 1 for each non-zero part to avoid double-counting
    // Actually simpler: use max(1, sum_of_parts) since parts return 0 outside their range
    let scale_expr = format!("max(1,{})", parts.join("+"));

    // Find the zoom-in center (use the first zoom-in keyframe for simplicity)
    let zoom_kf = keyframes.iter().find(|k| k.scale > 1.01);
    let (cx, cy) = match zoom_kf {
        Some(k) => (k.x, k.y),
        None => (video_w as f64 / 2.0, video_h as f64 / 2.0),
    };

    // Crop centered on (cx, cy), clamped to video bounds
    let s = &scale_expr;
    let crop_w = format!("trunc(iw/({s})/2)*2"); // ensure even
    let crop_h = format!("trunc(ih/({s})/2)*2");
    let crop_x = format!("trunc(min(max({cx:.0}-iw/({s})/2,0),iw-iw/({s}))/2)*2");
    let crop_y = format!("trunc(min(max({cy:.0}-ih/({s})/2,0),ih-ih/({s}))/2)*2");

    Some(format!(
        "crop=w='{crop_w}':h='{crop_h}':x='{crop_x}':y='{crop_y}',scale={video_w}:{video_h}:flags=lanczos"
    ))
}

/// Apply zoom keyframes using ffmpeg split + crop + overlay.
/// Each zoom-in/zoom-out pair creates a cropped+scaled overlay segment
/// that fades in/out over the base video using opacity transitions.
async fn apply_zoom_keyframes(
    input_path: &Path,
    keyframes: &[crate::recording::ZoomKeyframe],
    output_path: &Path,
) -> Option<String> {
    // Group keyframes into zoom-in/zoom-out pairs
    let mut zoom_segments: Vec<(f64, f64, f64, f64, f64)> = Vec::new(); // (start, end, x, y, scale)
    let mut i = 0;
    while i < keyframes.len() {
        let kf = &keyframes[i];
        if kf.scale > 1.01 {
            // Find matching zoom-out
            let zoom_out_t = if i + 1 < keyframes.len() {
                keyframes[i + 1].ts_ms as f64 / 1000.0
            } else {
                kf.ts_ms as f64 / 1000.0 + 3.0
            };
            zoom_segments.push((kf.ts_ms as f64 / 1000.0, zoom_out_t, kf.x, kf.y, kf.scale));
            i += 2; // skip the zoom-out keyframe
        } else {
            i += 1;
        }
    }

    if zoom_segments.is_empty() {
        return None;
    }

    // Build filter_complex: for each zoom segment, create a cropped+scaled overlay
    let transition = 0.5; // transition time for fade
    let n = zoom_segments.len();
    let mut inputs_count = n + 1; // base + one crop per segment
    let _ = inputs_count;

    // Build splits and crop chains
    let mut filter_parts = Vec::new();
    // Split the input into n+1 streams
    let split_labels: Vec<String> = (0..=n).map(|j| format!("[s{}]", j)).collect();
    filter_parts.push(format!("[0:v]split={}{}", n + 1, split_labels.join("")));

    // For each zoom segment, create a cropped+scaled version
    for (j, (start, end, x, y, scale)) in zoom_segments.iter().enumerate() {
        // Use static pixel values to avoid expression parsing issues
        let probe2 = tokio::process::Command::new("ffprobe")
            .args(&[
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                input_path.to_str()?,
            ])
            .output()
            .await
            .ok()?;
        let dims2: Vec<f64> = String::from_utf8_lossy(&probe2.stdout)
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect();
        if dims2.len() < 2 {
            return None;
        }
        let (vw_f, vh_f) = (dims2[0], dims2[1]);

        let crop_w = ((vw_f / scale) as u32 / 2 * 2) as u32;
        let crop_h = ((vh_f / scale) as u32 / 2 * 2) as u32;
        let crop_x = ((*x - crop_w as f64 / 2.0)
            .max(0.0)
            .min(vw_f - crop_w as f64)) as u32;
        let crop_y = ((*y - crop_h as f64 / 2.0)
            .max(0.0)
            .min(vh_f - crop_h as f64)) as u32;
        let out_w = vw_f as u32;
        let out_h = vh_f as u32;

        // All values are static pixels — no ffmpeg expressions needed
        filter_parts.push(format!(
            "[s{}]crop={}:{}:{}:{},scale={}:{}:flags=lanczos[z{}]",
            j + 1,
            crop_w,
            crop_h,
            crop_x,
            crop_y,
            out_w,
            out_h,
            j
        ));
    }

    // Chain overlays: base → overlay zoomed segments with enable timing
    let mut prev = format!("[s0]");
    for (j, (start, end, _, _, _)) in zoom_segments.iter().enumerate() {
        let out = format!("[v{}]", j);
        let enable_start = start + transition; // fade in complete
        let enable_end = end; // fade out starts at zoom-out keyframe
                              // Show the zoomed overlay during the zoom period
        filter_parts.push(format!(
            "{}[z{}]overlay=0:0:enable='between(t,{:.2},{:.2})'{}",
            prev,
            j,
            start + 0.1, // slight delay for transition feel
            end - 0.1,
            out
        ));
        prev = out;
    }

    let filter_complex = filter_parts.join(";");
    let last_label = format!("[v{}]", n - 1);

    tracing::info!(filter = %filter_complex, "Zoom filter_complex");
    let output = tokio::process::Command::new("ffmpeg")
        .args(&[
            "-i",
            input_path.to_str()?,
            "-filter_complex",
            &filter_complex,
            "-map",
            &last_label,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "fast",
            "-y",
            output_path.to_str()?,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr
            .chars()
            .rev()
            .take(300)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        tracing::warn!(stderr = %tail, "Zoom filter_complex failed");
    }

    if output.status.success() {
        Some(output_path.to_str()?.to_string())
    } else {
        None
    }
}

/// Apply motion interpolation to upscale frame rate for smooth playback.
/// Uses ffmpeg minterpolate with blend mode (fast, good quality for UI content).
async fn apply_minterpolate(input_path: &Path, output_path: &Path, target_fps: u8) -> bool {
    let filter = format!("minterpolate=fps={}:mi_mode=blend", target_fps);
    let status = tokio::process::Command::new("ffmpeg")
        .args(&[
            "-i",
            input_path.to_str().unwrap_or(""),
            "-vf",
            &filter,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "fast",
            "-y",
            output_path.to_str().unwrap_or(""),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .ok();

    matches!(status, Some(s) if s.success())
}

/// Apply window chrome post-processing: gradient background, rounded corners, shadow.
/// Takes the raw video and produces a polished version with padding and effects.
pub async fn apply_window_chrome(
    input_path: &Path,
    output_path: &Path,
    padding: u32,
    corner_radius: u32,
    bg_color_start: &str,
    bg_color_end: &str,
) -> Option<String> {
    // Step 1: Generate gradient background image with ImageMagick
    let tmp = std::env::temp_dir();

    // Get video dimensions
    let probe = tokio::process::Command::new("ffprobe")
        .args(&[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            input_path.to_str()?,
        ])
        .output()
        .await
        .ok()?;

    let dims: Vec<u32> = String::from_utf8_lossy(&probe.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    if dims.len() < 2 {
        return None;
    }
    let (vid_w, vid_h) = (dims[0], dims[1]);
    let canvas_w = vid_w + padding * 2;
    let canvas_h = vid_h + padding * 2;

    // Generate a rounded-corner mask for the video
    let mask_path = tmp.join("__pr_mask.png");
    let mask_status = tokio::process::Command::new("magick")
        .args(&[
            "-size",
            &format!("{}x{}", vid_w, vid_h),
            "xc:none",
            "-fill",
            "white",
            "-draw",
            &format!(
                "roundrectangle 0,0 {},{} {},{}",
                vid_w - 1,
                vid_h - 1,
                corner_radius,
                corner_radius
            ),
            mask_path.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .ok()?;

    if !mask_status.success() {
        return None;
    }

    // Generate gradient background
    let bg_path = tmp.join("__pr_bg.png");
    let bg_status = tokio::process::Command::new("magick")
        .args(&[
            "-size",
            &format!("{}x{}", canvas_w, canvas_h),
            &format!("gradient:{}-{}", bg_color_start, bg_color_end),
            bg_path.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .ok()?;

    if !bg_status.success() {
        return None;
    }

    // Build ffmpeg filter: apply rounded mask, then overlay on gradient background
    // [0] = input video, [1] = mask, [2] = background
    let filter = format!(
        "[0:v][1:v]alphamerge[masked];[2:v][masked]overlay={}:{}",
        padding, padding
    );

    let status = tokio::process::Command::new("ffmpeg")
        .args(&[
            "-i",
            input_path.to_str()?,
            "-i",
            mask_path.to_str().unwrap(),
            "-i",
            bg_path.to_str().unwrap(),
            "-filter_complex",
            &filter,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "fast",
            "-y",
            output_path.to_str()?,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .ok()?;

    // Cleanup temp files
    let _ = std::fs::remove_file(&mask_path);
    let _ = std::fs::remove_file(&bg_path);

    if status.success() {
        Some(output_path.to_str()?.to_string())
    } else {
        None
    }
}

/// Overlay appearance settings, resolved from per-call args > config.toml > defaults.
pub struct OverlayStyle {
    pub position: String,   // "top" or "bottom"
    pub font: String,       // ImageMagick font name
    pub font_size: u32,     // points
    pub text_color: String, // color name or hex
    pub bg_color: String,   // color with alpha, e.g. "#000000AA"
    pub bar_height: u32,    // pixels
}

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
            if end > total_duration_ms {
                total_duration_ms
            } else {
                end
            }
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

/// Burn subtitle overlays into a video using ImageMagick (for text→PNG) + ffmpeg overlay filter.
/// Returns the annotated video path on success, None if magick/ffmpeg aren't available.
async fn burn_subtitles_overlay(
    video_path: &Path,
    markers: &[Marker],
    total_duration_ms: u64,
    output_path: &Path,
    style: &OverlayStyle,
) -> Option<String> {
    if markers.is_empty() {
        return None;
    }

    // Get video dimensions
    let probe = tokio::process::Command::new("ffprobe")
        .args(&[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            video_path.to_str()?,
        ])
        .output()
        .await
        .ok()?;

    let dims: Vec<u32> = String::from_utf8_lossy(&probe.stdout)
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    if dims.len() < 2 {
        return None;
    }
    let (w, h) = (dims[0], dims[1]);
    let bar_height = style.bar_height.min(h / 10); // cap at 10% of video height
    let pointsize = if style.font_size > 0 {
        style.font_size.min(bar_height)
    } else {
        (bar_height / 3).max(16)
    };

    // Generate overlay PNGs with ImageMagick
    let tmp = std::env::temp_dir();
    for (i, mk) in markers.iter().enumerate() {
        let text = if let Some(desc) = &mk.description {
            format!("{}  —  {}", mk.label, desc)
        } else {
            mk.label.clone()
        };

        let out = tmp.join(format!("pagerunner_sub_{}.png", i));
        let status = tokio::process::Command::new("magick")
            .args(&[
                "-size",
                &format!("{}x{}", w, bar_height),
                "xc:none",
                "-fill",
                &style.bg_color,
                "-draw",
                &format!("rectangle 0,0 {},{}", w, bar_height),
                "-fill",
                &style.text_color,
                "-font",
                &style.font,
                "-pointsize",
                &pointsize.to_string(),
                "-gravity",
                "West",
                "-annotate",
                "+20+0",
                &text,
                out.to_str().unwrap(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .ok()?;

        if !status.success() {
            tracing::warn!(
                "magick failed to generate subtitle overlay — is ImageMagick installed?"
            );
            return None;
        }
    }

    // Build ffmpeg overlay filter chain
    let mut inputs = vec!["-i".to_string(), video_path.to_str()?.to_string()];
    for i in 0..markers.len() {
        inputs.push("-i".to_string());
        inputs.push(
            tmp.join(format!("pagerunner_sub_{}.png", i))
                .to_str()?
                .to_string(),
        );
    }

    let mut filter_parts = Vec::new();
    let mut prev = "[0:v]".to_string();
    for (i, mk) in markers.iter().enumerate() {
        let start_s = mk.ts_ms as f64 / 1000.0;
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
        let y_pos = if style.position == "top" {
            0
        } else {
            h - bar_height
        };
        let out_label = format!("[v{}]", i);
        filter_parts.push(format!(
            "{}[{}:v]overlay=0:{}:enable='between(t,{:.2},{:.2})'{}",
            prev,
            i + 1,
            y_pos,
            start_s,
            end_s,
            out_label
        ));
        prev = out_label;
    }
    let last_label = format!("[v{}]", markers.len() - 1);
    let filter_complex = filter_parts.join(";");

    let mut args = inputs;
    args.extend_from_slice(&[
        "-filter_complex".to_string(),
        filter_complex,
        "-map".to_string(),
        last_label,
        "-c:v".to_string(),
        "libx264".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-y".to_string(),
        output_path.to_str()?.to_string(),
    ]);

    let status = tokio::process::Command::new("ffmpeg")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .ok()?;

    // Cleanup temp PNGs
    for i in 0..markers.len() {
        let _ = std::fs::remove_file(tmp.join(format!("pagerunner_sub_{}.png", i)));
    }

    if status.success() {
        Some(output_path.to_str()?.to_string())
    } else {
        None
    }
}

/// Render an annotated recording. Generates an SRT subtitle file alongside the video.
/// If ffmpeg has drawtext support, also renders an annotated video with burned-in overlays.
pub async fn render_annotated(
    db: &crate::db::Db,
    recording_id: &str,
    _output_format: Option<&str>,
    with_overlays: bool,
    style: &OverlayStyle,
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

    // Step 0: Motion interpolation — upscale frame rate for smooth motion
    let interpolated_path = dir.join(format!("interpolated.{}", entry.format));
    let smooth_source = if apply_minterpolate(&video_path, &interpolated_path, 30).await {
        tracing::info!("Motion interpolation complete");
        interpolated_path.clone()
    } else {
        tracing::info!("Skipping motion interpolation — using raw video");
        video_path.clone()
    };

    // Step 0.5: Apply zoom keyframes via split+crop+overlay
    let zoomed_path = dir.join(format!("zoomed.{}", entry.format));
    let zoom_source = if !metadata.zoom_keyframes.is_empty() {
        match apply_zoom_keyframes(&smooth_source, &metadata.zoom_keyframes, &zoomed_path).await {
            Some(path) => {
                tracing::info!("Zoom post-processing complete");
                let _ = std::fs::remove_file(&interpolated_path);
                PathBuf::from(path)
            }
            None => {
                tracing::info!("Zoom not applied — using unzoomed video");
                smooth_source.clone()
            }
        }
    } else {
        smooth_source.clone()
    };

    // Step 1: Burn subtitles into the video if requested
    let subtitled_path = if with_overlays && !metadata.markers.is_empty() {
        let path = dir.join(format!("subtitled.{}", entry.format));
        burn_subtitles_overlay(&zoom_source, &metadata.markers, total_ms, &path, style).await
    } else {
        None
    };

    // Step 2: Apply window chrome (gradient background + rounded corners)
    let source = subtitled_path
        .as_ref()
        .map(|p| PathBuf::from(p))
        .unwrap_or_else(|| zoom_source.clone());
    let polished_path = dir.join(format!("annotated.{}", entry.format));
    let polished = apply_window_chrome(
        &source,
        &polished_path,
        40,        // padding
        16,        // corner radius
        "#1e1e2e", // dark gradient start (catppuccin mocha)
        "#313244", // dark gradient end
    )
    .await;

    // Cleanup intermediate files
    let _ = std::fs::remove_file(&interpolated_path);
    let _ = std::fs::remove_file(&zoomed_path);
    if polished.is_some() {
        if let Some(ref sub_path) = subtitled_path {
            let _ = std::fs::remove_file(sub_path);
        }
    }

    let final_video = polished.as_deref().or(subtitled_path.as_deref());

    Ok(serde_json::json!({
        "srt_path": srt_path.to_str().unwrap(),
        "annotated_video": final_video,
        "video_path": video_path.to_str().unwrap(),
    })
    .to_string())
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
            Marker {
                ts_ms: 1000,
                label: "A".into(),
                description: None,
            },
            Marker {
                ts_ms: 5000,
                label: "B".into(),
                description: None,
            },
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
        let markers = vec![Marker {
            ts_ms: 28000,
            label: "End".into(),
            description: None,
        }];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
        // Should clamp to 30.00, not 33.00
        assert!(filter.contains("between(t,28.00,30.00)"));
    }

    #[test]
    fn test_filter_last_marker_within_5s() {
        // Marker at 10s, video is 30s — last marker shows for 5s
        let markers = vec![Marker {
            ts_ms: 10000,
            label: "Mid".into(),
            description: None,
        }];
        let filter = build_subtitle_filter(&markers, 30000).unwrap();
        assert!(filter.contains("between(t,10.00,15.00)"));
    }

    #[test]
    fn test_filter_with_description() {
        let markers = vec![Marker {
            ts_ms: 0,
            label: "Start".into(),
            description: Some("Beginning of recording".into()),
        }];
        let filter = build_subtitle_filter(&markers, 10000).unwrap();
        assert!(filter.contains("Start\\: Beginning of recording"));
    }

    #[test]
    fn test_filter_without_description() {
        let markers = vec![Marker {
            ts_ms: 0,
            label: "Just a label".into(),
            description: None,
        }];
        let filter = build_subtitle_filter(&markers, 10000).unwrap();
        assert!(filter.contains("Just a label"));
        // Without description, text is just the label — no "label: desc" colon separator
        assert!(!filter.contains("Just a label\\:"));
    }

    #[test]
    fn test_filter_special_chars_in_label() {
        let markers = vec![Marker {
            ts_ms: 0,
            label: "Step [1]: it's a test".into(),
            description: None,
        }];
        let filter = build_subtitle_filter(&markers, 10000).unwrap();
        // Special chars should be escaped
        assert!(filter.contains("\\[1\\]"));
        assert!(filter.contains("\\'"));
    }

    #[test]
    fn test_filter_zero_duration_video() {
        let markers = vec![Marker {
            ts_ms: 0,
            label: "X".into(),
            description: None,
        }];
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
            Marker {
                ts_ms: 5000,
                label: "A".into(),
                description: None,
            },
            Marker {
                ts_ms: 5000,
                label: "B".into(),
                description: None,
            },
        ];
        let filter = build_subtitle_filter(&markers, 10000).unwrap();
        // Both should appear; A shows from 5-5 (zero duration), B from 5-10
        assert!(filter.contains("A"));
        assert!(filter.contains("B"));
    }
}
