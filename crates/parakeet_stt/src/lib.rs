use anyhow::Context;
use features::{FeatureConfig, LogMelExtractor};
use parakeet_trt::{ParakeetSessionSafe, TranscriptionEvent};
use std::path::Path;

fn wav_to_f32_mono_16k(path: &Path) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("open wav: {}", path.display()))?;
    let spec = reader.spec();

    if spec.sample_rate != 16000 || spec.channels != 1 {
        anyhow::bail!(
            "WAV must be 16kHz mono (got {} Hz, {} channels)",
            spec.sample_rate,
            spec.channels
        );
    }

    let audio: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            if bits == 0 || bits > 32 {
                anyhow::bail!("Unsupported PCM bit depth: {}", bits);
            }
            let denom = (1u64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / denom)
                .collect()
        }
    };

    Ok(audio)
}

fn to_bct(feat_tc: &[f32], frames: usize, n_mels: usize) -> Vec<f32> {
    // Input is [T, C] (frame-major), output is [C, T] (mel-major) to match encoder [B,C,T].
    let mut out = vec![0.0f32; n_mels * frames];
    for t in 0..frames {
        for m in 0..n_mels {
            out[m * frames + t] = feat_tc[t * n_mels + m];
        }
    }
    out
}

/// Offline WAV transcription (one utterance).
///
/// Notes:
/// - The current TensorRT engine profiles in `parakeet` are built for **T <= 256** frames
///   (~2.56s at 10ms hop). Longer audio should be chunked (not implemented yet).
pub fn transcribe_wav(model_dir: &Path, wav_path: &Path, device_id: i32) -> anyhow::Result<String> {
    let audio = wav_to_f32_mono_16k(wav_path)?;

    let config = FeatureConfig::default();
    let extractor = LogMelExtractor::new(config);
    let n_mels = extractor.n_mels();

    let features_tc = extractor.compute(&audio);
    let num_frames = features_tc.len() / n_mels;
    let features_bct = to_bct(&features_tc, num_frames, n_mels);

    let session = ParakeetSessionSafe::new(model_dir.to_str().context("model_dir not utf-8")?, device_id, true)?;
    session.push_features(&features_bct, num_frames)?;

    while let Some(ev) = session.poll_event() {
        match ev {
            TranscriptionEvent::FinalText { text, .. } => return Ok(text),
            TranscriptionEvent::Error { message } => anyhow::bail!("Parakeet error: {}", message),
            _ => {}
        }
    }

    Ok(String::new())
}


