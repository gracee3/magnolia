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

fn push_sample(buf: &mut VecDeque<u64>, val: u64, max_samples: usize) {
    if buf.len() >= max_samples {
        let _ = buf.pop_front();
    }
    buf.push_back(val);
}

#[derive(Debug, Clone)]
pub struct TranscriptConfig {
    pub revision_tokens: usize,
    pub stable_updates: u32,
    pub max_age_ms: u64,
    pub min_update_interval_ms: u64,
    pub report_every: usize,
    pub report_interval_ms: u64,
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
            report_interval_ms: 5000,
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
        if let Ok(v) = std::env::var("PARAKEET_LATENCY_REPORT_INTERVAL_MS") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.report_interval_ms = n.max(250);
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
    pub last_capture_end_ms: u64,
    pub last_dsp_done_ms: u64,
    pub last_infer_done_ms: u64,
    pub last_q_audio_len: usize,
    pub last_q_audio_age_ms: u64,
    pub last_q_staging_samples: usize,
    pub last_q_staging_ms: u64,
    pub hyp_received: u64,
    pub hyp_rendered: u64,
    pub hyp_dropped_throttle: u64,

    cfg: TranscriptConfig,
    last_partial_text: String,
    stable_updates: u32,
    last_change_at: Instant,
    last_ui_emit: Instant,
    pending_partial: Option<String>,
    pending_set_at: Option<Instant>,
    last_report_at: Instant,
    last_report_hyp_received: u64,
    last_report_hyp_rendered: u64,
    last_report_hyp_dropped: u64,
    stt_latency_samples: VecDeque<u64>,
    decode_samples: VecDeque<u64>,
    cap_to_dsp_samples: VecDeque<u64>,
    dsp_to_infer_samples: VecDeque<u64>,
    infer_to_ui_samples: VecDeque<u64>,
    e2e_samples: VecDeque<u64>,
    window_max_audio_queue_len: usize,
    window_max_audio_queue_age_ms: u64,
    window_max_staging_samples: usize,
    window_max_staging_ms: u64,
    window_max_ui_pending_len: usize,
    window_max_ui_pending_age_ms: u64,
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
            last_capture_end_ms: 0,
            last_dsp_done_ms: 0,
            last_infer_done_ms: 0,
            last_q_audio_len: 0,
            last_q_audio_age_ms: 0,
            last_q_staging_samples: 0,
            last_q_staging_ms: 0,
            hyp_received: 0,
            hyp_rendered: 0,
            hyp_dropped_throttle: 0,
            cfg,
            last_partial_text: String::new(),
            stable_updates: 0,
            last_change_at: Instant::now(),
            last_ui_emit: Instant::now() - Duration::from_secs(1),
            pending_partial: None,
            pending_set_at: None,
            last_report_at: Instant::now(),
            last_report_hyp_received: 0,
            last_report_hyp_rendered: 0,
            last_report_hyp_dropped: 0,
            stt_latency_samples: VecDeque::new(),
            decode_samples: VecDeque::new(),
            cap_to_dsp_samples: VecDeque::new(),
            dsp_to_infer_samples: VecDeque::new(),
            infer_to_ui_samples: VecDeque::new(),
            e2e_samples: VecDeque::new(),
            window_max_audio_queue_len: 0,
            window_max_audio_queue_age_ms: 0,
            window_max_staging_samples: 0,
            window_max_staging_ms: 0,
            window_max_ui_pending_len: 0,
            window_max_ui_pending_age_ms: 0,
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
        self.last_capture_end_ms = 0;
        self.last_dsp_done_ms = 0;
        self.last_infer_done_ms = 0;
        self.last_q_audio_len = 0;
        self.last_q_audio_age_ms = 0;
        self.last_q_staging_samples = 0;
        self.last_q_staging_ms = 0;
        self.hyp_received = 0;
        self.hyp_rendered = 0;
        self.hyp_dropped_throttle = 0;
        self.last_partial_text.clear();
        self.stable_updates = 0;
        self.last_change_at = Instant::now();
        self.last_ui_emit = Instant::now() - Duration::from_secs(1);
        self.pending_partial = None;
        self.pending_set_at = None;
        self.last_report_at = Instant::now();
        self.last_report_hyp_received = 0;
        self.last_report_hyp_rendered = 0;
        self.last_report_hyp_dropped = 0;
        self.stt_latency_samples.clear();
        self.decode_samples.clear();
        self.cap_to_dsp_samples.clear();
        self.dsp_to_infer_samples.clear();
        self.infer_to_ui_samples.clear();
        self.e2e_samples.clear();
        self.window_max_audio_queue_len = 0;
        self.window_max_audio_queue_age_ms = 0;
        self.window_max_staging_samples = 0;
        self.window_max_staging_ms = 0;
        self.window_max_ui_pending_len = 0;
        self.window_max_ui_pending_age_ms = 0;
        self.update_count = 0;
    }

    pub fn ui_min_update_interval_ms(&self) -> u64 {
        self.cfg.min_update_interval_ms
    }

    pub fn set_ui_min_update_interval_ms(&mut self, ms: u64) {
        self.cfg.min_update_interval_ms = ms;
    }

    pub fn apply_metrics(&mut self, metrics: SttMetrics) {
        self.last_stt_latency_ms = metrics.latency_ms;
        self.last_decode_ms = metrics.decode_ms;
        let max_samples = self.cfg.max_samples;
        push_sample(&mut self.stt_latency_samples, metrics.latency_ms, max_samples);
        push_sample(&mut self.decode_samples, metrics.decode_ms, max_samples);
        self.flush_pending_if_due(Instant::now());
    }

    pub fn apply_event(&mut self, ev: SttEvent) {
        let now = Instant::now();
        self.last_update_ms = now_ms();
        if ev.t_capture_end_ms > 0 {
            self.last_capture_end_ms = ev.t_capture_end_ms;
        }
        if ev.t_dsp_done_ms > 0 {
            self.last_dsp_done_ms = ev.t_dsp_done_ms;
        }
        if ev.t_infer_done_ms > 0 {
            self.last_infer_done_ms = ev.t_infer_done_ms;
        }
        if ev.t_dsp_done_ms > 0 || ev.t_capture_end_ms > 0 || ev.t_infer_done_ms > 0 {
            self.last_q_audio_len = ev.q_audio_len;
            self.last_q_audio_age_ms = ev.q_audio_age_ms;
            self.last_q_staging_samples = ev.q_staging_samples;
            self.last_q_staging_ms = ev.q_staging_ms;
            self.window_max_audio_queue_len =
                self.window_max_audio_queue_len.max(self.last_q_audio_len);
            self.window_max_audio_queue_age_ms = self
                .window_max_audio_queue_age_ms
                .max(self.last_q_audio_age_ms);
            self.window_max_staging_samples =
                self.window_max_staging_samples.max(self.last_q_staging_samples);
            self.window_max_staging_ms = self.window_max_staging_ms.max(self.last_q_staging_ms);
        }

        match ev.kind.as_str() {
            "partial" => {
                self.apply_partial(&ev.text, now);
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

    }

    fn apply_partial(&mut self, new_full: &str, now: Instant) {
        self.hyp_received = self.hyp_received.saturating_add(1);
        if self.should_throttle(now) {
            if self.pending_partial.is_some() {
                self.hyp_dropped_throttle = self.hyp_dropped_throttle.saturating_add(1);
            }
            self.pending_partial = Some(new_full.to_string());
            self.pending_set_at = Some(now);
            return;
        }
        self.pending_partial = None;
        self.pending_set_at = None;
        self.apply_partial_internal(new_full, now, 0, 0);
        self.last_ui_emit = now;
    }

    fn apply_partial_internal(&mut self, new_full: &str, now: Instant, ui_pending_len: usize, ui_pending_age_ms: u64) {
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
        self.hyp_rendered = self.hyp_rendered.saturating_add(1);
        self.record_flush(now, ui_pending_len, ui_pending_age_ms);
    }

    fn apply_final(&mut self, text: &str, now: Instant) {
        self.pending_partial = None;
        self.pending_set_at = None;
        if self.partial_text.is_empty() && !text.trim().is_empty() {
            self.committed_text.push_str(text);
        } else {
            self.commit_segment(now);
        }
        let display = format!("{}{}", self.committed_text, self.partial_text);
        self.update_patch("", &display);
        self.last_ui_emit = now;
        self.record_flush(now, 0, 0);
    }

    fn commit_segment(&mut self, now: Instant) {
        self.pending_partial = None;
        self.pending_set_at = None;
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

    fn record_flush(&mut self, now: Instant, ui_pending_len: usize, ui_pending_age_ms: u64) {
        let now_ms = now_ms();
        let mut cap_to_dsp_ms = 0;
        let mut dsp_to_infer_ms = 0;
        let mut infer_to_ui_ms = 0;
        let mut e2e_ms = 0;
        let max_samples = self.cfg.max_samples;

        if self.last_capture_end_ms > 0 && self.last_dsp_done_ms >= self.last_capture_end_ms {
            cap_to_dsp_ms = self.last_dsp_done_ms - self.last_capture_end_ms;
            push_sample(&mut self.cap_to_dsp_samples, cap_to_dsp_ms, max_samples);
        }
        if self.last_dsp_done_ms > 0 && self.last_infer_done_ms >= self.last_dsp_done_ms {
            dsp_to_infer_ms = self.last_infer_done_ms - self.last_dsp_done_ms;
            push_sample(&mut self.dsp_to_infer_samples, dsp_to_infer_ms, max_samples);
        }
        if self.last_infer_done_ms > 0 && now_ms >= self.last_infer_done_ms {
            infer_to_ui_ms = now_ms - self.last_infer_done_ms;
            push_sample(&mut self.infer_to_ui_samples, infer_to_ui_ms, max_samples);
        }
        if self.last_capture_end_ms > 0 && now_ms >= self.last_capture_end_ms {
            e2e_ms = now_ms - self.last_capture_end_ms;
            self.last_latency_ms = e2e_ms;
            push_sample(&mut self.e2e_samples, e2e_ms, max_samples);
        }

        self.window_max_ui_pending_len = self.window_max_ui_pending_len.max(ui_pending_len);
        self.window_max_ui_pending_age_ms =
            self.window_max_ui_pending_age_ms.max(ui_pending_age_ms);

        if self.last_capture_end_ms > 0 || self.last_infer_done_ms > 0 {
            log::debug!(
                "[transcription] latency_trace cap_to_dsp_ms={} dsp_to_infer_ms={} infer_to_ui_ms={} e2e_ms={} audio_q_len={} audio_q_age_ms={} staging_samples={} staging_ms={} ui_pending_len={} ui_pending_age_ms={}",
                cap_to_dsp_ms,
                dsp_to_infer_ms,
                infer_to_ui_ms,
                e2e_ms,
                self.last_q_audio_len,
                self.last_q_audio_age_ms,
                self.last_q_staging_samples,
                self.last_q_staging_ms,
                ui_pending_len,
                ui_pending_age_ms
            );
        }

        self.update_count = self.update_count.saturating_add(1);
        self.maybe_report(now);
    }

    fn maybe_report(&mut self, now: Instant) {
        let due_by_time = self.cfg.report_interval_ms > 0
            && now.duration_since(self.last_report_at)
                >= Duration::from_millis(self.cfg.report_interval_ms);
        let due_by_count =
            self.cfg.report_every > 0 && (self.update_count % self.cfg.report_every) == 0;
        if !due_by_time && !due_by_count {
            return;
        }
        self.last_report_at = now;
        self.log_latency_stats();
    }

    fn log_latency_stats(&mut self) {
        let e2e_p50 = percentile(&self.e2e_samples, 0.50);
        let e2e_p95 = percentile(&self.e2e_samples, 0.95);
        let e2e_p99 = percentile(&self.e2e_samples, 0.99);
        let cap_p50 = percentile(&self.cap_to_dsp_samples, 0.50);
        let cap_p95 = percentile(&self.cap_to_dsp_samples, 0.95);
        let cap_p99 = percentile(&self.cap_to_dsp_samples, 0.99);
        let dsp_p50 = percentile(&self.dsp_to_infer_samples, 0.50);
        let dsp_p95 = percentile(&self.dsp_to_infer_samples, 0.95);
        let dsp_p99 = percentile(&self.dsp_to_infer_samples, 0.99);
        let ui_p50 = percentile(&self.infer_to_ui_samples, 0.50);
        let ui_p95 = percentile(&self.infer_to_ui_samples, 0.95);
        let ui_p99 = percentile(&self.infer_to_ui_samples, 0.99);
        let stt_p50 = percentile(&self.stt_latency_samples, 0.50);
        let stt_p95 = percentile(&self.stt_latency_samples, 0.95);
        let stt_p99 = percentile(&self.stt_latency_samples, 0.99);
        let dec_p50 = percentile(&self.decode_samples, 0.50);
        let dec_p95 = percentile(&self.decode_samples, 0.95);
        let dec_p99 = percentile(&self.decode_samples, 0.99);
        log::info!(
            "[transcription] latency_ms e2e_p50={} e2e_p95={} e2e_p99={} cap_dsp_p50={} cap_dsp_p95={} cap_dsp_p99={} dsp_inf_p50={} dsp_inf_p95={} dsp_inf_p99={} inf_ui_p50={} inf_ui_p95={} inf_ui_p99={} stt_p50={} stt_p95={} stt_p99={} decode_p50={} decode_p95={} decode_p99={}",
            e2e_p50,
            e2e_p95,
            e2e_p99,
            cap_p50,
            cap_p95,
            cap_p99,
            dsp_p50,
            dsp_p95,
            dsp_p99,
            ui_p50,
            ui_p95,
            ui_p99,
            stt_p50,
            stt_p95,
            stt_p99,
            dec_p50,
            dec_p95,
            dec_p99
        );

        let hyp_recv = self.hyp_received.saturating_sub(self.last_report_hyp_received);
        let hyp_render = self.hyp_rendered.saturating_sub(self.last_report_hyp_rendered);
        let hyp_drop = self
            .hyp_dropped_throttle
            .saturating_sub(self.last_report_hyp_dropped);
        log::info!(
            "[transcription] queues audio_len_max={} audio_age_max_ms={} staging_samples_max={} staging_ms_max={} ui_pending_max={} ui_pending_age_ms={} hyp_recv={} hyp_render={} hyp_drop={}",
            self.window_max_audio_queue_len,
            self.window_max_audio_queue_age_ms,
            self.window_max_staging_samples,
            self.window_max_staging_ms,
            self.window_max_ui_pending_len,
            self.window_max_ui_pending_age_ms,
            hyp_recv,
            hyp_render,
            hyp_drop
        );

        self.last_report_hyp_received = self.hyp_received;
        self.last_report_hyp_rendered = self.hyp_rendered;
        self.last_report_hyp_dropped = self.hyp_dropped_throttle;
        self.window_max_audio_queue_len = 0;
        self.window_max_audio_queue_age_ms = 0;
        self.window_max_staging_samples = 0;
        self.window_max_staging_ms = 0;
        self.window_max_ui_pending_len = 0;
        self.window_max_ui_pending_age_ms = 0;
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
            let ui_pending_age_ms = self
                .pending_set_at
                .map(|t| now.duration_since(t).as_millis() as u64)
                .unwrap_or(0);
            self.pending_set_at = None;
            self.apply_partial_internal(&text, now, 1, ui_pending_age_ms);
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
