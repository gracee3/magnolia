mod dataset;
mod gpu_telemetry;
mod infer;
mod wer;

use anyhow::Context;
use chrono::Local;
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, ValueEnum)]
enum Mode {
    /// Generate `manifest.jsonl` and exit.
    Manifest,
    /// Run only a small deterministic prefix of the manifest.
    Smoke,
    /// Run the full manifest.
    Full,
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
pub(crate) struct Args {
    /// Dataset directory to scan recursively (expects LibriSpeech layout with .wav + *.trans.txt).
    #[arg(long)]
    dataset: PathBuf,

    /// Engine to run (currently only `parakeet`).
    #[arg(long, default_value = "parakeet")]
    engine: String,

    #[arg(long, value_enum, default_value_t = Mode::Smoke)]
    mode: Mode,

    /// Number of utterances to run in smoke mode.
    #[arg(long, default_value_t = 3)]
    smoke_n: usize,

    /// Max utterances to run (0 = no limit). Overrides mode defaults.
    #[arg(long, default_value_t = 0)]
    limit: usize,

    /// Output directory for manifest + results (default: target/asr_test).
    #[arg(long, default_value = "target/asr_test")]
    out_dir: PathBuf,

    /// Chunk size in milliseconds for deterministic WAV chunking.
    #[arg(long, default_value_t = 40)]
    chunk_ms: u32,

    /// Pace audio at real-time speed to avoid overflowing the STT input queue.
    #[arg(long, alias = "pace", default_value_t = false)]
    realtime: bool,

    /// Override pacing sleep (ms) between chunks when --realtime is enabled.
    #[arg(long, alias = "pace-ms")]
    realtime_ms: Option<u32>,

    /// Optional model dir override (otherwise uses daemon layout.toml loader).
    #[arg(long)]
    model_dir: Option<PathBuf>,

    /// Optional device id override (otherwise uses daemon layout.toml loader).
    #[arg(long)]
    device_id: Option<i32>,

    /// Optional aggregate WER threshold for pass/fail.
    #[arg(long)]
    wer_threshold: Option<f32>,

    /// Trailing silence (ms) to flush the decoder at end-of-utterance.
    #[arg(long, default_value_t = 4000)]
    flush_ms: u32,

    /// Print partial hypotheses (debug only; not used for scoring).
    #[arg(long)]
    print_partials: bool,

    /// Smoke selection seed (0 = time-based for varied samples).
    #[arg(long, default_value_t = 0)]
    smoke_seed: u64,

    /// Comma-separated utterance IDs to run (overrides smoke sampling).
    #[arg(long, value_delimiter = ',')]
    ids: Vec<String>,

    /// Save the selected smoke IDs to {out_dir}/smoke_last.json.
    #[arg(long)]
    smoke_lock: bool,

    /// Replay the last smoke selection from {out_dir}/smoke_last.json.
    #[arg(long)]
    smoke_use_last: bool,

    /// Decode the entire utterance at stop (offline path) instead of streaming.
    #[arg(long, default_value_t = false)]
    offline: bool,

    /// Block audio sender when the worker is behind (deterministic offline runs).
    #[arg(long, action = clap::ArgAction::Set)]
    backpressure: Option<bool>,

    /// Feature normalization mode: per_chunk, running, or none.
    #[arg(long, default_value = "per_chunk")]
    normalize_mode: String,

    /// Pre-gain applied before feature extraction.
    #[arg(long, default_value_t = 8.0)]
    pre_gain: f32,

    /// Blank penalty (logit units). Higher discourages blank; 0 disables.
    #[arg(long, default_value_t = 0.0)]
    blank_penalty: f32,

    /// Blank penalty delta applied only during post-stop finalization.
    #[arg(long, default_value_t = 0.2)]
    final_blank_penalty_delta: f32,

    /// Comma-separated utterance IDs to enable top-k debug logging for.
    #[arg(long, value_delimiter = ',')]
    debug_ids: Vec<String>,

    /// Extra end-of-stream padding (ms) applied on truncation retry.
    #[arg(long, default_value_t = 0)]
    eos_pad_ms: u32,

    /// Print stop stats for every utterance.
    #[arg(long, default_value_t = false)]
    verbose_stop: bool,

    /// Hard per-utterance timeout (ms). Defaults: 60000 (smoke) / 120000 (non-smoke).
    #[arg(long)]
    utterance_timeout_ms: Option<u64>,

    /// Max time (ms) to wait for stop stats after stop (debug/failure paths).
    #[arg(long, default_value_t = 2000)]
    stop_stats_timeout_ms: u64,

    /// Max time (ms) to wait on backpressure send before erroring.
    #[arg(long, default_value_t = 2000)]
    backpressure_timeout_ms: u64,

    /// Sleep between backpressure retries (microseconds).
    #[arg(long, default_value_t = 200)]
    backpressure_retry_sleep_us: u64,

    /// Max inflight audio chunks (ack-based pacing). 0 disables pacing.
    #[arg(long, default_value_t = 1)]
    inflight_chunks: usize,

    /// Number of worker jobs to process utterances in parallel.
    #[arg(long, default_value_t = 1)]
    jobs: usize,

    // Real-time streaming controls
    /// Minimum time (ms) between partial hypothesis emissions.
    #[arg(long)]
    min_partial_emit_ms: Option<u64>,

    /// Silence hangover (ms) before speech is considered "active".
    #[arg(long)]
    silence_hangover_ms: Option<u64>,

    /// Auto-flush after this many ms of silence (0 = disabled).
    #[arg(long)]
    auto_flush_silence_ms: Option<u64>,

    /// Energy gate threshold for silence detection (0.0 = disabled, 0.005 = typical).
    #[arg(long)]
    gate_threshold: Option<f32>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.engine != "parakeet" {
        anyhow::bail!("Unsupported --engine {} (only 'parakeet' is supported)", args.engine);
    }

    std::fs::create_dir_all(&args.out_dir)
        .with_context(|| format!("Failed to create out dir {}", args.out_dir.display()))?;

    let dataset_root = args
        .dataset
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize dataset {}", args.dataset.display()))?;

    let mut manifest = dataset::build_manifest(&dataset_root)?;
    manifest.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.wav.cmp(&b.wav)));

    let manifest_path = args.out_dir.join("manifest.jsonl");
    dataset::write_manifest_jsonl(&manifest_path, &manifest)?;
    eprintln!(
        "[asr_test] manifest: {} entries -> {}",
        manifest.len(),
        manifest_path.display()
    );

    if matches!(args.mode, Mode::Manifest) {
        return Ok(());
    }

    let default_n = match args.mode {
        Mode::Smoke => args.smoke_n,
        Mode::Full => usize::MAX,
        Mode::Manifest => 0,
    };
    let n = if args.limit > 0 { args.limit } else { default_n };
    let n_for_ids = if args.limit > 0 {
        args.limit
    } else {
        args.ids.len().max(1)
    };

    let mut smoke_seed_used: Option<u64> = None;
    let selected = if !args.ids.is_empty() {
        select_by_ids(&manifest, &args.ids, n_for_ids)?
    } else if args.smoke_use_last {
        let last_ids = read_smoke_last(&args.out_dir)?;
        select_by_ids(&manifest, &last_ids, n)?
    } else {
        match args.mode {
            Mode::Smoke => {
                let seed = if args.smoke_seed == 0 {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64
                } else {
                    args.smoke_seed
                };
                smoke_seed_used = Some(seed);
                manifest.sort_by_key(|entry| smoke_hash(seed, &entry.id, &entry.wav));
                let n = n.min(manifest.len());
                manifest.into_iter().take(n).collect::<Vec<_>>()
            }
            Mode::Full => {
                let n = n.min(manifest.len());
                manifest.into_iter().take(n).collect::<Vec<_>>()
            }
            Mode::Manifest => unreachable!(),
        }
    };

    if matches!(args.mode, Mode::Smoke) || !args.ids.is_empty() || args.smoke_use_last {
        let ids = selected.iter().map(|e| e.id.as_str()).collect::<Vec<_>>();
        eprintln!("[asr_test] selected_ids: {}", ids.join(","));
    }
    if matches!(args.mode, Mode::Smoke) || args.smoke_use_last || !args.ids.is_empty() {
        let seed_label = smoke_seed_used
            .map(|seed| seed.to_string())
            .unwrap_or_else(|| "locked_or_manual".to_string());
        eprintln!("[asr_test] smoke_seed={}", seed_label);
    }

    if args.smoke_lock && matches!(args.mode, Mode::Smoke) {
        let ids = selected.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        write_smoke_last(
            &args.out_dir,
            &ids,
            &args.dataset,
            &args.engine,
            &args.mode,
            args.smoke_n,
            args.blank_penalty,
            args.eos_pad_ms,
            args.chunk_ms,
            smoke_seed_used,
        )?;
    }

    let engine = infer::resolve_parakeet_engine(&args)?;
    if args.blank_penalty.is_finite() {
        std::env::set_var("PARAKEET_BLANK_PENALTY", args.blank_penalty.to_string());
    }
    eprintln!("[asr_test] blank_penalty={}", args.blank_penalty);
    eprintln!("[asr_test] eos_pad_ms={}", args.eos_pad_ms);
    eprintln!("[asr_test] chunk_ms={}", args.chunk_ms);
    let backpressure = args.backpressure.unwrap_or(!args.realtime);
    let utterance_timeout_ms = args.utterance_timeout_ms.unwrap_or_else(|| {
        if matches!(args.mode, Mode::Smoke) {
            60_000
        } else {
            120_000
        }
    });
    eprintln!(
        "[asr_test] offline={} realtime={} realtime_ms={:?} backpressure={} backpressure_timeout_ms={} backpressure_retry_sleep_us={} inflight_chunks={} normalize_mode={} pre_gain={} jobs={} final_blank_penalty_delta={} verbose_stop={} utterance_timeout_ms={} stop_stats_timeout_ms={}",
        args.offline,
        args.realtime,
        args.realtime_ms,
        backpressure,
        args.backpressure_timeout_ms,
        args.backpressure_retry_sleep_us,
        args.inflight_chunks,
        args.normalize_mode,
        args.pre_gain,
        args.jobs,
        args.final_blank_penalty_delta,
        args.verbose_stop,
        utterance_timeout_ms,
        args.stop_stats_timeout_ms
    );

    let streaming = infer::StreamingSettings {
        min_partial_emit_ms: args.min_partial_emit_ms,
        silence_hangover_ms: args.silence_hangover_ms,
        auto_flush_silence_ms: args.auto_flush_silence_ms,
        gate_threshold: args.gate_threshold,
    };

    let results = infer::run_manifest(
        &selected,
        &engine,
        args.chunk_ms,
        args.flush_ms,
        args.realtime,
        args.print_partials,
        args.offline,
        backpressure,
        args.final_blank_penalty_delta,
        args.verbose_stop,
        &args.normalize_mode,
        args.pre_gain,
        &args.debug_ids,
        args.eos_pad_ms,
        args.realtime_ms,
        utterance_timeout_ms,
        args.stop_stats_timeout_ms,
        args.backpressure_timeout_ms,
        args.backpressure_retry_sleep_us,
        args.inflight_chunks,
        args.jobs,
        &streaming,
    )
    .await?;

    let results_path = args.out_dir.join("results.jsonl");
    infer::write_results_jsonl(&results_path, &results)?;

    let summary = infer::summarize(&results);
    let summary_path = args.out_dir.join("summary.json");
    std::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)?;

    infer::print_summary_table(&summary, &results, args.wer_threshold);

    if let Some(th) = args.wer_threshold {
        if summary.aggregate_wer > th {
            anyhow::bail!(
                "aggregate WER {:.4} exceeds threshold {:.4}",
                summary.aggregate_wer,
                th
            );
        }
    }

    Ok(())
}

fn smoke_hash(seed: u64, id: &str, wav: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    id.hash(&mut hasher);
    wav.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Serialize, Deserialize)]
struct SmokeLast {
    dataset: String,
    engine: String,
    mode: String,
    locked_at: String,
    params: SmokeLastParams,
    ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SmokeLastParams {
    smoke_n: usize,
    blank_penalty: f32,
    eos_pad_ms: u32,
    chunk_ms: u32,
    smoke_seed: Option<u64>,
}

fn select_by_ids(
    manifest: &[dataset::ManifestEntry],
    ids: &[String],
    limit: usize,
) -> anyhow::Result<Vec<dataset::ManifestEntry>> {
    let mut selected = Vec::with_capacity(ids.len());
    for id in ids.iter().take(if limit > 0 { limit } else { ids.len() }) {
        let entry = manifest
            .iter()
            .find(|e| e.id == *id)
            .with_context(|| format!("Unknown utterance id {}", id))?;
        selected.push(entry.clone());
    }
    Ok(selected)
}

fn read_smoke_last(out_dir: &PathBuf) -> anyhow::Result<Vec<String>> {
    let path = out_dir.join("smoke_last.json");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let data: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    if let Some(ids) = data.as_array() {
        return Ok(ids
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect());
    }
    let ids = data
        .get("ids")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Missing ids in {}", path.display()))?;
    Ok(ids
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect())
}

fn write_smoke_last(
    out_dir: &PathBuf,
    ids: &[String],
    dataset: &PathBuf,
    engine: &str,
    mode: &Mode,
    smoke_n: usize,
    blank_penalty: f32,
    eos_pad_ms: u32,
    chunk_ms: u32,
    smoke_seed: Option<u64>,
) -> anyhow::Result<()> {
    let path = out_dir.join("smoke_last.json");
    let data = SmokeLast {
        dataset: dataset.to_string_lossy().to_string(),
        engine: engine.to_string(),
        mode: format!("{:?}", mode).to_lowercase(),
        locked_at: Local::now().to_rfc3339(),
        params: SmokeLastParams {
            smoke_n,
            blank_penalty,
            eos_pad_ms,
            chunk_ms,
            smoke_seed,
        },
        ids: ids.to_vec(),
    };
    std::fs::write(&path, serde_json::to_string_pretty(&data)?)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}
