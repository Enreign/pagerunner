use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;

use crate::{
    stt::{Result as SttResult, SttProvider},
    tts::{Result as TtsResult, TtsProvider},
    vad::VadDetector,
};

// ---------------------------------------------------------------------------
// MockStt
// ---------------------------------------------------------------------------

/// Mock STT provider that accumulates audio chunks and returns a fixed
/// transcription after `chunks_required` chunks have been fed.
pub struct MockStt {
    /// Number of chunks to accumulate before returning a transcription.
    chunks_required: usize,
    /// Running count of chunks received since the last transcription.
    chunk_count: AtomicUsize,
    /// Fixed transcription string returned.
    transcription: String,
    /// Buffered text for flush (set after each complete utterance detection).
    buffer: Mutex<Option<String>>,
}

impl MockStt {
    pub fn new(chunks_required: usize) -> Self {
        Self {
            chunks_required,
            chunk_count: AtomicUsize::new(0),
            transcription: "mock transcription".to_string(),
            buffer: Mutex::new(None),
        }
    }

    pub fn with_transcription(mut self, text: impl Into<String>) -> Self {
        self.transcription = text.into();
        self
    }
}

impl Default for MockStt {
    fn default() -> Self {
        Self::new(3)
    }
}

#[async_trait]
impl SttProvider for MockStt {
    async fn transcribe_chunk(&self, _audio: &[f32], _sample_rate: u32) -> SttResult<Option<String>> {
        let count = self.chunk_count.fetch_add(1, Ordering::SeqCst) + 1;
        if count >= self.chunks_required {
            self.chunk_count.store(0, Ordering::SeqCst);
            let text = self.transcription.clone();
            *self.buffer.lock().unwrap() = Some(text.clone());
            Ok(Some(text))
        } else {
            Ok(None)
        }
    }

    async fn flush(&self) -> SttResult<Option<String>> {
        let held = self.buffer.lock().unwrap().take();
        Ok(held)
    }

    fn name(&self) -> &str {
        "mock-stt"
    }
}

// ---------------------------------------------------------------------------
// MockTts
// ---------------------------------------------------------------------------

/// Mock TTS provider that returns a 440 Hz sine wave for any text.
pub struct MockTts {
    sample_rate: u32,
    /// Duration in seconds of synthesized audio.
    duration_secs: f32,
}

impl MockTts {
    pub fn new(sample_rate: u32, duration_secs: f32) -> Self {
        Self { sample_rate, duration_secs }
    }
}

impl Default for MockTts {
    fn default() -> Self {
        Self::new(16_000, 0.1)
    }
}

#[async_trait]
impl TtsProvider for MockTts {
    async fn synthesize(&self, _text: &str) -> TtsResult<Vec<f32>> {
        let n_samples = (self.sample_rate as f32 * self.duration_secs) as usize;
        let freq = 440.0_f32;
        let samples: Vec<f32> = (0..n_samples)
            .map(|i| {
                let t = i as f32 / self.sample_rate as f32;
                (2.0 * std::f32::consts::PI * freq * t).sin()
            })
            .collect();
        Ok(samples)
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn name(&self) -> &str {
        "mock-tts"
    }
}

// ---------------------------------------------------------------------------
// MockVad
// ---------------------------------------------------------------------------

/// Mock VAD detector that returns true when the RMS energy of the audio chunk
/// exceeds `energy_threshold`.
pub struct MockVad {
    energy_threshold: f32,
    /// Whether the detector currently considers speech to be active.
    speaking: bool,
}

impl MockVad {
    pub fn new(energy_threshold: f32) -> Self {
        Self { energy_threshold, speaking: false }
    }
}

impl Default for MockVad {
    fn default() -> Self {
        Self::new(0.01)
    }
}

impl VadDetector for MockVad {
    fn process(&mut self, audio: &[f32], _sample_rate: u32) -> bool {
        if audio.is_empty() {
            self.speaking = false;
            return false;
        }
        let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
        self.speaking = rms > self.energy_threshold;
        self.speaking
    }

    fn reset(&mut self) {
        self.speaking = false;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_stt_returns_transcription_after_n_chunks() {
        let stt = MockStt::new(2);
        let audio = vec![0.1_f32; 160];

        let first = stt.transcribe_chunk(&audio, 16_000).await.unwrap();
        assert!(first.is_none(), "should not transcribe on first chunk");

        let second = stt.transcribe_chunk(&audio, 16_000).await.unwrap();
        assert_eq!(second, Some("mock transcription".to_string()));
    }

    #[tokio::test]
    async fn mock_stt_resets_count_after_transcription() {
        let stt = MockStt::new(1);
        let audio = vec![0.1_f32; 160];

        let first = stt.transcribe_chunk(&audio, 16_000).await.unwrap();
        assert!(first.is_some());

        // Counter should have reset; next chunk starts a new cycle.
        let second = stt.transcribe_chunk(&audio, 16_000).await.unwrap();
        assert!(second.is_some(), "should transcribe again after reset");
    }

    #[tokio::test]
    async fn mock_stt_flush_returns_buffered_text() {
        let stt = MockStt::new(1);
        let audio = vec![0.1_f32; 160];

        stt.transcribe_chunk(&audio, 16_000).await.unwrap();
        let flushed = stt.flush().await.unwrap();
        assert_eq!(flushed, Some("mock transcription".to_string()));

        // Second flush should be empty.
        let empty = stt.flush().await.unwrap();
        assert!(empty.is_none());
    }

    #[tokio::test]
    async fn mock_tts_returns_samples() {
        let tts = MockTts::new(16_000, 0.1);
        let samples = tts.synthesize("hello").await.unwrap();
        assert_eq!(samples.len(), 1_600);
        assert!(samples.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn mock_vad_detects_energy() {
        let mut vad = MockVad::new(0.01);
        let loud = vec![0.5_f32; 160];
        let silent = vec![0.0_f32; 160];

        assert!(vad.process(&loud, 16_000));
        vad.reset();
        assert!(!vad.process(&silent, 16_000));
    }
}
