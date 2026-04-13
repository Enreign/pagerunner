//! Voice session lifecycle — the main listen/act/speak loop.
//!
//! Wires mic audio through the [`VoicePipeline`] and dispatches recognised
//! utterances to the Pagerunner daemon via Unix socket.

use crate::audio;
use crate::narrator;
use anyhow::{Context, Result};
use pagerunner_voice::{PipelineConfig, TtsProvider, VoicePipeline};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum VoiceMode {
    #[value(name = "always")]
    AlwaysListening,
    #[value(name = "ptt")]
    PushToTalk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum NarrationMode {
    #[value(name = "full")]
    Full,
    #[value(name = "summary")]
    Summary,
    #[value(name = "off")]
    Off,
}

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
    /// Emit JSON events instead of human-readable lines.
    pub json_output: bool,
    /// Listening mode for microphone capture.
    pub mode: VoiceMode,
    /// Spoken narration verbosity.
    pub narration: NarrationMode,
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

#[derive(Debug)]
enum ControlCommand {
    StartListening,
    StopListening,
    Mute,
    Unmute,
    Speak(String),
}

#[derive(Debug, Deserialize)]
struct ControlEnvelope {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

/// Classify an event JSON into a string tag. The daemon serialises
/// `AgentEvent` as `{"type": "variant_name", ...}` thanks to serde
/// `rename_all = "snake_case"` + tag.
fn event_type(v: &serde_json::Value) -> &str {
    v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown")
}

fn emit_stdout(config: &VoiceSessionConfig, message: &str) {
    if !config.json_output {
        println!("{}", message);
    }
}

fn emit_json_event(config: &VoiceSessionConfig, event_type: &str, data: serde_json::Value) {
    if config.json_output {
        println!("{}", json!({ "type": event_type, "data": data }));
    }
}

fn effective_voice_status(config: &VoiceSessionConfig, listening: bool) -> &'static str {
    match (config.mode, listening) {
        (VoiceMode::PushToTalk, false) => "idle",
        _ => "listening",
    }
}

fn should_narrate(mode: NarrationMode, event_type: &str) -> bool {
    match mode {
        NarrationMode::Full => true,
        NarrationMode::Summary => matches!(event_type, "done" | "error" | "approval_required"),
        NarrationMode::Off => false,
    }
}

fn parse_control_command(line: &str) -> Option<ControlCommand> {
    let envelope = serde_json::from_str::<ControlEnvelope>(line).ok()?;
    match envelope.kind.as_str() {
        "start_listening" => Some(ControlCommand::StartListening),
        "stop_listening" => Some(ControlCommand::StopListening),
        "mute" => Some(ControlCommand::Mute),
        "unmute" => Some(ControlCommand::Unmute),
        "speak" => envelope.text.map(ControlCommand::Speak),
        _ => None,
    }
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

    let (control_tx, mut control_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut stdin = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(line)) = stdin.next_line().await {
            if let Some(cmd) = parse_control_command(&line) {
                let _ = control_tx.send(cmd);
            }
        }
    });

    tracing::info!("Microphone open. Listening...");
    if config.json_output {
        emit_json_event(
            &config,
            effective_voice_status(&config, config.mode == VoiceMode::AlwaysListening),
            json!({}),
        );
    } else {
        println!("Ready. Speak a command...");
    }

    let mut muted = false;

    // -- Main loop ---------------------------------------------------------
    loop {
        let mut listening = config.mode == VoiceMode::AlwaysListening;

        if config.mode == VoiceMode::PushToTalk {
            listening = wait_for_start_listening(
                &config,
                &mut pipeline,
                tts_rate,
                &mic_rx,
                &mut control_rx,
                &mut muted,
            )
            .await?;
        }

        // 1. Listen for an utterance
        let goal = match listen_for_utterance(
            &config,
            &mut pipeline,
            &mic_rx,
            &mut control_rx,
            &mut muted,
            &mut listening,
            tts_rate,
        )
        .await?
        {
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

        emit_stdout(&config, &format!("[You]: {}", trimmed));
        emit_json_event(&config, "utterance", json!({ "text": trimmed }));
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
        let mut current_run_id: Option<String> = None;
        let _ = &current_run_id; // suppress unused-assignment warning for initial None

        loop {
            let line = tokio::select! {
                maybe_cmd = control_rx.recv() => {
                    match maybe_cmd {
                        Some(ControlCommand::Mute) => {
                            muted = true;
                        }
                        Some(ControlCommand::Unmute) => {
                            muted = false;
                        }
                        Some(ControlCommand::Speak(text)) => {
                            emit_json_event(&config, "speaking", json!({ "text": text }));
                            if !muted {
                                speak_interruptible(&mut pipeline, &text, tts_rate, &mic_rx).await?;
                            }
                        }
                        Some(ControlCommand::StartListening | ControlCommand::StopListening) => {}
                        None => {}
                    }
                    continue;
                }
                line = conn.read_line() => line?,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Try parsing as a daemon event first
            if let Ok(ev) = serde_json::from_str::<DaemonEventLine>(line) {
                current_run_id = Some(ev.run_id.clone());
                let event_json = ev.event.clone();
                let etype = event_type(&event_json);

                emit_json_event(
                    &config,
                    "agent_event",
                    json!({
                        "run_id": ev.run_id,
                        "event": event_json.clone(),
                    }),
                );

                // Narrate the event
                if !muted && should_narrate(config.narration, etype) {
                    if let Some(phrase) = narrator::narrate(etype, &event_json) {
                        emit_stdout(&config, &format!("[Agent]: {}", phrase));
                        emit_json_event(&config, "speaking", json!({ "text": phrase }));
                        speak_interruptible(&mut pipeline, &phrase, tts_rate, &mic_rx).await?;
                    }
                }

                // Handle approval requests
                if etype == "approval_required" && !config.json_output {
                    if let Some(run_id) = &current_run_id {
                        let response = wait_for_approval(&mut pipeline, &mic_rx).await?;
                        let approve_msg = serde_json::json!({
                            "type": "agent_approve",
                            "id": uuid::Uuid::new_v4().to_string(),
                            "run_id": run_id,
                            "approved": response,
                        });
                        conn.send(&approve_msg).await?;
                        if response {
                            emit_stdout(&config, "[You]: Approved.");
                        } else {
                            emit_stdout(&config, "[You]: Denied.");
                        }
                    }
                }

                // Terminal events end the agent run
                if matches!(etype, "done" | "error" | "interrupted" | "budget_exceeded") {
                    break;
                }

                continue;
            }

            // Try parsing as a final response (legacy format)
            if let Ok(resp) = serde_json::from_str::<DaemonResponse>(line) {
                if let Some(err) = resp.error {
                    tracing::error!(err, "Daemon error");
                    let phrase = format!("Error: {}", err);
                    emit_json_event(&config, "error", json!({ "message": err }));
                    emit_stdout(&config, &format!("[Agent]: {}", phrase));
                    if !muted {
                        emit_json_event(&config, "speaking", json!({ "text": phrase }));
                        speak_interruptible(&mut pipeline, &phrase, tts_rate, &mic_rx).await?;
                    }
                }
                break;
            }

            tracing::warn!(line, "Unrecognised daemon message");
        }

        pipeline.reset_vad();
        emit_json_event(&config, effective_voice_status(&config, config.mode == VoiceMode::AlwaysListening), json!({}));
        if !config.json_output {
            println!("Ready. Speak a command...");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn wait_for_start_listening(
    config: &VoiceSessionConfig,
    pipeline: &mut VoicePipeline,
    sample_rate: u32,
    mic_rx: &std::sync::mpsc::Receiver<Vec<f32>>,
    control_rx: &mut mpsc::UnboundedReceiver<ControlCommand>,
    muted: &mut bool,
) -> Result<bool> {
    loop {
        match control_rx.recv().await {
            Some(ControlCommand::StartListening) => {
                emit_json_event(config, "listening", json!({}));
                return Ok(true);
            }
            Some(ControlCommand::Mute) => *muted = true,
            Some(ControlCommand::Unmute) => *muted = false,
            Some(ControlCommand::Speak(text)) => {
                emit_json_event(config, "speaking", json!({ "text": text }));
                if !*muted {
                    speak_interruptible(pipeline, &text, sample_rate, mic_rx).await?;
                }
                emit_json_event(config, "idle", json!({}));
            }
            Some(ControlCommand::StopListening) => {
                pipeline.reset_vad();
                emit_json_event(config, "idle", json!({}));
            }
            None => return Ok(false),
        }
    }
}

/// Block until the pipeline captures a complete utterance from the mic.
async fn listen_for_utterance(
    config: &VoiceSessionConfig,
    pipeline: &mut VoicePipeline,
    mic_rx: &std::sync::mpsc::Receiver<Vec<f32>>,
    control_rx: &mut mpsc::UnboundedReceiver<ControlCommand>,
    muted: &mut bool,
    listening: &mut bool,
    sample_rate: u32,
) -> Result<Option<String>> {
    loop {
        while let Ok(cmd) = control_rx.try_recv() {
            match cmd {
                ControlCommand::StartListening => {
                    *listening = true;
                }
                ControlCommand::StopListening => {
                    *listening = false;
                    pipeline.reset_vad();
                    emit_json_event(config, "idle", json!({}));
                    return Ok(None);
                }
                ControlCommand::Mute => *muted = true,
                ControlCommand::Unmute => *muted = false,
                ControlCommand::Speak(text) => {
                    emit_json_event(config, "speaking", json!({ "text": text }));
                    if !*muted {
                        speak_interruptible(pipeline, &text, sample_rate, mic_rx).await?;
                    }
                    emit_json_event(
                        config,
                        effective_voice_status(config, *listening),
                        json!({}),
                    );
                }
            }
        }

        if !*listening {
            pipeline.reset_vad();
            return Ok(None);
        }

        // Block on the sync channel in a way that doesn't starve tokio.
        let chunk = tokio::task::block_in_place(|| -> Result<Option<Vec<f32>>> {
            match mic_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(chunk) => Ok(Some(chunk)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    Err(anyhow::anyhow!("Mic channel closed"))
                }
            }
        })?;

        let chunk = match chunk {
            Some(chunk) => chunk,
            None => continue,
        };

        if let Some(text) = pipeline
            .process_audio(&chunk, audio::TARGET_SAMPLE_RATE)
            .await
            .map_err(|e| anyhow::anyhow!("Pipeline error: {}", e))?
        {
            return Ok(Some(text));
        }
    }
}

/// Speak a phrase via TTS, then clear any echoed mic audio before listening
/// again so the sidecar doesn't transcribe its own narration.
async fn speak_interruptible(
    pipeline: &mut VoicePipeline,
    text: &str,
    sample_rate: u32,
    mic_rx: &std::sync::mpsc::Receiver<Vec<f32>>,
) -> Result<()> {
    let samples = pipeline
        .speak(text)
        .await
        .map_err(|e| anyhow::anyhow!("TTS error: {}", e))?;

    if samples.is_empty() {
        return Ok(());
    }

    // Play on a background thread so we can keep draining the mic queue while
    // speaking. We intentionally avoid feeding playback audio into the STT
    // pipeline here; otherwise the sidecar can hear and transcribe itself.
    let samples_clone = samples.clone();
    let playing = Arc::new(AtomicBool::new(true));
    let playing_bg = playing.clone();

    let play_handle = std::thread::spawn(move || {
        if let Err(e) = audio::play_audio(&samples_clone, sample_rate) {
            tracing::error!(%e, "Playback error");
        }
        playing_bg.store(false, Ordering::Relaxed);
    });

    // Drain any mic input that arrives during playback so we don't build up a
    // backlog of our own narrated speech.
    while playing.load(Ordering::Relaxed) {
        match mic_rx.try_recv() {
            Ok(_) => {}
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

    pipeline.reset_vad();
    drain_mic_queue(mic_rx);
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    drain_mic_queue(mic_rx);

    Ok(())
}

/// Wait for the user to say "yes" or "no" for an approval prompt.
///
/// Returns `true` for approval, `false` for denial. Times out after 30 s
/// and returns `false`.
async fn wait_for_approval(
    pipeline: &mut VoicePipeline,
    mic_rx: &std::sync::mpsc::Receiver<Vec<f32>>,
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

fn drain_mic_queue(mic_rx: &std::sync::mpsc::Receiver<Vec<f32>>) {
    while mic_rx.try_recv().is_ok() {}
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

    #[test]
    fn test_parse_control_command() {
        assert!(matches!(
            parse_control_command(r#"{"type":"start_listening"}"#),
            Some(ControlCommand::StartListening)
        ));
        assert!(matches!(
            parse_control_command(r#"{"type":"stop_listening"}"#),
            Some(ControlCommand::StopListening)
        ));
        assert!(matches!(
            parse_control_command(r#"{"type":"mute"}"#),
            Some(ControlCommand::Mute)
        ));
        assert!(matches!(
            parse_control_command(r#"{"type":"unmute"}"#),
            Some(ControlCommand::Unmute)
        ));
        assert!(matches!(
            parse_control_command(r#"{"type":"speak","text":"Hello"}"#),
            Some(ControlCommand::Speak(text)) if text == "Hello"
        ));
        assert!(parse_control_command(r#"{"type":"speak"}"#).is_none());
    }

    #[test]
    fn test_should_narrate_by_mode() {
        assert!(should_narrate(NarrationMode::Full, "tool_call"));
        assert!(should_narrate(NarrationMode::Summary, "done"));
        assert!(should_narrate(NarrationMode::Summary, "approval_required"));
        assert!(!should_narrate(NarrationMode::Summary, "tool_call"));
        assert!(!should_narrate(NarrationMode::Off, "done"));
    }
}
