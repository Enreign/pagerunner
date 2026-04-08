/// Voice activity detection — detects speech start/end in audio.
///
/// VAD detectors are synchronous since they typically run lightweight
/// energy-based or model-based heuristics on each incoming chunk.
pub trait VadDetector: Send + Sync {
    /// Process an audio chunk. Returns true if speech is detected.
    fn process(&mut self, audio: &[f32], sample_rate: u32) -> bool;

    /// Reset internal state.
    fn reset(&mut self);
}
