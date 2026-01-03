use async_trait::async_trait;
use magnolia_core::{DataType, ModuleSchema, Port, PortDirection, Signal, Sink};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{SlowChunk, SttEvent, SttMetrics, StopStats};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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

fn split_by_token_window(text: &str, keep_tokens: usize) -> (&str, &str) {
    if text.is_empty() || keep_tokens == 0 {
        return (text, "");
    }
    let mut starts = Vec::new();
    let mut in_token = false;
    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            in_token = false;
        } else if !in_token {
            starts.push(idx);
            in_token = true;
        }
    }
    if starts.len() <= keep_tokens {
        return ("", text);
    }
    let split_at = starts[starts.len() - keep_tokens];
    (&text[..split_at], &text[split_at..])
}

#[derive(Debug, Clone)]
pub struct TranscriptConfig {
    pub revision_tokens: usize,
    pub stable_updates: u32,
    pub max_age_ms: u64,
    pub min_update_interval_ms: u64,
    pub report_every: usize,
    pub max_samples: usize,
}

impl Default for TranscriptConfig {
    fn default() -> Self {
        Self {
            revision_tokens: 8,
            stable_updates: 3,
            max_age_ms: 1500,
            min_update_interval_ms: 40,
            report_every: 20,
            max_samples: 200,
        }
    }
}

impl TranscriptConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(v) = std::env::var("PARAKEET_REVISION_TOKENS") {
            if let Ok(n) = v.parse::<usize>() {
                cfg.revision_tokens = n;
            }
        }
        if let Ok(v) = std::env::var("PARAKEET_REVISION_STABLE_UPDATES") {
            if let Ok(n) = v.parse::<u32>() {
                cfg.stable_updates = n;
            }
        }
        if let Ok(v) = std::env::var("PARAKEET_REVISION_MAX_AGE_MS") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.max_age_ms = n;
            }
        }
        if let Ok(v) = std::env::var("PARAKEET_UI_MIN_UPDATE_MS") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.min_update_interval_ms = n;
            }
        }
        if let Ok(v) = std::env::var("PARAKEET_LATENCY_REPORT_EVERY") {
            if let Ok(n) = v.parse::<usize>() {
                cfg.report_every = n.max(1);
            }
        }
        if let Ok(v) = std::env::var("PARAKEET_LATENCY_MAX_SAMPLES") {
            if let Ok(n) = v.parse::<usize>() {
                cfg.max_samples = n.max(10);
            }
        }
        cfg
    }
}

#[derive(Debug)]
pub struct TranscriptionState {
    pub committed_text: String,
    pub partial_text: String,
    pub last_error: Option<String>,
    pub last_update_ms: u64,
    pub last_latency_ms: u64,
    pub last_stt_latency_ms: u64,
    pub last_decode_ms: u64,
    pub last_patch_start: usize,
    pub last_patch_text: String,
    pub dropped_audio_total: u64,

    cfg: TranscriptConfig,
    last_partial_text: String,
    stable_updates: u32,
    last_change_at: Instant,
    last_ui_emit: Instant,
    pending_partial: Option<String>,
    pending_partial_t_ms: u64,
    latency_samples: VecDeque<u64>,
    stt_latency_samples: VecDeque<u64>,
    decode_samples: VecDeque<u64>,
    update_count: usize,
}

impl TranscriptionState {
    pub fn new(cfg: TranscriptConfig) -> Self {
        Self {
            committed_text: String::new(),
            partial_text: String::new(),
            last_error: None,
            last_update_ms: 0,
            last_latency_ms: 0,
            last_stt_latency_ms: 0,
            last_decode_ms: 0,
            last_patch_start: 0,
            last_patch_text: String::new(),
            dropped_audio_total: 0,
            cfg,
            last_partial_text: String::new(),
            stable_updates: 0,
            last_change_at: Instant::now(),
            last_ui_emit: Instant::now() - Duration::from_secs(1),
            pending_partial: None,
            pending_partial_t_ms: 0,
            latency_samples: VecDeque::new(),
            stt_latency_samples: VecDeque::new(),
            decode_samples: VecDeque::new(),
            update_count: 0,
        }
    }

    pub fn reset(&mut self) {
        self.committed_text.clear();
        self.partial_text.clear();
        self.last_error = None;
        self.last_update_ms = 0;
        self.last_latency_ms = 0;
        self.last_stt_latency_ms = 0;
        self.last_decode_ms = 0;
        self.last_patch_start = 0;
        self.last_patch_text.clear();
        self.dropped_audio_total = 0;
        self.last_partial_text.clear();
        self.stable_updates = 0;
        self.last_change_at = Instant::now();
        self.last_ui_emit = Instant::now() - Duration::from_secs(1);
        self.pending_partial = None;
        self.pending_partial_t_ms = 0;
        self.latency_samples.clear();
        self.stt_latency_samples.clear();
        self.decode_samples.clear();
        self.update_count = 0;
    }

    pub fn apply_metrics(&mut self, metrics: SttMetrics) {
        self.last_stt_latency_ms = metrics.latency_ms;
        self.last_decode_ms = metrics.decode_ms;
        self.push_sample(&mut self.stt_latency_samples, metrics.latency_ms);
        self.push_sample(&mut self.decode_samples, metrics.decode_ms);
        self.flush_pending_if_due(Instant::now());
    }

    pub fn apply_event(&mut self, ev: SttEvent) {
        let now = Instant::now();
        let now_ms = now_ms();
        self.last_update_ms = now_ms;
        if ev.t_ms > 0 && now_ms >= ev.t_ms {
            self.last_latency_ms = now_ms - ev.t_ms;
            self.push_sample(&mut self.latency_samples, self.last_latency_ms);
        }

        match ev.kind.as_str() {
            "partial" => {
                self.apply_partial(&ev.text, ev.t_ms, now);
            }
            "final" => {
                self.apply_final(&ev.text, now);
            }
            "endpoint" => {
                self.commit_segment(now);
            }
            "error" => {
                self.last_error = Some(ev.message);
            }
            "audio_dropped" => {
                if ev.code > 0 {
                    self.dropped_audio_total = ev.code as u64;
                }
            }
            _ => {}
        }

        self.update_count = self.update_count.saturating_add(1);
        if self.cfg.report_every > 0 && (self.update_count % self.cfg.report_every) == 0 {
            self.log_latency_stats();
        }
    }

    fn apply_partial(&mut self, new_full: &str, t_ms: u64, now: Instant) {
        if self.should_throttle(now) {
            self.pending_partial = Some(new_full.to_string());
            self.pending_partial_t_ms = t_ms;
            return;
        }
        self.pending_partial = None;
        self.pending_partial_t_ms = 0;
        self.apply_partial_internal(new_full, now);
        self.last_ui_emit = now;
    }

    fn apply_partial_internal(&mut self, new_full: &str, now: Instant) {
        let old_display = format!("{}{}", self.committed_text, self.partial_text);
        let new_partial = if self.committed_text.is_empty() {
            new_full
        } else if let Some(rest) = new_full.strip_prefix(&self.committed_text) {
            rest
        } else {
            self.last_error = Some("partial mismatch with committed prefix".to_string());
            new_full
        };

        if new_partial == self.last_partial_text {
            self.stable_updates = self.stable_updates.saturating_add(1);
        } else {
            self.stable_updates = 0;
            self.last_change_at = now;
            self.last_partial_text = new_partial.to_string();
        }

        let stable_ready = self.cfg.stable_updates > 0 && self.stable_updates >= self.cfg.stable_updates;
        let age_ready = self.cfg.max_age_ms > 0
            && now.duration_since(self.last_change_at) >= Duration::from_millis(self.cfg.max_age_ms);
        if stable_ready || age_ready {
            let (commit_prefix, keep_suffix) = split_by_token_window(new_partial, self.cfg.revision_tokens);
            if !commit_prefix.is_empty() {
                self.committed_text.push_str(commit_prefix);
                self.partial_text = keep_suffix.to_string();
                self.last_partial_text = self.partial_text.clone();
                self.stable_updates = 0;
                self.last_change_at = now;
            } else {
                self.partial_text = new_partial.to_string();
                self.last_partial_text = self.partial_text.clone();
            }
        } else {
            self.partial_text = new_partial.to_string();
            self.last_partial_text = self.partial_text.clone();
        }

        let new_display = format!("{}{}", self.committed_text, self.partial_text);
        self.update_patch(&old_display, &new_display);
    }

    fn apply_final(&mut self, text: &str, now: Instant) {
        self.pending_partial = None;
        self.pending_partial_t_ms = 0;
        if self.partial_text.is_empty() && !text.trim().is_empty() {
            self.committed_text.push_str(text);
        } else {
            self.commit_segment(now);
        }
        let display = format!("{}{}", self.committed_text, self.partial_text);
        self.update_patch("", &display);
        self.last_ui_emit = now;
    }

    fn commit_segment(&mut self, now: Instant) {
        self.pending_partial = None;
        self.pending_partial_t_ms = 0;
        if !self.partial_text.is_empty() {
            self.committed_text.push_str(&self.partial_text);
            self.partial_text.clear();
        }
        if !self.committed_text.ends_with('\n') && !self.committed_text.is_empty() {
            self.committed_text.push('\n');
        }
        self.last_partial_text.clear();
        self.stable_updates = 0;
        self.last_change_at = now;
        self.last_ui_emit = now;
    }

    fn update_patch(&mut self, old_display: &str, new_display: &str) {
        let prefix = common_prefix_len_chars(old_display, new_display);
        self.last_patch_start = prefix;
        self.last_patch_text = new_display.chars().skip(prefix).collect();
    }

    fn push_sample(&mut self, buf: &mut VecDeque<u64>, val: u64) {
        if buf.len() >= self.cfg.max_samples {
            let _ = buf.pop_front();
        }
        buf.push_back(val);
    }

    fn log_latency_stats(&self) {
        let p50 = percentile(&self.latency_samples, 0.50);
        let p95 = percentile(&self.latency_samples, 0.95);
        let stt_p50 = percentile(&self.stt_latency_samples, 0.50);
        let stt_p95 = percentile(&self.stt_latency_samples, 0.95);
        let dec_p50 = percentile(&self.decode_samples, 0.50);
        let dec_p95 = percentile(&self.decode_samples, 0.95);
        log::info!(
            "[transcription] latency_ms p50={} p95={} stt_ms p50={} p95={} decode_ms p50={} p95={}",
            p50,
            p95,
            stt_p50,
            stt_p95,
            dec_p50,
            dec_p95
        );
    }

    fn should_throttle(&self, now: Instant) -> bool {
        if self.cfg.min_update_interval_ms == 0 {
            return false;
        }
        now.duration_since(self.last_ui_emit)
            < Duration::from_millis(self.cfg.min_update_interval_ms)
    }

    fn flush_pending_if_due(&mut self, now: Instant) {
        if self.pending_partial.is_none() {
            return;
        }
        if self.should_throttle(now) {
            return;
        }
        if let Some(text) = self.pending_partial.take() {
            self.pending_partial_t_ms = 0;
            self.apply_partial_internal(&text, now);
            self.last_ui_emit = now;
        }
    }
}

fn percentile(buf: &VecDeque<u64>, q: f64) -> u64 {
    if buf.is_empty() {
        return 0;
    }
    let mut v: Vec<u64> = buf.iter().copied().collect();
    v.sort_unstable();
    let rank = ((v.len() - 1) as f64 * q).round() as usize;
    v.get(rank).copied().unwrap_or(0)
}

pub struct TranscriptionSink {
    id: String,
    enabled: bool,
    state: Arc<Mutex<TranscriptionState>>,
}

impl TranscriptionSink {
    pub fn new(id: &str, state: Arc<Mutex<TranscriptionState>>) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            state,
        }
    }
}

#[async_trait]
impl Sink for TranscriptionSink {
    fn name(&self) -> &str {
        "Transcription Sink"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Transcription Sink".to_string(),
            description: "Consumes STT events and maintains committed/partial transcript state".to_string(),
            ports: vec![Port {
                id: "stt_in".to_string(),
                label: "STT Events".to_string(),
                data_type: DataType::Any,
                direction: PortDirection::Input,
            }],
            settings_schema: None,
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn consume(&self, signal: Signal) -> anyhow::Result<Option<Signal>> {
        if !self.enabled {
            return Ok(None);
        }

        let Signal::Computed { source, content } = signal else {
            return Ok(None);
        };

        if source == "stt_metrics" {
            if let Ok(metrics) = serde_json::from_str::<SttMetrics>(&content) {
                if let Ok(mut st) = self.state.lock() {
                    st.apply_metrics(metrics);
                }
            }
            return Ok(None);
        }

        if source == "stt_slow_chunk" {
            if let Ok(chunk) = serde_json::from_str::<SlowChunk>(&content) {
                let queue_ms = chunk.queue_ms.unwrap_or(0);
                log::warn!(
                    "[transcription] slow_chunk decode_ms={} queue_ms={} enc_shape={} profile={}",
                    chunk.decode_ms,
                    queue_ms,
                    chunk.enc_shape,
                    chunk.profile_idx
                );
            }
            return Ok(None);
        }

        if source == "stt_stop_stats" {
            if let Ok(stats) = serde_json::from_str::<StopStats>(&content) {
                log::info!(
                    "[transcription] stop_stats queued_before={} queued_after={} audio_chunks_seen={}",
                    stats.queued_before,
                    stats.queued_after,
                    stats.audio_chunks_seen
                );
            }
            return Ok(None);
        }

        if source == "stt_reset_ack" {
            if let Ok(mut st) = self.state.lock() {
                st.reset();
            }
            return Ok(None);
        }

        if matches!(
            source.as_str(),
            "stt_partial" | "stt_final" | "stt_error" | "stt_endpoint" | "stt_audio_dropped"
        ) {
            if let Ok(ev) = serde_json::from_str::<SttEvent>(&content) {
                if let Ok(mut st) = self.state.lock() {
                    st.apply_event(ev);
                }
            }
        }

        Ok(None)
    }
}

// V2 interface placeholders (fan-out, lanes, aggregators).
pub struct HypothesisCandidate {
    pub lane_id: String,
    pub text: String,
    pub t_ms: u64,
}

pub trait EngineLane {
    fn id(&self) -> &str;
}

pub trait HypothesisAggregator {
    fn submit(&mut self, candidate: HypothesisCandidate);
    fn best(&self) -> Option<&HypothesisCandidate>;
}
