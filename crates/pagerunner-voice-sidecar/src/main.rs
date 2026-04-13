//! `pagerunner-voice` — voice-controlled Pagerunner agent.
//!
//! A standalone sidecar binary that connects microphone input to the
//! Pagerunner daemon. Speak a goal, the agent acts, and results are
//! narrated back through the speaker.

mod audio;
mod narrator;
mod session;

use clap::Parser;
use pagerunner_voice::PipelineConfig;
use session::VoiceSessionConfig;
use std::fs::File;
use std::io::Write;

#[derive(Parser)]
#[command(
    name = "pagerunner-voice",
    about = "Voice-controlled Pagerunner agent",
    version
)]
struct Cli {
    /// Chrome profile to use.
    #[arg(long, default_value = "personal")]
    profile: String,

    /// Whisper model name (whisper-tiny or whisper-base).
    #[arg(long, default_value = "whisper-tiny")]
    model: String,

    /// VAD speech probability threshold (0.0 - 1.0).
    #[arg(long, default_value = "0.5")]
    vad_threshold: f32,

    /// Wake word — if set, only activate after hearing this phrase.
    /// (Not yet implemented in v1.)
    #[arg(long)]
    wake_word: Option<String>,

    /// Silence timeout in seconds before ending an utterance.
    #[arg(long, default_value = "0.3")]
    silence_timeout: f32,

    /// Output events as JSON lines to stdout (for integration with menu bar).
    #[arg(long)]
    json: bool,

    /// Voice mode: "ptt" (push-to-talk) or "always" (always listening).
    #[arg(long, default_value = "always")]
    mode: String,

    /// Narration: "full" (narrate every action), "summary" (clean summary at end), "off".
    #[arg(long, default_value = "summary")]
    narration: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise tracing (respects RUST_LOG env var).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Acquire instance lock — prevents multiple sidecars fighting over the mic
    let _lock = acquire_instance_lock()?;

    // Validate threshold
    if !(0.0..=1.0).contains(&cli.vad_threshold) {
        anyhow::bail!("--vad-threshold must be between 0.0 and 1.0");
    }

    // Validate mode
    if !matches!(cli.mode.as_str(), "always" | "ptt") {
        anyhow::bail!("--mode must be 'always' or 'ptt'");
    }

    // Validate narration
    if !matches!(cli.narration.as_str(), "full" | "summary" | "off") {
        anyhow::bail!("--narration must be 'full', 'summary', or 'off'");
    }

    // Check audio devices
    if !audio::has_input_device() {
        anyhow::bail!("No microphone found. Please connect an audio input device.");
    }
    if !audio::has_output_device() {
        anyhow::bail!("No speaker found. Please connect an audio output device.");
    }

    let pipeline_config = PipelineConfig {
        silence_timeout_secs: cli.silence_timeout,
        stt_model: cli.model.clone(),
        ..PipelineConfig::default()
    };

    let session_config = VoiceSessionConfig {
        profile: cli.profile,
        pipeline: pipeline_config,
        vad_threshold: cli.vad_threshold,
        wake_word: cli.wake_word,
        json: cli.json,
        mode: cli.mode,
        narration: cli.narration,
    };

    if session_config.wake_word.is_some() {
        tracing::warn!("Wake word is not yet implemented in v1 — always listening.");
    }

    tracing::info!(
        profile = session_config.profile,
        model = cli.model,
        vad_threshold = cli.vad_threshold,
        "Starting voice session"
    );

    if let Err(e) = session::run_voice_session(session_config).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }

    Ok(())
}

/// Acquire an exclusive file lock to prevent multiple voice sidecar instances.
///
/// The returned `File` must be held for the lifetime of the process — the lock
/// is released automatically when the file handle is dropped.
fn acquire_instance_lock() -> anyhow::Result<File> {
    let lock_path = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
        .join(".pagerunner/voice.lock");

    // Ensure parent directory exists
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(&lock_path)?;

    // Try to get an exclusive non-blocking lock
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let result = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };

    if result != 0 {
        anyhow::bail!("Another pagerunner-voice instance is already running. Stop it first.");
    }

    // Write PID for diagnostics
    let mut f = file;
    writeln!(f, "{}", std::process::id())?;

    Ok(f)
}
