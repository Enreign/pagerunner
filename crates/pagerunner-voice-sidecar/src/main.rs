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
use session::{NarrationMode, VoiceMode, VoiceSessionConfig};

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

    /// Emit machine-readable JSON events on stdout.
    #[arg(long, default_value_t = false)]
    json: bool,

    /// Listening mode: always-on or push-to-talk.
    #[arg(long, default_value = "always")]
    mode: VoiceMode,

    /// Narration level for spoken agent updates.
    #[arg(long, default_value = "full")]
    narration: NarrationMode,
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

    // Validate threshold
    if !(0.0..=1.0).contains(&cli.vad_threshold) {
        anyhow::bail!("--vad-threshold must be between 0.0 and 1.0");
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
        json_output: cli.json,
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

    session::run_voice_session(session_config).await
}
