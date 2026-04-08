use async_trait::async_trait;
use crate::VoiceError;

pub type Result<T> = std::result::Result<T, VoiceError>;

/// Text-to-speech provider trait.
///
/// Implementations synthesize speech from text and return raw PCM audio as
/// mono f32 samples at the sample rate reported by `sample_rate()`.
#[async_trait]
pub trait TtsProvider: Send + Sync {
    /// Synthesize speech from text. Returns f32 audio samples (mono).
    async fn synthesize(&self, text: &str) -> Result<Vec<f32>>;

    /// Sample rate of synthesized audio.
    fn sample_rate(&self) -> u32;

    /// Provider name.
    fn name(&self) -> &str;
}
