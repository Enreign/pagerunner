//! `pagerunner-voice` — standalone voice pipeline (STT + TTS + VAD).
//!
//! This crate has **zero Pagerunner dependencies** and is designed to be
//! usable independently.  It exposes:
//!
//! - [`SttProvider`] — speech-to-text trait
//! - [`TtsProvider`] — text-to-speech trait
//! - [`VadDetector`] — voice activity detection trait
//! - [`VoicePipeline`] — orchestrator that wires the three together
//! - [`mock`] — mock implementations for tests (behind `mock` feature or `#[cfg(test)]`)
//! - [`WhisperStt`] — whisper.cpp STT backend (behind `whisper` feature)
//! - [`PiperTts`] — Piper neural TTS backend (behind `piper-tts` feature)
//! - [`VoiceError`] — unified error type

#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod stt;
pub mod tts;
pub mod vad;

#[cfg(feature = "whisper")]
pub mod whisper_stt;
#[cfg(feature = "whisper")]
pub use whisper_stt::WhisperStt;

#[cfg(any(feature = "silero-vad", feature = "piper-tts"))]
pub(crate) mod runtime;

#[cfg(feature = "silero-vad")]
pub mod silero_vad;
#[cfg(feature = "silero-vad")]
pub use silero_vad::SileroVad;

#[cfg(feature = "piper-tts")]
pub mod piper_tts;
#[cfg(feature = "piper-tts")]
pub use piper_tts::PiperTts;

pub use stt::SttProvider;
pub use tts::TtsProvider;
pub use vad::VadDetector;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("STT error: {0}")]
    Stt(String),
    #[error("TTS error: {0}")]
    Tts(String),
    #[error("VAD error: {0}")]
    Vad(String),
    #[error("Audio error: {0}")]
    Audio(String),
}

pub type Result<T> = std::result::Result<T, VoiceError>;

// ---------------------------------------------------------------------------
// PipelineConfig
// ---------------------------------------------------------------------------

/// Configuration for the voice pipeline.
///
/// Controls silence detection timing and STT model selection. Use the
/// [`Default`], [`accurate`], or [`fast`] presets, or customize individual
/// fields.
///
/// [`accurate`]: PipelineConfig::accurate
/// [`fast`]: PipelineConfig::fast
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Silence duration (in seconds) required to end an utterance. Default: 0.3.
    pub silence_timeout_secs: f32,
    /// VAD chunk size used for silence frame counting. Default: 512 (32 ms at 16 kHz).
    pub vad_chunk_size: usize,
    /// Whisper model to use. `"whisper-tiny"` (faster, ~75 MB) or `"whisper-base"` (better, ~142 MB).
    pub stt_model: String,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            silence_timeout_secs: 0.3,
            vad_chunk_size: 512,
            stt_model: "whisper-tiny".to_string(),
        }
    }
}

impl PipelineConfig {
    /// Preset for accuracy — uses whisper-base, longer silence timeout.
    pub fn accurate() -> Self {
        Self {
            silence_timeout_secs: 0.5,
            stt_model: "whisper-base".to_string(),
            ..Default::default()
        }
    }

    /// Preset for speed — uses whisper-tiny, shorter silence timeout.
    pub fn fast() -> Self {
        Self {
            silence_timeout_secs: 0.2,
            stt_model: "whisper-tiny".to_string(),
            ..Default::default()
        }
    }

    /// Compute the number of silence frames required to end an utterance,
    /// based on `silence_timeout_secs` and `vad_chunk_size` at 16 kHz.
    pub(crate) fn silence_frames_required(&self) -> usize {
        let chunk_duration_secs = self.vad_chunk_size as f32 / 16_000.0;
        (self.silence_timeout_secs / chunk_duration_secs).ceil() as usize
    }
}

// ---------------------------------------------------------------------------
// VoicePipeline
// ---------------------------------------------------------------------------

/// Orchestrates STT, TTS, and VAD into a single coherent voice pipeline.
///
/// Typical call flow:
///
/// 1. Feed raw PCM chunks via [`process_audio`] — VAD gates STT. Audio is
///    buffered while speech is active and sent to STT as a single utterance
///    once silence is detected (after a configurable debounce).
/// 2. When `process_audio` returns `Some(text)` a complete utterance has been
///    transcribed; the pipeline auto-resets for the next utterance.
/// 3. Use [`speak`] to synthesize a response.
///
/// [`process_audio`]: VoicePipeline::process_audio
/// [`speak`]: VoicePipeline::speak
pub struct VoicePipeline {
    stt: Box<dyn SttProvider>,
    tts: Box<dyn TtsProvider>,
    vad: Box<dyn VadDetector>,
    /// Whether the current VAD frame detected speech.
    speaking: bool,
    /// Whether the previous VAD frame detected speech (for edge detection).
    was_speaking: bool,
    /// Accumulates audio samples while VAD detects speech.
    utterance_buffer: Vec<f32>,
    /// Number of consecutive silence frames needed to end an utterance.
    silence_frames_required: usize,
    /// Current count of consecutive silence frames.
    silence_frame_count: usize,
    /// Sample rate of buffered audio.
    buffer_sample_rate: u32,
}

impl VoicePipeline {
    /// Construct a new pipeline from concrete provider/detector instances.
    pub fn new(
        stt: impl SttProvider + 'static,
        tts: impl TtsProvider + 'static,
        vad: impl VadDetector + 'static,
    ) -> Self {
        Self::with_config(stt, tts, vad, PipelineConfig::default())
    }

    /// Construct a new pipeline with explicit configuration.
    pub fn with_config(
        stt: impl SttProvider + 'static,
        tts: impl TtsProvider + 'static,
        vad: impl VadDetector + 'static,
        config: PipelineConfig,
    ) -> Self {
        let silence_frames_required = config.silence_frames_required();
        Self {
            stt: Box::new(stt),
            tts: Box::new(tts),
            vad: Box::new(vad),
            speaking: false,
            was_speaking: false,
            utterance_buffer: Vec::new(),
            silence_frames_required,
            silence_frame_count: 0,
            buffer_sample_rate: 0,
        }
    }

    /// Process an audio chunk through VAD, buffering speech until silence ends
    /// the utterance, then transcribing the full utterance in one STT call.
    ///
    /// Returns `Ok(None)` while accumulating speech or during silence.
    /// Returns `Ok(Some(text))` when a complete utterance is transcribed.
    pub async fn process_audio(
        &mut self,
        audio: &[f32],
        sample_rate: u32,
    ) -> Result<Option<String>> {
        self.was_speaking = self.speaking;
        self.speaking = self.vad.process(audio, sample_rate);

        if self.speaking {
            // Speech detected — accumulate in buffer
            tracing::trace!("VAD: speech detected, buffering");
            self.utterance_buffer.extend_from_slice(audio);
            self.buffer_sample_rate = sample_rate;
            self.silence_frame_count = 0;
            return Ok(None);
        }

        // Silence detected
        if self.was_speaking || !self.utterance_buffer.is_empty() {
            // Was speaking or have buffered audio — debounce silence
            self.silence_frame_count += 1;

            if self.silence_frame_count < self.silence_frames_required {
                // Still debouncing — include trailing silence in buffer
                tracing::trace!(
                    silence_frames = self.silence_frame_count,
                    required = self.silence_frames_required,
                    "VAD: silence debounce, still buffering"
                );
                self.utterance_buffer.extend_from_slice(audio);
                return Ok(None);
            }

            // Enough silence — utterance is complete
            if self.utterance_buffer.is_empty() {
                self.reset_utterance_state();
                return Ok(None);
            }

            let buffer = std::mem::take(&mut self.utterance_buffer);
            let sr = self.buffer_sample_rate;
            self.reset_utterance_state();

            tracing::trace!(
                samples = buffer.len(),
                "VAD: utterance complete, sending to STT"
            );
            return self
                .stt
                .transcribe_chunk(&buffer, sr)
                .await
                .map_err(|e| VoiceError::Stt(e.to_string()));
        }

        // Pure silence, no buffered audio
        tracing::trace!("VAD: no speech detected, skipping STT");
        Ok(None)
    }

    /// Reset utterance buffering state (not VAD model state).
    fn reset_utterance_state(&mut self) {
        self.utterance_buffer.clear();
        self.silence_frame_count = 0;
        self.was_speaking = false;
        self.speaking = false;
        self.buffer_sample_rate = 0;
    }

    /// Synthesize speech from `text` using the TTS provider.
    ///
    /// Returns raw mono f32 PCM samples at [`tts_sample_rate`].
    ///
    /// [`tts_sample_rate`]: VoicePipeline::tts_sample_rate
    pub async fn speak(&self, text: &str) -> Result<Vec<f32>> {
        self.tts
            .synthesize(text)
            .await
            .map_err(|e| VoiceError::Tts(e.to_string()))
    }

    /// Returns the sample rate of audio produced by [`speak`].
    ///
    /// [`speak`]: VoicePipeline::speak
    pub fn tts_sample_rate(&self) -> u32 {
        self.tts.sample_rate()
    }

    /// Returns `true` if the most recent [`process_audio`] call detected
    /// speech activity.
    ///
    /// [`process_audio`]: VoicePipeline::process_audio
    pub fn is_speaking(&self) -> bool {
        self.speaking
    }

    /// Reset VAD state (e.g. after processing a complete utterance).
    pub fn reset_vad(&mut self) {
        self.vad.reset();
        self.reset_utterance_state();
    }

    /// Flush any audio buffered inside the utterance buffer and/or STT
    /// provider and return remaining transcription text, if any.
    pub async fn flush_stt(&mut self) -> Result<Option<String>> {
        // If there's buffered audio in the utterance buffer, send it to STT
        if !self.utterance_buffer.is_empty() {
            let buffer = std::mem::take(&mut self.utterance_buffer);
            let sr = self.buffer_sample_rate;
            self.reset_utterance_state();
            return self
                .stt
                .transcribe_chunk(&buffer, sr)
                .await
                .map_err(|e| VoiceError::Stt(e.to_string()));
        }

        // Otherwise delegate to the STT provider's own flush
        self.stt
            .flush()
            .await
            .map_err(|e| VoiceError::Stt(e.to_string()))
    }

    /// Name of the active STT provider.
    pub fn stt_name(&self) -> &str {
        self.stt.name()
    }

    /// Name of the active TTS provider.
    pub fn tts_name(&self) -> &str {
        self.tts.name()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{MockStt, MockTts, MockVad};

    /// Build a pipeline with a short silence timeout for fast tests.
    /// Uses `silence_frames_required = 2` so only 2 silence chunks end an
    /// utterance. MockStt with `chunks_required = 1` transcribes on the
    /// first call (gets the full utterance buffer in one shot).
    fn make_pipeline() -> VoicePipeline {
        let config = PipelineConfig {
            silence_timeout_secs: 0.064, // 2 frames at 512/16kHz = 64ms
            vad_chunk_size: 512,
            stt_model: "whisper-tiny".to_string(),
        };
        VoicePipeline::with_config(
            MockStt::new(1),
            MockTts::default(),
            MockVad::new(0.01),
            config,
        )
    }

    /// Helper: feed N silence chunks to trigger utterance end.
    async fn feed_silence(p: &mut VoicePipeline, n: usize) -> Vec<Option<String>> {
        let silent = vec![0.0_f32; 160];
        let mut results = Vec::new();
        for _ in 0..n {
            results.push(p.process_audio(&silent, 16_000).await.unwrap());
        }
        results
    }

    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    #[test]
    fn pipeline_construction_with_mocks() {
        let p = make_pipeline();
        assert_eq!(p.stt_name(), "mock-stt");
        assert_eq!(p.tts_name(), "mock-tts");
        assert!(!p.is_speaking());
    }

    // -----------------------------------------------------------------------
    // process_audio — speech buffering
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn speech_chunk_buffers_and_returns_none() {
        let mut p = make_pipeline();
        let loud = vec![0.5_f32; 160];

        // Speech chunk should buffer, not transcribe yet
        let result = p.process_audio(&loud, 16_000).await.unwrap();
        assert!(
            result.is_none(),
            "speech should buffer, not transcribe immediately"
        );
        assert!(p.is_speaking());
    }

    #[tokio::test]
    async fn speech_then_silence_triggers_transcription() {
        let mut p = make_pipeline();
        let loud = vec![0.5_f32; 160];

        // Feed speech
        let r = p.process_audio(&loud, 16_000).await.unwrap();
        assert!(r.is_none());

        // Feed enough silence to end the utterance (silence_frames_required = 2)
        let results = feed_silence(&mut p, 3).await;
        // One of the silence chunks should trigger transcription
        let transcriptions: Vec<_> = results.into_iter().flatten().collect();
        assert_eq!(transcriptions.len(), 1);
        assert_eq!(transcriptions[0], "mock transcription");
    }

    #[tokio::test]
    async fn short_silence_does_not_end_utterance() {
        let mut p = make_pipeline();
        let loud = vec![0.5_f32; 160];

        // Speech
        p.process_audio(&loud, 16_000).await.unwrap();

        // One silence frame (less than required 2) — should still be buffering
        let silent = vec![0.0_f32; 160];
        let result = p.process_audio(&silent, 16_000).await.unwrap();
        assert!(
            result.is_none(),
            "single silence frame should not end utterance"
        );

        // More speech resumes buffering
        let result = p.process_audio(&loud, 16_000).await.unwrap();
        assert!(result.is_none());
        assert!(p.is_speaking());
    }

    #[tokio::test]
    async fn multiple_speech_chunks_form_single_utterance() {
        let mut p = make_pipeline();
        let loud = vec![0.5_f32; 160];

        // Feed 5 speech chunks
        for _ in 0..5 {
            let r = p.process_audio(&loud, 16_000).await.unwrap();
            assert!(r.is_none());
        }

        // End with silence
        let results = feed_silence(&mut p, 3).await;
        let transcriptions: Vec<_> = results.into_iter().flatten().collect();
        assert_eq!(
            transcriptions.len(),
            1,
            "should transcribe exactly once for the full utterance"
        );
    }

    // -----------------------------------------------------------------------
    // process_audio — silence path
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn pure_silence_returns_none() {
        let mut p = make_pipeline();
        let silent = vec![0.0_f32; 160];

        let result = p.process_audio(&silent, 16_000).await.unwrap();
        assert!(result.is_none());
        assert!(!p.is_speaking());
    }

    // -----------------------------------------------------------------------
    // speak
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn speak_returns_audio_samples() {
        let p = make_pipeline();
        let samples = p.speak("hello world").await.unwrap();
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|s| s.abs() <= 1.0 + f32::EPSILON));
    }

    #[tokio::test]
    async fn speak_sample_rate_matches_tts_provider() {
        let p = make_pipeline();
        assert_eq!(p.tts_sample_rate(), 16_000);
    }

    // -----------------------------------------------------------------------
    // VAD state tracking
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn is_speaking_reflects_vad_result() {
        let mut p = make_pipeline();
        assert!(!p.is_speaking());

        let loud = vec![0.5_f32; 160];
        p.process_audio(&loud, 16_000).await.unwrap();
        assert!(p.is_speaking());

        // After enough silence, speaking should be false
        feed_silence(&mut p, 3).await;
        assert!(!p.is_speaking());
    }

    #[tokio::test]
    async fn reset_vad_clears_speaking_flag() {
        let mut p = make_pipeline();
        let loud = vec![0.5_f32; 160];
        p.process_audio(&loud, 16_000).await.unwrap();
        assert!(p.is_speaking());

        p.reset_vad();
        assert!(!p.is_speaking());
    }

    // -----------------------------------------------------------------------
    // STT flush
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn flush_stt_returns_buffered_utterance() {
        let mut p = make_pipeline();
        let loud = vec![0.5_f32; 160];

        // Speech is buffered inside VoicePipeline (not yet sent to STT)
        p.process_audio(&loud, 16_000).await.unwrap();

        // Flush should send the buffered audio to STT
        let flushed = p.flush_stt().await.unwrap();
        assert_eq!(flushed, Some("mock transcription".to_string()));
    }

    #[tokio::test]
    async fn flush_stt_returns_none_when_nothing_buffered() {
        let mut p = make_pipeline();

        // No speech fed — flush should return None
        let flushed = p.flush_stt().await.unwrap();
        assert!(flushed.is_none());
    }

    // -----------------------------------------------------------------------
    // PipelineConfig
    // -----------------------------------------------------------------------

    #[test]
    fn pipeline_config_default_silence_frames() {
        let config = PipelineConfig::default();
        // 0.3s / (512/16000) = 0.3 / 0.032 = 9.375 → ceil = 10
        assert_eq!(config.silence_frames_required(), 10);
    }

    #[test]
    fn pipeline_config_accurate_preset() {
        let config = PipelineConfig::accurate();
        assert_eq!(config.stt_model, "whisper-base");
        assert!(config.silence_timeout_secs > 0.3);
    }

    #[test]
    fn pipeline_config_fast_preset() {
        let config = PipelineConfig::fast();
        assert_eq!(config.stt_model, "whisper-tiny");
        assert!(config.silence_timeout_secs < 0.3);
    }
}
