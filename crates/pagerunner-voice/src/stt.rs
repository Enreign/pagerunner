use crate::VoiceError;
use async_trait::async_trait;

pub type Result<T> = std::result::Result<T, VoiceError>;

/// Speech-to-text provider trait.
///
/// Implementations receive raw audio chunks and emit transcribed text when a
/// complete utterance is detected. The provider is responsible for its own
/// buffering strategy.
#[async_trait]
pub trait SttProvider: Send + Sync {
    /// Feed an audio chunk (f32 samples, mono). Returns transcribed text if a
    /// complete utterance was detected.
    async fn transcribe_chunk(&self, audio: &[f32], sample_rate: u32) -> Result<Option<String>>;

    /// Flush any buffered audio and return remaining text.
    async fn flush(&self) -> Result<Option<String>>;

    /// Provider name.
    fn name(&self) -> &str;
}
