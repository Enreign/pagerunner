//! Demo: synthesize speech using Piper TTS and write to a WAV file.
//!
//! Run:
//!   cargo run -p pagerunner-voice --features piper-tts --example tts_demo -- "Hello, I am Pagerunner."
//!
//! First run downloads en_US-amy-low (~17MB).
//! Output: /tmp/piper_output.wav

use std::time::Instant;

fn main() {
    let text = std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ");

    if text.is_empty() {
        eprintln!("Usage: tts_demo <text to speak>");
        eprintln!();
        eprintln!("Example:");
        eprintln!("  cargo run -p pagerunner-voice --features piper-tts --example tts_demo -- \"Hello world\"");
        std::process::exit(1);
    }

    println!("Input text: \"{text}\"");
    println!();

    // Load Piper model
    println!("Loading Piper TTS model (first run downloads ~17MB)...");
    let load_start = Instant::now();

    let tts = pagerunner_voice::PiperTts::new(None).expect("Failed to load Piper TTS");

    println!(
        "Model loaded in {:.1}s (voice: {}, sample rate: {} Hz)",
        load_start.elapsed().as_secs_f32(),
        tts.voice_name(),
        {
            use pagerunner_voice::TtsProvider;
            tts.sample_rate()
        }
    );
    println!();

    // Synthesize
    println!("Synthesizing...");
    let synth_start = Instant::now();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let samples = rt.block_on(async {
        use pagerunner_voice::TtsProvider;
        tts.synthesize(&text).await
    })
    .expect("Synthesis failed");

    let synth_elapsed = synth_start.elapsed();
    let sample_rate = {
        use pagerunner_voice::TtsProvider;
        tts.sample_rate()
    };
    let duration_secs = samples.len() as f32 / sample_rate as f32;

    println!(
        "Generated {:.1}s of audio ({} samples at {} Hz)",
        duration_secs,
        samples.len(),
        sample_rate
    );
    println!(
        "Synthesis time: {:.2}s ({:.1}x realtime)",
        synth_elapsed.as_secs_f32(),
        duration_secs / synth_elapsed.as_secs_f32()
    );
    println!();

    // Write WAV
    let output_path = "/tmp/piper_output.wav";
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(output_path, spec).expect("Failed to create WAV file");
    for &sample in &samples {
        // Clamp and convert f32 [-1, 1] to i16
        let clamped = sample.clamp(-1.0, 1.0);
        let int_sample = (clamped * 32767.0) as i16;
        writer.write_sample(int_sample).expect("Failed to write sample");
    }
    writer.finalize().expect("Failed to finalize WAV");

    println!("Output written to: {output_path}");
    println!();
    println!("Play with: afplay {output_path}");
}
