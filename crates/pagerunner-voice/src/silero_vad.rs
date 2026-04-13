//! Silero VAD backend using the Silero VAD v5 ONNX model via `ort`.
//!
//! Gated behind the `silero-vad` feature flag.

use std::path::{Path, PathBuf};

use ndarray::{Array1, Array2, Array3};
use ort::session::Session;
use ort::value::{Tensor, TensorRef};

use crate::vad::VadDetector;
use crate::VoiceError;

// ---------------------------------------------------------------------------
// Model constants
// ---------------------------------------------------------------------------

const MODEL_URL: &str =
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx";
const MODEL_FILENAME: &str = "silero_vad.onnx";

/// Chunk size expected by Silero VAD at 16 kHz.
const CHUNK_SIZE_16K: usize = 512;
/// Chunk size expected by Silero VAD at 8 kHz.
const CHUNK_SIZE_8K: usize = 256;

// ---------------------------------------------------------------------------
// Model directory helpers
// ---------------------------------------------------------------------------

fn models_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".pagerunner")
        .join("models")
        .join("vad")
}

/// Ensure model file exists locally, downloading if necessary.
fn ensure_model() -> Result<PathBuf, VoiceError> {
    let dir = models_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| VoiceError::Vad(format!("failed to create models dir: {e}")))?;

    let path = dir.join(MODEL_FILENAME);
    if path.exists() {
        tracing::info!(path = %path.display(), "silero VAD model already downloaded");
        return Ok(path);
    }

    download_model(&path)?;
    Ok(path)
}

fn download_model(dest: &Path) -> Result<(), VoiceError> {
    tracing::info!(url = MODEL_URL, "downloading silero VAD model (~2MB)");

    let response = reqwest::blocking::get(MODEL_URL)
        .map_err(|e| VoiceError::Vad(format!("model download failed: {e}")))?;

    if !response.status().is_success() {
        return Err(VoiceError::Vad(format!(
            "model download returned HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .map_err(|e| VoiceError::Vad(format!("failed to read model bytes: {e}")))?;

    std::fs::write(dest, &bytes)
        .map_err(|e| VoiceError::Vad(format!("failed to write model file: {e}")))?;

    tracing::info!(path = %dest.display(), "silero VAD model downloaded");
    Ok(())
}

// ---------------------------------------------------------------------------
// SileroVad
// ---------------------------------------------------------------------------

/// Voice activity detector backed by the Silero VAD v5 ONNX model.
///
/// Processes audio in fixed-size chunks (512 samples at 16 kHz, 256 at 8 kHz)
/// and returns `true` when speech probability exceeds the configured threshold.
///
/// The model maintains an internal hidden state that is carried between calls,
/// enabling it to track speech context across chunks.
pub struct SileroVad {
    session: Session,
    /// Hidden state tensor [2, 1, 128] — carried between inference calls.
    state: Array3<f32>,
    /// Context from previous chunk (last 64 samples at 16kHz, 32 at 8kHz).
    /// Prepended to the next chunk before inference (as the Python API does).
    context: Vec<f32>,
    /// Speech probability threshold (0.0–1.0). Default: 0.5.
    threshold: f32,
    /// Whether the last processed chunk was classified as speech.
    speaking: bool,
}

impl std::fmt::Debug for SileroVad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SileroVad")
            .field("threshold", &self.threshold)
            .field("speaking", &self.speaking)
            .finish_non_exhaustive()
    }
}

impl SileroVad {
    /// Create a new Silero VAD detector, downloading the model if needed.
    ///
    /// `threshold` controls the speech probability cutoff (default 0.5).
    /// Lower values are more sensitive; higher values reduce false positives.
    pub fn new(threshold: Option<f32>) -> Result<Self, VoiceError> {
        let model_path = ensure_model()?;
        Self::from_file(&model_path, threshold)
    }

    /// Create a new Silero VAD from a model file path (skips download).
    ///
    /// Useful for testing or when the model is bundled with the application.
    pub fn from_file(model_path: &Path, threshold: Option<f32>) -> Result<Self, VoiceError> {
        crate::runtime::ensure_ort_initialized();

        let session = Session::builder()
            .map_err(|e| VoiceError::Vad(format!("failed to create ONNX session builder: {e}")))?
            .with_intra_threads(1)
            .map_err(|e| VoiceError::Vad(format!("failed to set thread count: {e}")))?
            .commit_from_file(model_path)
            .map_err(|e| VoiceError::Vad(format!("failed to load silero VAD model: {e}")))?;

        tracing::info!(
            threshold = threshold.unwrap_or(0.5),
            "silero VAD initialized"
        );

        // Log model input/output names for debugging
        Ok(Self {
            session,
            state: Array3::zeros((2, 1, 128)),
            context: Vec::new(),
            threshold: threshold.unwrap_or(0.5),
            speaking: false,
        })
    }

    /// Run a single inference step on a chunk of audio.
    ///
    /// Returns the speech probability (0.0–1.0).
    fn infer(&mut self, chunk: &[f32], sample_rate: u32) -> Result<f32, VoiceError> {
        let chunk_size = chunk.len();

        // Build owned tensors via Tensor::from_array (takes ndarray types)
        let input_arr = Array2::from_shape_vec((1, chunk_size), chunk.to_vec())
            .map_err(|e| VoiceError::Vad(format!("failed to create input array: {e}")))?;
        let sr_arr = ndarray::arr0(sample_rate as i64);

        let input_tensor = Tensor::from_array(input_arr)
            .map_err(|e| VoiceError::Vad(format!("failed to create input tensor: {e}")))?;
        let sr_tensor = Tensor::from_array(sr_arr)
            .map_err(|e| VoiceError::Vad(format!("failed to create sr tensor: {e}")))?;
        let state_tensor = TensorRef::from_array_view(self.state.view())
            .map_err(|e| VoiceError::Vad(format!("failed to create state tensor: {e}")))?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input" => input_tensor,
                "state" => state_tensor,
                "sr" => sr_tensor,
            ])
            .map_err(|e| VoiceError::Vad(format!("silero VAD inference failed: {e}")))?;

        // Extract speech probability — try_extract_tensor returns (&Shape, &[T])
        let (_shape, prob_data) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(|e| VoiceError::Vad(format!("failed to extract output tensor: {e}")))?;
        let prob = prob_data[0];

        // Update hidden state — the output name may be "stateN" or "hn"/"cn"
        // depending on model version. Try "stateN" first (v5).
        if let Ok((_shape, state_data)) = outputs["stateN"].try_extract_tensor::<f32>() {
            if let Ok(arr) = Array3::from_shape_vec((2, 1, 128), state_data.to_vec()) {
                self.state = arr;
            }
        }

        Ok(prob)
    }

    /// Returns the current speech probability threshold.
    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// Set a new speech probability threshold.
    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold;
    }

    /// Returns `true` if the last processed chunk was classified as speech.
    pub fn is_speaking(&self) -> bool {
        self.speaking
    }
}

impl VadDetector for SileroVad {
    fn process(&mut self, audio: &[f32], sample_rate: u32) -> bool {
        let chunk_size = if sample_rate == 16000 {
            CHUNK_SIZE_16K
        } else {
            CHUNK_SIZE_8K
        };
        let context_size: usize = if sample_rate == 16000 { 64 } else { 32 };

        // Take the last chunk_size samples, or zero-pad if shorter
        let chunk: Vec<f32> = if audio.len() >= chunk_size {
            audio[audio.len() - chunk_size..].to_vec()
        } else {
            let mut padded = vec![0.0f32; chunk_size];
            let offset = chunk_size - audio.len();
            padded[offset..].copy_from_slice(audio);
            padded
        };

        // Prepend context (like Python: x = torch.cat([self._context, x], dim=1))
        if self.context.is_empty() {
            self.context = vec![0.0f32; context_size];
        }
        let mut input_with_context = Vec::with_capacity(context_size + chunk_size);
        input_with_context.extend_from_slice(&self.context);
        input_with_context.extend_from_slice(&chunk);

        // Save context for next call (last context_size samples of the chunk)
        self.context = chunk[chunk.len() - context_size..].to_vec();

        match self.infer(&input_with_context, sample_rate) {
            Ok(prob) => {
                self.speaking = prob > self.threshold;
                tracing::trace!(
                    prob = prob,
                    threshold = self.threshold,
                    speaking = self.speaking,
                    "silero VAD"
                );
                self.speaking
            }
            Err(e) => {
                tracing::error!(error = %e, "silero VAD inference failed, returning previous state");
                self.speaking
            }
        }
    }

    fn reset(&mut self) {
        self.state = Array3::zeros((2, 1, 128));
        self.context.clear();
        self.speaking = false;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_dir_is_under_pagerunner() {
        let dir = models_dir();
        assert!(dir.ends_with("models/vad"));
    }

    /// Requires the model to be downloaded.
    /// Run: cargo test -p pagerunner-voice --features silero-vad -- --ignored silero_loads
    #[test]
    #[ignore]
    fn silero_loads_model() {
        let vad = SileroVad::new(None);
        assert!(vad.is_ok(), "failed to load silero VAD: {:?}", vad.err());
    }

    /// Silence should not trigger speech detection.
    /// Run: cargo test -p pagerunner-voice --features silero-vad -- --ignored silero_silence
    #[test]
    #[ignore]
    fn silero_silence_is_not_speech() {
        let mut vad = SileroVad::new(Some(0.5)).expect("failed to load model");
        let silence = vec![0.0f32; 512];
        // Process a few chunks of silence
        for _ in 0..10 {
            let is_speech = vad.process(&silence, 16000);
            assert!(!is_speech, "silence should not be detected as speech");
        }
    }

    /// A loud sine wave should eventually trigger (model-dependent).
    /// Run: cargo test -p pagerunner-voice --features silero-vad -- --ignored silero_tone
    #[test]
    #[ignore]
    fn silero_tone_detection() {
        let mut vad = SileroVad::new(Some(0.3)).expect("failed to load model");
        // Generate 440 Hz tone
        let samples: Vec<f32> = (0..512)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16000.0).sin() * 0.8)
            .collect();
        // Just verify it doesn't panic
        let _result = vad.process(&samples, 16000);
    }

    #[test]
    #[ignore]
    fn silero_reset_clears_state() {
        let mut vad = SileroVad::new(None).expect("failed to load model");
        let audio = vec![0.5f32; 512];
        vad.process(&audio, 16000);
        vad.reset();
        assert!(!vad.is_speaking());
        // State should be zeros
        assert!(vad.state.iter().all(|&v| v == 0.0));
    }
}
