//! Piper TTS backend using ONNX Runtime for local neural speech synthesis.
//!
//! Gated behind the `piper-tts` feature flag.
//!
//! Pipeline: text -> espeak-ng (phonemization) -> phoneme ID mapping -> ONNX inference -> audio

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use ndarray::{Array1, Array2};
use ort::session::Session;
use ort::value::Tensor;

use crate::tts::{Result as TtsResult, TtsProvider};
use crate::VoiceError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEFAULT_VOICE: &str = "en_US-amy-low";

/// Base URL for Piper voice models on Hugging Face.
fn model_url(voice: &str) -> (String, String) {
    // Voice name format: {lang}_{REGION}-{name}-{quality}
    // e.g. en_US-amy-low -> en/en_US/amy/low/en_US-amy-low.onnx
    let parts: Vec<&str> = voice.split('-').collect();
    if parts.len() < 3 {
        // Fallback: just use voice name directly
        let base = format!(
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/amy/low/{voice}"
        );
        return (format!("{base}.onnx"), format!("{base}.onnx.json"));
    }

    let lang_region = parts[0]; // e.g. en_US
    let name = parts[1]; // e.g. amy
    let quality = parts[2]; // e.g. low

    // lang_region -> lang code (before underscore)
    let lang = lang_region
        .split('_')
        .next()
        .unwrap_or("en");

    let base = format!(
        "https://huggingface.co/rhasspy/piper-voices/resolve/main/{lang}/{lang_region}/{name}/{quality}/{voice}"
    );
    (format!("{base}.onnx"), format!("{base}.onnx.json"))
}

// ---------------------------------------------------------------------------
// Model directory helpers
// ---------------------------------------------------------------------------

fn models_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".pagerunner")
        .join("models")
        .join("piper")
}

/// Ensure both model files (.onnx and .onnx.json) exist locally, downloading if needed.
fn ensure_model(voice: &str) -> Result<(PathBuf, PathBuf), VoiceError> {
    let dir = models_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| VoiceError::Tts(format!("failed to create models dir: {e}")))?;

    let onnx_path = dir.join(format!("{voice}.onnx"));
    let json_path = dir.join(format!("{voice}.onnx.json"));

    let (onnx_url, json_url) = model_url(voice);

    if !onnx_path.exists() {
        download_file(&onnx_url, &onnx_path, "ONNX model")?;
    } else {
        tracing::info!(path = %onnx_path.display(), "piper model already downloaded");
    }

    if !json_path.exists() {
        download_file(&json_url, &json_path, "config JSON")?;
    } else {
        tracing::info!(path = %json_path.display(), "piper config already downloaded");
    }

    Ok((onnx_path, json_path))
}

fn download_file(url: &str, dest: &Path, label: &str) -> Result<(), VoiceError> {
    tracing::info!(url = url, "downloading piper {label}");

    let response = reqwest::blocking::get(url)
        .map_err(|e| VoiceError::Tts(format!("{label} download failed: {e}")))?;

    if !response.status().is_success() {
        return Err(VoiceError::Tts(format!(
            "{label} download returned HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .map_err(|e| VoiceError::Tts(format!("failed to read {label} bytes: {e}")))?;

    std::fs::write(dest, &bytes)
        .map_err(|e| VoiceError::Tts(format!("failed to write {label}: {e}")))?;

    tracing::info!(path = %dest.display(), "piper {label} downloaded");
    Ok(())
}

// ---------------------------------------------------------------------------
// Config parsing
// ---------------------------------------------------------------------------

/// Parsed Piper model configuration from the .onnx.json file.
struct PiperConfig {
    sample_rate: u32,
    noise_scale: f32,
    length_scale: f32,
    noise_w: f32,
    /// Maps phoneme strings (may be multi-char) to phoneme ID arrays.
    phoneme_id_map: HashMap<String, Vec<i64>>,
    /// espeak-ng voice identifier (e.g. "en-us").
    espeak_voice: String,
    /// Sorted keys from phoneme_id_map, longest first (for greedy matching).
    sorted_keys: Vec<String>,
}

impl PiperConfig {
    fn from_json(json_path: &Path) -> Result<Self, VoiceError> {
        let data = std::fs::read_to_string(json_path)
            .map_err(|e| VoiceError::Tts(format!("failed to read config JSON: {e}")))?;
        let root: serde_json::Value = serde_json::from_str(&data)
            .map_err(|e| VoiceError::Tts(format!("failed to parse config JSON: {e}")))?;

        let audio = &root["audio"];
        let sample_rate = audio["sample_rate"].as_u64().unwrap_or(22050) as u32;

        let inference = &root["inference"];
        let noise_scale = inference["noise_scale"].as_f64().unwrap_or(0.667) as f32;
        let length_scale = inference["length_scale"].as_f64().unwrap_or(1.0) as f32;
        let noise_w = inference["noise_w"].as_f64().unwrap_or(0.8) as f32;

        // espeak voice
        let espeak_voice = root["espeak"]["voice"]
            .as_str()
            .unwrap_or("en-us")
            .to_string();

        // Parse phoneme_id_map: { "a": [10], "aɪ": [25], ... }
        let mut phoneme_id_map = HashMap::new();
        if let Some(map) = root["phoneme_id_map"].as_object() {
            for (key, val) in map {
                if let Some(arr) = val.as_array() {
                    let ids: Vec<i64> = arr
                        .iter()
                        .filter_map(|v| v.as_i64())
                        .collect();
                    phoneme_id_map.insert(key.clone(), ids);
                }
            }
        }

        // Pre-sort keys longest-first for greedy matching
        let mut sorted_keys: Vec<String> = phoneme_id_map.keys().cloned().collect();
        sorted_keys.sort_by(|a, b| b.len().cmp(&a.len()));

        Ok(PiperConfig {
            sample_rate,
            noise_scale,
            length_scale,
            noise_w,
            phoneme_id_map,
            espeak_voice,
            sorted_keys,
        })
    }
}

// ---------------------------------------------------------------------------
// espeak-ng discovery
// ---------------------------------------------------------------------------

fn find_espeak_ng() -> Result<String, VoiceError> {
    // Try `which espeak-ng` first
    if let Ok(output) = std::process::Command::new("which")
        .arg("espeak-ng")
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }

    // Fallback paths
    for path in &["/opt/homebrew/bin/espeak-ng", "/usr/bin/espeak-ng", "/usr/local/bin/espeak-ng"] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    Err(VoiceError::Tts(
        "espeak-ng not found. Install it: brew install espeak-ng (macOS) or apt install espeak-ng (Linux)".to_string(),
    ))
}

// ---------------------------------------------------------------------------
// PiperTts
// ---------------------------------------------------------------------------

/// Piper TTS backend using ONNX Runtime for neural speech synthesis.
///
/// Uses espeak-ng for phonemization and a Piper ONNX model for audio synthesis.
/// Models are automatically downloaded from Hugging Face on first use.
pub struct PiperTts {
    session: Mutex<Session>,
    config: PiperConfig,
    espeak_path: String,
    voice_name: String,
}

impl std::fmt::Debug for PiperTts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PiperTts")
            .field("voice", &self.voice_name)
            .field("sample_rate", &self.config.sample_rate)
            .finish_non_exhaustive()
    }
}

impl PiperTts {
    /// Create a new Piper TTS instance, downloading the model if needed.
    ///
    /// `voice` selects the Piper voice model. Pass `None` for the default
    /// (`en_US-amy-low`, ~17MB).
    pub fn new(voice: Option<&str>) -> Result<Self, VoiceError> {
        let voice_name = voice.unwrap_or(DEFAULT_VOICE);
        let (onnx_path, json_path) = ensure_model(voice_name)?;
        Self::from_files(&onnx_path, &json_path, voice_name)
    }

    /// Create from existing model files on disk (skips download).
    pub fn from_files(
        onnx_path: &Path,
        json_path: &Path,
        voice_name: &str,
    ) -> Result<Self, VoiceError> {
        crate::runtime::ensure_ort_initialized();

        let espeak_path = find_espeak_ng()?;
        let config = PiperConfig::from_json(json_path)?;

        let session = Session::builder()
            .map_err(|e| VoiceError::Tts(format!("failed to create ONNX session builder: {e}")))?
            .with_intra_threads(1)
            .map_err(|e| VoiceError::Tts(format!("failed to set thread count: {e}")))?
            .commit_from_file(onnx_path)
            .map_err(|e| VoiceError::Tts(format!("failed to load piper ONNX model: {e}")))?;

        tracing::info!(
            voice = voice_name,
            sample_rate = config.sample_rate,
            espeak_voice = %config.espeak_voice,
            "piper TTS initialized"
        );

        Ok(Self {
            session: Mutex::new(session),
            config,
            espeak_path,
            voice_name: voice_name.to_string(),
        })
    }

    /// Convert text to phoneme IDs via espeak-ng.
    fn phonemize(&self, text: &str) -> Result<Vec<i64>, VoiceError> {
        let output = std::process::Command::new(&self.espeak_path)
            .args(["-q", "--ipa=2", "-v", &self.config.espeak_voice])
            .arg(text)
            .output()
            .map_err(|e| VoiceError::Tts(format!("espeak-ng failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VoiceError::Tts(format!("espeak-ng error: {stderr}")));
        }

        let phonemes = String::from_utf8_lossy(&output.stdout);
        let phonemes = phonemes.trim();

        if phonemes.is_empty() {
            return Err(VoiceError::Tts("espeak-ng returned empty output".to_string()));
        }

        let pad_id = self.config.phoneme_id_map
            .get("_")
            .cloned()
            .unwrap_or_else(|| vec![0]);
        let bos_id = self.config.phoneme_id_map
            .get("^")
            .cloned()
            .unwrap_or_else(|| vec![1]);
        let eos_id = self.config.phoneme_id_map
            .get("$")
            .cloned()
            .unwrap_or_else(|| vec![2]);

        let mut ids = Vec::new();

        // BOS
        ids.extend(&bos_id);
        ids.extend(&pad_id);

        // Process phoneme string with greedy longest-match
        let chars: Vec<char> = phonemes.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            // Skip newlines/carriage returns
            if chars[i] == '\n' || chars[i] == '\r' {
                i += 1;
                continue;
            }

            // Try longest match first
            let mut matched = false;
            for key in &self.config.sorted_keys {
                let key_chars: Vec<char> = key.chars().collect();
                if key_chars.is_empty() {
                    continue;
                }
                // Skip meta-characters in greedy matching
                if key == "_" || key == "^" || key == "$" {
                    continue;
                }
                if i + key_chars.len() <= chars.len() {
                    let slice: String = chars[i..i + key_chars.len()].iter().collect();
                    if slice == *key {
                        if let Some(phoneme_ids) = self.config.phoneme_id_map.get(key) {
                            ids.extend(phoneme_ids);
                            ids.extend(&pad_id);
                            i += key_chars.len();
                            matched = true;
                            break;
                        }
                    }
                }
            }

            if !matched {
                // Try single character fallback
                let ch = chars[i].to_string();
                if let Some(phoneme_ids) = self.config.phoneme_id_map.get(&ch) {
                    ids.extend(phoneme_ids);
                    ids.extend(&pad_id);
                }
                // Skip unknown characters silently
                i += 1;
            }
        }

        // EOS
        ids.extend(&eos_id);

        Ok(ids)
    }

    /// Run ONNX inference to synthesize audio from phoneme IDs.
    fn infer(&self, phoneme_ids: &[i64]) -> Result<Vec<f32>, VoiceError> {
        let n = phoneme_ids.len();

        let input = Array2::from_shape_vec((1, n), phoneme_ids.to_vec())
            .map_err(|e| VoiceError::Tts(format!("failed to create input array: {e}")))?;
        let input_lengths = Array1::from_vec(vec![n as i64]);
        let scales = Array1::from_vec(vec![
            self.config.noise_scale,
            self.config.length_scale,
            self.config.noise_w,
        ]);

        let input_tensor = Tensor::from_array(input)
            .map_err(|e| VoiceError::Tts(format!("failed to create input tensor: {e}")))?;
        let lengths_tensor = Tensor::from_array(input_lengths)
            .map_err(|e| VoiceError::Tts(format!("failed to create lengths tensor: {e}")))?;
        let scales_tensor = Tensor::from_array(scales)
            .map_err(|e| VoiceError::Tts(format!("failed to create scales tensor: {e}")))?;

        let mut session = self.session.lock()
            .map_err(|e| VoiceError::Tts(format!("session lock poisoned: {e}")))?;

        let outputs = session
            .run(ort::inputs![
                input_tensor,
                lengths_tensor,
                scales_tensor,
            ])
            .map_err(|e| VoiceError::Tts(format!("piper ONNX inference failed: {e}")))?;

        // Output shape: [1, 1, audio_length]
        let (_shape, audio_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| VoiceError::Tts(format!("failed to extract audio tensor: {e}")))?;

        Ok(audio_data.to_vec())
    }

    /// Returns the voice name.
    pub fn voice_name(&self) -> &str {
        &self.voice_name
    }
}

#[async_trait]
impl TtsProvider for PiperTts {
    async fn synthesize(&self, text: &str) -> TtsResult<Vec<f32>> {
        let phoneme_ids = self.phonemize(text)?;
        self.infer(&phoneme_ids)
    }

    fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    fn name(&self) -> &str {
        "piper-tts"
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
        assert!(dir.ends_with("models/piper"));
    }

    #[test]
    fn model_url_parses_standard_voice() {
        let (onnx, json) = model_url("en_US-amy-low");
        assert!(onnx.contains("en/en_US/amy/low/en_US-amy-low.onnx"));
        assert!(!onnx.ends_with(".json"));
        assert!(json.ends_with(".onnx.json"));
    }

    #[test]
    fn model_url_handles_different_locales() {
        let (onnx, _json) = model_url("de_DE-thorsten-low");
        assert!(onnx.contains("de/de_DE/thorsten/low/de_DE-thorsten-low.onnx"));
    }

    #[test]
    fn espeak_ng_is_discoverable() {
        // This test verifies espeak-ng can be found on the system.
        // It may fail on CI without espeak-ng installed.
        let result = find_espeak_ng();
        if std::path::Path::new("/opt/homebrew/bin/espeak-ng").exists()
            || std::path::Path::new("/usr/bin/espeak-ng").exists()
        {
            assert!(result.is_ok(), "espeak-ng should be found: {:?}", result.err());
        }
    }

    /// Requires model to be downloaded and espeak-ng installed.
    /// Run: cargo test -p pagerunner-voice --features piper-tts -- --ignored piper_loads
    #[test]
    #[ignore]
    fn piper_loads_model() {
        let tts = PiperTts::new(None);
        assert!(tts.is_ok(), "failed to load piper TTS: {:?}", tts.err());
    }

    /// End-to-end synthesis test.
    /// Run: cargo test -p pagerunner-voice --features piper-tts -- --ignored piper_synthesizes
    #[test]
    #[ignore]
    fn piper_synthesizes_audio() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tts = PiperTts::new(None).expect("failed to load piper TTS");
        let samples = rt.block_on(tts.synthesize("Hello world.")).expect("synthesis failed");
        assert!(!samples.is_empty(), "synthesis produced no audio");
        assert!(
            samples.iter().any(|s| s.abs() > 0.01),
            "synthesis produced only silence"
        );
    }
}
