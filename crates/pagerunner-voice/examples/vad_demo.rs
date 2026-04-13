//! Demo: detect speech segments in a WAV file using Silero VAD.
//!
//! Generate test audio:
//!   say -o /tmp/test_speech.aiff "Hello, can you summarize the top stories on Hacker News?"
//!   afconvert /tmp/test_speech.aiff /tmp/test_speech.wav -d LEI16 -f WAVE -c 1 --mix
//!
//! Run:
//!   cargo run -p pagerunner-voice --features silero-vad --example vad_demo -- /tmp/test_speech.wav
//!
//! First run downloads the Silero VAD model (~2MB).

use std::time::Instant;

fn main() {
    let wav_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: vad_demo <path-to-wav>");
        eprintln!();
        eprintln!("Generate test audio:");
        eprintln!(
            "  say -o /tmp/test.aiff \"Hello world, this is a test of voice activity detection.\""
        );
        eprintln!("  afconvert /tmp/test.aiff /tmp/test.wav -d LEI16 -f WAVE -c 1 --mix");
        std::process::exit(1);
    });

    println!("Loading WAV: {wav_path}");

    // Read WAV file
    let reader = hound::WavReader::open(&wav_path).expect("Failed to open WAV file");
    let spec = reader.spec();
    println!(
        "Format: {} channels, {} Hz, {:?} {}bit",
        spec.channels, spec.sample_rate, spec.sample_format, spec.bits_per_sample
    );

    // Convert to f32 mono
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
    };

    // If stereo, take left channel only
    let mono: Vec<f32> = if spec.channels > 1 {
        samples
            .chunks(spec.channels as usize)
            .map(|chunk| chunk[0])
            .collect()
    } else {
        samples
    };

    // Resample to 16kHz if needed (simple linear interpolation)
    let audio_16k: Vec<f32> = if spec.sample_rate != 16000 {
        println!("Resampling from {} Hz to 16000 Hz...", spec.sample_rate);
        let ratio = spec.sample_rate as f64 / 16000.0;
        let new_len = (mono.len() as f64 / ratio) as usize;
        (0..new_len)
            .map(|i| {
                let src_pos = i as f64 * ratio;
                let idx = src_pos as usize;
                let frac = src_pos - idx as f64;
                let a = mono.get(idx).copied().unwrap_or(0.0);
                let b = mono.get(idx + 1).copied().unwrap_or(a);
                a + (b - a) * frac as f32
            })
            .collect()
    } else {
        mono
    };

    let duration_secs = audio_16k.len() as f32 / 16000.0;
    println!(
        "Audio: {:.1}s ({} samples at 16kHz)",
        duration_secs,
        audio_16k.len()
    );
    println!();

    // Load Silero VAD model
    println!("Loading Silero VAD model (first run downloads ~2MB)...");
    let load_start = Instant::now();

    let threshold = std::env::var("VAD_THRESHOLD")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.5);
    println!("Threshold: {threshold} (set VAD_THRESHOLD=0.3 to adjust)");

    let mut vad =
        pagerunner_voice::SileroVad::new(Some(threshold)).expect("Failed to load Silero VAD model");
    println!("Model loaded in {:.1}s", load_start.elapsed().as_secs_f32());
    println!();

    // Process audio in 512-sample chunks (32ms at 16kHz)
    let chunk_size = 512;
    let mut was_speaking = false;
    let mut speech_start: Option<f32> = None;
    let mut segments: Vec<(f32, f32)> = Vec::new();

    let process_start = Instant::now();

    let verbose = std::env::var("VERBOSE").is_ok();

    for (i, chunk) in audio_16k.chunks(chunk_size).enumerate() {
        use pagerunner_voice::VadDetector;

        let is_speech = vad.process(chunk, 16000);
        let time_sec = i as f32 * chunk_size as f32 / 16000.0;

        if verbose {
            let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
            println!("  [{:6.2}s] speech={} rms={:.4}", time_sec, is_speech, rms);
        }

        if is_speech && !was_speaking {
            // Speech started
            speech_start = Some(time_sec);
            println!("  [{:6.2}s] SPEECH START", time_sec);
        } else if !is_speech && was_speaking {
            // Speech ended
            if let Some(start) = speech_start.take() {
                segments.push((start, time_sec));
                println!(
                    "  [{:6.2}s] SPEECH END   (duration: {:.2}s)",
                    time_sec,
                    time_sec - start
                );
            }
        }

        was_speaking = is_speech;
    }

    // Close any open segment
    if was_speaking {
        if let Some(start) = speech_start {
            segments.push((start, duration_secs));
            println!(
                "  [{:6.2}s] SPEECH END   (duration: {:.2}s, at file end)",
                duration_secs,
                duration_secs - start
            );
        }
    }

    let elapsed = process_start.elapsed();
    println!();
    println!("─────────────────────────────────────────");
    println!("Speech segments: {}", segments.len());
    for (i, (start, end)) in segments.iter().enumerate() {
        println!(
            "  Segment {}: {:.2}s – {:.2}s ({:.2}s)",
            i + 1,
            start,
            end,
            end - start
        );
    }

    let total_speech: f32 = segments.iter().map(|(s, e)| e - s).sum();
    println!();
    println!(
        "Total speech: {:.2}s / {:.2}s ({:.0}%)",
        total_speech,
        duration_secs,
        total_speech / duration_secs * 100.0
    );
    println!(
        "Processing time: {:.2}s ({:.0}x realtime)",
        elapsed.as_secs_f32(),
        duration_secs / elapsed.as_secs_f32()
    );
}
