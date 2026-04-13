//! Full pipeline demo: VAD + STT + TTS end-to-end.
//!
//! Reads a WAV file, runs it through the complete VoicePipeline
//! (VAD-gated utterance buffering → Whisper STT → Piper TTS response),
//! then writes the TTS output to a WAV file.
//!
//! Generate test audio:
//!   say -o /tmp/test.aiff "Hello, can you summarize the top stories on Hacker News?"
//!   afconvert /tmp/test.aiff /tmp/test.wav -d LEI16 -f WAVE -c 1 --mix
//!
//! Run:
//!   cargo run -p pagerunner-voice --features whisper,silero-vad,piper-tts --example pipeline_demo -- /tmp/test.wav
//!
//! Or with a real human speech sample for better results.

use std::time::Instant;

fn main() {
    let wav_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: pipeline_demo <path-to-wav>");
        eprintln!();
        eprintln!("Generate test audio:");
        eprintln!("  say -o /tmp/test.aiff \"Hello, summarize the top stories on Hacker News\"");
        eprintln!("  afconvert /tmp/test.aiff /tmp/test.wav -d LEI16 -f WAVE -c 1 --mix");
        std::process::exit(1);
    });

    println!();
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║  Pagerunner Voice Pipeline — Full Benchmark              ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();

    // ── Load WAV ────────────────────────────────────────────────────
    println!("Loading: {wav_path}");
    let reader = hound::WavReader::open(&wav_path).expect("Failed to open WAV");
    let spec = reader.spec();
    println!(
        "Format: {}ch, {} Hz, {:?} {}bit",
        spec.channels, spec.sample_rate, spec.sample_format, spec.bits_per_sample
    );

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

    let mono: Vec<f32> = if spec.channels > 1 {
        samples
            .chunks(spec.channels as usize)
            .map(|c| c[0])
            .collect()
    } else {
        samples
    };

    let audio_16k: Vec<f32> = if spec.sample_rate != 16000 {
        let ratio = spec.sample_rate as f64 / 16000.0;
        let new_len = (mono.len() as f64 / ratio) as usize;
        (0..new_len)
            .map(|i| {
                let pos = i as f64 * ratio;
                let idx = pos as usize;
                let frac = pos - idx as f64;
                let a = mono.get(idx).copied().unwrap_or(0.0);
                let b = mono.get(idx + 1).copied().unwrap_or(a);
                a + (b - a) * frac as f32
            })
            .collect()
    } else {
        mono
    };

    let audio_duration = audio_16k.len() as f32 / 16000.0;
    println!(
        "Audio: {:.1}s ({} samples at 16kHz)",
        audio_duration,
        audio_16k.len()
    );
    println!();

    // ── Load models ─────────────────────────────────────────────────
    println!("─── Loading Models ───────────────────────────────────────");
    println!();

    let t = Instant::now();
    let vad = pagerunner_voice::SileroVad::new(Some(0.5)).expect("Failed to load Silero VAD");
    let vad_time = t.elapsed();
    println!(
        "  ✓ Silero VAD     {:>6.1}s  (~2MB model)",
        vad_time.as_secs_f32()
    );

    let t = Instant::now();
    let stt =
        pagerunner_voice::WhisperStt::new("whisper-tiny").expect("Failed to load Whisper STT");
    let stt_time = t.elapsed();
    println!(
        "  ✓ Whisper STT    {:>6.1}s  (~77MB model, whisper-tiny)",
        stt_time.as_secs_f32()
    );

    let t = Instant::now();
    let tts =
        pagerunner_voice::PiperTts::new(Some("en_US-amy-low")).expect("Failed to load Piper TTS");
    let tts_time = t.elapsed();
    println!(
        "  ✓ Piper TTS      {:>6.1}s  (~17MB model, en_US-amy-low)",
        tts_time.as_secs_f32()
    );

    let total_load = vad_time + stt_time + tts_time;
    println!();
    println!("  Total load time: {:.1}s", total_load.as_secs_f32());
    println!();

    // ── Build pipeline ──────────────────────────────────────────────
    let config = pagerunner_voice::PipelineConfig::default();
    println!(
        "Pipeline config: silence_timeout={:.1}s, vad_chunk={}, model={}",
        config.silence_timeout_secs, config.vad_chunk_size, config.stt_model
    );
    println!();

    let mut pipeline = pagerunner_voice::VoicePipeline::with_config(stt, tts, vad, config);

    // ── Process audio ───────────────────────────────────────────────
    println!("─── Processing Audio ─────────────────────────────────────");
    println!();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let chunk_size = 512; // 32ms at 16kHz — matches VAD expectation
    let mut utterances: Vec<(f32, f32, String)> = Vec::new(); // (start, end, text)
    let mut speech_start: Option<usize> = None;

    let process_start = Instant::now();

    rt.block_on(async {
        for (i, chunk) in audio_16k.chunks(chunk_size).enumerate() {
            let sample_offset = i * chunk_size;
            let time_sec = sample_offset as f32 / 16000.0;

            // Track speech start
            if pipeline.is_speaking() && speech_start.is_none() {
                // Will be set after first speech detection
            }

            match pipeline.process_audio(chunk, 16000).await {
                Ok(Some(text)) => {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        let start = speech_start.map(|s| s as f32 / 16000.0).unwrap_or(0.0);
                        println!("  [{:5.1}s → {:5.1}s] \"{}\"", start, time_sec, text);
                        utterances.push((start, time_sec, text));
                    }
                    speech_start = None;
                }
                Ok(None) => {
                    if pipeline.is_speaking() && speech_start.is_none() {
                        speech_start = Some(sample_offset);
                    }
                }
                Err(e) => eprintln!("  Error: {e}"),
            }
        }

        // Flush any remaining audio
        match pipeline.flush_stt().await {
            Ok(Some(text)) => {
                let text = text.trim().to_string();
                if !text.is_empty() {
                    let start = speech_start.map(|s| s as f32 / 16000.0).unwrap_or(0.0);
                    println!(
                        "  [{:5.1}s → {:5.1}s] \"{}\" (flushed)",
                        start, audio_duration, text
                    );
                    utterances.push((start, audio_duration, text));
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("  Flush error: {e}"),
        }
    });

    let process_time = process_start.elapsed();
    println!();

    // ── STT Results ─────────────────────────────────────────────────
    println!("─── STT Results ──────────────────────────────────────────");
    println!();
    if utterances.is_empty() {
        println!("  (no utterances detected)");
    } else {
        let full_text: String = utterances
            .iter()
            .map(|(_, _, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        println!("  Full transcription:");
        println!("  \"{}\"", full_text);
        println!();
        println!("  {} utterance(s) detected", utterances.len());
    }
    println!();

    // ── TTS benchmark ───────────────────────────────────────────────
    println!("─── TTS Benchmark ────────────────────────────────────────");
    println!();

    let test_texts = [
        "I'll navigate to Hacker News for you.",
        "The page is loading.",
        "Here are the top stories.",
        "Navigating...",
        "Done.",
    ];

    let mut total_tts_audio = 0.0f32;
    let mut total_tts_time = std::time::Duration::ZERO;

    rt.block_on(async {
        for text in &test_texts {
            let t = Instant::now();
            match pipeline.speak(text).await {
                Ok(samples) => {
                    let elapsed = t.elapsed();
                    let audio_secs = samples.len() as f32 / pipeline.tts_sample_rate() as f32;
                    total_tts_audio += audio_secs;
                    total_tts_time += elapsed;
                    println!(
                        "  \"{}\"\n    → {:.1}s audio in {:.0}ms ({:.1}x RT)",
                        text,
                        audio_secs,
                        elapsed.as_millis(),
                        audio_secs / elapsed.as_secs_f32()
                    );
                }
                Err(e) => eprintln!("  TTS error: {e}"),
            }
        }

        // Test cache hit (second call to same text)
        let t = Instant::now();
        let _ = pipeline.speak("Navigating...").await;
        let cached_time = t.elapsed();
        println!();
        println!(
            "  Cache hit: \"Navigating...\" → {:.0}ms (vs first call above)",
            cached_time.as_millis()
        );
    });

    println!();

    // ── Summary ─────────────────────────────────────────────────────
    println!("─── Summary ──────────────────────────────────────────────");
    println!();
    println!("  Input audio:     {:.1}s", audio_duration);
    println!(
        "  VAD + STT time:  {:.2}s ({:.1}x realtime)",
        process_time.as_secs_f32(),
        audio_duration / process_time.as_secs_f32()
    );
    println!(
        "  TTS total:       {:.1}s audio in {:.0}ms ({:.1}x realtime)",
        total_tts_audio,
        total_tts_time.as_millis(),
        total_tts_audio / total_tts_time.as_secs_f32()
    );
    println!(
        "  Model load:      {:.1}s (one-time)",
        total_load.as_secs_f32()
    );
    println!();

    // Write first TTS output to file for playback
    rt.block_on(async {
        if let Ok(samples) = pipeline
            .speak("Hello, I am the Pagerunner voice agent. How can I help you?")
            .await
        {
            let out_path = "/tmp/pipeline_tts_output.wav";
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: pipeline.tts_sample_rate(),
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            if let Ok(mut writer) = hound::WavWriter::create(out_path, spec) {
                for s in &samples {
                    let _ = writer.write_sample(*s);
                }
                let _ = writer.finalize();
                println!("  TTS sample written to: {out_path}");
                println!("  Play: afplay {out_path}");
            }
        }
    });
    println!();
}
