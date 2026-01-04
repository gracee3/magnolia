use async_trait::async_trait;
use crossbeam_channel::{Receiver, Sender, SendTimeoutError, TrySendError};
use features::{FeatureConfig, LogMelExtractor};
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use magnolia_core::{DataType, ModuleSchema, Port, PortDirection, Processor, Signal};

use realfft::num_complex;

#[cfg(feature = "tile-rendering")]
pub mod tile;
pub mod transcription;

pub use transcription::{TranscriptConfig, TranscriptionSink, TranscriptionState};

/// Simple offline WAV transcription for the demo binary.
pub fn transcribe_wav(
    model_dir: &std::path::Path,
    wav_path: &std::path::Path,
    device_id: i32,
) -> anyhow::Result<String> {
    let mut reader = hound::WavReader::open(wav_path)?;
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

    let config = FeatureConfig::default();
    let extractor = LogMelExtractor::new(config);
    let n_mels = extractor.n_mels();
    let features_tc = extractor.compute(&audio);
    let num_frames = features_tc.len() / n_mels;
    if num_frames == 0 {
        return Ok(String::new());
    }

    let mut features_bct = vec![0.0f32; n_mels * num_frames];
    for t in 0..num_frames {
        for m in 0..n_mels {
            features_bct[m * num_frames + t] = features_tc[t * n_mels + m];
        }
    }

    let model_dir = model_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("model_dir must be valid UTF-8"))?;
    let session = parakeet_trt::ParakeetSessionSafe::new(model_dir, device_id, true)?;
    session.push_features(&features_bct, num_frames)?;

    let mut out = String::new();
    while let Some(event) = session.poll_event() {
        match event {
            parakeet_trt::TranscriptionEvent::FinalText { text, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&text);
            }
            parakeet_trt::TranscriptionEvent::Error { message } => {
                anyhow::bail!("Inference error: {}", message);
            }
            _ => {}
        }
    }

    Ok(out)
}

// #region agent log
fn dbglog(hypothesis_id: &str, location: &str, message: &str, data: serde_json::Value) {
    let payload = serde_json::json!({
        "sessionId": "debug-session",
        "runId": "pre-fix",
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/home/emmy/git/parakeet/.cursor/debug.log")
    {
        let _ = writeln!(f, "{}", payload.to_string());
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn should_log_throttled(last_ms: &AtomicU64, interval_ms: u64) -> bool {
    let now = now_ms();
    let prev = last_ms.load(Ordering::Relaxed);
    if now.saturating_sub(prev) < interval_ms {
        return false;
    }
    last_ms
        .compare_exchange(prev, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

static DBG_STT_AUDIO_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_OUT_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_AUDIO_DROP_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_PUSH_OK_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_PUSH_ERR_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_GATE_FLIP_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_WORKER_EV_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_CHUNK_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_RMS_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_RMS_ACTIVE_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_RMS_SILENT_N: AtomicU64 = AtomicU64::new(0);
static DBG_STT_AUDIO_RX_MS: AtomicU64 = AtomicU64::new(0);
static DBG_STT_FEATURE_MS: AtomicU64 = AtomicU64::new(0);
static DBG_STT_ENQUEUE_MS: AtomicU64 = AtomicU64::new(0);
// #endregion

static BLANK_PENALTY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn with_blank_penalty_delta<F: FnOnce()>(delta: f32, f: F) {
    if !delta.is_finite() || delta == 0.0 {
        f();
        return;
    }
    let lock = BLANK_PENALTY_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().unwrap();
    let prev = std::env::var("PARAKEET_BLANK_PENALTY").ok();
    let base = prev
        .as_deref()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0);
    let next = base + delta;
    std::env::set_var("PARAKEET_BLANK_PENALTY", next.to_string());
    f();
    match prev {
        Some(v) => std::env::set_var("PARAKEET_BLANK_PENALTY", v),
        None => std::env::remove_var("PARAKEET_BLANK_PENALTY"),
    }
}

// =============================================================================
// Event contract (1C)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttEvent {
    pub schema_version: u32,
    pub kind: String, // "partial" | "final" | "error"
    pub seq: u64,

    #[serde(default)]
    pub utterance_seq: u64,

    #[serde(default)]
    pub t_ms: u64,

    #[serde(default)]
    pub text: String,

    #[serde(default)]
    pub stable_prefix_len: usize,

    #[serde(default)]
    pub t_capture_end_ms: u64,

    #[serde(default)]
    pub t_dsp_done_ms: u64,

    #[serde(default)]
    pub t_infer_done_ms: u64,

    #[serde(default)]
    pub q_audio_len: usize,

    #[serde(default)]
    pub q_audio_age_ms: u64,

    #[serde(default)]
    pub q_staging_samples: usize,

    #[serde(default)]
    pub q_staging_ms: u64,

    #[serde(default)]
    pub code: i32,

    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SttTrace {
    pub t_capture_end_ms: u64,
    pub t_dsp_done_ms: u64,
    pub t_infer_done_ms: u64,
    pub q_audio_len: usize,
    pub q_audio_age_ms: u64,
    pub q_staging_samples: usize,
    pub q_staging_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttMetrics {
    pub schema_version: u32,
    pub kind: String, // "metrics"
    pub seq: u64,

    #[serde(default)]
    pub utterance_seq: u64,

    pub t_ms: u64,
    pub latency_ms: u64,
    pub decode_ms: u64,
    pub rtf: f32,
    pub audio_rms: f32,
    pub audio_peak: f32,
}

impl SttMetrics {
    pub fn to_signal(self) -> Signal {
        Signal::Computed {
            source: "stt_metrics".to_string(),
            content: serde_json::to_string(&self).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopStats {
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    #[serde(default)]
    pub utterance_seq: u64,
    pub staging_samples: usize,
    pub queued_before: usize,
    pub queued_after: usize,
    pub offline_frames: usize,
    pub tail_flush_decodes: usize,
    pub post_stop_decode_iters: usize,
    pub post_stop_events: u32,
    pub final_blank_penalty_delta: f32,
    pub emitted_partial_pre: u64,
    pub emitted_partial_post: u64,
    pub emitted_final_pre: u64,
    pub emitted_final_post: u64,
    pub emitted_final_empty: u64,
    pub emitted_final_nonempty: u64,
    pub suppressed_junk_partial: u64,
    pub suppressed_junk_final: u64,
    pub filter_junk: bool,
    pub offline_mode: bool,
    pub audio_chunks_seen: u64,
    pub audio_samples_seen: u64,
    pub audio_samples_resampled: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_reset_start_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_reset_done_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_first_audio_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_first_feature_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_first_decode_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_first_partial_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_first_final_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_stop_received_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_finalize_start_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_finalize_done_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub t_stop_stats_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub finalize_ms: Option<u64>,
    #[serde(default)]
    pub slow_chunk_threshold_ms: u64,
    #[serde(default)]
    pub slow_chunk_count: u64,
    #[serde(default)]
    pub slowest_chunk_ms: u64,
    #[serde(default)]
    pub slowest_chunk_idx: u64,
    #[serde(default)]
    pub slowest_chunk_audio_idx: u64,
    #[serde(default)]
    pub last_audio_chunk_idx: u64,
    #[serde(default)]
    pub last_feature_chunk_idx: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub abort_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetAck {
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    #[serde(default)]
    pub utterance_seq: u64,
    #[serde(default)]
    pub drained_audio: usize,
    #[serde(default)]
    pub ctrl_queued: usize,
    #[serde(default)]
    pub offline_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowChunk {
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    #[serde(default)]
    pub utterance_seq: u64,
    pub feature_idx: u64,
    pub audio_chunk_idx: u64,
    pub decode_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub queue_ms: Option<u64>,
    pub enc_shape: String,
    pub length_shape: String,
    pub profile_idx: u32,
    pub post_stop: bool,
    pub offline_mode: bool,
}

impl SlowChunk {
    pub fn to_signal(self) -> Signal {
        Signal::Computed {
            source: "stt_slow_chunk".to_string(),
            content: serde_json::to_string(&self).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

impl ResetAck {
    pub fn to_signal(self) -> Signal {
        Signal::Computed {
            source: "stt_reset_ack".to_string(),
            content: serde_json::to_string(&self).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkAck {
    pub schema_version: u32,
    #[serde(default)]
    pub utterance_seq: u64,
    #[serde(default)]
    pub chunk_idx: u64,
}

impl ChunkAck {
    pub fn to_signal(self) -> Signal {
        Signal::Computed {
            source: "stt_chunk_ack".to_string(),
            content: serde_json::to_string(&self).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

impl StopStats {
    pub fn to_signal(self) -> Signal {
        Signal::Computed {
            source: "stt_stop_stats".to_string(),
            content: serde_json::to_string(&self).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ParakeetSttState {
    pub status: String,
    pub latency_ms: u64,
    pub decode_ms: u64,
    pub rtf: f32,
    pub last_seq: u64,
}

#[derive(Debug, Clone)]
pub struct ParakeetRuntimeConfig {
    pub model_dir: String,
    pub device_id: i32,
    pub use_fp16: bool,
    pub encoder_override_path: Option<String>,
    pub use_streaming_encoder: bool,
    pub chunk_frames: usize,
    pub advance_frames: usize,
}

impl ParakeetRuntimeConfig {
    pub fn is_streaming_encoder(&self) -> bool {
        if self.use_streaming_encoder {
            return true;
        }
        self.encoder_override_path
            .as_deref()
            .map(|path| path.contains("encoder_streaming"))
            .unwrap_or(false)
    }
}

impl SttEvent {
    pub fn partial(text: String, stable_prefix_len: usize, t_ms: u64, seq: u64) -> Self {
        Self {
            schema_version: 1,
            kind: "partial".to_string(),
            seq,
            utterance_seq: 0,
            t_ms,
            text,
            stable_prefix_len,
            t_capture_end_ms: 0,
            t_dsp_done_ms: 0,
            t_infer_done_ms: 0,
            q_audio_len: 0,
            q_audio_age_ms: 0,
            q_staging_samples: 0,
            q_staging_ms: 0,
            code: 0,
            message: String::new(),
        }
    }

    pub fn final_(text: String, t_ms: u64, seq: u64) -> Self {
        Self {
            schema_version: 1,
            kind: "final".to_string(),
            seq,
            utterance_seq: 0,
            t_ms,
            text,
            stable_prefix_len: 0,
            t_capture_end_ms: 0,
            t_dsp_done_ms: 0,
            t_infer_done_ms: 0,
            q_audio_len: 0,
            q_audio_age_ms: 0,
            q_staging_samples: 0,
            q_staging_ms: 0,
            code: 0,
            message: String::new(),
        }
    }

    pub fn error(code: i32, message: String, t_ms: u64, seq: u64) -> Self {
        Self {
            schema_version: 1,
            kind: "error".to_string(),
            seq,
            utterance_seq: 0,
            t_ms,
            text: String::new(),
            stable_prefix_len: 0,
            t_capture_end_ms: 0,
            t_dsp_done_ms: 0,
            t_infer_done_ms: 0,
            q_audio_len: 0,
            q_audio_age_ms: 0,
            q_staging_samples: 0,
            q_staging_ms: 0,
            code,
            message,
        }
    }

    /// Emitted when silence is detected after speech (VAD endpoint).
    /// `silence_ms` is the duration of silence that triggered the endpoint.
    pub fn endpoint(silence_ms: u64, t_ms: u64, seq: u64) -> Self {
        Self {
            schema_version: 1,
            kind: "endpoint".to_string(),
            seq,
            utterance_seq: 0,
            t_ms,
            text: String::new(),
            stable_prefix_len: silence_ms as usize, // Reuse field for silence duration
            t_capture_end_ms: 0,
            t_dsp_done_ms: 0,
            t_infer_done_ms: 0,
            q_audio_len: 0,
            q_audio_age_ms: 0,
            q_staging_samples: 0,
            q_staging_ms: 0,
            code: 0,
            message: String::new(),
        }
    }

    /// Emitted when audio chunks are dropped due to backpressure.
    pub fn audio_dropped(dropped_count: u64, total_dropped: u64, t_ms: u64, seq: u64) -> Self {
        Self {
            schema_version: 1,
            kind: "audio_dropped".to_string(),
            seq,
            utterance_seq: 0,
            t_ms,
            text: String::new(),
            stable_prefix_len: dropped_count as usize,
            t_capture_end_ms: 0,
            t_dsp_done_ms: 0,
            t_infer_done_ms: 0,
            q_audio_len: 0,
            q_audio_age_ms: 0,
            q_staging_samples: 0,
            q_staging_ms: 0,
            code: total_dropped as i32,
            message: String::new(),
        }
    }

    pub fn with_utterance_seq(mut self, utterance_seq: u64) -> Self {
        self.utterance_seq = utterance_seq;
        self
    }

    pub fn with_trace(mut self, trace: SttTrace) -> Self {
        self.t_capture_end_ms = trace.t_capture_end_ms;
        self.t_dsp_done_ms = trace.t_dsp_done_ms;
        self.t_infer_done_ms = trace.t_infer_done_ms;
        self.q_audio_len = trace.q_audio_len;
        self.q_audio_age_ms = trace.q_audio_age_ms;
        self.q_staging_samples = trace.q_staging_samples;
        self.q_staging_ms = trace.q_staging_ms;
        self
    }
}

impl SttEvent {
    pub fn to_signal(self) -> Signal {
        let source = match self.kind.as_str() {
            "partial" => "stt_partial",
            "final" => "stt_final",
            "error" => "stt_error",
            "endpoint" => "stt_endpoint",
            "audio_dropped" => "stt_audio_dropped",
            _ => "stt_error",
        };
        Signal::Computed {
            source: source.to_string(),
            content: serde_json::to_string(&self).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

// =============================================================================
// Streaming audio normalization + framing (2A boundary)
// =============================================================================

#[derive(Debug, Clone)]
struct AudioChunk {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
    timestamp_us: u64,
    chunk_idx: u64,
    is_tick: bool,
    received_at: Instant,
}

// =============================================================================
// Streaming log-mel (incremental)
// =============================================================================

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct FeatureConfigLocal {
    sample_rate: u32,
    n_fft: usize,
    win_length: usize,
    hop_length: usize,
    n_mels: usize,
    preemphasis: f32,
}

impl Default for FeatureConfigLocal {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            n_fft: 512,
            win_length: 400,
            hop_length: 160,
            n_mels: 128,
            preemphasis: 0.97,
        }
    }
}

#[allow(dead_code)]
fn hann_window(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (size - 1) as f32).cos()))
        .collect()
}

#[allow(dead_code)]
fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

#[allow(dead_code)]
fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0f32.powf(mel / 2595.0) - 1.0)
}

#[allow(dead_code)]
fn create_mel_filterbank(
    n_mels: usize,
    n_fft: usize,
    sample_rate: f32,
    f_min: f32,
    f_max: f32,
) -> Vec<Vec<f32>> {
    let min_mel = hz_to_mel(f_min);
    let max_mel = hz_to_mel(f_max);

    let mut mel_points = Vec::with_capacity(n_mels + 2);
    for i in 0..(n_mels + 2) {
        let mel = min_mel + (max_mel - min_mel) * (i as f32 / (n_mels + 1) as f32);
        mel_points.push(mel_to_hz(mel));
    }

    let bin_count = n_fft / 2 + 1;
    let mut filterbank = vec![vec![0.0; bin_count]; n_mels];

    for m in 0..n_mels {
        let left = mel_points[m];
        let center = mel_points[m + 1];
        let right = mel_points[m + 2];

        for i in 0..bin_count {
            let freq = i as f32 * sample_rate / n_fft as f32;
            if freq > left && freq < center {
                filterbank[m][i] = (freq - left) / (center - left);
            } else if freq >= center && freq < right {
                filterbank[m][i] = (right - freq) / (right - center);
            }
        }
    }

    filterbank
}

#[allow(dead_code)]
struct StreamingLogMel {
    cfg: FeatureConfigLocal,
    mel_filterbank: Vec<Vec<f32>>,
    window: Vec<f32>,
    fft_plan: std::sync::Arc<dyn realfft::RealToComplex<f32>>,

    // sample staging for incremental framing
    samples_16k: VecDeque<f32>,
    prev_sample: f32,

    // scratch
    fft_in: Vec<f32>,
    fft_out: Vec<num_complex::Complex<f32>>,
    power_spec: Vec<f32>,
}

#[allow(dead_code)]
impl StreamingLogMel {
    fn new(cfg: FeatureConfigLocal) -> Self {
        let window = hann_window(cfg.win_length);
        let mel_filterbank = create_mel_filterbank(
            cfg.n_mels,
            cfg.n_fft,
            cfg.sample_rate as f32,
            0.0,
            (cfg.sample_rate / 2) as f32,
        );
        let mut planner = RealFftPlanner::<f32>::new();
        let fft_plan = planner.plan_fft_forward(cfg.n_fft);
        let fft_in = fft_plan.make_input_vec();
        let fft_out = fft_plan.make_output_vec();
        let power_spec = vec![0.0; fft_out.len()];

        Self {
            cfg,
            mel_filterbank,
            window,
            fft_plan,
            samples_16k: VecDeque::new(),
            prev_sample: 0.0,
            fft_in,
            fft_out,
            power_spec,
        }
    }

    fn push_samples(&mut self, samples: &[f32]) {
        self.samples_16k.extend(samples.iter().copied());
    }

    /// Extract as many new frames as possible, returning features in **TC** layout:
    /// `[t0_m0, t0_m1, ... t0_m127, t1_m0, ...]`.
    fn pop_frames_tc(&mut self, out_tc: &mut Vec<f32>) -> usize {
        out_tc.clear();
        if self.samples_16k.len() < self.cfg.win_length {
            return 0;
        }

        let mut frames = 0usize;
        while self.samples_16k.len() >= self.cfg.win_length {
            // Copy one window into fft_in with preemphasis + Hann.
            for i in 0..self.cfg.win_length {
                let x = *self.samples_16k.get(i).unwrap_or(&0.0);
                let y = if i == 0 {
                    x - self.cfg.preemphasis * self.prev_sample
                } else {
                    let x_prev = *self.samples_16k.get(i - 1).unwrap_or(&0.0);
                    x - self.cfg.preemphasis * x_prev
                };
                self.fft_in[i] = y * self.window[i];
            }
            if frames == 0 {
                let max_in = self.fft_in.iter().map(|v| v.abs()).fold(0.0, f32::max);
                log::trace!("FFT Input check: max={:.6}", max_in);
            }
            for i in self.cfg.win_length..self.cfg.n_fft {
                self.fft_in[i] = 0.0;
            }

            self.fft_plan
                .process(&mut self.fft_in, &mut self.fft_out)
                .expect("fft process failed");

            // Power spectrum
            for (i, c) in self.fft_out.iter().enumerate() {
                self.power_spec[i] = c.re * c.re + c.im * c.im;
            }

            // Mel filtering + log
            for mel_bin in &self.mel_filterbank {
                let mut energy = 0.0f32;
                for (i, &w) in mel_bin.iter().enumerate() {
                    energy += self.power_spec[i] * w;
                }
                out_tc.push((energy + 1e-5).ln());
            }

            if frames % 50 == 0 {
                let avg = out_tc.iter().sum::<f32>() / out_tc.len() as f32;
                log::trace!("STT LogMel: frames={}, avg_ln={:.4}", frames, avg);
            }

            frames += 1;

            // Advance hop: drop hop_length samples.
            for _ in 0..self.cfg.hop_length {
                if let Some(s) = self.samples_16k.pop_front() {
                    self.prev_sample = s;
                } else {
                    break;
                }
            }
        }

        frames
    }
}

/// Simple streaming resampler (linear interpolation) to 16kHz mono.
/// Good enough for 1C; we can swap to a high-quality resampler later.
struct LinearResampler16k {
    src_rate: u32,
    phase: f32, // in source-sample units
    prev_sample: f32,
}

impl LinearResampler16k {
    fn new(src_rate: u32) -> Self {
        Self {
            src_rate,
            phase: 0.0,
            prev_sample: 0.0,
        }
    }

    fn reset_rate(&mut self, src_rate: u32) {
        self.src_rate = src_rate;
        self.phase = 0.0;
    }

    fn resample_mono(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() || self.src_rate == 0 {
            return;
        }
        let step = self.src_rate as f32 / 16000.0;
        
        while self.phase < input.len() as f32 {
            let idx = self.phase.floor() as i32;
            let frac = self.phase - self.phase.floor();
            
            let s0 = if idx < 0 { self.prev_sample } else { input[idx as usize] };
            let s1 = if idx + 1 >= input.len() as i32 {
                input[input.len() - 1]
            } else {
                input[(idx + 1) as usize]
            };
            
            out.push(s0 + (s1 - s0) * frac);
            self.phase += step;
        }
        
        if !input.is_empty() {
            self.prev_sample = input[input.len() - 1];
        }
        self.phase -= input.len() as f32;
    }
}

struct Leveler {
    enabled: bool,
    sample_rate: u32,
    target_rms: f32,
    min_rms: f32,
    max_gain_up: f32,
    max_gain_down: f32,
    attack_ms: f32,
    release_ms: f32,
    limiter: f32,
    hp_alpha: f32,
    hp_x1: f32,
    hp_y1: f32,
    rms_s: f32,
    gain: f32,
}

impl Leveler {
    fn new(sample_rate: u32) -> Self {
        let hp_freq = 30.0f32;
        let hp_alpha = (-2.0 * std::f32::consts::PI * hp_freq / sample_rate as f32).exp();
        Self {
            enabled: false,
            sample_rate,
            target_rms: 0.1,
            min_rms: 0.0056,
            max_gain_up: 7.94,
            max_gain_down: 0.063,
            attack_ms: 20.0,
            release_ms: 500.0,
            limiter: 0.89,
            hp_alpha,
            hp_x1: 0.0,
            hp_y1: 0.0,
            rms_s: 0.0,
            gain: 1.0,
        }
    }

    fn coeff_for_chunk(&self, len: usize, tau_ms: f32) -> f32 {
        if len == 0 {
            return 0.0;
        }
        let tau_s = (tau_ms / 1000.0).max(1.0e-4);
        let n = len as f32;
        let sr = self.sample_rate as f32;
        1.0 - (-n / (tau_s * sr)).exp()
    }

    fn process(&mut self, samples: &mut [f32]) {
        if !self.enabled || samples.is_empty() {
            return;
        }

        let mut sum_sq = 0.0f32;
        for s in samples.iter_mut() {
            let x = *s;
            let y = x - self.hp_x1 + self.hp_alpha * self.hp_y1;
            self.hp_x1 = x;
            self.hp_y1 = y;
            *s = y;
            sum_sq += y * y;
        }

        let rms = (sum_sq / samples.len() as f32).sqrt();
        let rms_coeff = self.coeff_for_chunk(samples.len(), 20.0);
        self.rms_s = self.rms_s + rms_coeff * (rms - self.rms_s);

        let denom = self.rms_s.max(self.min_rms);
        let mut desired = self.target_rms / denom;
        if desired > self.max_gain_up {
            desired = self.max_gain_up;
        } else if desired < self.max_gain_down {
            desired = self.max_gain_down;
        }

        let gain_coeff = if desired > self.gain {
            self.coeff_for_chunk(samples.len(), self.attack_ms)
        } else {
            self.coeff_for_chunk(samples.len(), self.release_ms)
        };
        self.gain = self.gain + gain_coeff * (desired - self.gain);

        for s in samples.iter_mut() {
            let y = *s * self.gain;
            *s = (y / self.limiter).tanh() * self.limiter;
        }
    }
}

fn downmix_to_mono(input_interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    out.clear();
    if channels <= 1 {
        out.extend_from_slice(input_interleaved);
        return;
    }
    let ch = channels as usize;
    out.reserve(input_interleaved.len() / ch);
    for frame in input_interleaved.chunks_exact(ch) {
        let mut sum = 0.0f32;
        for &s in frame {
            sum += s;
        }
        out.push(sum / ch as f32);
    }
}

// =============================================================================
// Processor skeleton + worker plumbing (we’ll fill in decode in later todos)
// =============================================================================

pub struct ParakeetSttProcessor {
    id: String,
    enabled: bool,

    in_tx: Sender<AudioChunk>,
    ctrl_tx: Sender<SttControl>,
    out_rx: Receiver<WorkerOut>,

    pending_out: VecDeque<Signal>,
    pre_gain: f32,
    gate_threshold: f32,
    filter_junk: bool,
    normalize_mode: NormalizeMode,
    offline_mode: bool,
    backpressure: bool,
    backpressure_timeout_ms: u64,
    utterance_seq: u64,
    final_blank_penalty_delta: f32,
    next_chunk_idx: u64,
    total_audio_dropped: u64,

    // Ensures the worker thread is cleanly stopped before process exit (avoids FFI teardown races).
    worker_join: Option<std::thread::JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NormalizeMode {
    PerChunk,
    Running,
    None,
}

impl NormalizeMode {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "per_chunk" => Some(Self::PerChunk),
            "running" => Some(Self::Running),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum SttControl {
    Start,
    Stop,
    ResetWithMeta {
        utterance_id: Option<String>,
        utterance_seq: u64,
        offline_mode: bool,
    },
    SetGain(f32),
    SetGateThreshold(f32),
    SetFilterJunk(bool),
    SetNormalizeMode(NormalizeMode),
    SetOfflineMode(bool),
    SetFinalBlankPenaltyDelta(f32),
    SetUtteranceId(String),
    SetUtteranceSeq(u64),
    // Real-time streaming controls
    SetMinPartialEmitMs(u64),
    SetSilenceHangoverMs(u64),
    SetAutoFlushSilenceMs(u64), // 0 = disabled
    Shutdown,
}

#[derive(Debug, Clone)]
enum WorkerOut {
    Event(SttEvent),
    Metrics(SttMetrics),
    StopStats(StopStats),
    ResetAck(ResetAck),
    ChunkAck(ChunkAck),
    SlowChunk(SlowChunk),
}

impl ParakeetSttProcessor {
    pub fn new(id: &str, runtime: ParakeetRuntimeConfig, state: Arc<Mutex<ParakeetSttState>>) -> anyhow::Result<Self> {
        // A slightly larger buffer reduces short-term backpressure drops which can
        // starve the decoder of contiguous audio.
        let mut audio_queue_cap = 256usize;
        if let Ok(v) = std::env::var("PARAKEET_AUDIO_QUEUE_CAP") {
            if let Ok(n) = v.parse::<usize>() {
                if n > 0 {
                    audio_queue_cap = n;
                }
            }
        }
        let (in_tx, in_rx) = crossbeam_channel::bounded::<AudioChunk>(audio_queue_cap);
        let (ctrl_tx, ctrl_rx) = crossbeam_channel::bounded::<SttControl>(64);
        let (out_tx, out_rx) = crossbeam_channel::bounded::<WorkerOut>(128);

        let worker_join = Some(spawn_worker(runtime, in_rx, ctrl_rx, out_tx, state.clone()));

        Ok(Self {
            id: id.to_string(),
            enabled: true,
            in_tx,
            ctrl_tx,
            out_rx,
            pending_out: VecDeque::new(),
            pre_gain: 1.0,
            gate_threshold: 0.005,
            filter_junk: true,
            normalize_mode: NormalizeMode::PerChunk,
            offline_mode: false,
            backpressure: false,
            backpressure_timeout_ms: 0,
            utterance_seq: 0,
            final_blank_penalty_delta: 0.0,
            next_chunk_idx: 0,
            total_audio_dropped: 0,
            worker_join,
        })
    }

    fn send_ctrl_with_timeout(&self, ctrl: SttControl, label: &str) -> anyhow::Result<()> {
        let timeout_ms = self.backpressure_timeout_ms.max(2000);
        let timeout = Duration::from_millis(timeout_ms);
        match self.ctrl_tx.send_timeout(ctrl, timeout) {
            Ok(()) => Ok(()),
            Err(SendTimeoutError::Timeout(_)) => {
                Err(anyhow::anyhow!("control_timeout {}", label))
            }
            Err(SendTimeoutError::Disconnected(_)) => {
                Err(anyhow::anyhow!("control_disconnected {}", label))
            }
        }
    }

    fn drain_worker_out(&mut self) {
        while let Ok(ev) = self.out_rx.try_recv() {
            let sig = match ev {
                WorkerOut::Event(e) => e.to_signal(),
                WorkerOut::Metrics(m) => m.to_signal(),
                WorkerOut::StopStats(s) => s.to_signal(),
                WorkerOut::ResetAck(a) => a.to_signal(),
                WorkerOut::ChunkAck(a) => a.to_signal(),
                WorkerOut::SlowChunk(s) => s.to_signal(),
            };
            // #region agent log
            let k = DBG_STT_OUT_N.fetch_add(1, Ordering::Relaxed);
            if k < 12 {
                let (source, content_len) = match &sig {
                    Signal::Computed { source, content } => (source.as_str(), content.len()),
                    _ => ("<non-computed>", 0),
                };
                dbglog(
                    "H3",
                    "crates/parakeet_stt/src/lib.rs:ParakeetSttProcessor:drain_worker_out",
                    "Drain worker -> pending_out",
                    serde_json::json!({"computed_source": source, "content_len": content_len}),
                );
            }
            // #endregion
            push_with_backpressure(&mut self.pending_out, sig, 64);
        }
    }
}

impl Drop for ParakeetSttProcessor {
    fn drop(&mut self) {
        // Best-effort: ask worker to shutdown, then wake it with a tiny audio tick.
        let _ = self.ctrl_tx.try_send(SttControl::Shutdown);
        let _ = self.in_tx.try_send(AudioChunk {
            samples: vec![0.0],
            sample_rate: 16_000,
            channels: 1,
            timestamp_us: 0,
            chunk_idx: 0,
            is_tick: true,
            received_at: Instant::now(),
        });
        if let Some(h) = self.worker_join.take() {
            let timeout_ms = std::env::var("PARAKEET_WORKER_JOIN_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            if timeout_ms == 0 {
                let _ = h.join();
            } else {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = h.join();
                    let _ = tx.send(());
                });
                if rx.recv_timeout(Duration::from_millis(timeout_ms)).is_err() {
                    eprintln!(
                        "[parakeet_stt] WARNING: worker join timed out after {}ms",
                        timeout_ms
                    );
                    eprintln!("[parakeet_stt] FATAL: exiting due to worker join timeout");
                    std::process::exit(42);
                }
            }
        }
    }
}

fn is_partial(sig: &Signal) -> bool {
    matches!(
        sig,
        Signal::Computed { source, .. } if source == "stt_partial"
    )
}

fn is_priority(sig: &Signal) -> bool {
    matches!(
        sig,
        Signal::Computed { source, .. }
            if source == "stt_final"
                || source == "stt_error"
                || source == "stt_chunk_ack"
                || source == "stt_reset_ack"
                || source == "stt_stop_stats"
    )
}

fn push_with_backpressure(queue: &mut VecDeque<Signal>, sig: Signal, cap: usize) {
    if cap == 0 {
        return;
    }
    if queue.len() < cap {
        queue.push_back(sig);
        return;
    }

    // Make room by dropping older partials.
    if is_partial(&sig) {
        // Drop one oldest partial, if any.
        if let Some(idx) = queue.iter().position(is_partial) {
            // Remove by rotating to front.
            for _ in 0..idx {
                if let Some(front) = queue.pop_front() {
                    queue.push_back(front);
                }
            }
            let _ = queue.pop_front();
            queue.push_back(sig);
        }
        // If no partials to drop, drop this partial.
        return;
    }

    if is_priority(&sig) {
        // Keep trying to drop partials until there's space.
        while queue.len() >= cap {
            if let Some(idx) = queue.iter().position(is_partial) {
                for _ in 0..idx {
                    if let Some(front) = queue.pop_front() {
                        queue.push_back(front);
                    }
                }
                let _ = queue.pop_front();
                continue;
            }
            // No partials available to drop; as a last resort, wait briefly for consumer to drain.
            break;
        }
        if queue.len() < cap {
            queue.push_back(sig);
        } else {
            // If we still can't enqueue, drop oldest (extremely unlikely); log via stderr.
            eprintln!("[parakeet_stt] WARNING: dropping event due to full queue (final/error backlog)");
            let _ = queue.pop_front();
            queue.push_back(sig);
        }
        return;
    }

    // Unknown: drop newest.
 }

#[derive(Default)]
struct StageTimes {
    t0: Option<Instant>,
    reset_start: Option<Instant>,
    reset_done: Option<Instant>,
    first_audio: Option<Instant>,
    first_feature: Option<Instant>,
    first_decode: Option<Instant>,
    first_partial: Option<Instant>,
    first_final: Option<Instant>,
    stop_received: Option<Instant>,
    finalize_start: Option<Instant>,
    finalize_done: Option<Instant>,
}

impl StageTimes {
    fn begin_reset(&mut self) {
        *self = StageTimes::default();
        let now = Instant::now();
        self.t0 = Some(now);
        self.reset_start = Some(now);
    }

    fn mark_reset_done(&mut self) {
        self.reset_done = Some(Instant::now());
    }

    fn mark_if_none(slot: &mut Option<Instant>) {
        if slot.is_none() {
            *slot = Some(Instant::now());
        }
    }

    fn ms_since(&self, t: Option<Instant>) -> Option<u64> {
        self.t0
            .and_then(|t0| t.map(|v| v.duration_since(t0).as_millis() as u64))
    }

    fn delta_ms(start: Option<Instant>, end: Option<Instant>) -> Option<u64> {
        match (start, end) {
            (Some(s), Some(e)) => Some(e.duration_since(s).as_millis() as u64),
            _ => None,
        }
    }
}

struct StopStatsSnapshot<'a> {
    utterance_id: &'a str,
    utterance_seq: u64,
    staging_samples: usize,
    queued_before: usize,
    queued_after: usize,
    offline_frames: usize,
    tail_flush_decodes: usize,
    post_stop_decode_iters: usize,
    post_stop_events: u32,
    final_blank_penalty_delta: f32,
    emitted_partial_pre: u64,
    emitted_partial_post: u64,
    emitted_final_pre: u64,
    emitted_final_post: u64,
    emitted_final_empty: u64,
    emitted_final_nonempty: u64,
    suppressed_junk_partial: u64,
    suppressed_junk_final: u64,
    filter_junk: bool,
    offline_mode: bool,
    audio_chunks_seen: u64,
    audio_samples_seen: u64,
    audio_samples_resampled: u64,
    slow_chunk_threshold_ms: u64,
    slow_chunk_count: u64,
    slowest_chunk_ms: u64,
    slowest_chunk_idx: u64,
    slowest_chunk_audio_idx: u64,
    last_audio_chunk_idx: u64,
    last_feature_chunk_idx: u64,
    abort_reason: Option<&'a str>,
}

fn build_stop_stats(
    snapshot: &StopStatsSnapshot<'_>,
    stage_times: &StageTimes,
    phase: &str,
    emitted_at: Instant,
) -> StopStats {
    StopStats {
        schema_version: 2,
        phase: Some(phase.to_string()),
        id: if snapshot.utterance_id.is_empty() {
            None
        } else {
            Some(snapshot.utterance_id.to_string())
        },
        utterance_seq: snapshot.utterance_seq,
        staging_samples: snapshot.staging_samples,
        queued_before: snapshot.queued_before,
        queued_after: snapshot.queued_after,
        offline_frames: snapshot.offline_frames,
        tail_flush_decodes: snapshot.tail_flush_decodes,
        post_stop_decode_iters: snapshot.post_stop_decode_iters,
        post_stop_events: snapshot.post_stop_events,
        final_blank_penalty_delta: snapshot.final_blank_penalty_delta,
        emitted_partial_pre: snapshot.emitted_partial_pre,
        emitted_partial_post: snapshot.emitted_partial_post,
        emitted_final_pre: snapshot.emitted_final_pre,
        emitted_final_post: snapshot.emitted_final_post,
        emitted_final_empty: snapshot.emitted_final_empty,
        emitted_final_nonempty: snapshot.emitted_final_nonempty,
        suppressed_junk_partial: snapshot.suppressed_junk_partial,
        suppressed_junk_final: snapshot.suppressed_junk_final,
        filter_junk: snapshot.filter_junk,
        offline_mode: snapshot.offline_mode,
        audio_chunks_seen: snapshot.audio_chunks_seen,
        audio_samples_seen: snapshot.audio_samples_seen,
        audio_samples_resampled: snapshot.audio_samples_resampled,
        t_reset_start_ms: stage_times.ms_since(stage_times.reset_start),
        t_reset_done_ms: stage_times.ms_since(stage_times.reset_done),
        t_first_audio_ms: stage_times.ms_since(stage_times.first_audio),
        t_first_feature_ms: stage_times.ms_since(stage_times.first_feature),
        t_first_decode_ms: stage_times.ms_since(stage_times.first_decode),
        t_first_partial_ms: stage_times.ms_since(stage_times.first_partial),
        t_first_final_ms: stage_times.ms_since(stage_times.first_final),
        t_stop_received_ms: stage_times.ms_since(stage_times.stop_received),
        t_finalize_start_ms: stage_times.ms_since(stage_times.finalize_start),
        t_finalize_done_ms: stage_times.ms_since(stage_times.finalize_done),
        t_stop_stats_ms: stage_times.ms_since(Some(emitted_at)),
        finalize_ms: StageTimes::delta_ms(stage_times.finalize_start, stage_times.finalize_done),
        slow_chunk_threshold_ms: snapshot.slow_chunk_threshold_ms,
        slow_chunk_count: snapshot.slow_chunk_count,
        slowest_chunk_ms: snapshot.slowest_chunk_ms,
        slowest_chunk_idx: snapshot.slowest_chunk_idx,
        slowest_chunk_audio_idx: snapshot.slowest_chunk_audio_idx,
        last_audio_chunk_idx: snapshot.last_audio_chunk_idx,
        last_feature_chunk_idx: snapshot.last_feature_chunk_idx,
        abort_reason: snapshot.abort_reason.map(|v| v.to_string()),
    }
}

fn spawn_worker(
    runtime: ParakeetRuntimeConfig,
    in_rx: Receiver<AudioChunk>,
    ctrl_rx: Receiver<SttControl>,
    out_tx: Sender<WorkerOut>,
    state: Arc<Mutex<ParakeetSttState>>,
) -> std::thread::JoinHandle<()> {
    use parakeet_trt::ParakeetSessionSafe;
    thread::Builder::new()
        .name("parakeet_stt_worker".to_string())
        .spawn(move || {
            let _running = Arc::new(AtomicBool::new(true));
            let seq = AtomicU64::new(1);
            let mut is_running = true;
            let mut should_exit = false;
            let mut pending_stop = false;

            {
                if let Ok(mut s) = state.lock() {
                    s.status = "running".to_string();
                }
            }

            let mut utterance_seq = 0u64;
            if let Some(path) = runtime.encoder_override_path.as_deref() {
                std::env::set_var("PARAKEET_STREAMING_ENCODER_PATH", path);
                log::debug!(
                    "parakeet_stt streaming encoder override: {}",
                    path
                );
            } else {
                std::env::remove_var("PARAKEET_STREAMING_ENCODER_PATH");
            }

            let session = match ParakeetSessionSafe::new(&runtime.model_dir, runtime.device_id, runtime.use_fp16) {
                Ok(s) => s,
                Err(e) => {
                    let ev = SttEvent::error(
                        -1,
                        format!("failed to create parakeet session: {e}"),
                        0,
                        seq.fetch_add(1, Ordering::Relaxed),
                    )
                    .with_utterance_seq(utterance_seq);
                    let _ = out_tx.send(WorkerOut::Event(ev));
                    return;
                }
            };

            let mut mono = Vec::<f32>::new();
            let mut resampled = Vec::<f32>::new();
            let mut resampler: Option<LinearResampler16k> = None;

            let streaming_encoder = runtime.is_streaming_encoder();

            // Tradeoff: chunk size impacts both decoding quality and responsiveness.
            // Streaming encoder uses large context (592/584) with small advance to keep latency low.
            let mut chunk_frames: usize = runtime.chunk_frames;
            let mut advance_frames: usize = runtime.advance_frames;
            if let Ok(v) = std::env::var("PARAKEET_CHUNK_FRAMES") {
                if let Ok(n) = v.parse::<usize>() {
                    chunk_frames = n;
                }
            }
            if let Ok(v) = std::env::var("PARAKEET_ADVANCE_FRAMES") {
                if let Ok(n) = v.parse::<usize>() {
                    advance_frames = n;
                }
            }
            log::debug!(
                "parakeet_stt runtime resolved: streaming_encoder={} use_streaming_encoder={} model_dir={} chunk_frames={} advance_frames={}",
                streaming_encoder,
                runtime.use_streaming_encoder,
                runtime.model_dir,
                chunk_frames,
                advance_frames
            );
            let (min_chunk, max_chunk, default_chunk) = if streaming_encoder {
                (584usize, 592usize, 592usize)
            } else {
                (16usize, 256usize, 256usize)
            };
            if chunk_frames < min_chunk || chunk_frames > max_chunk {
                eprintln!(
                    "[parakeet_stt] invalid chunk_frames={} (expected {}..={} for {})",
                    chunk_frames,
                    min_chunk,
                    max_chunk,
                    if streaming_encoder { "streaming encoder" } else { "classic encoder" }
                );
                chunk_frames = default_chunk;
            }
            if advance_frames == 0 || advance_frames > chunk_frames {
                let fallback = if streaming_encoder {
                    8usize
                } else {
                    (chunk_frames / 2).max(1)
                };
                eprintln!(
                    "[parakeet_stt] invalid advance_frames={} (chunk_frames={}), using {}",
                    advance_frames,
                    chunk_frames,
                    fallback
                );
                advance_frames = fallback;
            }
            if streaming_encoder {
                if chunk_frames != 584 {
                    eprintln!(
                        "[parakeet_stt] streaming: expected chunk_frames=584, got {}",
                        chunk_frames
                    );
                }
                if advance_frames != 8 {
                    eprintln!(
                        "[parakeet_stt] streaming: expected advance_frames=8, got {}",
                        advance_frames
                    );
                }
            }
            let cfg = FeatureConfig::default();
            let n_mels = cfg.n_mels;
            let extractor = LogMelExtractor::new(cfg);
            let needed_samples = extractor_needed_samples(chunk_frames);
            let advance_samples = extractor_advance_samples(advance_frames).max(1);

            let mut bct = vec![0.0f32; n_mels * chunk_frames];
            let mut chunk_t0_us: Option<u64> = None;

            let mut last_partial_text = String::new();

            // Best-effort cadence control (we’ll also drop partials when queue is full).
            let mut min_emit = Duration::from_millis(100);
            let mut last_emit = Instant::now() - Duration::from_secs(1);
            let mut last_metrics_emit = Instant::now() - Duration::from_secs(1);
            let metrics_interval = Duration::from_millis(500);


            // Real-time streaming: VAD/endpoint detection state
            let mut silence_hangover_ms: u64 = 300;  // Time speech must be stable before "active"
            let mut auto_flush_silence_ms: u64 = 0;  // 0 = disabled, >0 = flush after N ms silence
            let mut speech_active = false;
            let mut silence_start: Option<Instant> = None;
            let mut speech_onset: Option<Instant> = None;  // When non-silent audio first appeared
            let mut endpoint_emitted = false;  // Prevent multiple endpoint events per silence
            let mut running_rms = 0.0f32;
            let mut running_peak = 0.0f32;
            let mut samples_seen = 0usize;
            let mut current_gain = 5.0f32; 
            let mut current_gate_threshold = 0.005f32;
            let mut current_filter_junk = true;
            let mut normalize_mode = NormalizeMode::PerChunk;
            let mut offline_mode = false;
            let mut utterance_id = String::new();
            let mut last_audio_ts_us = 0u64;
            let mut final_blank_penalty_delta = 0.0f32;
            let mut emitted_partial_pre = 0u64;
            let mut emitted_partial_post = 0u64;
            let mut emitted_final_pre = 0u64;
            let mut emitted_final_post = 0u64;
            let mut emitted_final_empty = 0u64;
            let mut emitted_final_nonempty = 0u64;
            let mut suppressed_junk_partial = 0u64;
            let mut suppressed_junk_final = 0u64;
            let mut audio_chunks_seen = 0u64;
            let mut audio_samples_seen = 0u64;
            let mut audio_samples_resampled = 0u64;
            let mut stage_times = StageTimes::default();
            let mut last_audio_chunk_idx = 0u64;
            let mut feature_chunk_idx = 0u64;
            let mut last_feature_chunk_idx = 0u64;
            let mut slow_chunk_count = 0u64;
            let mut slowest_chunk_ms = 0u64;
            let mut slowest_chunk_idx = 0u64;
            let mut slowest_chunk_audio_idx = 0u64;
            let mut abort_utterance = false;
            let mut abort_reason: Option<String> = None;
            let slow_chunk_threshold_ms: u64 = std::env::var("PARAKEET_SLOW_CHUNK_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(250);
            let abort_slow_chunk_ms: u64 = std::env::var("PARAKEET_ABORT_SLOW_CHUNK_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000);

            let mut norm_sum: Vec<f64> = vec![0.0; n_mels];
            let mut norm_sumsq: Vec<f64> = vec![0.0; n_mels];
            let mut norm_frames: usize = 0;
            let mut offline_audio: Vec<f32> = Vec::new();
            let mut leveler = Leveler::new(16_000);

            // NOTE: Do not shadow `chunk_frames` here. Using 16-frame chunks tends to produce
            // empty/1-char hypotheses with this demo runtime (confirmed in debug logs).

            // Audio staging for feature extraction: keep STFT overlap across chunks.
            let mut sample_buf_16k: VecDeque<f32> = VecDeque::new();
            let mut segment = Vec::<f32>::with_capacity(needed_samples);
            // NeMo preprocessor settings from the model's `model_config.yaml`.
            // - normalize: per_feature (default per-chunk unless overridden)
            // - dither: 1e-5
            let dither = 1.0e-5f32;
            // Tiny RNG for dither (deterministic, no external deps).
            let mut dither_state: u64 = 0x9E3779B97F4A7C15;

            // #region agent log
            let mut last_gate_is_silent: Option<bool> = None;
            // #endregion
            let queued_chunks = |samples: usize| -> usize {
                if samples < needed_samples {
                    0
                } else {
                    1 + (samples - needed_samples) / advance_samples
                }
            };
            macro_rules! process_chunk {
                (
                    $timestamp_us:expr,
                    $capture_end_us:expr,
                    $received_at:expr,
                    $post_stop:expr,
                    $post_stop_events:expr,
                    $audio_chunk_idx:expr,
                    $feature_chunk_idx:expr
                ) => {{
                    let timestamp_us: u64 = $timestamp_us;
                    let capture_end_us: u64 = $capture_end_us;
                    let received_at_opt: Option<Instant> = $received_at;
                    let post_stop: bool = $post_stop;
                    let post_stop_events = $post_stop_events;
                    let audio_chunk_idx: u64 = $audio_chunk_idx;
                    let feature_chunk_idx: u64 = $feature_chunk_idx;
                    let mut ok = true;

                    if sample_buf_16k.len() < needed_samples {
                        ok = false;
                    }

                    if ok {
                        segment.clear();
                        segment.extend(sample_buf_16k.iter().take(needed_samples).copied());
                        // Apply very small dither (matches NeMo default in the model config) before feature extraction.
                        if dither > 0.0 {
                            for s in &mut segment {
                                // xorshift64*
                                dither_state ^= dither_state >> 12;
                                dither_state ^= dither_state << 25;
                                dither_state ^= dither_state >> 27;
                                let r = (dither_state.wrapping_mul(2685821657736338717) >> 40) as u32;
                                let u01 = (r as f32) / (u32::MAX as f32);
                                let noise = (u01 * 2.0 - 1.0) * dither;
                                *s += noise;
                            }
                        }

                        StageTimes::mark_if_none(&mut stage_times.first_feature);
                        let mut feats_tc = extractor.compute(&segment); // TC order (frame-major)
                        if feats_tc.len() != chunk_frames * n_mels {
                            // #region agent log
                            dbglog(
                                "H4",
                                "crates/parakeet_stt/src/lib.rs:worker:feature_shape",
                                "Unexpected feature length",
                                serde_json::json!({"got": feats_tc.len(), "expected": chunk_frames * n_mels}),
                            );
                            // #endregion
                            ok = false;
                        }

                        if ok {
                            // NeMo `normalize: per_feature`: per mel-bin normalize over time frames.
                            // This is important for stable decoding with Parakeet checkpoints.
                            let ln_eps = -11.512925f32; // ln(1e-5); used as a reference floor threshold in logs
                            let mut raw_floor_ct: u32 = 0;
                            for &v in &feats_tc {
                                if v <= ln_eps + 1e-3 {
                                    raw_floor_ct += 1;
                                }
                            }

                            if normalize_mode != NormalizeMode::None {
                                let frames = chunk_frames;
                                if normalize_mode == NormalizeMode::Running {
                                    for m in 0..n_mels {
                                        let mut sum = 0.0f64;
                                        let mut sumsq = 0.0f64;
                                        for t in 0..frames {
                                            let v = feats_tc[t * n_mels + m] as f64;
                                            sum += v;
                                            sumsq += v * v;
                                        }
                                        norm_sum[m] += sum;
                                        norm_sumsq[m] += sumsq;
                                    }
                                    norm_frames += frames;
                                }

                                let denom_frames = if normalize_mode == NormalizeMode::Running {
                                    (norm_frames.max(1)) as f64
                                } else {
                                    frames as f64
                                };

                                for m in 0..n_mels {
                                    let (sum, sumsq) = if normalize_mode == NormalizeMode::Running {
                                        (norm_sum[m], norm_sumsq[m])
                                    } else {
                                        let mut sum = 0.0f64;
                                        let mut sumsq = 0.0f64;
                                        for t in 0..frames {
                                            let v = feats_tc[t * n_mels + m] as f64;
                                            sum += v;
                                            sumsq += v * v;
                                        }
                                        (sum, sumsq)
                                    };

                                    let mean = (sum / denom_frames) as f32;
                                    let mut var = (sumsq / denom_frames) - (mean as f64) * (mean as f64);
                                    if var < 0.0 {
                                        var = 0.0;
                                    }
                                    let mut std = (var as f32).sqrt();
                                    if std < 1.0e-5 {
                                        std = 1.0e-5;
                                    }
                                    for t in 0..frames {
                                        let idx = t * n_mels + m;
                                        feats_tc[idx] = (feats_tc[idx] - mean) / std;
                                    }
                                }
                            }

                            // Transpose TC -> CT (BCT without batch): [C, T]
                            for t in 0..chunk_frames {
                                for m in 0..n_mels {
                                    bct[m * chunk_frames + t] = feats_tc[t * n_mels + m];
                                }
                            }

                            // #region agent log
                            let chunk_n = DBG_STT_CHUNK_N.fetch_add(1, Ordering::Relaxed);
                            if chunk_n < 12 {
                                let mut mn = f32::INFINITY;
                                let mut mx = f32::NEG_INFINITY;
                                let mut sum = 0.0f64;
                                let mut floor_ct: u32 = 0;
                                for &v in &bct[..(n_mels * chunk_frames)] {
                                    mn = mn.min(v);
                                    mx = mx.max(v);
                                    sum += v as f64;
                                }
                                // Count how many values are near the minimum; high % suggests "all-floor" features (often no-speech / bad scale).
                                let floor_thr = mn + 1e-3;
                                for &v in &bct[..(n_mels * chunk_frames)] {
                                    if v <= floor_thr {
                                        floor_ct += 1;
                                    }
                                }
                                let mean = (sum / (n_mels * chunk_frames) as f64) as f32;
                                dbglog(
                                    "H4",
                                    "crates/parakeet_stt/src/lib.rs:worker:chunk_stats",
                                    "BCT feature stats (pre push_features)",
                                    serde_json::json!({
                                        "chunk_frames": chunk_frames,
                                        "n_mels": n_mels,
                                        "min": mn,
                                        "max": mx,
                                        "mean": mean
                                        ,"floor_ct": floor_ct
                                        ,"floor_frac": (floor_ct as f64) / ((n_mels * chunk_frames) as f64)
                                        ,"raw_floor_ct": raw_floor_ct
                                        ,"raw_floor_frac": (raw_floor_ct as f64) / ((n_mels * chunk_frames) as f64)
                                    }),
                                );
                            }
                            // #endregion
                            if should_log_throttled(&DBG_STT_FEATURE_MS, 1000) {
                                let mode = if feature_chunk_idx == 0 {
                                    "first"
                                } else {
                                    "steady"
                                };
                                log::debug!(
                                    "parakeet_stt features: shape=[1, {}, {}] length={} mode={} streaming={}",
                                    n_mels,
                                    chunk_frames,
                                    chunk_frames,
                                    mode,
                                    streaming_encoder
                                );
                            }

                            // Push features to runtime.
                            // NOTE: current Parakeet runtime is chunk-scoped; later todo will make it true token-streaming.
                            let t_capture_end_ms = if capture_end_us > 0 {
                                capture_end_us / 1000
                            } else {
                                0
                            };
                            let t_dsp_done_ms = now_ms();
                            let q_audio_len = in_rx.len();
                            let q_audio_age_ms = if t_capture_end_ms > 0 && t_dsp_done_ms >= t_capture_end_ms {
                                t_dsp_done_ms - t_capture_end_ms
                            } else {
                                0
                            };
                            let q_staging_samples = sample_buf_16k.len();
                            let q_staging_ms = ((q_staging_samples as u64) * 1000) / 16_000;
                            StageTimes::mark_if_none(&mut stage_times.first_decode);
                            let t_decode0 = Instant::now();
                            session.set_debug_context(
                                &utterance_id,
                                utterance_seq,
                                audio_chunk_idx,
                                feature_chunk_idx,
                            );
                            // #region agent log
                            let push_idx = DBG_STT_PUSH_OK_N.load(Ordering::Relaxed)
                                + DBG_STT_PUSH_ERR_N.load(Ordering::Relaxed);
                            if push_idx < 12 {
                                dbglog(
                                    "H23",
                                    "crates/parakeet_stt/src/lib.rs:worker:push_features",
                                    "About to call push_features",
                                    serde_json::json!({
                                        "push_idx": push_idx,
                                        "chunk_frames": chunk_frames,
                                        "needed_samples": needed_samples,
                                        "advance_samples": advance_samples,
                                        "buf_len": sample_buf_16k.len(),
                                    }),
                                );
                            }
                            // #endregion

                            let enc_shape = format!("1x{}x{}", n_mels, chunk_frames);
                            let length_val = chunk_frames;
                            let mut push_err: Option<String> = None;
                            match session.push_features(&bct, chunk_frames) {
                                Ok(_) => {
                                    // #region agent log
                                    let k = DBG_STT_PUSH_OK_N.fetch_add(1, Ordering::Relaxed);
                                    if k < 12 {
                                        dbglog(
                                            "H23",
                                            "crates/parakeet_stt/src/lib.rs:worker:push_features",
                                            "push_features OK",
                                            serde_json::json!({"ok_idx": k}),
                                        );
                                    }
                                    // #endregion
                                }
                                Err(e) => {
                                    // #region agent log
                                    let k = DBG_STT_PUSH_ERR_N.fetch_add(1, Ordering::Relaxed);
                                    if k < 12 {
                                        dbglog(
                                            "H24",
                                            "crates/parakeet_stt/src/lib.rs:worker:push_features",
                                            "push_features ERR",
                                            serde_json::json!({"err_idx": k, "err": e.to_string()}),
                                        );
                                    }
                                    // #endregion
                                    push_err = Some(e.to_string());
                                }
                            }
                            if should_log_throttled(&DBG_STT_ENQUEUE_MS, 1000) {
                                if let Some(err) = push_err.as_deref() {
                                    log::debug!(
                                        "parakeet_stt enqueue: ok=false err=\"{}\" enc_shape={} length={}",
                                        err,
                                        enc_shape,
                                        length_val
                                    );
                                } else {
                                    log::debug!(
                                        "parakeet_stt enqueue: ok=true enc_shape={} length={}",
                                        enc_shape,
                                        length_val
                                    );
                                }
                            }

                            let t_infer_done_ms = now_ms();
                            let trace = SttTrace {
                                t_capture_end_ms,
                                t_dsp_done_ms,
                                t_infer_done_ms,
                                q_audio_len,
                                q_audio_age_ms,
                                q_staging_samples,
                                q_staging_ms,
                            };

                            if let Some(err) = push_err {
                                let t_ms = chunk_t0_us.unwrap_or(timestamp_us) / 1000;
                                let ev = SttEvent::error(
                                    -4,
                                    format!("push_features failed: {err}"),
                                    t_ms,
                                    seq.fetch_add(1, Ordering::Relaxed),
                                )
                                .with_utterance_seq(utterance_seq)
                                .with_trace(trace);
                                let _ = out_tx.send(WorkerOut::Event(ev));
                                ok = false;
                            }

                            let decode_ms = t_decode0.elapsed().as_millis() as u64;
                            let length_shape = "1".to_string();
                            let profile_idx = 0u32;
                            if decode_ms >= slow_chunk_threshold_ms {
                                slow_chunk_count = slow_chunk_count.saturating_add(1);
                                if decode_ms > slowest_chunk_ms {
                                    slowest_chunk_ms = decode_ms;
                                    slowest_chunk_idx = feature_chunk_idx;
                                    slowest_chunk_audio_idx = audio_chunk_idx;
                                }
                                if slow_chunk_count <= 8 {
                                    let queue_ms = received_at_opt.map(|t| t.elapsed().as_millis() as u64);
                                    eprintln!(
                                        "[parakeet_stt] slow_chunk id={} utt_seq={} feature_idx={} audio_chunk_idx={} decode_ms={} queue_ms={:?} enc_shape={} length_shape={} profile_idx={} post_stop={} offline_mode={}",
                                        utterance_id,
                                        utterance_seq,
                                        feature_chunk_idx,
                                        audio_chunk_idx,
                                        decode_ms,
                                        queue_ms,
                                        enc_shape,
                                        length_shape,
                                        profile_idx,
                                        post_stop,
                                        offline_mode
                                    );
                                    let slow_event = SlowChunk {
                                        schema_version: 1,
                                        id: if utterance_id.is_empty() {
                                            None
                                        } else {
                                            Some(utterance_id.clone())
                                        },
                                        utterance_seq,
                                        feature_idx: feature_chunk_idx,
                                        audio_chunk_idx,
                                        decode_ms,
                                        queue_ms,
                                        enc_shape: enc_shape.clone(),
                                        length_shape: length_shape.clone(),
                                        profile_idx,
                                        post_stop,
                                        offline_mode,
                                    };
                                    let _ = out_tx.try_send(WorkerOut::SlowChunk(slow_event));
                                }
                            }
                            if abort_slow_chunk_ms > 0
                                && decode_ms >= abort_slow_chunk_ms
                                && !abort_utterance
                            {
                                let t_ms = chunk_t0_us.unwrap_or(timestamp_us) / 1000;
                                let msg = format!(
                                    "slow_chunk_abort decode_ms={} feature_idx={} audio_chunk_idx={} enc_shape={} length_shape={} profile_idx={}",
                                    decode_ms,
                                    feature_chunk_idx,
                                    audio_chunk_idx,
                                    enc_shape,
                                    length_shape,
                                    profile_idx
                                );
                                let ev = SttEvent::error(
                                    -5,
                                    msg,
                                    t_ms,
                                    seq.fetch_add(1, Ordering::Relaxed),
                                )
                                .with_utterance_seq(utterance_seq);
                                let _ = out_tx.send(WorkerOut::Event(ev));
                                abort_utterance = true;
                                abort_reason = Some("slow_chunk_abort".to_string());
                                pending_stop = true;
                                ok = false;
                            }
                            if ok {
                                // Poll runtime events
                                // #region agent log
                                let mut saw_any = false;
                                // #endregion
                                while let Some(ev) = session.poll_event() {
                                    // #region agent log
                                    saw_any = true;
                                    // #endregion
                                    match ev {
                                        parakeet_trt::TranscriptionEvent::PartialText { text, .. } => {
                                            StageTimes::mark_if_none(&mut stage_times.first_partial);
                                            emitted_partial_pre = emitted_partial_pre.saturating_add(1);
                                            if current_filter_junk && (text.is_empty() || is_junk(&text)) {
                                                suppressed_junk_partial =
                                                    suppressed_junk_partial.saturating_add(1);
                                                // #region agent log
                                                let k = DBG_STT_WORKER_EV_N.fetch_add(1, Ordering::Relaxed);
                                                if k < 8 {
                                                    dbglog(
                                                        "H4",
                                                        "crates/parakeet_stt/src/lib.rs:worker:poll_event",
                                                        "Worker PartialText (filtered)",
                                                        serde_json::json!({"text_len": text.len()}),
                                                    );
                                                }
                                                // #endregion
                                                continue;
                                            }
                                            emitted_partial_post = emitted_partial_post.saturating_add(1);
                                            // #region agent log
                                            let k = DBG_STT_WORKER_EV_N.fetch_add(1, Ordering::Relaxed);
                                            if k < 2 {
                                                dbglog(
                                                    "H4",
                                                    "crates/parakeet_stt/src/lib.rs:worker:poll_event",
                                                    "Worker PartialText (passed filter)",
                                                    serde_json::json!({"text_len": text.len()}),
                                                );
                                            }
                                            // #endregion
                                            let t_ms = chunk_t0_us.unwrap_or(timestamp_us) / 1000;
                                            if last_emit.elapsed() < min_emit {
                                                last_partial_text = text;
                                                continue;
                                            }

                                            let stable_prefix_len =
                                                common_prefix_len_chars(&last_partial_text, &text);
                                            let stable_prefix_len = stable_prefix_len.min(text.chars().count());
                                            last_partial_text = text.clone();

                                            let ev = SttEvent::partial(
                                                text,
                                                stable_prefix_len,
                                                t_ms,
                                                seq.fetch_add(1, Ordering::Relaxed),
                                            )
                                            .with_utterance_seq(utterance_seq)
                                            .with_trace(trace);
                                            let _ = out_tx.try_send(WorkerOut::Event(ev));
                                            if post_stop {
                                                *post_stop_events = (*post_stop_events).saturating_add(1);
                                            }
                                            last_emit = Instant::now();
                                        }
                                        parakeet_trt::TranscriptionEvent::FinalText { text, .. } => {
                                            emitted_final_pre = emitted_final_pre.saturating_add(1);
                                            if text.trim().is_empty() {
                                                emitted_final_empty = emitted_final_empty.saturating_add(1);
                                            } else {
                                                emitted_final_nonempty = emitted_final_nonempty.saturating_add(1);
                                            }
                                            if current_filter_junk && (text.is_empty() || is_junk(&text)) {
                                                suppressed_junk_final =
                                                    suppressed_junk_final.saturating_add(1);
                                                // #region agent log
                                                let alnum = text.chars().filter(|c| c.is_alphanumeric()).count();
                                                let punct = text.chars().filter(|c| c.is_ascii_punctuation()).count();
                                                dbglog(
                                                    "H4",
                                                    "crates/parakeet_stt/src/lib.rs:worker:poll_event",
                                                    "Worker FinalText (filtered)",
                                                    serde_json::json!({
                                                        "text_len": text.len(),
                                                        "text_preview": text.chars().take(64).collect::<String>(),
                                                        "alnum": alnum,
                                                        "punct": punct
                                                    }),
                                                );
                                                // #endregion
                                                continue;
                                            }
                                            emitted_final_post = emitted_final_post.saturating_add(1);
                                            // #region agent log
                                            let alnum = text.chars().filter(|c| c.is_alphanumeric()).count();
                                            let punct = text.chars().filter(|c| c.is_ascii_punctuation()).count();
                                            dbglog(
                                                "H4",
                                                "crates/parakeet_stt/src/lib.rs:worker:poll_event",
                                                "Worker FinalText (passed filter)",
                                                serde_json::json!({
                                                    "text_len": text.len(),
                                                    "text_preview": text.chars().take(64).collect::<String>(),
                                                    "alnum": alnum,
                                                    "punct": punct
                                                }),
                                            );
                                            // #endregion
                                            let t_ms = chunk_t0_us.unwrap_or(timestamp_us) / 1000;
                                            let ev = SttEvent::final_(
                                                text,
                                                t_ms,
                                                seq.fetch_add(1, Ordering::Relaxed),
                                            )
                                            .with_utterance_seq(utterance_seq)
                                            .with_trace(trace);
                                            let _ = out_tx.send(WorkerOut::Event(ev));
                                            if post_stop {
                                                *post_stop_events = (*post_stop_events).saturating_add(1);
                                            }
                                            last_partial_text.clear();
                                            last_emit = Instant::now();
                                        }
                                        parakeet_trt::TranscriptionEvent::Error { message } => {
                                            // #region agent log
                                            dbglog(
                                                "H4",
                                                "crates/parakeet_stt/src/lib.rs:worker:poll_event",
                                                "Worker Error event",
                                                serde_json::json!({"message_len": message.len()}),
                                            );
                                            // #endregion
                                            let ev = SttEvent::error(
                                                -3,
                                                message,
                                                chunk_t0_us.unwrap_or(timestamp_us) / 1000,
                                                seq.fetch_add(1, Ordering::Relaxed),
                                            )
                                            .with_utterance_seq(utterance_seq);
                                            let _ = out_tx.send(WorkerOut::Event(ev));
                                        }
                                    }
                                }
                                // #region agent log
                                if chunk_n < 6 && !saw_any {
                                    dbglog(
                                        "H4",
                                        "crates/parakeet_stt/src/lib.rs:worker:poll_event",
                                        "No runtime events after push_features",
                                        serde_json::json!({"chunk_frames": chunk_frames}),
                                    );
                                }
                                // #endregion

                                if let Some(received_at) = received_at_opt {
                                    // Telemetry (best-effort): latency and RTF.
                                    if last_metrics_emit.elapsed() >= metrics_interval {
                                        let audio_chunk_ms = (advance_frames as u64) * 10; // advance frames * 10ms hop
                                        let rtf = if audio_chunk_ms > 0 {
                                            decode_ms as f32 / audio_chunk_ms as f32
                                        } else {
                                            0.0
                                        };
                                        let t_ms = chunk_t0_us.unwrap_or(timestamp_us) / 1000;
                                        let latency_ms = received_at.elapsed().as_millis() as u64;
                                        let avg_rms = if samples_seen > 0 {
                                            running_rms / samples_seen as f32
                                        } else {
                                            0.0
                                        };
                                        let metrics = SttMetrics {
                                            schema_version: 1,
                                            kind: "metrics".to_string(),
                                            seq: seq.fetch_add(1, Ordering::Relaxed),
                                            utterance_seq,
                                            t_ms,
                                            latency_ms,
                                            decode_ms,
                                            rtf,
                                            audio_rms: avg_rms,
                                            audio_peak: running_peak,
                                        };
                                        let _ = out_tx.try_send(WorkerOut::Metrics(metrics.clone()));

                                        running_rms = 0.0;
                                        running_peak = 0.0;
                                        samples_seen = 0;
                                        last_metrics_emit = Instant::now();

                                        if let Ok(mut s) = state.lock() {
                                            s.latency_ms = metrics.latency_ms;
                                            s.decode_ms = metrics.decode_ms;
                                            s.rtf = metrics.rtf;
                                            s.last_seq = metrics.seq;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if ok {
                        // Advance by hop_length*advance_frames samples; keep STFT overlap in buffer.
                        for _ in 0..advance_samples {
                            let _ = sample_buf_16k.pop_front();
                        }

                        // Next chunk starts at next audio timestamp we see.
                        chunk_t0_us = None;
                    }

                    ok
                }};
            }
            while let Ok(chunk) = in_rx.recv() {
                let mut drop_chunk = false;
                let mut reset_seen = false;
                let mut reset_drained_audio = 0usize;
                let mut reset_ctrl_q = 0usize;
                // Handle any pending controls (non-blocking).
                while let Ok(ctrl) = ctrl_rx.try_recv() {
                    match ctrl {
                        SttControl::Start => {
                            is_running = true;
                            pending_stop = false;
                            if let Ok(mut s) = state.lock() {
                                s.status = "running".to_string();
                            }
                        }
                        SttControl::Stop => {
                            pending_stop = true;
                            stage_times.stop_received = Some(Instant::now());
                            if let Ok(mut s) = state.lock() {
                                s.status = "stopping".to_string();
                            }
                        }
                        SttControl::ResetWithMeta {
                            utterance_id: new_id,
                            utterance_seq: new_seq,
                            offline_mode: new_offline,
                        } => {
                            stage_times.begin_reset();
                            let audio_q = in_rx.len();
                            let ctrl_q = ctrl_rx.len();
                            let mut drained_audio = 0usize;
                            while let Ok(_) = in_rx.try_recv() {
                                drained_audio += 1;
                            }
                            let log_reset = matches!(
                                std::env::var("PARAKEET_LOG_RESET").ok().as_deref(),
                                Some("1")
                            );
                            if log_reset
                                || audio_q > 0
                                || ctrl_q > 0
                                || drained_audio > 0
                                || pending_stop
                            {
                                eprintln!(
                                    "[parakeet_stt] reset id={} utt_seq={} audio_q={} drained_audio={} ctrl_q={} running={} pending_stop={} offline_mode={}",
                                    utterance_id,
                                    utterance_seq,
                                    audio_q,
                                    drained_audio,
                                    ctrl_q,
                                    is_running,
                                    pending_stop,
                                    offline_mode
                                );
                            }
                            reset_seen = true;
                            reset_drained_audio = drained_audio;
                            reset_ctrl_q = ctrl_q;
                            drop_chunk = true;
                            is_running = true;
                            pending_stop = false;
                            utterance_id.clear();
                            if let Ok(mut s) = state.lock() {
                                s.status = "running".to_string();
                            }
                            last_partial_text.clear();
                            sample_buf_16k.clear();
                            chunk_t0_us = None;
                            norm_frames = 0;
                            for v in &mut norm_sum {
                                *v = 0.0;
                            }
                            for v in &mut norm_sumsq {
                                *v = 0.0;
                            }
                            offline_mode = new_offline;
                            offline_audio.clear();
                            session.reset();
                            seq.store(1, Ordering::Relaxed);
                            emitted_partial_pre = 0;
                            emitted_partial_post = 0;
                            emitted_final_pre = 0;
                            emitted_final_post = 0;
                            emitted_final_empty = 0;
                            emitted_final_nonempty = 0;
                            suppressed_junk_partial = 0;
                            suppressed_junk_final = 0;
                            audio_chunks_seen = 0;
                            audio_samples_seen = 0;
                            audio_samples_resampled = 0;
                            feature_chunk_idx = 0;
                            last_feature_chunk_idx = 0;
                            last_audio_chunk_idx = 0;
                            slow_chunk_count = 0;
                            slowest_chunk_ms = 0;
                            slowest_chunk_idx = 0;
                            slowest_chunk_audio_idx = 0;
                            abort_utterance = false;
                            abort_reason = None;
                            utterance_seq = new_seq;
                            utterance_id = new_id.unwrap_or_default();
                            stage_times.mark_reset_done();
                        }
                        SttControl::SetGain(g) => {
                            current_gain = g;
                        }
                        SttControl::SetGateThreshold(t) => {
                            current_gate_threshold = t;
                        }
                        SttControl::SetFilterJunk(f) => {
                            current_filter_junk = f;
                        }
                        SttControl::SetNormalizeMode(m) => {
                            normalize_mode = m;
                            norm_frames = 0;
                            for v in &mut norm_sum {
                                *v = 0.0;
                            }
                            for v in &mut norm_sumsq {
                                *v = 0.0;
                            }
                        }
                        SttControl::SetOfflineMode(v) => {
                            offline_mode = v;
                            offline_audio.clear();
                            norm_frames = 0;
                            for val in &mut norm_sum {
                                *val = 0.0;
                            }
                            for val in &mut norm_sumsq {
                                *val = 0.0;
                            }
                        }
                        SttControl::SetFinalBlankPenaltyDelta(v) => {
                            final_blank_penalty_delta = v;
                        }
                        SttControl::SetUtteranceId(id) => {
                            utterance_id = id;
                        }
                        SttControl::SetUtteranceSeq(seq_val) => {
                            utterance_seq = seq_val;
                        }
                        SttControl::SetMinPartialEmitMs(ms) => {
                            min_emit = Duration::from_millis(ms);
                        }
                        SttControl::SetSilenceHangoverMs(ms) => {
                            silence_hangover_ms = ms;
                        }
                        SttControl::SetAutoFlushSilenceMs(ms) => {
                            auto_flush_silence_ms = ms;
                            // Reset endpoint state when changing this setting
                            endpoint_emitted = false;
                        }
                        SttControl::Shutdown => {
                            should_exit = true;
                            is_running = false;
                            if let Ok(mut s) = state.lock() {
                                s.status = "stopped".to_string();
                            }
                        }
                    }
                }

                if reset_seen {
                    let ack = ResetAck {
                        schema_version: 1,
                        id: if utterance_id.is_empty() {
                            None
                        } else {
                            Some(utterance_id.clone())
                        },
                        utterance_seq,
                        drained_audio: reset_drained_audio,
                        ctrl_queued: reset_ctrl_q,
                        offline_mode,
                    };
                    let _ = out_tx.send(WorkerOut::ResetAck(ack));
                }

                if should_exit {
                    break;
                }

                if drop_chunk {
                    continue;
                }

                if !is_running {
                    continue;
                }

                // Flag to skip audio processing but still reach finalization check
                let skip_to_finalize;
                
                if chunk.is_tick {
                    // If we received a tick while pending_stop is true, 
                    // this is the signal to finalize the utterance
                    if pending_stop {
                        // Drain any remaining audio that might be in the queue
                        // so the finalization check will pass
                        while let Ok(_) = in_rx.try_recv() {}
                        // Don't process audio, just jump to finalization check
                        skip_to_finalize = true;
                    } else {
                        continue;
                    }
                } else {
                    skip_to_finalize = false;
                }
                
                if !skip_to_finalize {
                StageTimes::mark_if_none(&mut stage_times.first_audio);
                let ack = ChunkAck {
                    schema_version: 1,
                    utterance_seq,
                    chunk_idx: chunk.chunk_idx,
                };
                let _ = out_tx.send(WorkerOut::ChunkAck(ack));
                last_audio_chunk_idx = chunk.chunk_idx;
                audio_chunks_seen = audio_chunks_seen.saturating_add(1);
                audio_samples_seen = audio_samples_seen.saturating_add(chunk.samples.len() as u64);
                last_audio_ts_us = chunk.timestamp_us;

                downmix_to_mono(&chunk.samples, chunk.channels, &mut mono);
                let rs = resampler.get_or_insert_with(|| LinearResampler16k::new(chunk.sample_rate));
                if rs.src_rate != chunk.sample_rate {
                    rs.reset_rate(chunk.sample_rate);
                }
                resampled.clear();
                rs.resample_mono(&mono, &mut resampled);
                audio_samples_resampled =
                    audio_samples_resampled.saturating_add(resampled.len() as u64);

                if !resampled.is_empty() && samples_seen % 5000 < resampled.len() {
                    log::trace!("Resample check: mono[0]={:.4}, resampled[0]={:.4}, rate={}", 
                                mono[0], resampled[0], chunk.sample_rate);
                }

                if !resampled.is_empty() {
                    // 1. Calculate raw RMS (already has DSP gain applied, but not internal 10x)
                    let mut sum_sq: f32 = 0.0;
                    let mut max_abs: f32 = 0.0;
                    for s in &resampled {
                        max_abs = max_abs.max(s.abs());
                        sum_sq += *s * *s;
                    }
                    let raw_rms = (sum_sq / resampled.len() as f32).sqrt();

                    // 2. Gate early: If quiet, zero everything out before boost.
                    // Evidence-driven: 0.02 was too aggressive and caused long "all blank" spans.
                    // Lower threshold to preserve low-amplitude speech.
                    let is_silent = raw_rms < current_gate_threshold;

                    // #region agent log
                    // Log when the gate flips state (SILENT <-> ACTIVE) to correlate with "blank spans".
                    if last_gate_is_silent.map(|v| v != is_silent).unwrap_or(true) {
                        let k = DBG_STT_GATE_FLIP_N.fetch_add(1, Ordering::Relaxed);
                        if k < 30 {
                            dbglog(
                                "H25",
                                "crates/parakeet_stt/src/lib.rs:worker:gate_rms",
                                "Audio gate state flip",
                                serde_json::json!({
                                    "raw_rms": raw_rms,
                                    "max_abs": max_abs,
                                    "is_silent": is_silent,
                                    "gain": current_gain,
                                    "n_samples": resampled.len()
                                }),
                            );
                        }
                        last_gate_is_silent = Some(is_silent);
                    }
                    // #endregion

                    // #region agent log
                    let _k = DBG_STT_RMS_N.fetch_add(1, Ordering::Relaxed);
                    if !is_silent {
                        let k2 = DBG_STT_RMS_ACTIVE_N.fetch_add(1, Ordering::Relaxed);
                        if k2 < 20 {
                            dbglog(
                                "H7",
                                "crates/parakeet_stt/src/lib.rs:worker:gate_rms",
                                "Audio gate decision (ACTIVE)",
                                serde_json::json!({
                                    "raw_rms": raw_rms,
                                    "max_abs": max_abs,
                                    "is_silent": is_silent,
                                    "gain": current_gain,
                                    "n_samples": resampled.len()
                                }),
                            );
                        }
                    } else {
                        let k1 = DBG_STT_RMS_SILENT_N.fetch_add(1, Ordering::Relaxed);
                        if k1 < 20 {
                            dbglog(
                                "H7",
                                "crates/parakeet_stt/src/lib.rs:worker:gate_rms",
                                "Audio gate decision (SILENT)",
                                serde_json::json!({
                                    "raw_rms": raw_rms,
                                    "max_abs": max_abs,
                                    "is_silent": is_silent,
                                    "gain": current_gain,
                                    "n_samples": resampled.len()
                                }),
                            );
                        }
                    }
                    if false {
                        dbglog(
                            "H7",
                            "crates/parakeet_stt/src/lib.rs:worker:gate_rms",
                            "Audio gate decision",
                            serde_json::json!({
                                "raw_rms": raw_rms,
                                "is_silent": is_silent,
                                "gain": current_gain,
                                "gate_threshold": current_gate_threshold,
                                "n_samples": resampled.len()
                            }),
                        );
                    }
                    // #endregion

                    // Real-time streaming: VAD/endpoint detection
                    if is_silent {
                        // Silence detected - reset speech onset tracking
                        speech_onset = None;
                        
                        if speech_active {
                            // Speech was active, start silence timer
                            if silence_start.is_none() {
                                silence_start = Some(Instant::now());
                            }
                            // Check if silence has exceeded auto_flush threshold
                            if let Some(start) = silence_start {
                                let silence_ms = start.elapsed().as_millis() as u64;
                                // Auto-flush endpoint detection
                                if auto_flush_silence_ms > 0 
                                    && silence_ms >= auto_flush_silence_ms 
                                    && !endpoint_emitted 
                                {
                                    endpoint_emitted = true;
                                    let evt = SttEvent::endpoint(
                                        silence_ms,
                                        chunk.timestamp_us / 1000,
                                        seq.fetch_add(1, Ordering::Relaxed),
                                    ).with_utterance_seq(utterance_seq);
                                    let _ = out_tx.try_send(WorkerOut::Event(evt));
                                }
                            }
                        }
                    } else {
                        // Speech (non-silent audio) detected
                        silence_start = None;  // Reset silence tracking
                        
                        if !speech_active {
                            // Track when speech started
                            if speech_onset.is_none() {
                                speech_onset = Some(Instant::now());
                            }
                            // Check if speech has been stable for hangover period
                            if let Some(onset) = speech_onset {
                                let speech_duration_ms = onset.elapsed().as_millis() as u64;
                                if speech_duration_ms >= silence_hangover_ms {
                                    speech_active = true;
                                    endpoint_emitted = false;
                                }
                            }
                        }
                    }

                    // 3. Apply internal boost
                    for s in &mut resampled {
                        if is_silent {
                            *s = 0.0;
                        } else {
                            *s *= current_gain;
                        }
                    }

                    if leveler.enabled {
                        leveler.process(&mut resampled);
                    }

                    let mut sum_sq: f32 = 0.0;
                    for s in &resampled {
                        sum_sq += *s * *s;
                        running_peak = running_peak.max(s.abs());
                    }
                    let rms = (sum_sq / resampled.len() as f32).sqrt();
                    running_rms += rms * resampled.len() as f32;
                    samples_seen += resampled.len();

                    if !is_silent {
                        // log::debug!("STT Audio Input (active): rms={:.6}, boosted={:.6}", raw_rms, boosted_rms);
                    }
                }

                if !abort_utterance {
                    if offline_mode {
                        offline_audio.extend(resampled.iter().copied());
                        if chunk_t0_us.is_none() {
                            chunk_t0_us = Some(chunk.timestamp_us);
                        }
                    } else {
                        // Feed normalized stream into 16k staging buffer (used for feature extraction).
                        sample_buf_16k.extend(resampled.iter().copied());
                        if chunk_t0_us.is_none() {
                            chunk_t0_us = Some(chunk.timestamp_us);
                        }

                        // Process fixed-size audio windows into exactly `chunk_frames` log-mel frames.
                        let mut ignored_events = 0u32;
                        while sample_buf_16k.len() >= needed_samples {
                            let t_us = chunk_t0_us.unwrap_or(chunk.timestamp_us);
                            let feature_idx = feature_chunk_idx;
                            feature_chunk_idx = feature_chunk_idx.saturating_add(1);
                            last_feature_chunk_idx = feature_idx;
                            if !process_chunk!(
                                t_us,
                                chunk.timestamp_us,
                                Some(chunk.received_at),
                                false,
                                &mut ignored_events,
                                chunk.chunk_idx,
                                feature_idx
                            ) {
                                break;
                            }
                        }
                    }
                }
                } // end of if !skip_to_finalize

                if pending_stop && in_rx.is_empty() {
                    is_running = false;
                    pending_stop = false;
                    if let Ok(mut s) = state.lock() {
                        s.status = "stopped".to_string();
                    }
                    StageTimes::mark_if_none(&mut stage_times.finalize_start);

                    let mut post_stop_decode_iters = 0usize;
                    let mut post_stop_events = 0u32;
                    let mut tail_flush_decodes = 0usize;
                    let mut queued_before = 0usize;
                    let mut queued_after = 0usize;
                    let mut staging_samples = 0usize;
                    let mut offline_frames = 0usize;

                    let pre_snapshot = StopStatsSnapshot {
                        utterance_id: &utterance_id,
                        utterance_seq,
                        staging_samples,
                        queued_before,
                        queued_after,
                        offline_frames,
                        tail_flush_decodes,
                        post_stop_decode_iters,
                        post_stop_events,
                        final_blank_penalty_delta,
                        emitted_partial_pre,
                        emitted_partial_post,
                        emitted_final_pre,
                        emitted_final_post,
                        emitted_final_empty,
                        emitted_final_nonempty,
                        suppressed_junk_partial,
                        suppressed_junk_final,
                        filter_junk: current_filter_junk,
                        offline_mode,
                        audio_chunks_seen,
                        audio_samples_seen,
                        audio_samples_resampled,
                        slow_chunk_threshold_ms,
                        slow_chunk_count,
                        slowest_chunk_ms,
                        slowest_chunk_idx,
                        slowest_chunk_audio_idx,
                        last_audio_chunk_idx,
                        last_feature_chunk_idx,
                        abort_reason: abort_reason.as_deref(),
                    };
                    let pre_stats = build_stop_stats(&pre_snapshot, &stage_times, "pre", Instant::now());
                    if let Err(e) = out_tx.try_send(WorkerOut::StopStats(pre_stats)) {
                        eprintln!(
                            "[parakeet_stt] stop_stats_send_failed phase=pre id={} utt_seq={} err={}",
                            utterance_id, utterance_seq, e
                        );
                    }

                    with_blank_penalty_delta(final_blank_penalty_delta, || {
                        if offline_mode {
                            // Offline decode: compute features over the full utterance, normalize per feature,
                            // then push features in chunks to the runtime.
                            staging_samples = offline_audio.len();
                            if !offline_audio.is_empty() {
                                let mut offline_audio = std::mem::take(&mut offline_audio);
                                if dither > 0.0 {
                                    for s in &mut offline_audio {
                                        dither_state ^= dither_state >> 12;
                                        dither_state ^= dither_state << 25;
                                        dither_state ^= dither_state >> 27;
                                        let r =
                                            (dither_state.wrapping_mul(2685821657736338717) >> 40) as u32;
                                        let u01 = (r as f32) / (u32::MAX as f32);
                                        let noise = (u01 * 2.0 - 1.0) * dither;
                                        *s += noise;
                                    }
                                }

                                let mut feats_tc = extractor.compute(&offline_audio);
                                let total_frames = feats_tc.len() / n_mels;
                                offline_frames = total_frames;
                                if normalize_mode != NormalizeMode::None && total_frames > 0 {
                                    for m in 0..n_mels {
                                        let mut sum = 0.0f64;
                                        let mut sumsq = 0.0f64;
                                        for t in 0..total_frames {
                                            let v = feats_tc[t * n_mels + m] as f64;
                                            sum += v;
                                            sumsq += v * v;
                                        }
                                        let mean = (sum / total_frames as f64) as f32;
                                        let mut var =
                                            (sumsq / total_frames as f64) - (mean as f64) * (mean as f64);
                                        if var < 0.0 {
                                            var = 0.0;
                                        }
                                        let mut std = (var as f32).sqrt();
                                        if std < 1.0e-5 {
                                            std = 1.0e-5;
                                        }
                                        for t in 0..total_frames {
                                            let idx = t * n_mels + m;
                                            feats_tc[idx] = (feats_tc[idx] - mean) / std;
                                        }
                                    }
                                }

                                let mut emitted_final = false;
                                let mut t = 0usize;
                                while t < total_frames {
                                    let frames = (total_frames - t).min(chunk_frames);
                                    let mut bct_chunk = vec![0.0f32; n_mels * frames];
                                    for tt in 0..frames {
                                        let base_tc = (t + tt) * n_mels;
                                        for m in 0..n_mels {
                                            bct_chunk[m * frames + tt] = feats_tc[base_tc + m];
                                        }
                                    }

                                    let feature_idx = feature_chunk_idx;
                                    feature_chunk_idx = feature_chunk_idx.saturating_add(1);
                                    last_feature_chunk_idx = feature_idx;
                                    StageTimes::mark_if_none(&mut stage_times.first_feature);
                                    StageTimes::mark_if_none(&mut stage_times.first_decode);
                                    let t_decode0 = Instant::now();
                                    session.set_debug_context(
                                        &utterance_id,
                                        utterance_seq,
                                        last_audio_chunk_idx,
                                        feature_idx,
                                    );
                                    if let Err(e) = session.push_features(&bct_chunk, frames) {
                                        let t_ms = chunk_t0_us.unwrap_or(last_audio_ts_us) / 1000;
                                        let ev = SttEvent::error(
                                            -4,
                                            format!("push_features failed: {e}"),
                                            t_ms,
                                            seq.fetch_add(1, Ordering::Relaxed),
                                        )
                                        .with_utterance_seq(utterance_seq);
                                        let _ = out_tx.send(WorkerOut::Event(ev));
                                        let decode_ms = t_decode0.elapsed().as_millis() as u64;
                                        if decode_ms >= slow_chunk_threshold_ms {
                                            slow_chunk_count = slow_chunk_count.saturating_add(1);
                                            if decode_ms > slowest_chunk_ms {
                                                slowest_chunk_ms = decode_ms;
                                                slowest_chunk_idx = feature_idx;
                                                slowest_chunk_audio_idx = last_audio_chunk_idx;
                                            }
                                        }
                                        break;
                                    }
                                    let decode_ms = t_decode0.elapsed().as_millis() as u64;
                                    let enc_shape = format!("1x{}x{}", n_mels, frames);
                                    let length_shape = "1".to_string();
                                    let profile_idx = 0u32;
                                    if decode_ms >= slow_chunk_threshold_ms {
                                        slow_chunk_count = slow_chunk_count.saturating_add(1);
                                        if decode_ms > slowest_chunk_ms {
                                            slowest_chunk_ms = decode_ms;
                                            slowest_chunk_idx = feature_idx;
                                            slowest_chunk_audio_idx = last_audio_chunk_idx;
                                        }
                                        if slow_chunk_count <= 8 {
                                            eprintln!(
                                                "[parakeet_stt] slow_chunk id={} utt_seq={} feature_idx={} audio_chunk_idx={} decode_ms={} queue_ms={:?} enc_shape={} length_shape={} profile_idx={} post_stop=true offline_mode={}",
                                                utterance_id,
                                                utterance_seq,
                                                feature_idx,
                                                last_audio_chunk_idx,
                                                decode_ms,
                                                Option::<u64>::None,
                                                enc_shape,
                                                length_shape,
                                                profile_idx,
                                                offline_mode
                                            );
                                            let slow_event = SlowChunk {
                                                schema_version: 1,
                                                id: if utterance_id.is_empty() {
                                                    None
                                                } else {
                                                    Some(utterance_id.clone())
                                                },
                                                utterance_seq,
                                                feature_idx,
                                                audio_chunk_idx: last_audio_chunk_idx,
                                                decode_ms,
                                                queue_ms: None,
                                                enc_shape: enc_shape.clone(),
                                                length_shape: length_shape.clone(),
                                                profile_idx,
                                                post_stop: true,
                                                offline_mode,
                                            };
                                            let _ = out_tx.try_send(WorkerOut::SlowChunk(slow_event));
                                        }
                                    }
                                    if abort_slow_chunk_ms > 0
                                        && decode_ms >= abort_slow_chunk_ms
                                        && !abort_utterance
                                    {
                                        let t_ms = chunk_t0_us.unwrap_or(last_audio_ts_us) / 1000;
                                        let msg = format!(
                                            "slow_chunk_abort decode_ms={} feature_idx={} audio_chunk_idx={} enc_shape={} length_shape={} profile_idx={}",
                                            decode_ms,
                                            feature_idx,
                                            last_audio_chunk_idx,
                                            enc_shape,
                                            length_shape,
                                            profile_idx
                                        );
                                        let ev = SttEvent::error(
                                            -5,
                                            msg,
                                            t_ms,
                                            seq.fetch_add(1, Ordering::Relaxed),
                                        )
                                        .with_utterance_seq(utterance_seq);
                                        let _ = out_tx.send(WorkerOut::Event(ev));
                                        abort_utterance = true;
                                        abort_reason = Some("slow_chunk_abort".to_string());
                                        pending_stop = true;
                                        break;
                                    }

                                    while let Some(ev) = session.poll_event() {
                                        match ev {
                                            parakeet_trt::TranscriptionEvent::PartialText { text, .. } => {
                                                StageTimes::mark_if_none(&mut stage_times.first_partial);
                                                emitted_partial_pre = emitted_partial_pre.saturating_add(1);
                                                if current_filter_junk
                                                    && (text.is_empty() || is_junk(&text))
                                                {
                                                    suppressed_junk_partial =
                                                        suppressed_junk_partial.saturating_add(1);
                                                    continue;
                                                }
                                                emitted_partial_post = emitted_partial_post.saturating_add(1);
                                                let t_ms = chunk_t0_us.unwrap_or(last_audio_ts_us) / 1000;
                                                let stable_prefix_len =
                                                    common_prefix_len_chars(&last_partial_text, &text);
                                                let stable_prefix_len =
                                                    stable_prefix_len.min(text.chars().count());
                                                last_partial_text = text.clone();
                                                let ev = SttEvent::partial(
                                                    text,
                                                    stable_prefix_len,
                                                    t_ms,
                                                    seq.fetch_add(1, Ordering::Relaxed),
                                                )
                                                .with_utterance_seq(utterance_seq);
                                                let _ = out_tx.try_send(WorkerOut::Event(ev));
                                                post_stop_events = post_stop_events.saturating_add(1);
                                            }
                                            parakeet_trt::TranscriptionEvent::FinalText { text, .. } => {
                                                StageTimes::mark_if_none(&mut stage_times.first_final);
                                                emitted_final_pre = emitted_final_pre.saturating_add(1);
                                                if text.trim().is_empty() {
                                                    emitted_final_empty =
                                                        emitted_final_empty.saturating_add(1);
                                                } else {
                                                    emitted_final_nonempty =
                                                        emitted_final_nonempty.saturating_add(1);
                                                }
                                                if current_filter_junk
                                                    && (text.is_empty() || is_junk(&text))
                                                {
                                                    suppressed_junk_final =
                                                        suppressed_junk_final.saturating_add(1);
                                                    continue;
                                                }
                                                emitted_final_post = emitted_final_post.saturating_add(1);
                                                let t_ms = chunk_t0_us.unwrap_or(last_audio_ts_us) / 1000;
                                                let ev = SttEvent::final_(
                                                    text,
                                                    t_ms,
                                                    seq.fetch_add(1, Ordering::Relaxed),
                                                )
                                                .with_utterance_seq(utterance_seq);
                                                let _ = out_tx.send(WorkerOut::Event(ev));
                                                post_stop_events = post_stop_events.saturating_add(1);
                                                last_partial_text.clear();
                                                emitted_final = true;
                                            }
                                            parakeet_trt::TranscriptionEvent::Error { message } => {
                                                let ev = SttEvent::error(
                                                    -3,
                                                    message,
                                                    chunk_t0_us.unwrap_or(last_audio_ts_us) / 1000,
                                                    seq.fetch_add(1, Ordering::Relaxed),
                                                )
                                                .with_utterance_seq(utterance_seq);
                                                let _ = out_tx.send(WorkerOut::Event(ev));
                                            }
                                        }
                                    }

                                    t += frames;
                                    post_stop_decode_iters += 1;
                                }

                                if !emitted_final && !last_partial_text.is_empty() {
                                    StageTimes::mark_if_none(&mut stage_times.first_final);
                                    let t_ms = chunk_t0_us.unwrap_or(last_audio_ts_us) / 1000;
                                    let ev = SttEvent::final_(
                                        last_partial_text.clone(),
                                        t_ms,
                                        seq.fetch_add(1, Ordering::Relaxed),
                                    )
                                    .with_utterance_seq(utterance_seq);
                                    let _ = out_tx.send(WorkerOut::Event(ev));
                                    post_stop_events = post_stop_events.saturating_add(1);
                                }
                            }
                        } else {
                            staging_samples = sample_buf_16k.len();
                            queued_before = queued_chunks(staging_samples);
                            while sample_buf_16k.len() >= needed_samples {
                                let t_us = chunk_t0_us.unwrap_or(last_audio_ts_us);
                                let feature_idx = feature_chunk_idx;
                                feature_chunk_idx = feature_chunk_idx.saturating_add(1);
                                last_feature_chunk_idx = feature_idx;
                                if !process_chunk!(
                                    t_us,
                                    last_audio_ts_us,
                                    None,
                                    true,
                                    &mut post_stop_events,
                                    last_audio_chunk_idx,
                                    feature_idx
                                ) {
                                    break;
                                }
                                post_stop_decode_iters += 1;
                            }
                            queued_after = queued_chunks(sample_buf_16k.len());

                            let quiet_iters = 5usize;
                            let mut padded_once = false;
                            for _ in 0..quiet_iters {
                                if sample_buf_16k.len() < needed_samples {
                                    if !padded_once && sample_buf_16k.len() > 0 {
                                        tail_flush_decodes = 1;
                                        padded_once = true;
                                    }
                                    sample_buf_16k.resize(needed_samples, 0.0);
                                }
                                let t_us = chunk_t0_us.unwrap_or(last_audio_ts_us);
                                let feature_idx = feature_chunk_idx;
                                feature_chunk_idx = feature_chunk_idx.saturating_add(1);
                                last_feature_chunk_idx = feature_idx;
                                if !process_chunk!(
                                    t_us,
                                    last_audio_ts_us,
                                    None,
                                    true,
                                    &mut post_stop_events,
                                    last_audio_chunk_idx,
                                    feature_idx
                                ) {
                                    break;
                                }
                                post_stop_decode_iters += 1;
                            }
                            sample_buf_16k.clear();
                            // Flush final with last hypothesis (if any).
                            if !last_partial_text.is_empty() {
                                StageTimes::mark_if_none(&mut stage_times.first_final);
                                let t_ms = chunk_t0_us.unwrap_or(last_audio_ts_us) / 1000;
                                let ev = SttEvent::final_(
                                    last_partial_text.clone(),
                                    t_ms,
                                    seq.fetch_add(1, Ordering::Relaxed),
                                )
                                .with_utterance_seq(utterance_seq);
                                let _ = out_tx.send(WorkerOut::Event(ev));
                                post_stop_events = post_stop_events.saturating_add(1);
                            } else {
                                StageTimes::mark_if_none(&mut stage_times.first_final);
                                let t_ms = chunk_t0_us.unwrap_or(last_audio_ts_us) / 1000;
                                let ev = SttEvent::final_(
                                    String::new(),
                                    t_ms,
                                    seq.fetch_add(1, Ordering::Relaxed),
                                )
                                .with_utterance_seq(utterance_seq);
                                let _ = out_tx.send(WorkerOut::Event(ev));
                                post_stop_events = post_stop_events.saturating_add(1);
                            }
                        }
                    });
                    stage_times.finalize_done = Some(Instant::now());

                    let post_snapshot = StopStatsSnapshot {
                        utterance_id: &utterance_id,
                        utterance_seq,
                        staging_samples,
                        queued_before,
                        queued_after,
                        offline_frames,
                        tail_flush_decodes,
                        post_stop_decode_iters,
                        post_stop_events,
                        final_blank_penalty_delta,
                        emitted_partial_pre,
                        emitted_partial_post,
                        emitted_final_pre,
                        emitted_final_post,
                        emitted_final_empty,
                        emitted_final_nonempty,
                        suppressed_junk_partial,
                        suppressed_junk_final,
                        filter_junk: current_filter_junk,
                        offline_mode,
                        audio_chunks_seen,
                        audio_samples_seen,
                        audio_samples_resampled,
                        slow_chunk_threshold_ms,
                        slow_chunk_count,
                        slowest_chunk_ms,
                        slowest_chunk_idx,
                        slowest_chunk_audio_idx,
                        last_audio_chunk_idx,
                        last_feature_chunk_idx,
                        abort_reason: abort_reason.as_deref(),
                    };
                    let post_stats =
                        build_stop_stats(&post_snapshot, &stage_times, "post", Instant::now());
                    if let Err(e) = out_tx.try_send(WorkerOut::StopStats(post_stats)) {
                        eprintln!(
                            "[parakeet_stt] stop_stats_send_failed phase=post id={} utt_seq={} err={}",
                            utterance_id, utterance_seq, e
                        );
                    }
                    last_partial_text.clear();
                    sample_buf_16k.clear();
                    chunk_t0_us = None;
                    norm_frames = 0;
                    for v in &mut norm_sum {
                        *v = 0.0;
                    }
                    for v in &mut norm_sumsq {
                        *v = 0.0;
                    }
                    offline_audio.clear();
                    session.reset();
                }
            }
        })
        .expect("failed to spawn parakeet_stt worker")
}

fn extractor_needed_samples(chunk_frames: usize) -> usize {
    let cfg = FeatureConfig::default();
    cfg.win_length + cfg.hop_length * (chunk_frames.saturating_sub(1))
}

fn extractor_advance_samples(chunk_frames: usize) -> usize {
    let cfg = FeatureConfig::default();
    cfg.hop_length * chunk_frames
}

fn common_prefix_len_chars(a: &str, b: &str) -> usize {
    let mut count = 0usize;
    for (ca, cb) in a.chars().zip(b.chars()) {
        if ca != cb {
            break;
        }
        count += 1;
    }
    count
}

fn is_junk(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    // If the model emits only punctuation/whitespace (e.g. "..", "..."), treat as junk.
    // This matches the observed "dots-only" UI symptom.
    if t.chars().all(|c| !c.is_alphanumeric()) {
        return true;
    }
    let lower = t.to_lowercase();
    let junk = ["z", "w", "~", ".", ",", "-", "x", "y", " ", "v"];
    junk.contains(&lower.as_str())
}

#[async_trait]
impl Processor for ParakeetSttProcessor {
    fn name(&self) -> &str {
        "Parakeet STT"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Parakeet STT".to_string(),
            description: "Streaming speech-to-text (Parakeet TRT) emitting Partial/Final/Error events".to_string(),
            ports: vec![
                Port {
                    id: "audio_in".to_string(),
                    label: "Audio In".to_string(),
                    data_type: DataType::Audio,
                    direction: PortDirection::Input,
                },
                Port {
                    id: "stt_out".to_string(),
                    label: "STT Events Out".to_string(),
                    data_type: DataType::Any,
                    direction: PortDirection::Output,
                },
            ],
            settings_schema: Some(serde_json::json!({
                "title": "STT Settings",
                "type": "object",
                "properties": {
                    "pre_gain": {
                        "type": "number",
                        "description": "Additional gain for STT input",
                        "default": 1.0
                    },
                    "gate_threshold": {
                        "type": "number",
                        "description": "RMS threshold for audio gate (0.005 default)",
                        "default": 0.005
                    },
                    "filter_junk": {
                        "type": "boolean",
                        "description": "Filter empty/punctuation-only hypotheses (UI smoothing). Disable for testing.",
                        "default": true
                    },
                    "normalize_mode": {
                        "type": "string",
                        "description": "Feature normalization mode: per_chunk | running | none",
                        "default": "per_chunk"
                    },
                    "offline_mode": {
                        "type": "boolean",
                        "description": "Accumulate full utterance before decoding (offline only).",
                        "default": false
                    },
                    "backpressure": {
                        "type": "boolean",
                        "description": "Block audio sender when the worker is behind (deterministic offline).",
                        "default": false
                    },
                    "backpressure_timeout_ms": {
                        "type": "integer",
                        "description": "Max time (ms) to wait on backpressure send before returning an error.",
                        "default": 0
                    },
                    "final_blank_penalty_delta": {
                        "type": "number",
                        "description": "Blank penalty delta applied only during post-stop finalize.",
                        "default": 0.0
                    },
                    "utterance_id": {
                        "type": "string",
                        "description": "Optional utterance identifier for debug logging."
                    },
                    "utterance_seq": {
                        "type": "integer",
                        "description": "Optional per-utterance sequence number for event correlation."
                    }
                }
            })),
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn process(&mut self, signal: Signal) -> anyhow::Result<Option<Signal>> {
        if !self.enabled {
            return Ok(None);
        }

        self.drain_worker_out();
        let mut out = self.pending_out.pop_front();

        match signal {
            Signal::Audio {
                sample_rate,
                channels,
                timestamp_us,
                data,
            } => {
                // #region agent log
                let n = DBG_STT_AUDIO_N.fetch_add(1, Ordering::Relaxed);
                let is_tick = data.len() == 1
                    && timestamp_us == 0
                    && sample_rate == 16_000
                    && channels == 1;
                let log_audio_rx = !is_tick && should_log_throttled(&DBG_STT_AUDIO_RX_MS, 1000);
                let samples_len = data.len();
                let channels_usize = channels as usize;
                let samples_per_ch = if channels_usize > 0 {
                    samples_len / channels_usize
                } else {
                    samples_len
                };
                let (muted, max_abs) = if log_audio_rx {
                    let mut max_abs = 0.0f32;
                    for s in &data {
                        max_abs = max_abs.max(s.abs());
                    }
                    (max_abs <= 1.0e-5, max_abs)
                } else {
                    (false, 0.0f32)
                };
                let chunk_idx = if is_tick {
                    0
                } else {
                    let idx = self.next_chunk_idx;
                    self.next_chunk_idx = self.next_chunk_idx.saturating_add(1);
                    idx
                };
                let chunk = AudioChunk {
                    samples: data,
                    sample_rate,
                    channels,
                    timestamp_us,
                    chunk_idx,
                    is_tick,
                    received_at: Instant::now(),
                };
                let send_ok = if self.backpressure {
                    match self.in_tx.try_send(chunk) {
                        Ok(()) => true,
                        Err(TrySendError::Full(_)) => {
                            if let Some(sig) = out.take() {
                                self.pending_out.push_front(sig);
                            }
                            return Err(anyhow::anyhow!("backpressure_full"));
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("failed to send audio chunk: {}", e));
                        }
                    }
                } else {
                    let res = self.in_tx.try_send(chunk);
                    if res.is_err() {
                        let _ = DBG_STT_AUDIO_DROP_N.fetch_add(1, Ordering::Relaxed);
                        self.total_audio_dropped += 1;
                        // Emit audio_dropped event (throttled: every 10th drop)
                        if self.total_audio_dropped % 10 == 1 {
                            let evt = SttEvent::audio_dropped(
                                1,
                                self.total_audio_dropped,
                                timestamp_us / 1000,
                                0, // seq will be assigned by consumer if needed
                            );
                            push_with_backpressure(&mut self.pending_out, evt.to_signal(), 64);
                        }
                    }
                    res.is_ok()
                };
                if log_audio_rx {
                    let dropped = !send_ok && !self.backpressure;
                    log::debug!(
                        "parakeet_stt audio rx: samples={} per_ch={} channels={} muted={} max_abs={:.6} dropping={} drop_total={} backpressure={}",
                        samples_len,
                        samples_per_ch,
                        channels,
                        muted,
                        max_abs,
                        dropped,
                        DBG_STT_AUDIO_DROP_N.load(Ordering::Relaxed),
                        self.backpressure
                    );
                }
                if (n % 200) == 0 {
                    dbglog(
                        "H3",
                        "crates/parakeet_stt/src/lib.rs:ParakeetSttProcessor:process",
                        "STT processor got Audio",
                        serde_json::json!({
                            "sample_rate": sample_rate,
                            "channels": channels,
                            "send_ok": send_ok,
                            "backpressure": self.backpressure,
                            "drop_total": DBG_STT_AUDIO_DROP_N.load(Ordering::Relaxed)
                        }),
                    );
                }
                // #endregion
            }
            Signal::Control(ctrl) => match ctrl {
                magnolia_core::ControlSignal::Settings(v) => {
                    // Minimal control surface (1C): {"action":"start"|"stop"|"reset"}
                    let action = v.get("action").and_then(|x| x.as_str()).unwrap_or("");
                    let mut reset_with_meta = false;
                    match action {
                        "start" => {
                            self.send_ctrl_with_timeout(SttControl::Start, "start")?;
                        }
                        "stop" => {
                            self.send_ctrl_with_timeout(SttControl::Stop, "stop")?;
                        }
                        "reset" => {
                            let offline = v
                                .get("offline_mode")
                                .and_then(|x| x.as_bool())
                                .unwrap_or(self.offline_mode);
                            let utterance_seq = v
                                .get("utterance_seq")
                                .and_then(|x| x.as_u64())
                                .unwrap_or(self.utterance_seq);
                            let utterance_id = v
                                .get("utterance_id")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string());
                            self.offline_mode = offline;
                            self.utterance_seq = utterance_seq;
                            self.next_chunk_idx = 0;
                            self.send_ctrl_with_timeout(
                                SttControl::ResetWithMeta {
                                    utterance_id,
                                    utterance_seq,
                                    offline_mode: offline,
                                },
                                "reset",
                            )?;
                            reset_with_meta = true;
                        }
                        _ => {}
                    }
                    
                    if let Some(g) = v.get("pre_gain").and_then(|x| x.as_f64()) {
                        self.pre_gain = g as f32;
                        self.send_ctrl_with_timeout(
                            SttControl::SetGain(self.pre_gain),
                            "set_gain",
                        )?;
                    }
                    
                    if let Some(t) = v.get("gate_threshold").and_then(|x| x.as_f64()) {
                        self.gate_threshold = t as f32;
                        self.send_ctrl_with_timeout(
                            SttControl::SetGateThreshold(self.gate_threshold),
                            "set_gate_threshold",
                        )?;
                    }

                    if let Some(f) = v.get("filter_junk").and_then(|x| x.as_bool()) {
                        self.filter_junk = f;
                        self.send_ctrl_with_timeout(
                            SttControl::SetFilterJunk(self.filter_junk),
                            "set_filter_junk",
                        )?;
                    }

                    if let Some(m) = v.get("normalize_mode").and_then(|x| x.as_str()) {
                        if let Some(mode) = NormalizeMode::from_str(m) {
                            self.normalize_mode = mode;
                            self.send_ctrl_with_timeout(
                                SttControl::SetNormalizeMode(mode),
                                "set_normalize_mode",
                            )?;
                        }
                    }

                    if let Some(offline) = v.get("offline_mode").and_then(|x| x.as_bool()) {
                        self.offline_mode = offline;
                        if !reset_with_meta {
                            self.send_ctrl_with_timeout(
                                SttControl::SetOfflineMode(offline),
                                "set_offline_mode",
                            )?;
                        }
                    }

                    if let Some(backpressure) = v.get("backpressure").and_then(|x| x.as_bool()) {
                        self.backpressure = backpressure;
                    }

                    if let Some(timeout_ms) = v.get("backpressure_timeout_ms").and_then(|x| x.as_u64())
                    {
                        self.backpressure_timeout_ms = timeout_ms;
                    }

                    if let Some(delta) = v.get("final_blank_penalty_delta").and_then(|x| x.as_f64())
                    {
                        self.final_blank_penalty_delta = delta as f32;
                        self.send_ctrl_with_timeout(
                            SttControl::SetFinalBlankPenaltyDelta(self.final_blank_penalty_delta),
                            "set_final_blank_penalty_delta",
                        )?;
                    }

                    if let Some(id) = v.get("utterance_id").and_then(|x| x.as_str()) {
                        if !reset_with_meta {
                            self.send_ctrl_with_timeout(
                                SttControl::SetUtteranceId(id.to_string()),
                                "set_utterance_id",
                            )?;
                        }
                    }

                    if let Some(seq) = v.get("utterance_seq").and_then(|x| x.as_u64()) {
                        self.utterance_seq = seq;
                        if !reset_with_meta {
                            self.send_ctrl_with_timeout(
                                SttControl::SetUtteranceSeq(seq),
                                "set_utterance_seq",
                            )?;
                        }
                    }

                    // Real-time streaming controls
                    if let Some(ms) = v.get("min_partial_emit_ms").and_then(|x| x.as_u64()) {
                        self.send_ctrl_with_timeout(
                            SttControl::SetMinPartialEmitMs(ms),
                            "set_min_partial_emit_ms",
                        )?;
                    }

                    if let Some(ms) = v.get("silence_hangover_ms").and_then(|x| x.as_u64()) {
                        self.send_ctrl_with_timeout(
                            SttControl::SetSilenceHangoverMs(ms),
                            "set_silence_hangover_ms",
                        )?;
                    }

                    if let Some(ms) = v.get("auto_flush_silence_ms").and_then(|x| x.as_u64()) {
                        self.send_ctrl_with_timeout(
                            SttControl::SetAutoFlushSilenceMs(ms),
                            "set_auto_flush_silence_ms",
                        )?;
                    }
                }
                _ => {}
            }
            _ => {}
        }

        self.drain_worker_out();
        if out.is_some() {
            return Ok(out);
        }
        out = self.pending_out.pop_front();
        Ok(out)
    }
}
