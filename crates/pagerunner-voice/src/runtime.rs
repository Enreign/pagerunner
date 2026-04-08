//! Shared ONNX Runtime initialization.
//!
//! Both Silero VAD and Piper TTS use `ort`. This module provides a one-time
//! initialization call that configures the runtime thread pool before any
//! sessions are created.

use std::sync::Once;

static ORT_INIT: Once = Once::new();

/// Initialize the ONNX runtime once. Safe to call multiple times.
pub fn ensure_ort_initialized() {
    ORT_INIT.call_once(|| {
        if let Err(e) = ort::init().with_name("pagerunner-voice").commit() {
            tracing::warn!("ort init failed (may already be initialized): {}", e);
        }
    });
}
