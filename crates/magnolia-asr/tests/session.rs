use magnolia_asr::*;
use std::collections::VecDeque;
use uuid::Uuid;

fn provenance() -> ModelProvenance {
    ModelProvenance {
        provider: "cpu".into(),
        adapter_version: SHERPA_ADAPTER_VERSION.into(),
        model_name: ACCEPTED_MODEL_NAME.into(),
        model_sha256: "a".repeat(64),
    }
}

fn event(sequence: u64, revision: u64, id: Uuid, body: AsrEventBody) -> AsrEvent {
    AsrEvent {
        header: AsrEventHeader {
            schema_major: 1,
            schema_minor: 0,
            session_id: Uuid::from_u128(1),
            segment_id: Some(id),
            revision,
            sequence,
            runtime_monotonic_ns: sequence,
            audio_start_frame: 0,
            audio_end_frame: 160,
            provenance: provenance(),
        },
        body,
    }
}

#[test]
fn reducer_revises_partials_and_rejects_stale_or_conflicting_results() {
    let id = Uuid::from_u128(2);
    let mut reducer = TranscriptReducer::default();
    let mut start = event(0, 0, id, AsrEventBody::SessionStart);
    start.header.segment_id = None;
    reducer.apply(&start).unwrap();
    reducer
        .apply(&event(
            1,
            0,
            id,
            AsrEventBody::PartialCreate { text: "hel".into() },
        ))
        .unwrap();
    reducer
        .apply(&event(
            2,
            1,
            id,
            AsrEventBody::PartialRevise {
                text: "hello".into(),
            },
        ))
        .unwrap();
    reducer
        .apply(&event(
            3,
            2,
            id,
            AsrEventBody::Final {
                text: "hello".into(),
                words: vec![],
            },
        ))
        .unwrap();
    assert_eq!(reducer.finalised()[0].text, "hello");
    assert_eq!(
        reducer.apply(&event(
            4,
            3,
            id,
            AsrEventBody::Final {
                text: "changed".into(),
                words: vec![]
            },
        )),
        Err(ReducerError::FinalConflict(id))
    );
}

#[test]
fn durable_journal_recovers_only_ordered_finals() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    let id = Uuid::from_u128(3);
    let final_event = event(
        7,
        2,
        id,
        AsrEventBody::Final {
            text: "durable".into(),
            words: vec![],
        },
    );
    let (mut journal, recovered) = TranscriptJournal::open(&path).unwrap();
    assert!(recovered.is_empty());
    journal.append_final(&final_event).unwrap();
    drop(journal);
    let (_, recovered) = TranscriptJournal::open(&path).unwrap();
    assert_eq!(recovered, vec![final_event]);
}

struct FakeBackend {
    updates: VecDeque<Vec<BackendUpdate>>,
    resets: usize,
}

impl StreamingRecognizer for FakeBackend {
    fn accept(&mut self, _packet: &AudioPacket) -> Result<Vec<BackendUpdate>, String> {
        Ok(self.updates.pop_front().unwrap_or_default())
    }

    fn reset(&mut self) {
        self.resets += 1;
    }
}

#[test]
fn worker_orders_partial_final_gap_reset_and_clean_end() {
    let segment = Uuid::from_u128(5);
    let backend = FakeBackend {
        updates: VecDeque::from([vec![
            BackendUpdate::Partial {
                segment_id: segment,
                revision: 0,
                text: "one".into(),
            },
            BackendUpdate::Final {
                segment_id: segment,
                revision: 1,
                text: "one".into(),
            },
        ]]),
        resets: 0,
    };
    let worker = AsrWorker::start(backend, 4, Uuid::from_u128(4), provenance()).unwrap();
    worker
        .try_send(AudioPacket {
            start_frame: 20,
            monotonic_ns: 99,
            samples: vec![0.0; 160],
            discontinuity: Some((DiscontinuityReason::RecordedGap, 20)),
        })
        .unwrap();
    let events = worker.stop(false).unwrap();
    let kinds = events
        .iter()
        .map(|event| match event.body {
            AsrEventBody::SessionStart => "start",
            AsrEventBody::Discontinuity { .. } => "gap",
            AsrEventBody::Reset { .. } => "reset",
            AsrEventBody::PartialCreate { .. } => "partial",
            AsrEventBody::Final { .. } => "final",
            AsrEventBody::SessionEnd { .. } => "end",
            _ => "other",
        })
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["start", "gap", "reset", "partial", "final", "end"]);
    assert!(events
        .windows(2)
        .all(|pair| pair[0].header.sequence < pair[1].header.sequence));
}

#[test]
fn missing_authoritative_hash_stops_before_acquisition() {
    let lock = SherpaAcquisitionLock {
        schema_major: 1,
        adapter_version: SHERPA_ADAPTER_VERSION.into(),
        model_asset_id: ACCEPTED_MODEL_ASSET_ID,
        model: ArtifactLock {
            name: ACCEPTED_MODEL_NAME.into(),
            source_url: ACCEPTED_MODEL_URL.into(),
            expected_bytes: ACCEPTED_MODEL_ARCHIVE_BYTES,
            sha256: None,
            extracted_sha256: Default::default(),
            license: "Apache-2.0".into(),
        },
        native_library: ArtifactLock {
            name: ACCEPTED_NATIVE_NAME.into(),
            source_url: ACCEPTED_NATIVE_URL.into(),
            expected_bytes: ACCEPTED_NATIVE_ARCHIVE_BYTES,
            sha256: None,
            extracted_sha256: Default::default(),
            license: "Apache-2.0".into(),
        },
    };
    assert_eq!(
        lock.validate(),
        Err(AcquisitionError::MissingAuthoritativeHash("model"))
    );
}
