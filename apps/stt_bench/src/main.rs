use anyhow::{bail, Context, Result};
use audio_replay::load_wav_f32;
use speech_to_text::{AudioChunk, LocalSherpaBackend, SherpaConfig, SttBackend, SttEvent};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let mut args = std::env::args().skip(1);
    let wav = PathBuf::from(
        args.next()
            .context("usage: stt_bench <wav> <reference-text> [--realtime]")?,
    );
    let reference = args.next().context("missing reference text")?;
    let realtime = args.any(|arg| arg == "--realtime");
    let (sample_rate, channels, samples) = load_wav_f32(&wav)?;
    if sample_rate != 16_000 || channels != 1 {
        bail!(
            "{} must be 16 kHz mono (got {} Hz, {} channels)",
            wav.display(),
            sample_rate,
            channels
        );
    }

    let model_dir = std::env::var("MAGNOLIA_SHERPA_MODEL_DIR")
        .context("set MAGNOLIA_SHERPA_MODEL_DIR or the four explicit Sherpa paths")?;
    let model_dir = Path::new(&model_dir);
    let config = SherpaConfig {
        encoder: model_path(
            "MAGNOLIA_SHERPA_ENCODER",
            model_dir,
            "encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        )?,
        decoder: model_path(
            "MAGNOLIA_SHERPA_DECODER",
            model_dir,
            "decoder-epoch-99-avg-1-chunk-16-left-128.onnx",
        )?,
        joiner: model_path(
            "MAGNOLIA_SHERPA_JOINER",
            model_dir,
            "joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        )?,
        tokens: model_path("MAGNOLIA_SHERPA_TOKENS", model_dir, "tokens.txt")?,
        num_threads: std::env::var("MAGNOLIA_SHERPA_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2),
        endpointing: true,
    };

    let audio_duration = Duration::from_secs_f64(samples.len() as f64 / 16_000.0);
    let chunk_len = 16_000 * 160 / 1000;
    let mut backend = LocalSherpaBackend::new(config);
    backend.start("stt-bench")?;
    let wall_start = Instant::now();
    let mut first_partial = None;
    let mut first_final = None;
    let mut final_text = String::new();
    let mut previous_end = Duration::ZERO;

    for chunk in samples.chunks(chunk_len) {
        let timestamp = previous_end;
        previous_end += Duration::from_secs_f64(chunk.len() as f64 / 16_000.0);
        backend.push_audio(AudioChunk::mono_16khz(chunk.to_vec(), timestamp))?;
        let mut events = Vec::new();
        backend.poll_events(&mut events)?;
        for event in events {
            match event {
                SttEvent::Partial { text, .. } => {
                    if !text.trim().is_empty() && first_partial.is_none() {
                        first_partial = Some(wall_start.elapsed());
                    }
                }
                SttEvent::Final { text, .. } => {
                    record_final(&mut final_text, &mut first_final, &wall_start, text)
                }
                _ => {}
            }
        }
        if realtime {
            std::thread::sleep(Duration::from_millis(160));
        }
    }
    backend.finish_utterance()?;
    let mut events = Vec::new();
    backend.poll_events(&mut events)?;
    for event in events {
        if let SttEvent::Final { text, .. } = event {
            record_final(&mut final_text, &mut first_final, &wall_start, text);
        }
    }

    let elapsed = wall_start.elapsed();
    println!(
        "file={} audio_ms={} wall_ms={} rtf={:.3} first_partial_ms={} first_final_ms={} wer={:.3}",
        wav.display(),
        audio_duration.as_millis(),
        elapsed.as_millis(),
        elapsed.as_secs_f64() / audio_duration.as_secs_f64(),
        first_partial
            .map(|v| v.as_millis().to_string())
            .unwrap_or_else(|| "-".into()),
        first_final
            .map(|v| v.as_millis().to_string())
            .unwrap_or_else(|| "-".into()),
        word_error_rate(&reference, &final_text)
    );
    println!("reference={reference}");
    println!("hypothesis={final_text}");
    Ok(())
}

fn record_final(
    text: &mut String,
    first_final: &mut Option<Duration>,
    started: &Instant,
    segment: String,
) {
    if first_final.is_none() {
        *first_final = Some(started.elapsed());
    }
    if !segment.trim().is_empty() {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(segment.trim());
    }
}

fn model_path(var: &str, dir: &Path, file: &str) -> Result<PathBuf> {
    if let Ok(path) = std::env::var(var) {
        return Ok(path.into());
    }
    let path = dir.join(file);
    if !path.is_file() {
        bail!("missing model file: {}", path.display());
    }
    Ok(path)
}

fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let reference: Vec<_> = reference.split_whitespace().collect();
    let hypothesis: Vec<_> = hypothesis.split_whitespace().collect();
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    let mut row: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, expected) in reference.iter().enumerate() {
        let mut next = vec![i + 1; hypothesis.len() + 1];
        for (j, actual) in hypothesis.iter().enumerate() {
            next[j + 1] = if expected.eq_ignore_ascii_case(actual) {
                row[j]
            } else {
                1 + row[j].min(row[j + 1]).min(next[j])
            };
        }
        row = next;
    }
    row[hypothesis.len()] as f64 / reference.len() as f64
}

#[cfg(test)]
mod tests {
    use super::word_error_rate;
    #[test]
    fn computes_word_error_rate() {
        assert_eq!(word_error_rate("one two three", "one too three"), 1.0 / 3.0);
    }
}
