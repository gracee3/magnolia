use crate::dataset::ManifestEntry;
use crate::gpu_telemetry::GpuTelemetry;
use crate::wer;
use anyhow::Context;
use parakeet_stt::{ParakeetSttProcessor, ParakeetSttState, SttEvent, SttMetrics};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use magnolia_core::{ControlSignal, Processor, Signal};

#[derive(Debug, Clone)]
pub struct ParakeetEngine {
    pub model_dir: PathBuf,
    pub device_id: i32,
}

pub fn resolve_parakeet_engine(args: &crate::Args) -> anyhow::Result<ParakeetEngine> {
    // Preferred path: daemon loader (configs/layout.toml). Allow overrides.
    let base = magnolia_config::load_parakeet_stt_settings().ok();

    let model_dir = match (&args.model_dir, &base) {
        (Some(m), _) => m.clone(),
        (None, Some(b)) => b.model_dir.clone(),
        (None, None) => {
            anyhow::bail!("No model_dir provided and failed to load STT config from layout.toml; pass --model-dir explicitly.")
        }
    };

    let device_u32 = match (args.device_id, &base) {
        (Some(d), _) => d as u32,
        (None, Some(b)) => b.device,
        (None, None) => 0,
    };

    // Validate engines + vocab (even though vocab isn't needed directly here, this matches daemon behavior).
    let _vocab = magnolia_config::validate_parakeet_assets(&model_dir)?;

    Ok(ParakeetEngine {
        model_dir,
        device_id: device_u32 as i32,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtteranceResult {
    pub id: String,
    pub wav: String,
    pub reference: String,
    pub hypothesis: String,
    pub status: String, // ok | stt_error | no_final | empty_hyp | truncation
    pub wer: f32,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub partials: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metrics: Option<SttMetrics>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub total: usize,
    pub ok: usize,
    pub failures: usize,
    pub empty_hyp: usize,
    pub no_final: usize,
    pub stt_error: usize,
    pub truncation: usize,
    pub stop_stats_missing: usize,
    pub aggregate_wer: f32,
    pub sum_edits: usize,
    pub sum_ref_words: usize,
}

#[derive(Debug)]
struct RunOutcome {
    result: UtteranceResult,
    poison_reason: Option<String>,
}

fn poison_reason_from_error(err: &str) -> Option<String> {
    let err = err.trim();
    if err.contains("tick_timeout") {
        return Some("tick_timeout".to_string());
    }
    if err.contains("slow_chunk_abort") {
        return Some("slow_chunk_abort".to_string());
    }
    if err == "stop_stats_timeout" {
        return Some("stop_stats_timeout".to_string());
    }
    if err.starts_with("timeout_") {
        return Some(err.to_string());
    }
    None
}

fn utterance_error_result(entry: &ManifestEntry, err: anyhow::Error) -> UtteranceResult {
    let hypothesis = String::new();
    let wer_stats = wer::wer(&entry.text, &hypothesis);
    UtteranceResult {
        id: entry.id.clone(),
        wav: entry.wav.clone(),
        reference: entry.text.clone(),
        hypothesis,
        status: "stt_error".to_string(),
        wer: wer_stats.wer,
        partials: Vec::new(),
        metrics: None,
        error: Some(err.to_string()),
    }
}

fn update_progress_counts(
    status: &str,
    ok: &mut usize,
    truncation: &mut usize,
    empty_hyp: &mut usize,
    no_final: &mut usize,
    stt_error: &mut usize,
    stop_stats_missing: &mut usize,
) {
    match status {
        "ok" => *ok += 1,
        "stop_stats_missing_only" => {
            *ok += 1;
            *stop_stats_missing += 1;
        }
        "truncation" => *truncation += 1,
        "empty_hyp" => *empty_hyp += 1,
        "no_final" => *no_final += 1,
        "stt_error" => *stt_error += 1,
        _ => {}
    }
}

fn maybe_print_progress(
    completed: usize,
    total: usize,
    ok: usize,
    truncation: usize,
    empty_hyp: usize,
    no_final: usize,
    stt_error: usize,
    stop_stats_missing: usize,
) {
    if completed % 5 == 0 || completed == total {
        eprintln!(
            "[asr_test] progress {}/{} ok={} truncation={} empty_hyp={} no_final={} stt_error={} stop_stats_missing={}",
            completed, total, ok, truncation, empty_hyp, no_final, stt_error, stop_stats_missing
        );
    }
}

fn static_settings(
    normalize_mode: &str,
    pre_gain: f32,
    backpressure: bool,
    backpressure_timeout_ms: u64,
    final_blank_penalty_delta: f32,
) -> serde_json::Value {
    json!({
        "action": "configure",
        // Testing policy: disable RMS gate so low-amplitude LibriSpeech utterances are not zeroed.
        "gate_threshold": 0.0,
        // Slightly higher gain for low-amplitude LibriSpeech samples.
        "pre_gain": pre_gain,
        // Testing policy: do not filter punctuation-only/short hypotheses.
        "filter_junk": false,
        // Feature normalization mode.
        "normalize_mode": normalize_mode,
        "offline_mode": false,
        "backpressure": backpressure,
        "backpressure_timeout_ms": backpressure_timeout_ms,
        "final_blank_penalty_delta": final_blank_penalty_delta
    })
}

fn reset_settings(utterance_id: &str, utterance_seq: u64, offline_mode: bool) -> serde_json::Value {
    json!({
        "action": "reset",
        "utterance_id": utterance_id,
        "utterance_seq": utterance_seq,
        "offline_mode": offline_mode
    })
}

async fn build_worker(
    id: &str,
    engine: &ParakeetEngine,
    normalize_mode: &str,
    pre_gain: f32,
    backpressure: bool,
    backpressure_timeout_ms: u64,
    final_blank_penalty_delta: f32,
) -> anyhow::Result<ParakeetSttProcessor> {
    let stt_state = Arc::new(Mutex::new(ParakeetSttState::default()));
    let mut stt = ParakeetSttProcessor::new(
        id,
        engine.model_dir.to_string_lossy().to_string(),
        engine.device_id,
        stt_state,
    )?;
    let static_cfg = static_settings(
        normalize_mode,
        pre_gain,
        backpressure,
        backpressure_timeout_ms,
        final_blank_penalty_delta,
    );
    send_settings_and_tick(&mut stt, static_cfg, backpressure_timeout_ms).await?;
    Ok(stt)
}

#[derive(Debug, Clone, Deserialize)]
struct StopStats {
    schema_version: u32,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    utterance_seq: u64,
    staging_samples: usize,
    queued_before: usize,
    queued_after: usize,
    offline_frames: usize,
    tail_flush_decodes: usize,
    post_stop_decode_iters: usize,
    post_stop_events: u32,
    final_blank_penalty_delta: f32,
    #[serde(default)]
    emitted_partial_pre: u64,
    #[serde(default)]
    emitted_partial_post: u64,
    #[serde(default)]
    emitted_final_pre: u64,
    #[serde(default)]
    emitted_final_post: u64,
    #[serde(default)]
    emitted_final_empty: u64,
    #[serde(default)]
    emitted_final_nonempty: u64,
    #[serde(default)]
    suppressed_junk_partial: u64,
    #[serde(default)]
    suppressed_junk_final: u64,
    #[serde(default)]
    filter_junk: bool,
    #[serde(default)]
    offline_mode: bool,
    #[serde(default)]
    audio_chunks_seen: u64,
    #[serde(default)]
    audio_samples_seen: u64,
    #[serde(default)]
    audio_samples_resampled: u64,
    #[serde(default)]
    t_reset_start_ms: Option<u64>,
    #[serde(default)]
    t_reset_done_ms: Option<u64>,
    #[serde(default)]
    t_first_audio_ms: Option<u64>,
    #[serde(default)]
    t_first_feature_ms: Option<u64>,
    #[serde(default)]
    t_first_decode_ms: Option<u64>,
    #[serde(default)]
    t_first_partial_ms: Option<u64>,
    #[serde(default)]
    t_first_final_ms: Option<u64>,
    #[serde(default)]
    t_stop_received_ms: Option<u64>,
    #[serde(default)]
    t_finalize_start_ms: Option<u64>,
    #[serde(default)]
    t_finalize_done_ms: Option<u64>,
    #[serde(default)]
    t_stop_stats_ms: Option<u64>,
    #[serde(default)]
    finalize_ms: Option<u64>,
    #[serde(default)]
    slow_chunk_threshold_ms: u64,
    #[serde(default)]
    slow_chunk_count: u64,
    #[serde(default)]
    slowest_chunk_ms: u64,
    #[serde(default)]
    slowest_chunk_idx: u64,
    #[serde(default)]
    slowest_chunk_audio_idx: u64,
    #[serde(default)]
    last_audio_chunk_idx: u64,
    #[serde(default)]
    last_feature_chunk_idx: u64,
    #[serde(default)]
    abort_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResetAck {
    #[allow(dead_code)]
    schema_version: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    utterance_seq: u64,
    #[serde(default)]
    drained_audio: usize,
    #[serde(default)]
    ctrl_queued: usize,
    #[serde(default)]
    offline_mode: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ChunkAck {
    #[allow(dead_code)]
    schema_version: u32,
    #[serde(default)]
    utterance_seq: u64,
    #[serde(default)]
    chunk_idx: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct SlowChunk {
    #[allow(dead_code)]
    schema_version: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    utterance_seq: u64,
    #[serde(default)]
    feature_idx: u64,
    #[serde(default)]
    audio_chunk_idx: u64,
    #[serde(default)]
    decode_ms: u64,
    #[serde(default)]
    queue_ms: Option<u64>,
    #[serde(default)]
    enc_shape: String,
    #[serde(default)]
    length_shape: String,
    #[serde(default)]
    profile_idx: u32,
    #[serde(default)]
    post_stop: bool,
    #[serde(default)]
    offline_mode: bool,
}

#[derive(Clone, Debug)]
struct TimingStats {
    start: Instant,
    first_partial_ms: Option<u64>,
    first_final_ms: Option<u64>,
}

impl TimingStats {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            first_partial_ms: None,
            first_final_ms: None,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

#[derive(Debug, Default, Clone)]
struct PassDiagnostics {
    saw_slow_chunk: bool,
    saw_error_event: bool,
}

fn matches_utterance_seq(target: u64, event: u64) -> bool {
    target == 0 || event == 0 || target == event
}

fn preview_text(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in input.chars().take(max_chars) {
        if ch == '\n' {
            out.push_str("\\n");
        } else {
            out.push(ch);
        }
    }
    out
}

fn fmt_opt_ms(value: Option<u64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn gpu_tick(gpu: &mut Option<GpuTelemetry>) {
    if let Some(gpu) = gpu.as_mut() {
        gpu.maybe_sample();
    }
}

fn gpu_stage(
    gpu: &mut Option<GpuTelemetry>,
    utterance_id: &str,
    utterance_seq: u64,
    stage: &str,
    extra: Option<&str>,
) {
    if let Some(gpu) = gpu.as_mut() {
        gpu.mark_stage(utterance_id, utterance_seq, stage, extra);
    }
}

fn stop_stats_is_post(stats: &StopStats) -> bool {
    stats.phase.as_deref() == Some("post")
}

fn stop_stats_is_pre(stats: &StopStats) -> bool {
    stats.phase.as_deref() == Some("pre")
}

fn is_backpressure_full_err(err: &anyhow::Error) -> bool {
    err.to_string().trim() == "backpressure_full"
}

async fn process_audio_with_backpressure(
    stt: &mut ParakeetSttProcessor,
    sig: Signal,
    backpressure_timeout_ms: u64,
    backpressure_deadline_ms: u64,
    backpressure_retry_sleep_us: u64,
    utterance_timeout: Option<Duration>,
    timing: &TimingStats,
    utterance_id: &str,
    utterance_seq: u64,
    chunk_idx: usize,
    log_backpressure: bool,
    phase: &str,
    offline_mode: bool,
) -> anyhow::Result<Option<Signal>> {
    let deadline = if backpressure_deadline_ms > 0 {
        Some(Instant::now() + Duration::from_millis(backpressure_deadline_ms))
    } else {
        None
    };
    let mut retries: u64 = 0;
    let mut next_log = 1000u64;
    let mut first_full_at: Option<Instant> = None;
    loop {
        if let Some(limit) = utterance_timeout {
            if timing.elapsed_ms() >= limit.as_millis() as u64 {
                return Err(anyhow::anyhow!("timeout_feeding_audio"));
            }
        }
        match stt.process(sig.clone()).await {
            Ok(out) => return Ok(out),
            Err(e) if is_backpressure_full_err(&e) => {
                retries = retries.saturating_add(1);
                let start = first_full_at.get_or_insert_with(Instant::now);
                let should_log = log_backpressure || backpressure_timeout_ms > 0;
                if retries >= next_log && should_log {
                    let elapsed_ms = start.elapsed().as_millis();
                    eprintln!(
                        "[asr_test] backpressure retry id={} utt_seq={} chunk_idx={} retries={} elapsed_ms={} phase={} offline={}",
                        utterance_id,
                        utterance_seq,
                        chunk_idx,
                        retries,
                        elapsed_ms,
                        phase,
                        offline_mode
                    );
                    next_log = next_log.saturating_mul(2);
                }
                if let Some(deadline) = deadline {
                    if Instant::now() >= deadline {
                        let elapsed_ms = first_full_at
                            .map(|t| t.elapsed().as_millis())
                            .unwrap_or(0);
                        return Err(anyhow::anyhow!(
                            "backpressure_timeout id={} utt_seq={} chunk_idx={} retries={} elapsed_ms={} phase={} offline={}",
                            utterance_id,
                            utterance_seq,
                            chunk_idx,
                            retries,
                            elapsed_ms,
                            phase,
                            offline_mode
                        ));
                    }
                }
                let sleep_us = backpressure_retry_sleep_us.max(1);
                tokio::time::sleep(Duration::from_micros(sleep_us)).await;
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}

pub async fn run_manifest(
    manifest: &[ManifestEntry],
    engine: &ParakeetEngine,
    chunk_ms: u32,
    flush_ms: u32,
    realtime: bool,
    print_partials: bool,
    offline_mode: bool,
    backpressure: bool,
    final_blank_penalty_delta: f32,
    verbose_stop: bool,
    normalize_mode: &str,
    pre_gain: f32,
    debug_ids: &[String],
    eos_pad_ms: u32,
    realtime_ms: Option<u32>,
    utterance_timeout_ms: u64,
    stop_stats_timeout_ms: u64,
    backpressure_timeout_ms: u64,
    backpressure_retry_sleep_us: u64,
    inflight_chunks: usize,
    jobs: usize,
) -> anyhow::Result<Vec<UtteranceResult>> {
    if backpressure && std::env::var("PARAKEET_AUDIO_QUEUE_CAP").is_err() {
        std::env::set_var("PARAKEET_AUDIO_QUEUE_CAP", "2");
        eprintln!("[asr_test] set PARAKEET_AUDIO_QUEUE_CAP=2 (backpressure)");
    }
    let job_count = jobs.max(1).min(manifest.len().max(1));
    let total = manifest.len();
    let mut completed = 0usize;
    let mut ok = 0usize;
    let mut truncation = 0usize;
    let mut empty_hyp = 0usize;
    let mut no_final = 0usize;
    let mut stt_error = 0usize;
    let mut stop_stats_missing = 0usize;
    if job_count == 1 {
        let mut stt = build_worker(
            "asr_test",
            engine,
            normalize_mode,
            pre_gain,
            backpressure,
            backpressure_timeout_ms,
            final_blank_penalty_delta,
        )
        .await?;

        let mut results = Vec::with_capacity(manifest.len());

        for (i, entry) in manifest.iter().enumerate() {
            eprintln!("[asr_test] {}/{} {}", i + 1, manifest.len(), entry.id);
            let utterance_seq = i as u64 + 1;
            let RunOutcome {
                result: res,
                poison_reason,
            } = run_one(
                &mut stt,
                entry,
                utterance_seq,
                chunk_ms,
                flush_ms,
                realtime,
                print_partials,
                offline_mode,
                backpressure,
                final_blank_penalty_delta,
                verbose_stop,
                utterance_timeout_ms,
                stop_stats_timeout_ms,
                backpressure_timeout_ms,
                backpressure_retry_sleep_us,
                inflight_chunks,
                normalize_mode,
                pre_gain,
                debug_ids,
                eos_pad_ms,
                realtime_ms,
                engine.device_id as u32,
            )
            .await?;
            if let Some(reason) = poison_reason {
                eprintln!(
                    "[asr_test] worker_restart id={} utt_seq={} reason={}",
                    entry.id, utterance_seq, reason
                );
                stt = build_worker(
                    "asr_test",
                    engine,
                    normalize_mode,
                    pre_gain,
                    backpressure,
                    backpressure_timeout_ms,
                    final_blank_penalty_delta,
                )
                .await?;
            }
            completed += 1;
            update_progress_counts(
                res.status.as_str(),
                &mut ok,
                &mut truncation,
                &mut empty_hyp,
                &mut no_final,
                &mut stt_error,
                &mut stop_stats_missing,
            );
            maybe_print_progress(
                completed, total, ok, truncation, empty_hyp, no_final, stt_error, stop_stats_missing,
            );
            results.push(res);
        }

        return Ok(results);
    }

    let normalize_mode = normalize_mode.to_string();
    let (tx, rx) = tokio::sync::mpsc::channel::<(usize, ManifestEntry, u64)>(job_count * 2);
    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let (res_tx, mut res_rx) =
        tokio::sync::mpsc::channel::<(usize, anyhow::Result<UtteranceResult>)>(manifest.len());

    let mut handles = Vec::with_capacity(job_count);
    for worker_id in 0..job_count {
        let rx = Arc::clone(&rx);
        let res_tx = res_tx.clone();
        let engine = engine.clone();
        let debug_ids = debug_ids.to_vec();
        let normalize_mode = normalize_mode.clone();
        let handle = tokio::spawn(async move {
            let worker_label = format!("asr_test_worker_{}", worker_id);
            let mut stt = build_worker(
                &worker_label,
                &engine,
                &normalize_mode,
                pre_gain,
                backpressure,
                backpressure_timeout_ms,
                final_blank_penalty_delta,
            )
            .await?;

            loop {
                let item = {
                    let mut guard = rx.lock().await;
                    guard.recv().await
                };
                let Some((idx, entry, utterance_seq)) = item else { break; };
                let RunOutcome {
                    result: res,
                    poison_reason,
                } = run_one(
                    &mut stt,
                    &entry,
                    utterance_seq,
                    chunk_ms,
                    flush_ms,
                    realtime,
                    print_partials,
                    offline_mode,
                    backpressure,
                    final_blank_penalty_delta,
                    verbose_stop,
                    utterance_timeout_ms,
                    stop_stats_timeout_ms,
                    backpressure_timeout_ms,
                    backpressure_retry_sleep_us,
                    inflight_chunks,
                    &normalize_mode,
                    pre_gain,
                    &debug_ids,
                    eos_pad_ms,
                    realtime_ms,
                    engine.device_id as u32,
                )
                .await?;
                if let Some(reason) = poison_reason {
                    eprintln!(
                        "[asr_test] worker_restart worker={} id={} utt_seq={} reason={}",
                        worker_label, entry.id, utterance_seq, reason
                    );
                    stt = build_worker(
                        &worker_label,
                        &engine,
                        &normalize_mode,
                        pre_gain,
                        backpressure,
                        backpressure_timeout_ms,
                        final_blank_penalty_delta,
                    )
                    .await?;
                }
                let _ = res_tx.send((idx, Ok(res))).await;
            }
            anyhow::Ok(())
        });
        handles.push(handle);
    }
    drop(res_tx);

    for (i, entry) in manifest.iter().enumerate() {
        eprintln!("[asr_test] {}/{} {}", i + 1, manifest.len(), entry.id);
        let utterance_seq = i as u64 + 1;
        tx.send((i, entry.clone(), utterance_seq)).await?;
    }
    drop(tx);

    let mut results: Vec<Option<UtteranceResult>> = vec![None; manifest.len()];
    for _ in 0..manifest.len() {
        if let Some((idx, res)) = res_rx.recv().await {
            let res = res?;
            completed += 1;
            update_progress_counts(
                res.status.as_str(),
                &mut ok,
                &mut truncation,
                &mut empty_hyp,
                &mut no_final,
                &mut stt_error,
                &mut stop_stats_missing,
            );
            maybe_print_progress(
                completed, total, ok, truncation, empty_hyp, no_final, stt_error, stop_stats_missing,
            );
            results[idx] = Some(res);
        }
    }

    for handle in handles {
        let _ = handle.await;
    }

    Ok(results.into_iter().map(|r| r.unwrap()).collect())
}

async fn run_one(
    stt: &mut ParakeetSttProcessor,
    entry: &ManifestEntry,
    utterance_seq: u64,
    chunk_ms: u32,
    flush_ms: u32,
    realtime: bool,
    print_partials: bool,
    offline_mode: bool,
    backpressure: bool,
    final_blank_penalty_delta: f32,
    verbose_stop: bool,
    utterance_timeout_ms: u64,
    stop_stats_timeout_ms: u64,
    backpressure_timeout_ms: u64,
    backpressure_retry_sleep_us: u64,
    inflight_chunks: usize,
    normalize_mode: &str,
    pre_gain: f32,
    debug_ids: &[String],
    eos_pad_ms: u32,
    realtime_ms: Option<u32>,
    gpu_device_id: u32,
) -> anyhow::Result<RunOutcome> {
    let res = run_one_inner(
        stt,
        entry,
        utterance_seq,
        chunk_ms,
        flush_ms,
        realtime,
        print_partials,
        offline_mode,
        backpressure,
        final_blank_penalty_delta,
        verbose_stop,
        utterance_timeout_ms,
        stop_stats_timeout_ms,
        backpressure_timeout_ms,
        backpressure_retry_sleep_us,
        inflight_chunks,
        normalize_mode,
        pre_gain,
        debug_ids,
        eos_pad_ms,
        realtime_ms,
        gpu_device_id,
    )
    .await;
    if let Err(err) = &res {
        eprintln!(
            "[asr_test] stt_error id={} utt_seq={} err={}",
            entry.id, utterance_seq, err
        );
    }
    Ok(match res {
        Ok(result) => {
            let poison_reason = result
                .error
                .as_deref()
                .and_then(poison_reason_from_error);
            RunOutcome {
                result,
                poison_reason,
            }
        }
        Err(err) => {
            let err_str = err.to_string();
            let poison_reason = poison_reason_from_error(&err_str);
            RunOutcome {
                result: utterance_error_result(entry, err),
                poison_reason,
            }
        }
    })
}

async fn run_one_inner(
    stt: &mut ParakeetSttProcessor,
    entry: &ManifestEntry,
    utterance_seq: u64,
    chunk_ms: u32,
    flush_ms: u32,
    realtime: bool,
    print_partials: bool,
    offline_mode: bool,
    _backpressure: bool,
    _final_blank_penalty_delta: f32,
    verbose_stop: bool,
    utterance_timeout_ms: u64,
    stop_stats_timeout_ms: u64,
    backpressure_timeout_ms: u64,
    backpressure_retry_sleep_us: u64,
    inflight_chunks: usize,
    _normalize_mode: &str,
    _pre_gain: f32,
    debug_ids: &[String],
    eos_pad_ms: u32,
    realtime_ms: Option<u32>,
    gpu_device_id: u32,
) -> anyhow::Result<UtteranceResult> {
    let enable_debug = debug_ids.iter().any(|id| id == &entry.id);
    let log_mismatch = enable_debug || verbose_stop;
    std::env::set_var("PARAKEET_DEBUG_TOPK", if enable_debug { "1" } else { "0" });
    let wav_path = PathBuf::from(&entry.wav);
    let wav_path = if wav_path.is_absolute() {
        wav_path
    } else {
        // Most invocations will run from repo root, and manifest paths are relative.
        std::env::current_dir()?.join(wav_path)
    };

    let (sr, ch, audio_signals) = audio_replay::load_wav_audio_signals(&wav_path, chunk_ms)
        .with_context(|| format!("Failed to load wav {}", wav_path.display()))?;
    if sr != 16_000 || ch != 1 {
        anyhow::bail!(
            "Unsupported WAV format for {}: got {} Hz, {} ch (expected 16kHz mono)",
            wav_path.display(),
            sr,
            ch
        );
    }
    if enable_debug || verbose_stop {
        let mut total_samples = 0usize;
        for sig in &audio_signals {
            if let Signal::Audio { data, .. } = sig {
                total_samples += data.len();
            }
        }
        eprintln!(
            "[asr_test] input_audio id={} chunks={} total_samples={} sr={} ch={}",
            entry.id,
            audio_signals.len(),
            total_samples,
            sr,
            ch
        );
    }

    let base_settings = reset_settings(entry.id.as_str(), utterance_seq, offline_mode);

    let pass_label = if offline_mode { "offline" } else { "stream" };
    let (
        mut partials,
        mut final_text,
        final_parts,
        mut metrics,
        mut err,
        mut stop_stats,
        mut timing,
        mut diag,
    ) = run_pass(
        stt,
        entry.id.as_str(),
        &audio_signals,
        chunk_ms,
        flush_ms,
        realtime,
        print_partials,
        base_settings.clone(),
        0,
        realtime_ms,
        utterance_seq,
        utterance_timeout_ms,
        stop_stats_timeout_ms,
        backpressure_timeout_ms,
        backpressure_retry_sleep_us,
        inflight_chunks,
        offline_mode,
        log_mismatch,
        gpu_device_id,
        pass_label,
    )
    .await?;

    let mut hypothesis = if !final_parts.is_empty() {
        final_parts.join(" ")
    } else {
        final_text.clone().unwrap_or_default()
    };
    if hypothesis.trim().is_empty() {
        if let Some(last_partial) = partials.iter().rev().find(|t| !t.trim().is_empty()) {
            hypothesis = last_partial.clone();
        }
    }

    let mut did_offline_retry = false;
    if hypothesis.trim().is_empty() && err.is_none() && !offline_mode {
        did_offline_retry = true;
        let retry_settings = reset_settings(entry.id.as_str(), utterance_seq, true);
        let (
            r_partials,
            r_final_text,
            r_final_parts,
            r_metrics,
            r_err,
            r_stop_stats,
            r_timing,
            r_diag,
        ) = run_pass(
            stt,
            entry.id.as_str(),
            &audio_signals,
            chunk_ms,
            flush_ms,
            realtime,
            print_partials,
            retry_settings,
            0,
            realtime_ms,
            utterance_seq,
            utterance_timeout_ms,
            stop_stats_timeout_ms,
            backpressure_timeout_ms,
            backpressure_retry_sleep_us,
            inflight_chunks,
            true,
            log_mismatch,
            gpu_device_id,
            "offline_retry",
        )
        .await?;
        let mut retry_hyp = if !r_final_parts.is_empty() {
            r_final_parts.join(" ")
        } else {
            r_final_text.clone().unwrap_or_default()
        };
        if retry_hyp.trim().is_empty() {
            if let Some(last_partial) = r_partials.iter().rev().find(|t| !t.trim().is_empty()) {
                retry_hyp = last_partial.clone();
            }
        }
        if !retry_hyp.trim().is_empty() {
            hypothesis = retry_hyp;
            partials = r_partials;
            final_text = r_final_text;
            metrics = r_metrics;
            err = r_err;
            stop_stats = r_stop_stats;
            timing = r_timing;
            diag = r_diag;
        }
    }

    let ref_words = wer::tokenize_words(&wer::normalize_for_wer(&entry.text)).len();
    let mut hyp_words = wer::tokenize_words(&wer::normalize_for_wer(&hypothesis)).len();
    let mut is_truncation = err.is_none() && hyp_words > 0 && ref_words > 0 && hyp_words * 2 < ref_words;

    if is_truncation && err.is_none() && !did_offline_retry && !offline_mode {
        let retry_settings = reset_settings(entry.id.as_str(), utterance_seq, false);
        let (
            r_partials,
            r_final_text,
            r_final_parts,
            r_metrics,
            r_err,
            r_stop_stats,
            r_timing,
            r_diag,
        ) = run_pass(
            stt,
            entry.id.as_str(),
            &audio_signals,
            chunk_ms,
            flush_ms,
            realtime,
            print_partials,
            retry_settings,
            eos_pad_ms,
            realtime_ms,
            utterance_seq,
            utterance_timeout_ms,
            stop_stats_timeout_ms,
            backpressure_timeout_ms,
            backpressure_retry_sleep_us,
            inflight_chunks,
            false,
            log_mismatch,
            gpu_device_id,
            "stream_retry",
        )
        .await?;
        let mut retry_hyp = if !r_final_parts.is_empty() {
            r_final_parts.join(" ")
        } else {
            r_final_text.clone().unwrap_or_default()
        };
        if retry_hyp.trim().is_empty() {
            if let Some(last_partial) = r_partials.iter().rev().find(|t| !t.trim().is_empty()) {
                retry_hyp = last_partial.clone();
            }
        }
        let retry_hyp_words = wer::tokenize_words(&wer::normalize_for_wer(&retry_hyp)).len();
        if retry_hyp_words > hyp_words {
            hypothesis = retry_hyp;
            partials = r_partials;
            final_text = r_final_text;
            metrics = r_metrics;
            err = r_err;
            stop_stats = r_stop_stats;
            timing = r_timing;
            diag = r_diag;
            hyp_words = retry_hyp_words;
            is_truncation = err.is_none()
                && hyp_words > 0
                && ref_words > 0
                && hyp_words * 2 < ref_words;
        }
    }

    if is_truncation {
        if let Some(last_partial) = partials.iter().rev().find(|t| !t.trim().is_empty()) {
            let partial_words =
                wer::tokenize_words(&wer::normalize_for_wer(last_partial)).len();
            if partial_words > hyp_words {
                hypothesis = last_partial.clone();
                hyp_words = partial_words;
                is_truncation = err.is_none()
                    && hyp_words > 0
                    && ref_words > 0
                    && hyp_words * 2 < ref_words;
            }
        }
    }
    let mut status = if err.is_some() {
        "stt_error"
    } else if final_text.is_none() {
        "no_final"
    } else if hypothesis.trim().is_empty() {
        "empty_hyp"
    } else {
        "ok"
    };
    if status == "ok" && is_truncation {
        status = "truncation";
    }

    let mut stop_stats_missing_only = false;
    if stop_stats_timeout_ms > 0 {
        let should_wait = (status != "ok" || enable_debug || verbose_stop) && stop_stats.is_none();
        let should_wait_for_post = stop_stats
            .as_ref()
            .map(stop_stats_is_pre)
            .unwrap_or(false)
            && (enable_debug || verbose_stop);
        if should_wait || should_wait_for_post {
            let mut gpu = None;
            wait_for_stop_stats(
                stt,
                entry.id.as_str(),
                utterance_seq,
                &mut stop_stats,
                stop_stats_timeout_ms,
                log_mismatch,
                &mut gpu,
            )
            .await?;
        }
    }
    let has_post_stats = stop_stats
        .as_ref()
        .map(stop_stats_is_post)
        .unwrap_or(false);
    let has_nonempty_final = final_text
        .as_ref()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    if !has_post_stats {
        if let Some(stats) = stop_stats.as_ref() {
            if stop_stats_is_pre(stats) {
                eprintln!(
                    "[asr_test] stop_stats_post_missing id={} utt_seq={}",
                    stats.id.as_deref().unwrap_or(entry.id.as_str()),
                    stats.utterance_seq
                );
            }
        }
        if err.is_none() {
            if diag.saw_slow_chunk || diag.saw_error_event {
                if stop_stats_timeout_ms > 0 {
                    err = Some("stop_stats_timeout".to_string());
                }
            } else if has_nonempty_final {
                if enable_debug || verbose_stop {
                    err = Some("stop_stats_timeout".to_string());
                } else {
                    stop_stats_missing_only = true;
                }
            } else if stop_stats_timeout_ms > 0 {
                err = Some("stop_stats_timeout".to_string());
            }
        }
    }
    if err.is_none() {
        if let Some(stats) = stop_stats.as_ref() {
            if let Some(reason) = stats.abort_reason.as_deref() {
                err = Some(reason.to_string());
            }
        }
    }

    let mut status = if err.is_some() {
        "stt_error"
    } else if final_text.is_none() {
        "no_final"
    } else if hypothesis.trim().is_empty() {
        "empty_hyp"
    } else {
        "ok"
    };
    if status == "ok" && is_truncation {
        status = "truncation";
    }
    if status == "ok" && stop_stats_missing_only {
        status = "stop_stats_missing_only";
    }

    let wer_stats = wer::wer(&entry.text, &hypothesis);

    eprintln!(
        "[asr_test] timing id={} utt_seq={} first_partial_ms={} first_final_ms={} total_ms={}",
        entry.id,
        utterance_seq,
        timing
            .first_partial_ms
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string()),
        timing
            .first_final_ms
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string()),
        timing.elapsed_ms()
    );

    if enable_debug || verbose_stop {
        let final_text_val = final_text.as_deref().unwrap_or("");
        eprintln!(
            "[asr_test] final_text id={} utt_seq={} len={} preview=\"{}\"",
            entry.id,
            utterance_seq,
            final_text_val.len(),
            preview_text(final_text_val, 80)
        );
    }

    if status == "truncation" || enable_debug || verbose_stop {
        let trunc_reason = if status == "truncation" {
            if final_text.is_none() {
                "no_final"
            } else if hypothesis.trim().is_empty() {
                "final_received_but_empty"
            } else {
                "final_received_but_short"
            }
        } else {
            "n/a"
        };
        eprintln!(
            "[asr_test] truncation_reason id={} status={} reason={} ref_words={} hyp_words={} final_parts={} partials={} final_text_present={} final_text_len={} hypothesis_len={}",
            entry.id,
            status,
            trunc_reason,
            ref_words,
            hyp_words,
            final_parts.len(),
            partials.len(),
            final_text.as_ref().map(|t| !t.trim().is_empty()).unwrap_or(false),
            final_text.as_ref().map(|t| t.len()).unwrap_or(0),
            hypothesis.len()
        );
    }

    if status != "ok" || enable_debug || verbose_stop {
        if let Some(stats) = &stop_stats {
            eprintln!(
                "[asr_test] stop_stats id={} utt_seq={} staging_samples={} queued_before={} queued_after={} offline_frames={} tail_flush_decodes={} post_stop_decode_iters={} post_stop_events={} final_blank_penalty_delta={} emitted_partial_pre={} emitted_partial_post={} emitted_final_pre={} emitted_final_post={} emitted_final_empty={} emitted_final_nonempty={} suppressed_junk_partial={} suppressed_junk_final={} filter_junk={} offline_mode={} audio_chunks_seen={} audio_samples_seen={} audio_samples_resampled={} abort_reason={}",
                stats.id.as_deref().unwrap_or(entry.id.as_str()),
                stats.utterance_seq,
                stats.staging_samples,
                stats.queued_before,
                stats.queued_after,
                stats.offline_frames,
                stats.tail_flush_decodes,
                stats.post_stop_decode_iters,
                stats.post_stop_events,
                stats.final_blank_penalty_delta,
                stats.emitted_partial_pre,
                stats.emitted_partial_post,
                stats.emitted_final_pre,
                stats.emitted_final_post,
                stats.emitted_final_empty,
                stats.emitted_final_nonempty,
                stats.suppressed_junk_partial,
                stats.suppressed_junk_final,
                stats.filter_junk,
                stats.offline_mode,
                stats.audio_chunks_seen,
                stats.audio_samples_seen,
                stats.audio_samples_resampled,
                stats.abort_reason.as_deref().unwrap_or("-")
            );
            if stats.phase.is_some()
                || stats.slow_chunk_count > 0
                || stats.t_first_audio_ms.is_some()
                || stats.t_finalize_done_ms.is_some()
            {
                eprintln!(
                    "[asr_test] stop_stats_timing id={} utt_seq={} phase={} t_reset_start_ms={} t_reset_done_ms={} t_first_audio_ms={} t_first_feature_ms={} t_first_decode_ms={} t_first_partial_ms={} t_first_final_ms={} t_stop_received_ms={} t_finalize_start_ms={} t_finalize_done_ms={} t_stop_stats_ms={} finalize_ms={} slow_chunk_count={} slowest_chunk_ms={} slowest_chunk_idx={} slowest_chunk_audio_idx={} last_audio_chunk_idx={} last_feature_chunk_idx={} abort_reason={}",
                    stats.id.as_deref().unwrap_or(entry.id.as_str()),
                    stats.utterance_seq,
                    stats.phase.as_deref().unwrap_or("n/a"),
                    fmt_opt_ms(stats.t_reset_start_ms),
                    fmt_opt_ms(stats.t_reset_done_ms),
                    fmt_opt_ms(stats.t_first_audio_ms),
                    fmt_opt_ms(stats.t_first_feature_ms),
                    fmt_opt_ms(stats.t_first_decode_ms),
                    fmt_opt_ms(stats.t_first_partial_ms),
                    fmt_opt_ms(stats.t_first_final_ms),
                    fmt_opt_ms(stats.t_stop_received_ms),
                    fmt_opt_ms(stats.t_finalize_start_ms),
                    fmt_opt_ms(stats.t_finalize_done_ms),
                    fmt_opt_ms(stats.t_stop_stats_ms),
                    fmt_opt_ms(stats.finalize_ms),
                    stats.slow_chunk_count,
                    stats.slowest_chunk_ms,
                    stats.slowest_chunk_idx,
                    stats.slowest_chunk_audio_idx,
                    stats.last_audio_chunk_idx,
                    stats.last_feature_chunk_idx,
                    stats.abort_reason.as_deref().unwrap_or("-")
                );
            }
        } else {
            eprintln!(
                "[asr_test] stop_stats id={} utt_seq={} missing",
                entry.id, utterance_seq
            );
        }
    }

    Ok(UtteranceResult {
        id: entry.id.clone(),
        wav: entry.wav.clone(),
        reference: entry.text.clone(),
        hypothesis,
        status: status.to_string(),
        wer: wer_stats.wer,
        partials: if print_partials { partials } else { Vec::new() },
        metrics,
        error: err,
    })
}

async fn run_pass(
    stt: &mut ParakeetSttProcessor,
    utterance_id: &str,
    audio_signals: &[Signal],
    chunk_ms: u32,
    flush_ms: u32,
    realtime: bool,
    print_partials: bool,
    settings: serde_json::Value,
    eos_pad_ms: u32,
    realtime_ms: Option<u32>,
    utterance_seq: u64,
    utterance_timeout_ms: u64,
    stop_stats_timeout_ms: u64,
    backpressure_timeout_ms: u64,
    backpressure_retry_sleep_us: u64,
    inflight_chunks: usize,
    offline_mode: bool,
    log_mismatch: bool,
    gpu_device_id: u32,
    pass_label: &str,
) -> anyhow::Result<(
    Vec<String>,
    Option<String>,
    Vec<String>,
    Option<SttMetrics>,
    Option<String>,
    Option<StopStats>,
    TimingStats,
    PassDiagnostics,
)> {
    let mut timing = TimingStats::new();
    let mut gpu = GpuTelemetry::new_if_enabled(gpu_device_id);
    let mut diag = PassDiagnostics::default();
    // Reset between utterances (worker only processes control at chunk boundaries, so tick it).
    send_settings_and_tick(stt, settings.clone(), backpressure_timeout_ms).await?;
    let needs_reset_ack = settings
        .get("action")
        .and_then(|v| v.as_str())
        .map(|v| v == "reset")
        .unwrap_or(false);
    if needs_reset_ack && stop_stats_timeout_ms > 0 {
        let reset_ack =
            wait_for_reset_ack(
                stt,
                utterance_id,
                utterance_seq,
                stop_stats_timeout_ms,
                log_mismatch,
                &mut gpu,
            )
            .await?;
        if reset_ack.is_none() {
            return Err(anyhow::anyhow!("reset_ack_timeout"));
        }
    }
    let timeout = if utterance_timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(utterance_timeout_ms))
    };
    let inflight_limit = if inflight_chunks == 0 {
        None
    } else {
        Some(inflight_chunks.max(1))
    };
    let backpressure_deadline_ms = if inflight_chunks > 0 {
        0
    } else {
        backpressure_timeout_ms
    };
    let mut partials: Vec<String> = Vec::new();
    let mut final_text: Option<String> = None;
    let mut final_parts: Vec<String> = Vec::new();
    let mut metrics: Option<SttMetrics> = None;
    let mut err: Option<String> = None;
    let mut stop_stats: Option<StopStats> = None;
    let mut sent_chunks: u64 = 0;
    let mut acked_chunks: u64 = 0;

    let mut last_ts_us = 0u64;
    let mut first_audio_sent = false;
    for (idx, sig) in audio_signals.iter().cloned().enumerate() {
        gpu_tick(&mut gpu);
        if let Some(limit) = timeout {
            if timing.start.elapsed() >= limit {
                err = Some("timeout_feeding_audio".to_string());
                break;
            }
        }
        if let Signal::Audio { timestamp_us, .. } = sig {
            last_ts_us = timestamp_us;
        }
        if !first_audio_sent {
            gpu_stage(&mut gpu, utterance_id, utterance_seq, "first_audio_send", None);
            first_audio_sent = true;
        }
        if let Some(limit) = inflight_limit {
            loop {
                let inflight = sent_chunks.saturating_sub(acked_chunks);
                if inflight < limit as u64 {
                    break;
                }
                if let Some(limit) = timeout {
                    if timing.start.elapsed() >= limit {
                        err = Some("timeout_waiting_for_ack".to_string());
                        break;
                    }
                }
                drain(
                    stt,
                    utterance_seq,
                    &mut timing,
                    log_mismatch,
                    &mut partials,
                    &mut final_text,
                    &mut final_parts,
                    &mut metrics,
                    &mut err,
                    &mut stop_stats,
                    &mut acked_chunks,
                    print_partials,
                    utterance_id,
                    &mut gpu,
                    &mut diag,
                )
                .await?;
                if err.is_some() {
                    break;
                }
                gpu_tick(&mut gpu);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            if err.is_some() {
                break;
            }
        }
        match process_audio_with_backpressure(
            stt,
            sig,
            backpressure_timeout_ms,
            backpressure_deadline_ms,
            backpressure_retry_sleep_us,
            timeout,
            &timing,
            utterance_id,
            utterance_seq,
            idx,
            log_mismatch,
            "audio",
            offline_mode,
        )
        .await
        {
            Ok(out) => {
                sent_chunks = sent_chunks.saturating_add(1);
                handle_outputs(
                    out,
                    utterance_seq,
                    &mut timing,
                    log_mismatch,
                    &mut partials,
                    &mut final_text,
                    &mut final_parts,
                    &mut metrics,
                    &mut err,
                    &mut stop_stats,
                    &mut acked_chunks,
                    print_partials,
                    utterance_id,
                    &mut gpu,
                    &mut diag,
                )?;
            }
            Err(e) => {
                err = Some(e.to_string());
                break;
            }
        }
        drain(
            stt,
            utterance_seq,
            &mut timing,
            log_mismatch,
            &mut partials,
            &mut final_text,
            &mut final_parts,
            &mut metrics,
            &mut err,
            &mut stop_stats,
            &mut acked_chunks,
            print_partials,
            utterance_id,
            &mut gpu,
            &mut diag,
        )
        .await?;
        gpu_tick(&mut gpu);
        if realtime {
            let ms = realtime_ms.unwrap_or(chunk_ms.max(1)) as u64;
            tokio::time::sleep(Duration::from_millis(ms.max(1))).await;
        }
        if err.is_some() {
            break;
        }
    }

    if eos_pad_ms > 0 {
        let chunk_ms = chunk_ms.max(1);
        let samples_per_chunk = (16_000u64 * chunk_ms as u64 / 1000) as usize;
        let take = samples_per_chunk.max(1);
        let mut ts_us = last_ts_us + (chunk_ms as u64 * 1000);
        let chunks = (eos_pad_ms as u64 + chunk_ms as u64 - 1) / chunk_ms as u64;
        for (idx, _) in (0..chunks).enumerate() {
            if let Some(limit) = timeout {
                if timing.start.elapsed() >= limit {
                    err = Some("timeout_eos_pad".to_string());
                    break;
                }
            }
            let sig = Signal::Audio {
                sample_rate: 16_000,
                channels: 1,
                timestamp_us: ts_us,
                data: vec![0.0; take],
            };
            ts_us += chunk_ms as u64 * 1000;
            if let Some(limit) = inflight_limit {
                loop {
                    let inflight = sent_chunks.saturating_sub(acked_chunks);
                    if inflight < limit as u64 {
                        break;
                    }
                    if let Some(limit) = timeout {
                        if timing.start.elapsed() >= limit {
                            err = Some("timeout_waiting_for_ack".to_string());
                            break;
                        }
                    }
                    drain(
                        stt,
                        utterance_seq,
                        &mut timing,
                        log_mismatch,
                        &mut partials,
                        &mut final_text,
                        &mut final_parts,
                        &mut metrics,
                        &mut err,
                        &mut stop_stats,
                        &mut acked_chunks,
                        print_partials,
                        utterance_id,
                        &mut gpu,
                        &mut diag,
                    )
                    .await?;
                    if err.is_some() {
                        break;
                    }
                    gpu_tick(&mut gpu);
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                if err.is_some() {
                    break;
                }
            }
            match process_audio_with_backpressure(
                stt,
                sig,
                backpressure_timeout_ms,
                backpressure_deadline_ms,
                backpressure_retry_sleep_us,
                timeout,
                &timing,
                utterance_id,
                utterance_seq,
                audio_signals.len() + idx,
                log_mismatch,
                "eos_pad",
                offline_mode,
            )
            .await
            {
                Ok(out) => {
                    sent_chunks = sent_chunks.saturating_add(1);
                    handle_outputs(
                        out,
                        utterance_seq,
                        &mut timing,
                        log_mismatch,
                        &mut partials,
                        &mut final_text,
                        &mut final_parts,
                        &mut metrics,
                        &mut err,
                        &mut stop_stats,
                        &mut acked_chunks,
                        print_partials,
                        utterance_id,
                        &mut gpu,
                        &mut diag,
                    )?;
                }
                Err(e) => {
                    err = Some(e.to_string());
                    break;
                }
            }
            drain(
                stt,
                utterance_seq,
                &mut timing,
                log_mismatch,
                &mut partials,
                &mut final_text,
                &mut final_parts,
                &mut metrics,
                &mut err,
                &mut stop_stats,
                &mut acked_chunks,
                print_partials,
                utterance_id,
                &mut gpu,
                &mut diag,
            )
            .await?;
            gpu_tick(&mut gpu);
            if realtime {
                let ms = realtime_ms.unwrap_or(chunk_ms.max(1)) as u64;
                tokio::time::sleep(Duration::from_millis(ms.max(1))).await;
            }
            if err.is_some() {
                break;
            }
        }
    }

    if flush_ms > 0 {
        let chunk_ms = chunk_ms.max(1);
        let samples_per_chunk = (16_000u64 * chunk_ms as u64 / 1000) as usize;
        let take = (samples_per_chunk).max(1);
        let mut ts_us = last_ts_us + (chunk_ms as u64 * 1000);
        let chunks = (flush_ms as u64 + chunk_ms as u64 - 1) / chunk_ms as u64;
        for (idx, _) in (0..chunks).enumerate() {
            if let Some(limit) = timeout {
                if timing.start.elapsed() >= limit {
                    err = Some("timeout_flush".to_string());
                    break;
                }
            }
            let sig = Signal::Audio {
                sample_rate: 16_000,
                channels: 1,
                timestamp_us: ts_us,
                data: vec![0.0; take],
            };
            ts_us += chunk_ms as u64 * 1000;
            if let Some(limit) = inflight_limit {
                loop {
                    let inflight = sent_chunks.saturating_sub(acked_chunks);
                    if inflight < limit as u64 {
                        break;
                    }
                    if let Some(limit) = timeout {
                        if timing.start.elapsed() >= limit {
                            err = Some("timeout_waiting_for_ack".to_string());
                            break;
                        }
                    }
                    drain(
                        stt,
                        utterance_seq,
                        &mut timing,
                        log_mismatch,
                        &mut partials,
                        &mut final_text,
                        &mut final_parts,
                        &mut metrics,
                        &mut err,
                        &mut stop_stats,
                        &mut acked_chunks,
                        print_partials,
                        utterance_id,
                        &mut gpu,
                        &mut diag,
                    )
                    .await?;
                    if err.is_some() {
                        break;
                    }
                    gpu_tick(&mut gpu);
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                if err.is_some() {
                    break;
                }
            }
            match process_audio_with_backpressure(
                stt,
                sig,
                backpressure_timeout_ms,
                backpressure_deadline_ms,
                backpressure_retry_sleep_us,
                timeout,
                &timing,
                utterance_id,
                utterance_seq,
                audio_signals.len() + idx,
                log_mismatch,
                "flush",
                offline_mode,
            )
            .await
            {
                Ok(out) => {
                    sent_chunks = sent_chunks.saturating_add(1);
                    handle_outputs(
                        out,
                        utterance_seq,
                        &mut timing,
                        log_mismatch,
                        &mut partials,
                        &mut final_text,
                        &mut final_parts,
                        &mut metrics,
                        &mut err,
                        &mut stop_stats,
                        &mut acked_chunks,
                        print_partials,
                        utterance_id,
                        &mut gpu,
                        &mut diag,
                    )?;
                }
                Err(e) => {
                    err = Some(e.to_string());
                    break;
                }
            }
            drain(
                stt,
                utterance_seq,
                &mut timing,
                log_mismatch,
                &mut partials,
                &mut final_text,
                &mut final_parts,
                &mut metrics,
                &mut err,
                &mut stop_stats,
                &mut acked_chunks,
                print_partials,
                utterance_id,
                &mut gpu,
                &mut diag,
            )
            .await?;
            gpu_tick(&mut gpu);
            if realtime {
                let ms = realtime_ms.unwrap_or(chunk_ms.max(1)) as u64;
                tokio::time::sleep(Duration::from_millis(ms.max(1))).await;
            }
            if err.is_some() {
                break;
            }
        }
    }

    // Flush: stop and tick to ensure worker consumes the control message.
    send_settings_and_tick(stt, json!({ "action": "stop" }), backpressure_timeout_ms).await?;
    gpu_stage(&mut gpu, utterance_id, utterance_seq, "stop_sent", None);

    // Wait for a final/error deterministically (bounded by wall-clock to avoid hangs).
    let mut timed_out_wait = false;
    let mut empty_final_deadline: Option<Instant> = None;
    loop {
        gpu_tick(&mut gpu);
        if let Some(limit) = timeout {
            if timing.start.elapsed() >= limit {
                timed_out_wait = true;
                break;
            }
        }
        drain(
            stt,
            utterance_seq,
            &mut timing,
            log_mismatch,
            &mut partials,
            &mut final_text,
            &mut final_parts,
            &mut metrics,
            &mut err,
            &mut stop_stats,
            &mut acked_chunks,
            print_partials,
            utterance_id,
            &mut gpu,
            &mut diag,
        )
        .await?;
        if err.is_some() {
            break;
        }
        let has_nonempty_final = final_text
            .as_ref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);
        if has_nonempty_final {
            break;
        }
        if final_text.is_some() && empty_final_deadline.is_none() && stop_stats_timeout_ms > 0 {
            empty_final_deadline = Some(Instant::now() + Duration::from_millis(stop_stats_timeout_ms));
        }
        if let Some(deadline) = empty_final_deadline {
            if Instant::now() >= deadline {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    if timed_out_wait && err.is_none() && final_text.is_none() {
        err = Some("timeout_waiting_for_final".to_string());
    }

    if let Some(gpu) = gpu.as_mut() {
        gpu.finish(utterance_id, utterance_seq, pass_label);
    }
    Ok((partials, final_text, final_parts, metrics, err, stop_stats, timing, diag))
}

async fn send_settings_and_tick(
    stt: &mut ParakeetSttProcessor,
    settings: serde_json::Value,
    backpressure_timeout_ms: u64,
) -> anyhow::Result<()> {
    let ctrl = Signal::Control(ControlSignal::Settings(settings));
    let _ = stt.process(ctrl).await?;
    // The worker thread only consumes control messages when it receives an audio chunk.
    let tick = Signal::Audio {
        sample_rate: 16_000,
        channels: 1,
        timestamp_us: 0,
        data: vec![0.0],
    };
    let deadline = Instant::now()
        + Duration::from_millis(backpressure_timeout_ms.max(2000));
    loop {
        match stt.process(tick.clone()).await {
            Ok(_) => break,
            Err(e) if is_backpressure_full_err(&e) => {
                if Instant::now() >= deadline {
                    return Err(anyhow::anyhow!("backpressure_timeout"));
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

async fn wait_for_stop_stats(
    stt: &mut ParakeetSttProcessor,
    utterance_id: &str,
    target_utterance_seq: u64,
    stop_stats: &mut Option<StopStats>,
    timeout_ms: u64,
    log_mismatch: bool,
    gpu: &mut Option<GpuTelemetry>,
) -> anyhow::Result<()> {
    if timeout_ms == 0 {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while stop_stats.is_none() && Instant::now() < deadline {
        loop {
            gpu_tick(gpu);
            let out = stt.process(Signal::Pulse).await?;
            let Some(sig) = out else { break; };
            let Signal::Computed { source, content } = sig else { continue; };
            if source != "stt_stop_stats" {
                continue;
            }
            if let Ok(s) = serde_json::from_str::<StopStats>(&content) {
                if matches_utterance_seq(target_utterance_seq, s.utterance_seq) {
                    let replace = stop_stats
                        .as_ref()
                        .map(stop_stats_is_pre)
                        .unwrap_or(true);
                    if replace || stop_stats_is_post(&s) {
                        let phase = if stop_stats_is_post(&s) {
                            "stop_stats_post"
                        } else {
                            "stop_stats_pre"
                        };
                        gpu_stage(gpu, utterance_id, target_utterance_seq, phase, None);
                        *stop_stats = Some(s.clone());
                    }
                    if stop_stats_is_post(&s) {
                        break;
                    }
                } else if log_mismatch {
                    eprintln!(
                        "[asr_test] drop_event kind=stop_stats expected_utt_seq={} event_utt_seq={}",
                        target_utterance_seq, s.utterance_seq
                    );
                }
            }
        }
        if stop_stats.is_some() {
            break;
        }
        gpu_tick(gpu);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Ok(())
}

async fn wait_for_reset_ack(
    stt: &mut ParakeetSttProcessor,
    utterance_id: &str,
    target_utterance_seq: u64,
    timeout_ms: u64,
    log_mismatch: bool,
    gpu: &mut Option<GpuTelemetry>,
) -> anyhow::Result<Option<ResetAck>> {
    if timeout_ms == 0 {
        return Ok(None);
    }
    let mut reset_ack: Option<ResetAck> = None;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while reset_ack.is_none() && Instant::now() < deadline {
        loop {
            gpu_tick(gpu);
            let out = stt.process(Signal::Pulse).await?;
            let Some(sig) = out else { break; };
            let Signal::Computed { source, content } = sig else { continue; };
            if source != "stt_reset_ack" {
                continue;
            }
            if let Ok(a) = serde_json::from_str::<ResetAck>(&content) {
                if matches_utterance_seq(target_utterance_seq, a.utterance_seq) {
                    gpu_stage(gpu, utterance_id, target_utterance_seq, "reset_ack", None);
                    reset_ack = Some(a);
                    break;
                } else if log_mismatch {
                    eprintln!(
                        "[asr_test] drop_event kind=reset_ack expected_utt_seq={} event_utt_seq={}",
                        target_utterance_seq, a.utterance_seq
                    );
                }
            }
        }
        if reset_ack.is_some() {
            break;
        }
        gpu_tick(gpu);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    Ok(reset_ack)
}

async fn drain(
    stt: &mut ParakeetSttProcessor,
    target_utterance_seq: u64,
    timing: &mut TimingStats,
    log_mismatch: bool,
    partials: &mut Vec<String>,
    final_text: &mut Option<String>,
    final_parts: &mut Vec<String>,
    metrics: &mut Option<SttMetrics>,
    err: &mut Option<String>,
    stop_stats: &mut Option<StopStats>,
    chunk_acks: &mut u64,
    print_partials: bool,
    utterance_id: &str,
    gpu: &mut Option<GpuTelemetry>,
    diag: &mut PassDiagnostics,
) -> anyhow::Result<()> {
    loop {
        let out = stt.process(Signal::Pulse).await?;
        if out.is_none() {
            break;
        }
        handle_outputs(
            out,
            target_utterance_seq,
            timing,
            log_mismatch,
            partials,
            final_text,
            final_parts,
            metrics,
            err,
            stop_stats,
            chunk_acks,
            print_partials,
            utterance_id,
            gpu,
            diag,
        )?;
    }
    Ok(())
}

fn handle_outputs(
    out: Option<Signal>,
    target_utterance_seq: u64,
    timing: &mut TimingStats,
    log_mismatch: bool,
    partials: &mut Vec<String>,
    final_text: &mut Option<String>,
    final_parts: &mut Vec<String>,
    metrics: &mut Option<SttMetrics>,
    err: &mut Option<String>,
    stop_stats: &mut Option<StopStats>,
    chunk_acks: &mut u64,
    print_partials: bool,
    utterance_id: &str,
    gpu: &mut Option<GpuTelemetry>,
    diag: &mut PassDiagnostics,
) -> anyhow::Result<()> {
    let Some(sig) = out else { return Ok(()); };
    let Signal::Computed { source, content } = sig else { return Ok(()); };

    match source.as_str() {
        "stt_chunk_ack" => {
            if let Ok(a) = serde_json::from_str::<ChunkAck>(&content) {
                if matches_utterance_seq(target_utterance_seq, a.utterance_seq) {
                    *chunk_acks = chunk_acks.saturating_add(1);
                } else if log_mismatch {
                    eprintln!(
                        "[asr_test] drop_event kind=chunk_ack expected_utt_seq={} event_utt_seq={}",
                        target_utterance_seq, a.utterance_seq
                    );
                }
            }
            return Ok(());
        }
        "stt_partial" | "stt_final" | "stt_error" => {
            let ev: SttEvent = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse STT event payload from {}", source))?;
            if !matches_utterance_seq(target_utterance_seq, ev.utterance_seq) {
                if log_mismatch {
                    eprintln!(
                        "[asr_test] drop_event kind={} expected_utt_seq={} event_utt_seq={}",
                        ev.kind, target_utterance_seq, ev.utterance_seq
                    );
                }
                return Ok(());
            }
            match ev.kind.as_str() {
                "partial" => {
                    if timing.first_partial_ms.is_none() {
                        timing.first_partial_ms = Some(timing.elapsed_ms());
                        gpu_stage(gpu, utterance_id, target_utterance_seq, "first_partial", None);
                    }
                    if print_partials {
                        eprintln!("  [partial] {}", ev.text);
                    }
                    partials.push(ev.text);
                }
                "final" => {
                    if timing.first_final_ms.is_none() {
                        timing.first_final_ms = Some(timing.elapsed_ms());
                        gpu_stage(gpu, utterance_id, target_utterance_seq, "first_final", None);
                    }
                    let trimmed = ev.text.trim();
                    if trimmed.is_empty() {
                        if final_text
                            .as_ref()
                            .map(|t| !t.trim().is_empty())
                            .unwrap_or(false)
                        {
                            return Ok(());
                        }
                        if let Some(last_partial) = partials.iter().rev().find(|t| !t.trim().is_empty()) {
                            *final_text = Some(last_partial.clone());
                            return Ok(());
                        }
                    }
                    if !trimmed.is_empty() {
                        if final_parts.last().map(|t| t != trimmed).unwrap_or(true) {
                            final_parts.push(trimmed.to_string());
                        }
                    }
                    if *final_text != Some(ev.text.clone()) {
                        *final_text = Some(ev.text);
                    }
                }
                "error" => {
                    diag.saw_error_event = true;
                    *err = Some(ev.message);
                }
                _ => {}
            }
        }
        "stt_metrics" => {
            if let Ok(m) = serde_json::from_str::<SttMetrics>(&content) {
                if matches_utterance_seq(target_utterance_seq, m.utterance_seq) {
                    *metrics = Some(m);
                } else if log_mismatch {
                    eprintln!(
                        "[asr_test] drop_event kind=metrics expected_utt_seq={} event_utt_seq={}",
                        target_utterance_seq, m.utterance_seq
                    );
                }
            }
        }
        "stt_slow_chunk" => {
            if let Ok(slow) = serde_json::from_str::<SlowChunk>(&content) {
                if matches_utterance_seq(target_utterance_seq, slow.utterance_seq) {
                    diag.saw_slow_chunk = true;
                    let extra = format!(
                        "feature_idx={} audio_chunk_idx={} decode_ms={} queue_ms={} enc_shape={} length_shape={} profile_idx={} post_stop={} offline_mode={}",
                        slow.feature_idx,
                        slow.audio_chunk_idx,
                        slow.decode_ms,
                        slow.queue_ms.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
                        slow.enc_shape,
                        slow.length_shape,
                        slow.profile_idx,
                        slow.post_stop,
                        slow.offline_mode
                    );
                    gpu_stage(
                        gpu,
                        utterance_id,
                        target_utterance_seq,
                        "slow_chunk",
                        Some(extra.as_str()),
                    );
                } else if log_mismatch {
                    eprintln!(
                        "[asr_test] drop_event kind=slow_chunk expected_utt_seq={} event_utt_seq={}",
                        target_utterance_seq, slow.utterance_seq
                    );
                }
            }
        }
        "stt_stop_stats" => {
            if let Ok(s) = serde_json::from_str::<StopStats>(&content) {
                if matches_utterance_seq(target_utterance_seq, s.utterance_seq) {
                    let replace = stop_stats
                        .as_ref()
                        .map(stop_stats_is_pre)
                        .unwrap_or(true);
                    if replace || stop_stats_is_post(&s) {
                        let stage = if stop_stats_is_post(&s) {
                            "stop_stats_post"
                        } else {
                            "stop_stats_pre"
                        };
                        gpu_stage(gpu, utterance_id, target_utterance_seq, stage, None);
                        *stop_stats = Some(s);
                    }
                    if err.is_none() {
                        if let Some(reason) = stop_stats.as_ref().and_then(|v| v.abort_reason.as_deref()) {
                            *err = Some(reason.to_string());
                        }
                    }
                } else if log_mismatch {
                    eprintln!(
                        "[asr_test] drop_event kind=stop_stats expected_utt_seq={} event_utt_seq={}",
                        target_utterance_seq, s.utterance_seq
                    );
                }
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn write_results_jsonl(path: &Path, results: &[UtteranceResult]) -> anyhow::Result<()> {
    let mut out = String::new();
    for r in results {
        out.push_str(&serde_json::to_string(r)?);
        out.push('\n');
    }
    std::fs::write(path, out).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn summarize(results: &[UtteranceResult]) -> Summary {
    let total = results.len();
    let mut ok = 0usize;
    let mut stt_error = 0usize;
    let mut no_final = 0usize;
    let mut empty_hyp = 0usize;
    let mut truncation = 0usize;
    let mut stop_stats_missing = 0usize;
    let mut sum_edits = 0usize;
    let mut sum_ref_words = 0usize;

    for r in results {
        match r.status.as_str() {
            "ok" => ok += 1,
            "stop_stats_missing_only" => {
                ok += 1;
                stop_stats_missing += 1;
            }
            "stt_error" => stt_error += 1,
            "no_final" => no_final += 1,
            "empty_hyp" => empty_hyp += 1,
            "truncation" => truncation += 1,
            _ => {}
        }
        let ws = wer::wer(&r.reference, &r.hypothesis);
        sum_edits += ws.edits;
        sum_ref_words += ws.ref_words;
    }
    let failures = total - ok;
    let aggregate_wer = if sum_ref_words == 0 {
        0.0
    } else {
        sum_edits as f32 / sum_ref_words as f32
    };

    Summary {
        total,
        ok,
        failures,
        empty_hyp,
        no_final,
        stt_error,
        truncation,
        stop_stats_missing,
        aggregate_wer,
        sum_edits,
        sum_ref_words,
    }
}

pub fn print_summary_table(summary: &Summary, results: &[UtteranceResult], threshold: Option<f32>) {
    eprintln!();
    eprintln!("=== ASR Test Summary ===");
    eprintln!("total       : {}", summary.total);
    eprintln!("ok          : {}", summary.ok);
    eprintln!("failures    : {}", summary.failures);
    eprintln!("stt_error   : {}", summary.stt_error);
    eprintln!("stop_stats  : {}", summary.stop_stats_missing);
    eprintln!("no_final    : {}", summary.no_final);
    eprintln!("empty_hyp   : {}", summary.empty_hyp);
    eprintln!("truncation  : {}", summary.truncation);
    eprintln!(
        "agg WER     : {:.4} (edits={} / ref_words={})",
        summary.aggregate_wer, summary.sum_edits, summary.sum_ref_words
    );
    if let Some(t) = threshold {
        eprintln!("threshold   : {:.4}", t);
    }

    // Worst 10 by WER (excluding failures with empty ref_words).
    let mut worst = results
        .iter()
        .enumerate()
        .map(|(i, r)| (i, r.wer))
        .collect::<Vec<_>>();
    worst.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    eprintln!();
    eprintln!("worst_wer:");
    for (i, w) in worst.into_iter().take(10) {
        let r = &results[i];
        eprintln!("  {:>8.4}  {}  ({})", w, r.id, r.status);
    }
}
