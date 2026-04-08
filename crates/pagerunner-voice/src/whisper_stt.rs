//! Whisper STT backend using whisper.cpp via `whisper-rs`.
//!
//! Gated behind the `whisper` feature flag.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

use crate::stt::{Result as SttResult, SttProvider};
use crate::VoiceError;

// ---------------------------------------------------------------------------
// Known models
// ---------------------------------------------------------------------------

struct ModelInfo {
    name: &'static str,
    filename: &'static str,
    url: &'static str,
    sha256: &'static str,
    size_mb: u32,
}

const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "whisper-tiny",
        filename: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        size_mb: 75,
    },
    ModelInfo {
        name: "whisper-base",
        filename: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b7b49d4d24e067e215",
        size_mb: 142,
    },
];

fn find_model(name: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.name == name)
}

// ---------------------------------------------------------------------------
// Model directory helpers
// ---------------------------------------------------------------------------

fn models_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".pagerunner")
        .join("models")
        .join("whisper")
}

/// Ensure model file exists locally, downloading if necessary.
fn ensure_model(info: &ModelInfo) -> std::result::Result<PathBuf, VoiceError> {
    let dir = models_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| VoiceError::Stt(format!("failed to create models dir: {e}")))?;

    let path = dir.join(info.filename);
    if path.exists() {
        // Verify SHA-256
        if verify_sha256(&path, info.sha256)? {
            tracing::info!(model = info.name, path = %path.display(), "model already downloaded");
            return Ok(path);
        }
        tracing::warn!(model = info.name, "SHA-256 mismatch — re-downloading");
        std::fs::remove_file(&path)
            .map_err(|e| VoiceError::Stt(format!("failed to remove corrupt model: {e}")))?;
    }

    download_model(info, &path)?;
    if !verify_sha256(&path, info.sha256)? {
        return Err(VoiceError::Stt(format!(
            "SHA-256 verification failed after download for {}",
            info.name
        )));
    }

    Ok(path)
}

fn download_model(info: &ModelInfo, dest: &Path) -> std::result::Result<(), VoiceError> {
    tracing::info!(
        model = info.name,
        url = info.url,
        size_mb = info.size_mb,
        "downloading whisper model"
    );

    let response = reqwest::blocking::get(info.url)
        .map_err(|e| VoiceError::Stt(format!("model download failed: {e}")))?;

    if !response.status().is_success() {
        return Err(VoiceError::Stt(format!(
            "model download returned HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .map_err(|e| VoiceError::Stt(format!("failed to read model bytes: {e}")))?;

    std::fs::write(dest, &bytes)
        .map_err(|e| VoiceError::Stt(format!("failed to write model file: {e}")))?;

    tracing::info!(model = info.name, path = %dest.display(), "model downloaded");
    Ok(())
}

fn verify_sha256(path: &Path, expected: &str) -> std::result::Result<bool, VoiceError> {
    use sha2::{Digest, Sha256};

    let data = std::fs::read(path)
        .map_err(|e| VoiceError::Stt(format!("failed to read model for verification: {e}")))?;
    let hash = Sha256::digest(&data);
    let hex = format!("{hash:x}");
    Ok(hex == expected)
}

// ---------------------------------------------------------------------------
// WhisperStt
// ---------------------------------------------------------------------------

/// Speech-to-text provider backed by whisper.cpp.
///
/// Audio chunks are accumulated in an internal buffer. When the buffer reaches
/// `buffer_threshold_samples` (default: 16 000 = 1 second at 16 kHz), whisper
/// inference runs and the buffer is drained.
pub struct WhisperStt {
    state: Mutex<WhisperState>,
    // WhisperContext must outlive WhisperState — store it to prevent drop.
    _ctx: Box<WhisperContext>,
    buffer: Mutex<Vec<f32>>,
    /// Number of samples to accumulate before running inference.
    buffer_threshold_samples: usize,
}

impl std::fmt::Debug for WhisperStt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhisperStt")
            .field("buffer_threshold_samples", &self.buffer_threshold_samples)
            .finish_non_exhaustive()
    }
}

impl WhisperStt {
    /// Create a new WhisperStt provider.
    ///
    /// `model_name` must be one of `"whisper-tiny"` or `"whisper-base"`.
    /// The model is downloaded automatically on first use.
    pub fn new(model_name: &str) -> std::result::Result<Self, VoiceError> {
        Self::with_buffer_threshold(model_name, 16_000) // 1 second at 16 kHz
    }

    /// Create a new WhisperStt with a custom buffer threshold (in samples).
    pub fn with_buffer_threshold(
        model_name: &str,
        buffer_threshold_samples: usize,
    ) -> std::result::Result<Self, VoiceError> {
        let info = find_model(model_name).ok_or_else(|| {
            VoiceError::Stt(format!(
                "unknown model '{}'; known models: {}",
                model_name,
                MODELS.iter().map(|m| m.name).collect::<Vec<_>>().join(", ")
            ))
        })?;

        let model_path = ensure_model(info)?;
        let model_path_str = model_path.to_str().ok_or_else(|| {
            VoiceError::Stt("model path contains invalid UTF-8".to_string())
        })?;

        let ctx = WhisperContext::new_with_params(model_path_str, WhisperContextParameters::default())
            .map_err(|e| VoiceError::Stt(format!("failed to load whisper model: {e}")))?;

        let ctx = Box::new(ctx);

        let state = ctx
            .create_state()
            .map_err(|e| VoiceError::Stt(format!("failed to create whisper state: {e}")))?;

        tracing::info!(
            model = model_name,
            buffer_threshold = buffer_threshold_samples,
            "whisper STT initialized"
        );

        Ok(Self {
            state: Mutex::new(state),
            _ctx: ctx,
            buffer: Mutex::new(Vec::new()),
            buffer_threshold_samples,
        })
    }

    /// Run whisper inference on the given audio samples.
    fn transcribe_audio(
        state: &mut WhisperState,
        audio: &[f32],
    ) -> std::result::Result<Option<String>, VoiceError> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(1);
        params.set_single_segment(true);
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Suppress non-speech tokens
        params.set_suppress_blank(true);
        params.set_language(Some("en"));

        state
            .full(params, audio)
            .map_err(|e| VoiceError::Stt(format!("whisper inference failed: {e}")))?;

        let n_segments = state.full_n_segments();

        if n_segments == 0 {
            return Ok(None);
        }

        let mut text = String::new();
        for i in 0..n_segments {
            if let Some(segment) = state.get_segment(i) {
                let seg_text = segment
                    .to_str_lossy()
                    .map_err(|e| VoiceError::Stt(format!("failed to get segment text: {e}")))?;
                text.push_str(&seg_text);
            }
        }

        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed))
        }
    }
}

#[async_trait]
impl SttProvider for WhisperStt {
    async fn transcribe_chunk(
        &self,
        audio: &[f32],
        _sample_rate: u32,
    ) -> SttResult<Option<String>> {
        // Accumulate samples
        {
            let mut buf = self.buffer.lock().unwrap();
            buf.extend_from_slice(audio);
            if buf.len() < self.buffer_threshold_samples {
                return Ok(None);
            }
        }

        // Drain the buffer and run inference
        let samples: Vec<f32> = {
            let mut buf = self.buffer.lock().unwrap();
            std::mem::take(&mut *buf)
        };

        let mut state = self.state.lock().unwrap();
        Self::transcribe_audio(&mut state, &samples)
    }

    async fn flush(&self) -> SttResult<Option<String>> {
        let samples: Vec<f32> = {
            let mut buf = self.buffer.lock().unwrap();
            std::mem::take(&mut *buf)
        };

        if samples.is_empty() {
            return Ok(None);
        }

        let mut state = self.state.lock().unwrap();
        Self::transcribe_audio(&mut state, &samples)
    }

    fn name(&self) -> &str {
        "whisper-stt"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_model_known() {
        assert!(find_model("whisper-tiny").is_some());
        assert!(find_model("whisper-base").is_some());
    }

    #[test]
    fn find_model_unknown() {
        assert!(find_model("whisper-large-v3").is_none());
    }

    #[test]
    fn models_dir_is_under_pagerunner() {
        let dir = models_dir();
        assert!(dir.ends_with("models/whisper"));
    }

    /// This test requires the whisper model to be downloaded.
    /// Run with: cargo test -p pagerunner-voice --features whisper -- --ignored whisper_loads_model
    #[test]
    #[ignore]
    fn whisper_loads_model() {
        let stt = WhisperStt::new("whisper-tiny");
        assert!(stt.is_ok(), "failed to load whisper-tiny: {:?}", stt.err());
        assert_eq!(stt.unwrap().name(), "whisper-stt");
    }

    /// Transcribe silence — should return None or empty.
    #[tokio::test]
    #[ignore]
    async fn whisper_transcribe_silence() {
        let stt = WhisperStt::with_buffer_threshold("whisper-tiny", 16_000).unwrap();
        let silence = vec![0.0_f32; 16_000]; // 1 second of silence
        let result = stt.transcribe_chunk(&silence, 16_000).await.unwrap();
        // Whisper may return None or some blank text for silence
        if let Some(text) = &result {
            assert!(
                text.trim().is_empty() || text.starts_with('['),
                "unexpected text from silence: {text}"
            );
        }
    }

    /// Transcribe a 440 Hz sine wave — should return something (likely gibberish).
    #[tokio::test]
    #[ignore]
    async fn whisper_transcribe_tone() {
        let stt = WhisperStt::with_buffer_threshold("whisper-tiny", 16_000).unwrap();
        let samples: Vec<f32> = (0..16_000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0).sin() * 0.5)
            .collect();
        // Should not panic; result is model-dependent
        let _result = stt.transcribe_chunk(&samples, 16_000).await.unwrap();
    }

    #[test]
    fn unknown_model_returns_error() {
        let result = WhisperStt::new("whisper-nonexistent");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown model"), "unexpected error: {err}");
    }
}
