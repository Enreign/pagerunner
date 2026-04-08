//! Mic input and speaker output via `cpal`.
//!
//! Audio callbacks run on a dedicated OS thread managed by cpal. We use
//! [`std::sync::mpsc::sync_channel`] (not tokio) for the mic callback since
//! cpal callbacks cannot be async.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::sync::mpsc::{self, Receiver};

/// Target sample rate for the voice pipeline (VAD + STT expect 16 kHz).
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Opens the default input device and returns a running stream plus a receiver
/// that yields mono f32 audio chunks of `chunk_size` samples at 16 kHz.
///
/// The returned [`Stream`] must be kept alive — dropping it stops the mic.
pub fn open_mic(chunk_size: usize) -> Result<(Stream, Receiver<Vec<f32>>)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("No default audio input device found")?;

    tracing::info!(
        device = device.name().unwrap_or_default(),
        "Using input device"
    );

    let supported = device
        .default_input_config()
        .map_err(|e| {
            // cpal returns DefaultStreamConfigError which may indicate permission denial
            // on macOS. The OS-level error message is typically opaque, so provide guidance.
            let msg = format!("{e}");
            if msg.contains("permission")
                || msg.contains("denied")
                || msg.contains("not allowed")
                || matches!(e, cpal::DefaultStreamConfigError::DeviceNotAvailable)
            {
                anyhow::anyhow!(
                    "Microphone access denied. Grant permission in System Settings \u{2192} Privacy & Security \u{2192} Microphone."
                )
            } else {
                anyhow::anyhow!("Failed to get default input config: {e}")
            }
        })?;

    tracing::debug!(
        sample_rate = supported.sample_rate().0,
        channels = supported.channels(),
        format = ?supported.sample_format(),
        "Default input config"
    );

    let sample_rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();

    let config = StreamConfig {
        channels: supported.channels(),
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    // Bounded channel — if the consumer falls behind, we drop old audio rather
    // than letting memory grow unbounded. 64 chunks is ~2 s at 512-sample chunks.
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(64);

    // Accumulator for resampled mono samples until we have a full chunk.
    let needs_resample = sample_rate.0 != TARGET_SAMPLE_RATE;
    let rate_ratio = sample_rate.0 as f64 / TARGET_SAMPLE_RATE as f64;

    let mut accum: Vec<f32> = Vec::with_capacity(chunk_size * 2);

    let err_fn = |err: cpal::StreamError| {
        tracing::error!(%err, "Audio input stream error");
    };

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Downmix to mono
                let mono: Vec<f32> = if channels == 1 {
                    data.to_vec()
                } else {
                    data.chunks(channels)
                        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                        .collect()
                };

                // Simple nearest-neighbour resample to 16 kHz if needed
                let samples = if needs_resample {
                    let out_len = (mono.len() as f64 / rate_ratio).ceil() as usize;
                    (0..out_len)
                        .map(|i| {
                            let src = (i as f64 * rate_ratio).min((mono.len() - 1) as f64);
                            mono[src as usize]
                        })
                        .collect()
                } else {
                    mono
                };

                accum.extend_from_slice(&samples);

                // Emit full chunks
                while accum.len() >= chunk_size {
                    let chunk: Vec<f32> = accum.drain(..chunk_size).collect();
                    // If the receiver is full or disconnected, just drop the chunk.
                    let _ = tx.try_send(chunk);
                }
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                // Convert i16 → f32 then downmix
                let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                let mono: Vec<f32> = if channels == 1 {
                    f32_data
                } else {
                    f32_data
                        .chunks(channels)
                        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                        .collect()
                };

                let samples = if needs_resample {
                    let out_len = (mono.len() as f64 / rate_ratio).ceil() as usize;
                    (0..out_len)
                        .map(|i| {
                            let src = (i as f64 * rate_ratio).min((mono.len() - 1) as f64);
                            mono[src as usize]
                        })
                        .collect()
                } else {
                    mono
                };

                accum.extend_from_slice(&samples);
                while accum.len() >= chunk_size {
                    let chunk: Vec<f32> = accum.drain(..chunk_size).collect();
                    let _ = tx.try_send(chunk);
                }
            },
            err_fn,
            None,
        )?,
        other => anyhow::bail!("Unsupported input sample format: {:?}", other),
    };

    stream.play().map_err(|e| {
        let msg = format!("{e}");
        if msg.contains("permission")
            || msg.contains("denied")
            || msg.contains("not allowed")
        {
            anyhow::anyhow!(
                "Microphone access denied. Grant permission in System Settings \u{2192} Privacy & Security \u{2192} Microphone."
            )
        } else {
            anyhow::anyhow!("Failed to start input stream: {e}")
        }
    })?;

    Ok((stream, rx))
}

/// Plays f32 mono audio samples through the default output device at the given
/// sample rate. Blocks until all samples have been written to the device.
///
/// Returns an error if no output device is available.
pub fn play_audio(samples: &[f32], sample_rate: u32) -> Result<()> {
    if samples.is_empty() {
        return Ok(());
    }

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("No default audio output device found")?;

    let config = StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let samples = samples.to_vec();
    let len = samples.len();
    let pos = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let pos_cb = pos.clone();
    let done_cb = done.clone();
    let samples_cb = samples.clone();

    let stream = device.build_output_stream(
        &config,
        move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let current = pos_cb.load(std::sync::atomic::Ordering::Relaxed);
            for (i, sample) in output.iter_mut().enumerate() {
                let idx = current + i;
                if idx < len {
                    *sample = samples_cb[idx];
                } else {
                    *sample = 0.0;
                }
            }
            let new_pos = (current + output.len()).min(len);
            pos_cb.store(new_pos, std::sync::atomic::Ordering::Relaxed);
            if new_pos >= len {
                done_cb.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        },
        |err| tracing::error!(%err, "Audio output stream error"),
        None,
    )?;

    stream.play().context("Failed to start output stream")?;

    // Spin-wait for playback to finish (audio is typically short — a few seconds).
    while !done.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Small grace period to let the last buffer drain.
    std::thread::sleep(std::time::Duration::from_millis(50));

    drop(stream);
    Ok(())
}

/// Returns `true` if a microphone input device is available.
pub fn has_input_device() -> bool {
    cpal::default_host().default_input_device().is_some()
}

/// Returns `true` if a speaker output device is available.
pub fn has_output_device() -> bool {
    cpal::default_host().default_output_device().is_some()
}
