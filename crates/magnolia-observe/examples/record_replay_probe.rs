use magnolia_domain::WorkspaceDocument;
use magnolia_observe::{
    AnalyzerEngine, AnalyzerFrame, AnalyzerKind, BlockTiming, RecordedControl, RecordingManifest,
    RecordingStorageWorker, RecordingWriter, ReplayClock, ReplaySource, TimelineEntry,
    RECORDING_SCHEMA_MAJOR,
};
use std::{collections::BTreeMap, env, f32::consts::PI, path::PathBuf};
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: record_replay_probe OUTPUT_DIRECTORY")?;
    let manifest = RecordingManifest {
        schema_major: RECORDING_SCHEMA_MAJOR,
        schema_minor: 0,
        recording_id: Uuid::new_v4(),
        sample_rate: 48_000,
        channels: 1,
        build_sha: option_env!("MAGNOLIA_BUILD_SHA")
            .unwrap_or("development")
            .to_owned(),
        device_runtime_id: "seeded-synthetic-440hz".to_owned(),
        device_fingerprint: BTreeMap::from([("generator".to_owned(), "sine".to_owned())]),
        chunks: Vec::new(),
        file_hashes: BTreeMap::new(),
    };
    let writer = RecordingWriter::create(
        &root,
        "seeded-replay",
        WorkspaceDocument::default(),
        manifest,
    )?;
    let storage = RecordingStorageWorker::start(writer, 32)?;
    let samples = (0..4_096)
        .map(|frame| (2.0 * PI * 440.0 * frame as f32 / 48_000.0).sin() * 0.25)
        .collect::<Vec<_>>();
    storage.write_pcm(samples.clone())?;
    storage.write_timeline(TimelineEntry {
        sequence: 0,
        source_start: 0,
        source_end: samples.len() as u64,
        monotonic_ns: 1,
        dropped_frames_before: 0,
        discontinuity: None,
    })?;
    storage.write_control(RecordedControl {
        source_frame: 2_048,
        command: "capture_mute".to_owned(),
        value: serde_json::Value::Bool(false),
    })?;
    let mut analyzer = AnalyzerEngine::new(48_000, 1)?;
    for kind in AnalyzerKind::ALL {
        analyzer.set_leased(kind, true);
    }
    let frames = analyzer.process(
        &samples,
        BlockTiming {
            sequence: 0,
            source_start: 0,
            capture_monotonic_ns: 1,
            block_complete_monotonic_ns: 2,
            graph_monotonic_ns: 3,
            analyzer_monotonic_ns: 4,
            cumulative_dropped_frames: 0,
            discontinuity: false,
            queue_depth: 0,
            utilization_millionths: 10_000,
            processing_ns: 1_000,
            cumulative_discontinuities: 0,
        },
    )?;
    for frame in frames {
        storage.write_analyzer(serde_json::to_value(&frame)?)?;
        storage.write_telemetry(serde_json::json!({
            "sequence": frame_sequence(&frame),
            "kind": frame.kind(),
        }))?;
    }
    let bundle = storage.finalize()?;
    let first = ReplaySource::open(&bundle, ReplayClock::DeterministicStep)?;
    let second = ReplaySource::open(&bundle, ReplayClock::DeterministicStep)?;
    let first_hashes = first.deterministic_hashes()?;
    let second_hashes = second.deterministic_hashes()?;
    if first_hashes != second_hashes {
        return Err("deterministic replay hashes differed between identical runs".into());
    }
    println!("REPLAY bundle={} hashes={first_hashes:?}", bundle.display());
    Ok(())
}

fn frame_sequence(frame: &AnalyzerFrame) -> u64 {
    match frame {
        AnalyzerFrame::Meter(frame) => frame.header.sequence,
        AnalyzerFrame::Waveform(frame) => frame.header.sequence,
        AnalyzerFrame::Spectrum(frame) => frame.header.sequence,
        AnalyzerFrame::Diagnostics(frame) => frame.header.sequence,
    }
}
