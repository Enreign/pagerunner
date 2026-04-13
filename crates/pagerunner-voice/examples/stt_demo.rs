//! Demo: transcribe a WAV file using Whisper STT.
//!
//! Generate test audio:
//!   say -o /tmp/test_speech.aiff "Hello, can you summarize the top stories on Hacker News?"
//!   afconvert /tmp/test_speech.aiff /tmp/test_speech.wav -d LEI16 -f WAVE -c 1 --mix
//!
//! Run:
//!   cargo run -p pagerunner-voice --features whisper --example stt_demo -- /tmp/test_speech.wav
//!
//! First run downloads whisper-tiny (~75MB).

use std::time::Instant;

fn main() {
    let wav_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: stt_demo <path-to-wav>");
        eprintln!();
        eprintln!("Generate test audio:");
        eprintln!("  say -o /tmp/test.aiff \"Hello world\"");
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

    // Load whisper model
    println!("Loading whisper-tiny model (first run downloads ~75MB)...");
    let load_start = Instant::now();

    let stt =
        pagerunner_voice::WhisperStt::new("whisper-tiny").expect("Failed to load Whisper model");
    println!("Model loaded in {:.1}s", load_start.elapsed().as_secs_f32());
    println!();

    // Transcribe — feed all audio at once
    println!("Transcribing...");
    let transcribe_start = Instant::now();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let result = rt.block_on(async {
        use pagerunner_voice::SttProvider;
        // Feed audio in 1-second chunks to simulate streaming
        let chunk_size = 16000; // 1 second
        let mut last_text = None;
        for (i, chunk) in audio_16k.chunks(chunk_size).enumerate() {
            match stt.transcribe_chunk(chunk, 16000).await {
                Ok(Some(text)) => {
                    println!("  [chunk {}] \"{}\"", i + 1, text.trim());
                    last_text = Some(text);
                }
                Ok(None) => {
                    println!("  [chunk {}] (buffering...)", i + 1);
                }
                Err(e) => {
                    eprintln!("  [chunk {}] Error: {}", i + 1, e);
                }
            }
        }

        // Flush remaining
        match stt.flush().await {
            Ok(Some(text)) => {
                println!("  [flush] \"{}\"", text.trim());
                last_text = Some(text);
            }
            Ok(None) => {
                println!("  [flush] (nothing remaining)");
            }
            Err(e) => {
                eprintln!("  [flush] Error: {}", e);
            }
        }
        last_text
    });

    let elapsed = transcribe_start.elapsed();
    println!();
    println!("─────────────────────────────────────────");
    if let Some(text) = result {
        println!("Result: \"{}\"", text.trim());
    } else {
        println!("Result: (no transcription)");
    }
    println!(
        "Time: {:.2}s ({:.1}x realtime)",
        elapsed.as_secs_f32(),
        duration_secs / elapsed.as_secs_f32()
    );
}
