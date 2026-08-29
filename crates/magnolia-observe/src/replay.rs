use crate::{
    sha256_file, validate_bundle, RecordedControl, RecordingError, RecordingManifest, TimelineEntry,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "factor", rename_all = "snake_case")]
pub enum ReplayClock {
    RealTime,
    Accelerated(f32),
    DeterministicStep,
}

pub struct ReplaySource {
    root: PathBuf,
    manifest: RecordingManifest,
    clock: ReplayClock,
    chunk: usize,
    reader: Option<BufReader<File>>,
    started: Instant,
    emitted_frames: u64,
    timeline: Vec<TimelineEntry>,
    controls: Vec<RecordedControl>,
}

impl ReplaySource {
    pub fn open(path: impl AsRef<Path>, clock: ReplayClock) -> Result<Self, ReplayError> {
        if matches!(clock, ReplayClock::Accelerated(factor) if !factor.is_finite() || factor <= 0.0)
        {
            return Err(ReplayError::InvalidClock);
        }
        let root = path.as_ref().to_path_buf();
        let manifest = validate_bundle(&root)?;
        let timeline = read_json_lines(&root.join("timeline.jsonl"))?;
        let controls = read_json_lines(&root.join("controls.jsonl"))?;
        Ok(Self {
            root,
            manifest,
            clock,
            chunk: 0,
            reader: None,
            started: Instant::now(),
            emitted_frames: 0,
            timeline,
            controls,
        })
    }

    pub fn next_frames(&mut self, output: &mut [f32]) -> Result<usize, ReplayError> {
        let channels = usize::from(self.manifest.channels);
        if !output.len().is_multiple_of(channels) {
            return Err(ReplayError::MisalignedOutput);
        }
        let mut bytes = vec![0_u8; std::mem::size_of_val(output)];
        let mut written_bytes = 0;
        while written_bytes < bytes.len() && self.chunk < self.manifest.chunks.len() {
            if self.reader.is_none() {
                self.reader = Some(BufReader::new(File::open(
                    self.root.join(&self.manifest.chunks[self.chunk].path),
                )?));
            }
            let Some(reader) = self.reader.as_mut() else {
                return Err(ReplayError::ReaderUnavailable);
            };
            let read = reader.read(&mut bytes[written_bytes..])?;
            if read == 0 {
                self.reader = None;
                self.chunk += 1;
            } else {
                written_bytes += read;
            }
        }
        if !written_bytes.is_multiple_of(channels * 4) {
            return Err(ReplayError::CorruptPcmBoundary);
        }
        for (sample, encoded) in output
            .iter_mut()
            .zip(bytes[..written_bytes].chunks_exact(4))
        {
            *sample = f32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
        }
        let frames = written_bytes / (channels * 4);
        self.apply_clock(frames as u64);
        self.emitted_frames = self.emitted_frames.saturating_add(frames as u64);
        Ok(frames)
    }

    pub fn events_through(&self, source_frame: u64) -> (&[TimelineEntry], &[RecordedControl]) {
        let timeline_end = self
            .timeline
            .partition_point(|entry| entry.source_start <= source_frame);
        let control_end = self
            .controls
            .partition_point(|entry| entry.source_frame <= source_frame);
        (
            &self.timeline[..timeline_end],
            &self.controls[..control_end],
        )
    }

    pub fn deterministic_hashes(&self) -> Result<ReplayHashes, ReplayError> {
        let mut pcm = Sha256::new();
        for chunk in &self.manifest.chunks {
            pcm.update(fs::read(self.root.join(&chunk.path))?);
        }
        Ok(ReplayHashes {
            pcm_sha256: format!("{:x}", pcm.finalize()),
            timeline_sha256: sha256_file(&self.root.join("timeline.jsonl"))?,
            controls_sha256: sha256_file(&self.root.join("controls.jsonl"))?,
            manifest_sha256: sha256_file(&self.root.join("manifest.json"))?,
            analyzer_frames_sha256: sha256_file(&self.root.join("analyzers.jsonl"))?,
            telemetry_payloads_sha256: sha256_file(&self.root.join("telemetry.jsonl"))?,
        })
    }

    fn apply_clock(&self, frames: u64) {
        let factor = match self.clock {
            ReplayClock::RealTime => 1.0,
            ReplayClock::Accelerated(factor) => factor,
            ReplayClock::DeterministicStep => return,
        };
        let expected = Duration::from_secs_f64(
            (self.emitted_frames.saturating_add(frames)) as f64
                / f64::from(self.manifest.sample_rate)
                / f64::from(factor),
        );
        if let Some(remaining) = expected.checked_sub(self.started.elapsed()) {
            std::thread::sleep(remaining);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayHashes {
    pub pcm_sha256: String,
    pub timeline_sha256: String,
    pub controls_sha256: String,
    pub manifest_sha256: String,
    pub analyzer_frames_sha256: String,
    pub telemetry_payloads_sha256: String,
}

fn read_json_lines<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, ReplayError> {
    let mut values = Vec::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if !line.trim().is_empty() {
            values.push(serde_json::from_str(&line)?);
        }
    }
    Ok(values)
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("accelerated replay factor must be finite and positive")]
    InvalidClock,
    #[error("replay output must contain complete interleaved frames")]
    MisalignedOutput,
    #[error("recorded PCM ended at an invalid sample boundary")]
    CorruptPcmBoundary,
    #[error("replay chunk reader was not initialized")]
    ReaderUnavailable,
    #[error(transparent)]
    Recording(#[from] RecordingError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RecordingWriter, RECORDING_SCHEMA_MAJOR};
    use magnolia_domain::WorkspaceDocument;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    #[test]
    fn deterministic_replay_repeats_pcm_and_manifest_hashes() {
        let root = tempfile::tempdir().unwrap();
        let manifest = RecordingManifest {
            schema_major: RECORDING_SCHEMA_MAJOR,
            schema_minor: 0,
            recording_id: Uuid::from_u128(7),
            sample_rate: 48_000,
            channels: 1,
            build_sha: "test".to_owned(),
            device_runtime_id: "seeded".to_owned(),
            device_fingerprint: BTreeMap::new(),
            chunks: Vec::new(),
            file_hashes: BTreeMap::new(),
        };
        let mut writer = RecordingWriter::create(
            root.path(),
            "replay",
            WorkspaceDocument::default(),
            manifest,
        )
        .unwrap();
        writer.write_pcm(&[0.0, 0.25, -0.25, 0.5]).unwrap();
        writer
            .write_timeline(&TimelineEntry {
                sequence: 0,
                source_start: 0,
                source_end: 4,
                monotonic_ns: 1,
                dropped_frames_before: 0,
                discontinuity: None,
            })
            .unwrap();
        let path = writer.finalize().unwrap();
        let mut first = ReplaySource::open(&path, ReplayClock::DeterministicStep).unwrap();
        let second = ReplaySource::open(&path, ReplayClock::DeterministicStep).unwrap();
        assert_eq!(
            first.deterministic_hashes().unwrap(),
            second.deterministic_hashes().unwrap()
        );
        let mut output = [0.0; 4];
        assert_eq!(first.next_frames(&mut output).unwrap(), 4);
        assert_eq!(output, [0.0, 0.25, -0.25, 0.5]);
    }
}
