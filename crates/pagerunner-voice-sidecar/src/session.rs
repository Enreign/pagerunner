//! Voice session lifecycle — the main listen/act/speak loop.
//!
//! Wires mic audio through the [`VoicePipeline`] and dispatches recognised
//! utterances to the Pagerunner daemon via Unix socket.

use crate::audio;
use crate::narrator;
use anyhow::{Context, Result};
use pagerunner_voice::{PipelineConfig, TtsProvider, VoicePipeline};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Configuration for a voice session.
#[derive(Debug, Clone)]
pub struct VoiceSessionConfig {
    /// Chrome profile name for the agent.
    pub profile: String,
    /// Pipeline config (silence timeout, VAD chunk size, model).
    pub pipeline: PipelineConfig,
    /// VAD speech probability threshold.
    pub vad_threshold: f32,
    /// Optional wake word (unused in v1 — reserved for future).
    pub wake_word: Option<String>,
    /// Output structured JSON lines to stdout (for menu bar integration).
    pub json: bool,
    /// Voice mode: "always" (always listening) or "ptt" (push-to-talk).
    pub mode: String,
    /// Narration mode: "full", "summary", or "off".
    pub narration: String,
}

/// Stdin command from the menu bar (for PTT mode).
#[derive(Deserialize)]
struct StdinCommand {
    r#type: String,
}

/// Emit a structured JSON event to stdout (for `--json` mode).
fn emit_json(event_type: &str, data: serde_json::Value) {
    let line = serde_json::json!({"type": event_type, "data": data});
    println!("{}", line);
    use std::io::Write;
    std::io::stdout().flush().ok();
}

// ---------------------------------------------------------------------------
// Daemon socket client (thin inline impl — avoids importing the root crate)
// ---------------------------------------------------------------------------

const SOCKET_SUBPATH: &str = ".pagerunner/daemon.sock";

/// A minimal line-oriented client for the daemon Unix socket.
struct DaemonConn {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl DaemonConn {
    async fn connect() -> Result<Self> {
        let path = dirs::home_dir()
            .context("Cannot determine home directory")?
            .join(SOCKET_SUBPATH);
        let stream = UnixStream::connect(&path).await.map_err(|e| {
            anyhow::anyhow!(
                "Cannot connect to daemon at {}: {}. Is `pagerunner daemon` running?",
                path.display(),
                e
            )
        })?;
        let (r, w) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(r),
            writer: w,
        })
    }

    /// Send a JSON line.
    async fn send(&mut self, msg: &impl Serialize) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        Ok(())
    }

    /// Read one JSON line.
    async fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).await?;
        if n == 0 {
            anyhow::bail!("Daemon connection closed");
        }
        Ok(line)
    }
}

// Wire types matching ipc.rs (duplicated to avoid root crate dep).

#[derive(Serialize)]
struct AgentRunMsg {
    r#type: &'static str,
    id: String,
    goal: String,
    config: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct DaemonEventLine {
    run_id: String,
    event: serde_json::Value,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct DaemonResponse {
    id: String,
    result: Option<String>,
    error: Option<String>,
}

/// Classify an event JSON into a string tag. The daemon serialises
/// `AgentEvent` as `{"type": "variant_name", ...}` thanks to serde
/// `rename_all = "snake_case"` + tag.
fn event_type(v: &serde_json::Value) -> &str {
    v.get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown")
}

/// True if the transcribed text is an affirmative approval response.
fn is_approval(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    matches!(
        t.as_str(),
        "yes" | "yeah" | "yep" | "sure" | "go ahead" | "do it" | "okay" | "ok" | "approve"
    )
}

/// True if the transcribed text is a denial / interrupt.
fn is_denial(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    matches!(
        t.as_str(),
        "no" | "nope" | "stop" | "cancel" | "deny" | "don't" | "abort"
    )
}

// ---------------------------------------------------------------------------
// Main session loop
// ---------------------------------------------------------------------------

/// Run the voice session. This is the core event loop:
///
/// 1. Listen for speech via mic + VAD + STT.
/// 2. Send recognised goal to the daemon as `agent_run`.
/// 3. Stream agent events, narrate actions via TTS.
/// 4. Handle approval requests with yes/no voice input.
/// 5. Resume listening after the agent finishes.
pub async fn run_voice_session(config: VoiceSessionConfig) -> Result<()> {
    // -- Load models -------------------------------------------------------
    tracing::info!("Loading voice models...");

    let stt = pagerunner_voice::WhisperStt::new(&config.pipeline.stt_model)
        .map_err(|e| anyhow::anyhow!("Failed to load STT model: {}", e))?;
    let vad = pagerunner_voice::SileroVad::new(Some(config.vad_threshold))
        .map_err(|e| anyhow::anyhow!("Failed to load VAD model: {}", e))?;
    let tts = pagerunner_voice::PiperTts::new(None)
        .map_err(|e| anyhow::anyhow!("Failed to load TTS model: {}", e))?;

    let tts_rate = tts.sample_rate();
    let mut pipeline = VoicePipeline::with_config(stt, tts, vad, config.pipeline.clone());

    tracing::info!(
        stt = pipeline.stt_name(),
        tts = pipeline.tts_name(),
        "Voice models loaded"
    );

    // -- Open microphone ---------------------------------------------------
    let chunk_size = config.pipeline.vad_chunk_size;
    let (_mic_stream, mic_rx) = audio::open_mic(chunk_size)?;

    tracing::info!("Microphone open. Listening...");
    let is_ptt = config.mode == "ptt";
    if config.json {
        if is_ptt {
            emit_json("idle", serde_json::json!({}));
        } else {
            emit_json("listening", serde_json::json!({}));
        }
    } else {
        println!("Ready. Speak a command...");
    }

    // -- Stdin reader for PTT commands ------------------------------------
    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    if is_ptt {
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                match std::io::BufRead::read_line(&mut stdin.lock(), &mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let _ = stdin_tx.send(line.trim().to_string());
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // -- Main loop ---------------------------------------------------------
    let interrupted = Arc::new(AtomicBool::new(false));
    let json_mode = config.json;
    let narration_mode = config.narration.clone();

    loop {
        // In PTT mode, wait for start_listening command before capturing
        if is_ptt {
            loop {
                match stdin_rx.recv().await {
                    Some(line) => {
                        if let Ok(cmd) = serde_json::from_str::<StdinCommand>(&line) {
                            if cmd.r#type == "start_listening" {
                                if json_mode {
                                    emit_json("listening", serde_json::json!({}));
                                }
                                break;
                            }
                        }
                    }
                    None => return Ok(()), // Stdin closed
                }
            }
        }

        // 1. Listen for an utterance
        let goal = match listen_for_utterance(&mut pipeline, &mic_rx).await? {
            Some(text) => text,
            None => continue,
        };

        let trimmed = goal.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip common Whisper hallucination artefacts
        if is_whisper_noise(trimmed) {
            tracing::debug!(text = trimmed, "Skipping likely noise/hallucination");
            continue;
        }

        if json_mode {
            emit_json("utterance", serde_json::json!({"text": trimmed}));
        } else {
            println!("[You]: {}", trimmed);
        }
        tracing::info!(goal = trimmed, "Utterance captured");

        // 2. Send to daemon
        let mut conn = DaemonConn::connect().await?;
        let msg_id = uuid::Uuid::new_v4().to_string();
        conn.send(&AgentRunMsg {
            r#type: "agent_run",
            id: msg_id.clone(),
            goal: trimmed.to_string(),
            config: Some(serde_json::json!({
                "profile": config.profile,
            })),
        })
        .await?;

        // 3. Stream events
        interrupted.store(false, Ordering::Relaxed);
        let mut current_run_id: Option<String> = None;
        let _ = &current_run_id; // suppress unused-assignment warning for initial None

        loop {
            let line = conn.read_line().await?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Try parsing as a daemon event first
            if let Ok(ev) = serde_json::from_str::<DaemonEventLine>(line) {
                current_run_id = Some(ev.run_id.clone());
                let etype = event_type(&ev.event);

                // Emit JSON event for menu bar integration
                if json_mode {
                    emit_json("agent_event", serde_json::json!({
                        "event_type": etype,
                        "event": ev.event,
                    }));
                }

                // Narrate the event (respecting narration mode)
                let should_narrate = match narration_mode.as_str() {
                    "full" => true,
                    "summary" => etype == "done" || etype == "error",
                    _ => false, // "off"
                };

                if should_narrate {
                    if let Some(phrase) = narrator::narrate(etype, &ev.event) {
                        if json_mode {
                            emit_json("speaking", serde_json::json!({"text": &phrase}));
                        } else {
                            println!("[Agent]: {}", phrase);
                        }
                        speak_interruptible(&mut pipeline, &phrase, tts_rate, &mic_rx, &interrupted)
                            .await?;
                    }
                }

                // Handle approval requests
                if etype == "approval_required" {
                    if let Some(run_id) = &current_run_id {
                        if json_mode {
                            emit_json("approval", serde_json::json!({
                                "action": ev.event.get("action").and_then(|v| v.as_str()).unwrap_or("unknown"),
                                "description": ev.event.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                            }));
                        }
                        let response =
                            wait_for_approval(&mut pipeline, &mic_rx, &interrupted).await?;
                        let approve_msg = serde_json::json!({
                            "type": "agent_approve",
                            "id": uuid::Uuid::new_v4().to_string(),
                            "run_id": run_id,
                            "approved": response,
                        });
                        conn.send(&approve_msg).await?;
                        if json_mode {
                            emit_json("approval_response", serde_json::json!({"approved": response}));
                        } else if response {
                            println!("[You]: Approved.");
                        } else {
                            println!("[You]: Denied.");
                        }
                    }
                }

                // Terminal events end the agent run
                if matches!(
                    etype,
                    "done" | "error" | "interrupted" | "budget_exceeded"
                ) {
                    break;
                }

                continue;
            }

            // Try parsing as a final response (legacy format)
            if let Ok(resp) = serde_json::from_str::<DaemonResponse>(line) {
                if let Some(err) = resp.error {
                    tracing::error!(err, "Daemon error");
                    let phrase = format!("Error: {}", err);
                    if json_mode {
                        emit_json("speaking", serde_json::json!({"text": &phrase}));
                    } else {
                        println!("[Agent]: {}", phrase);
                    }
                    speak_interruptible(&mut pipeline, &phrase, tts_rate, &mic_rx, &interrupted)
                        .await?;
                }
                break;
            }

            tracing::warn!(line, "Unrecognised daemon message");
        }

        pipeline.reset_vad();

        // In PTT mode, drain any pending stop_listening commands
        if is_ptt {
            while stdin_rx.try_recv().is_ok() {}
        }

        if json_mode {
            if is_ptt {
                emit_json("idle", serde_json::json!({}));
            } else {
                emit_json("idle", serde_json::json!({}));
                // In always mode, we immediately go back to listening
                emit_json("listening", serde_json::json!({}));
            }
        } else {
            println!("Ready. Speak a command...");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Block until the pipeline captures a complete utterance from the mic.
async fn listen_for_utterance(
    pipeline: &mut VoicePipeline,
    mic_rx: &std::sync::mpsc::Receiver<Vec<f32>>,
) -> Result<Option<String>> {
    loop {
        // Block on the sync channel in a way that doesn't starve tokio.
        let chunk = tokio::task::block_in_place(|| {
            mic_rx
                .recv()
                .map_err(|_| anyhow::anyhow!("Mic channel closed"))
        })?;

        if let Some(text) = pipeline
            .process_audio(&chunk, audio::TARGET_SAMPLE_RATE)
            .await
            .map_err(|e| anyhow::anyhow!("Pipeline error: {}", e))?
        {
            return Ok(Some(text));
        }
    }
}

/// Speak a phrase via TTS while monitoring the mic for interruption.
///
/// If the user speaks during playback, sets `interrupted` to true and returns
/// early. The caller should then capture the new utterance.
async fn speak_interruptible(
    pipeline: &mut VoicePipeline,
    text: &str,
    sample_rate: u32,
    mic_rx: &std::sync::mpsc::Receiver<Vec<f32>>,
    interrupted: &Arc<AtomicBool>,
) -> Result<()> {
    let samples = pipeline
        .speak(text)
        .await
        .map_err(|e| anyhow::anyhow!("TTS error: {}", e))?;

    if samples.is_empty() {
        return Ok(());
    }

    // Play on a background thread so we can monitor VAD concurrently.
    let samples_clone = samples.clone();
    let playing = Arc::new(AtomicBool::new(true));
    let playing_bg = playing.clone();
    let interrupted_bg = interrupted.clone();

    let play_handle = std::thread::spawn(move || {
        if let Err(e) = audio::play_audio(&samples_clone, sample_rate) {
            tracing::error!(%e, "Playback error");
        }
        playing_bg.store(false, Ordering::Relaxed);
    });

    // Monitor mic for speech while playing
    while playing.load(Ordering::Relaxed) {
        match mic_rx.try_recv() {
            Ok(chunk) => {
                // Quick VAD check — if user is speaking, interrupt
                if let Ok(Some(_)) = pipeline
                    .process_audio(&chunk, audio::TARGET_SAMPLE_RATE)
                    .await
                    .map_err(|e| anyhow::anyhow!("VAD error during playback: {}", e))
                {
                    // User spoke a full utterance while we were playing — interrupt
                    interrupted_bg.store(true, Ordering::Relaxed);
                    break;
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }
    }

    // Wait for the playback thread to finish (it will if audio ends or we
    // just let it complete — we don't force-kill it since cpal streams drop
    // naturally).
    let _ = play_handle.join();

    Ok(())
}

/// Wait for the user to say "yes" or "no" for an approval prompt.
///
/// Returns `true` for approval, `false` for denial. Times out after 30 s
/// and returns `false`.
async fn wait_for_approval(
    pipeline: &mut VoicePipeline,
    mic_rx: &std::sync::mpsc::Receiver<Vec<f32>>,
    _interrupted: &Arc<AtomicBool>,
) -> Result<bool> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);

    loop {
        if tokio::time::Instant::now() >= deadline {
            tracing::info!("Approval timed out — denying");
            return Ok(false);
        }

        let chunk = tokio::task::block_in_place(|| {
            mic_rx
                .recv_timeout(std::time::Duration::from_millis(100))
                .ok()
        });

        if let Some(chunk) = chunk {
            if let Some(text) = pipeline
                .process_audio(&chunk, audio::TARGET_SAMPLE_RATE)
                .await
                .map_err(|e| anyhow::anyhow!("Pipeline error: {}", e))?
            {
                let trimmed = text.trim();
                println!("[You]: {}", trimmed);

                if is_approval(trimmed) {
                    return Ok(true);
                }
                if is_denial(trimmed) {
                    return Ok(false);
                }

                // Didn't understand — ask again
                tracing::debug!(text = trimmed, "Unrecognised approval response");
                println!("Say yes or no.");
            }
        }
    }
}

/// Returns `true` for known Whisper hallucination patterns (silence decoded
/// as subtitles, music notes, applause, etc.).
fn is_whisper_noise(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    // Common Whisper artefacts when decoding silence/noise
    t.is_empty()
        || t == "you"
        || t == "."
        || t.starts_with('[')
        || t.starts_with('(')
        || t.contains("subscribe")
        || t.contains("thank you for watching")
        || t.contains("thanks for watching")
        || t == "the"
        || t == "i"
        || (t.len() < 3 && !t.chars().any(|c| c.is_alphabetic()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_approval() {
        assert!(is_approval("yes"));
        assert!(is_approval("Yeah"));
        assert!(is_approval("  go ahead  "));
        assert!(is_approval("OK"));
        assert!(!is_approval("navigate"));
        assert!(!is_approval("no"));
    }

    #[test]
    fn test_is_denial() {
        assert!(is_denial("no"));
        assert!(is_denial("Stop"));
        assert!(is_denial("  cancel  "));
        assert!(!is_denial("yes"));
        assert!(!is_denial("navigate"));
    }

    #[test]
    fn test_is_whisper_noise() {
        assert!(is_whisper_noise(""));
        assert!(is_whisper_noise("[Music]"));
        assert!(is_whisper_noise("(applause)"));
        assert!(is_whisper_noise("you"));
        assert!(is_whisper_noise("Thank you for watching"));
        assert!(is_whisper_noise("The"));
        assert!(!is_whisper_noise("Navigate to google.com"));
        assert!(!is_whisper_noise("Click the login button"));
    }

    #[test]
    fn test_event_type_extraction() {
        let v = serde_json::json!({"type": "tool_call", "name": "navigate"});
        assert_eq!(event_type(&v), "tool_call");

        let v = serde_json::json!({"name": "navigate"});
        assert_eq!(event_type(&v), "unknown");
    }
}
