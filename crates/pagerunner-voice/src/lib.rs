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
// VoicePipeline
// ---------------------------------------------------------------------------

/// Orchestrates STT, TTS, and VAD into a single coherent voice pipeline.
///
/// Typical call flow:
///
/// 1. Feed raw PCM chunks via [`process_audio`] — VAD gates STT so that STT
///    only runs when speech is active.
/// 2. When `process_audio` returns `Some(text)` a complete utterance has been
///    transcribed; call [`reset_vad`] to prepare for the next utterance.
/// 3. Use [`speak`] to synthesize a response.
///
/// [`process_audio`]: VoicePipeline::process_audio
/// [`reset_vad`]: VoicePipeline::reset_vad
/// [`speak`]: VoicePipeline::speak
pub struct VoicePipeline {
    stt: Box<dyn SttProvider>,
    tts: Box<dyn TtsProvider>,
    vad: Box<dyn VadDetector>,
    /// Tracks whether the last VAD frame detected speech.
    speaking: bool,
}

impl VoicePipeline {
    /// Construct a new pipeline from concrete provider/detector instances.
    pub fn new(
        stt: impl SttProvider + 'static,
        tts: impl TtsProvider + 'static,
        vad: impl VadDetector + 'static,
    ) -> Self {
        Self {
            stt: Box::new(stt),
            tts: Box::new(tts),
            vad: Box::new(vad),
            speaking: false,
        }
    }

    /// Process an audio chunk through VAD then STT.
    ///
    /// VAD runs first.  If no speech is detected the chunk is discarded and
    /// `Ok(None)` is returned.  When speech is detected the chunk is forwarded
    /// to the STT provider; a complete utterance yields `Ok(Some(text))`.
    pub async fn process_audio(
        &mut self,
        audio: &[f32],
        sample_rate: u32,
    ) -> Result<Option<String>> {
        self.speaking = self.vad.process(audio, sample_rate);

        if !self.speaking {
            tracing::trace!("VAD: no speech detected, skipping STT");
            return Ok(None);
        }

        tracing::trace!("VAD: speech detected, forwarding to STT");
        self.stt
            .transcribe_chunk(audio, sample_rate)
            .await
            .map_err(|e| VoiceError::Stt(e.to_string()))
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
        self.speaking = false;
    }

    /// Flush any audio buffered inside the STT provider and return remaining
    /// transcription text, if any.
    pub async fn flush_stt(&mut self) -> Result<Option<String>> {
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

    fn make_pipeline(stt_chunks: usize) -> VoicePipeline {
        VoicePipeline::new(
            MockStt::new(stt_chunks),
            MockTts::default(),
            MockVad::new(0.01),
        )
    }

    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    #[test]
    fn pipeline_construction_with_mocks() {
        let p = make_pipeline(3);
        assert_eq!(p.stt_name(), "mock-stt");
        assert_eq!(p.tts_name(), "mock-tts");
        assert!(!p.is_speaking());
    }

    // -----------------------------------------------------------------------
    // process_audio — speech path
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn process_audio_with_speech_returns_transcription() {
        let mut p = make_pipeline(1); // transcribe on first chunk
        let loud = vec![0.5_f32; 160]; // energy > threshold → VAD fires

        let result = p.process_audio(&loud, 16_000).await.unwrap();
        assert_eq!(result, Some("mock transcription".to_string()));
        assert!(p.is_speaking());
    }

    #[tokio::test]
    async fn process_audio_accumulates_chunks_before_transcribing() {
        let mut p = make_pipeline(2); // needs 2 chunks
        let loud = vec![0.5_f32; 160];

        let first = p.process_audio(&loud, 16_000).await.unwrap();
        assert!(first.is_none(), "not enough chunks yet");

        let second = p.process_audio(&loud, 16_000).await.unwrap();
        assert_eq!(second, Some("mock transcription".to_string()));
    }

    // -----------------------------------------------------------------------
    // process_audio — silence path
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn process_audio_with_silence_returns_none() {
        let mut p = make_pipeline(1);
        let silent = vec![0.0_f32; 160]; // energy == 0 → VAD suppresses

        let result = p.process_audio(&silent, 16_000).await.unwrap();
        assert!(result.is_none());
        assert!(!p.is_speaking());
    }

    // -----------------------------------------------------------------------
    // speak
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn speak_returns_audio_samples() {
        let p = make_pipeline(1);
        let samples = p.speak("hello world").await.unwrap();
        assert!(!samples.is_empty());
        // Sine wave values must be within [-1, 1]
        assert!(samples.iter().all(|s| s.abs() <= 1.0 + f32::EPSILON));
    }

    #[tokio::test]
    async fn speak_sample_rate_matches_tts_provider() {
        let p = make_pipeline(1);
        assert_eq!(p.tts_sample_rate(), 16_000);
    }

    // -----------------------------------------------------------------------
    // VAD state tracking
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn is_speaking_reflects_vad_result() {
        let mut p = make_pipeline(3);
        assert!(!p.is_speaking());

        let loud = vec![0.5_f32; 160];
        p.process_audio(&loud, 16_000).await.unwrap();
        assert!(p.is_speaking());

        let silent = vec![0.0_f32; 160];
        p.process_audio(&silent, 16_000).await.unwrap();
        assert!(!p.is_speaking());
    }

    #[tokio::test]
    async fn reset_vad_clears_speaking_flag() {
        let mut p = make_pipeline(3);
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
    async fn flush_stt_returns_buffered_text() {
        let mut p = make_pipeline(1); // transcribes on first speech chunk
        let loud = vec![0.5_f32; 160];

        // Trigger transcription (buffers "mock transcription" inside MockStt).
        let transcribed = p.process_audio(&loud, 16_000).await.unwrap();
        assert!(transcribed.is_some());

        let flushed = p.flush_stt().await.unwrap();
        assert_eq!(flushed, Some("mock transcription".to_string()));
    }

    #[tokio::test]
    async fn flush_stt_returns_none_when_nothing_buffered() {
        let mut p = make_pipeline(3); // needs 3 chunks — nothing emitted yet
        let loud = vec![0.5_f32; 160];
        p.process_audio(&loud, 16_000).await.unwrap();

        // No transcription emitted yet, flush should return None.
        let flushed = p.flush_stt().await.unwrap();
        assert!(flushed.is_none());
    }
}
