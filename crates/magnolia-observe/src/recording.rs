use magnolia_domain::WorkspaceDocument;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender},
    thread::{self, JoinHandle},
};
use thiserror::Error;
use uuid::Uuid;

pub const RECORDING_SCHEMA_MAJOR: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingManifest {
    pub schema_major: u16,
    pub schema_minor: u16,
    pub recording_id: Uuid,
    pub sample_rate: u32,
    pub channels: u16,
    pub build_sha: String,
    pub device_runtime_id: String,
    pub device_fingerprint: BTreeMap<String, String>,
    pub chunks: Vec<ChunkManifest>,
    pub file_hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkManifest {
    pub path: String,
    pub start_frame: u64,
    pub frames: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub sequence: u64,
    pub source_start: u64,
    pub source_end: u64,
    pub monotonic_ns: u64,
    pub dropped_frames_before: u64,
    pub discontinuity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedControl {
    pub source_frame: u64,
    pub command: String,
    pub value: serde_json::Value,
}

pub struct RecordingWriter {
    root: PathBuf,
    staging: PathBuf,
    final_path: PathBuf,
    manifest: RecordingManifest,
    chunk_index: u32,
    current_start: u64,
    current_frames: u64,
    current_path: PathBuf,
    current: BufWriter<File>,
    timeline: BufWriter<File>,
    controls: BufWriter<File>,
    diagnostics: BufWriter<File>,
    analyzers: BufWriter<File>,
    telemetry: BufWriter<File>,
}

impl RecordingWriter {
    pub fn create(
        root: impl AsRef<Path>,
        name: &str,
        workspace: WorkspaceDocument,
        mut manifest: RecordingManifest,
    ) -> Result<Self, RecordingError> {
        if name.trim().is_empty() || name.contains('/') || name.contains("..") {
            return Err(RecordingError::InvalidName);
        }
        if manifest.schema_major != RECORDING_SCHEMA_MAJOR
            || manifest.sample_rate == 0
            || manifest.channels == 0
        {
            return Err(RecordingError::InvalidManifest);
        }
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let staging = root.join(format!(".{name}.incomplete-{}", Uuid::new_v4()));
        let final_path = root.join(name);
        if final_path.exists() {
            return Err(RecordingError::AlreadyExists(final_path));
        }
        fs::create_dir(&staging)?;
        fs::create_dir(staging.join("pcm"))?;
        fs::write(
            staging.join("INCOMPLETE.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_major": RECORDING_SCHEMA_MAJOR,
                "recording_id": manifest.recording_id,
            }))?,
        )?;
        fs::write(
            staging.join("staging-manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        fs::write(
            staging.join("workspace.json"),
            serde_json::to_vec_pretty(&workspace)?,
        )?;
        fs::write(staging.join("transcript.journal.jsonl"), b"")?;
        let current_path = staging.join("pcm/000000.f32le");
        let current = create_buffered(&current_path)?;
        let timeline = create_buffered(&staging.join("timeline.jsonl"))?;
        let controls = create_buffered(&staging.join("controls.jsonl"))?;
        let diagnostics = create_buffered(&staging.join("diagnostics.jsonl"))?;
        let analyzers = create_buffered(&staging.join("analyzers.jsonl"))?;
        let telemetry = create_buffered(&staging.join("telemetry.jsonl"))?;
        manifest.chunks.clear();
        manifest.file_hashes.clear();
        Ok(Self {
            root,
            staging,
            final_path,
            manifest,
            chunk_index: 0,
            current_start: 0,
            current_frames: 0,
            current_path,
            current,
            timeline,
            controls,
            diagnostics,
            analyzers,
            telemetry,
        })
    }

    pub fn write_pcm(&mut self, interleaved: &[f32]) -> Result<(), RecordingError> {
        let channels = usize::from(self.manifest.channels);
        if !interleaved.len().is_multiple_of(channels) {
            return Err(RecordingError::MisalignedPcm);
        }
        for sample in interleaved {
            self.current.write_all(&sample.to_le_bytes())?;
        }
        self.current_frames = self
            .current_frames
            .saturating_add((interleaved.len() / channels) as u64);
        Ok(())
    }

    pub fn write_timeline(&mut self, entry: &TimelineEntry) -> Result<(), RecordingError> {
        write_json_line(&mut self.timeline, entry)
    }

    pub fn write_control(&mut self, control: &RecordedControl) -> Result<(), RecordingError> {
        write_json_line(&mut self.controls, control)
    }

    pub fn write_diagnostics(&mut self, value: &serde_json::Value) -> Result<(), RecordingError> {
        write_json_line(&mut self.diagnostics, value)
    }

    pub fn write_analyzer(&mut self, value: &serde_json::Value) -> Result<(), RecordingError> {
        write_json_line(&mut self.analyzers, value)
    }

    pub fn write_telemetry(&mut self, value: &serde_json::Value) -> Result<(), RecordingError> {
        write_json_line(&mut self.telemetry, value)
    }

    pub fn rotate_chunk(&mut self) -> Result<(), RecordingError> {
        self.finish_current_chunk()?;
        self.chunk_index = self.chunk_index.saturating_add(1);
        self.current_start = self.current_start.saturating_add(self.current_frames);
        self.current_frames = 0;
        self.current_path = self
            .staging
            .join(format!("pcm/{:06}.f32le", self.chunk_index));
        self.current = create_buffered(&self.current_path)?;
        Ok(())
    }

    pub fn finalize(mut self) -> Result<PathBuf, RecordingError> {
        self.finish_current_chunk()?;
        flush_sync(&mut self.timeline)?;
        flush_sync(&mut self.controls)?;
        flush_sync(&mut self.diagnostics)?;
        flush_sync(&mut self.analyzers)?;
        flush_sync(&mut self.telemetry)?;
        for file in [
            "workspace.json",
            "timeline.jsonl",
            "controls.jsonl",
            "diagnostics.jsonl",
            "analyzers.jsonl",
            "telemetry.jsonl",
            "transcript.journal.jsonl",
        ] {
            self.manifest
                .file_hashes
                .insert(file.to_owned(), sha256_file(&self.staging.join(file))?);
        }
        fs::write(
            self.staging.join("manifest.json"),
            serde_json::to_vec_pretty(&self.manifest)?,
        )?;
        fs::remove_file(self.staging.join("staging-manifest.json"))?;
        File::open(&self.staging)?.sync_all()?;
        fs::remove_file(self.staging.join("INCOMPLETE.json"))?;
        fs::rename(&self.staging, &self.final_path)?;
        File::open(&self.root)?.sync_all()?;
        Ok(self.final_path)
    }

    fn finish_current_chunk(&mut self) -> Result<(), RecordingError> {
        flush_sync(&mut self.current)?;
        let relative = format!("pcm/{:06}.f32le", self.chunk_index);
        let hash = sha256_file(&self.current_path)?;
        if let Some(existing) = self
            .manifest
            .chunks
            .iter_mut()
            .find(|chunk| chunk.path == relative)
        {
            existing.frames = self.current_frames;
            existing.sha256 = hash;
        } else {
            self.manifest.chunks.push(ChunkManifest {
                path: relative,
                start_frame: self.current_start,
                frames: self.current_frames,
                sha256: hash,
            });
        }
        Ok(())
    }
}

enum StorageCommand {
    Pcm(Vec<f32>),
    Timeline(TimelineEntry),
    Control(RecordedControl),
    Analyzer(serde_json::Value),
    Telemetry(serde_json::Value),
    Finalize(mpsc::Sender<Result<PathBuf, RecordingError>>),
}

pub struct RecordingStorageWorker {
    sender: SyncSender<StorageCommand>,
    worker: Option<JoinHandle<()>>,
}

impl RecordingStorageWorker {
    pub fn start(writer: RecordingWriter, capacity: usize) -> Result<Self, RecordingError> {
        if capacity == 0 {
            return Err(RecordingError::InvalidWorkerCapacity);
        }
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let worker = thread::Builder::new()
            .name("magnolia-recording-storage".to_owned())
            .spawn(move || {
                let mut writer = Some(writer);
                while let Ok(command) = receiver.recv() {
                    let result = match command {
                        StorageCommand::Pcm(samples) => writer
                            .as_mut()
                            .ok_or(RecordingError::WorkerUnavailable)
                            .and_then(|writer| writer.write_pcm(&samples)),
                        StorageCommand::Timeline(entry) => writer
                            .as_mut()
                            .ok_or(RecordingError::WorkerUnavailable)
                            .and_then(|writer| writer.write_timeline(&entry)),
                        StorageCommand::Control(control) => writer
                            .as_mut()
                            .ok_or(RecordingError::WorkerUnavailable)
                            .and_then(|writer| writer.write_control(&control)),
                        StorageCommand::Analyzer(value) => writer
                            .as_mut()
                            .ok_or(RecordingError::WorkerUnavailable)
                            .and_then(|writer| writer.write_analyzer(&value)),
                        StorageCommand::Telemetry(value) => writer
                            .as_mut()
                            .ok_or(RecordingError::WorkerUnavailable)
                            .and_then(|writer| writer.write_telemetry(&value)),
                        StorageCommand::Finalize(response) => {
                            let result = writer
                                .take()
                                .ok_or(RecordingError::WorkerUnavailable)
                                .and_then(RecordingWriter::finalize);
                            let _ = response.send(result);
                            break;
                        }
                    };
                    if result.is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    pub fn write_pcm(&self, samples: Vec<f32>) -> Result<(), RecordingError> {
        self.send(StorageCommand::Pcm(samples))
    }

    pub fn write_timeline(&self, entry: TimelineEntry) -> Result<(), RecordingError> {
        self.send(StorageCommand::Timeline(entry))
    }

    pub fn write_control(&self, control: RecordedControl) -> Result<(), RecordingError> {
        self.send(StorageCommand::Control(control))
    }

    pub fn write_analyzer(&self, value: serde_json::Value) -> Result<(), RecordingError> {
        self.send(StorageCommand::Analyzer(value))
    }

    pub fn write_telemetry(&self, value: serde_json::Value) -> Result<(), RecordingError> {
        self.send(StorageCommand::Telemetry(value))
    }

    pub fn finalize(mut self) -> Result<PathBuf, RecordingError> {
        let (sender, receiver) = mpsc::channel();
        self.send(StorageCommand::Finalize(sender))?;
        let result = receiver
            .recv()
            .map_err(|_| RecordingError::WorkerUnavailable)?;
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| RecordingError::WorkerPanicked)?;
        }
        result
    }

    fn send(&self, command: StorageCommand) -> Result<(), RecordingError> {
        self.sender
            .send(command)
            .map_err(|_| RecordingError::WorkerUnavailable)
    }
}

pub fn validate_bundle(path: impl AsRef<Path>) -> Result<RecordingManifest, RecordingError> {
    let path = path.as_ref();
    if path.join("INCOMPLETE.json").exists() {
        return Err(RecordingError::Incomplete(path.to_path_buf()));
    }
    let manifest: RecordingManifest =
        serde_json::from_slice(&fs::read(path.join("manifest.json"))?)?;
    if manifest.schema_major != RECORDING_SCHEMA_MAJOR {
        return Err(RecordingError::UnsupportedMajor(manifest.schema_major));
    }
    for chunk in &manifest.chunks {
        let chunk_path = safe_relative(path, &chunk.path)?;
        let bytes = fs::metadata(&chunk_path)?.len();
        let expected = chunk
            .frames
            .saturating_mul(u64::from(manifest.channels))
            .saturating_mul(4);
        if bytes != expected || sha256_file(&chunk_path)? != chunk.sha256 {
            return Err(RecordingError::HashOrBoundary(chunk.path.clone()));
        }
    }
    for (file, expected) in &manifest.file_hashes {
        let file_path = safe_relative(path, file)?;
        if sha256_file(&file_path)? != *expected {
            return Err(RecordingError::HashOrBoundary(file.clone()));
        }
    }
    Ok(manifest)
}

pub fn inspect_incomplete(path: impl AsRef<Path>) -> Result<Vec<PathBuf>, RecordingError> {
    let mut found = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("INCOMPLETE.json").is_file() {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

pub fn recover_incomplete(
    staging: impl AsRef<Path>,
    recovered_name: &str,
) -> Result<PathBuf, RecordingError> {
    if recovered_name.trim().is_empty()
        || recovered_name.contains('/')
        || recovered_name.contains("..")
    {
        return Err(RecordingError::InvalidName);
    }
    let staging = staging.as_ref();
    if !staging.join("INCOMPLETE.json").is_file() {
        return Err(RecordingError::NotIncomplete(staging.to_path_buf()));
    }
    let root = staging.parent().ok_or(RecordingError::UnsafeRecoveryRoot)?;
    let final_path = root.join(recovered_name);
    if final_path.exists() {
        return Err(RecordingError::AlreadyExists(final_path));
    }
    let mut manifest: RecordingManifest =
        serde_json::from_slice(&fs::read(staging.join("staging-manifest.json"))?)?;
    if manifest.schema_major != RECORDING_SCHEMA_MAJOR || manifest.channels == 0 {
        return Err(RecordingError::InvalidManifest);
    }
    manifest.chunks.clear();
    let mut start = 0_u64;
    let mut chunks = fs::read_dir(staging.join("pcm"))?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|value| value == "f32le")
        })
        .collect::<Vec<_>>();
    chunks.sort_by_key(std::fs::DirEntry::path);
    for entry in chunks {
        let bytes = entry.metadata()?.len();
        let frame_bytes = u64::from(manifest.channels).saturating_mul(4);
        if bytes % frame_bytes != 0 {
            return Err(RecordingError::HashOrBoundary(
                entry.file_name().to_string_lossy().into_owned(),
            ));
        }
        let frames = bytes / frame_bytes;
        let relative = format!("pcm/{}", entry.file_name().to_string_lossy());
        manifest.chunks.push(ChunkManifest {
            path: relative,
            start_frame: start,
            frames,
            sha256: sha256_file(&entry.path())?,
        });
        start = start.saturating_add(frames);
    }
    manifest.file_hashes.clear();
    for file in [
        "workspace.json",
        "timeline.jsonl",
        "controls.jsonl",
        "diagnostics.jsonl",
        "analyzers.jsonl",
        "telemetry.jsonl",
        "transcript.journal.jsonl",
    ] {
        validate_json_lines_if_needed(&staging.join(file), file)?;
        manifest
            .file_hashes
            .insert(file.to_owned(), sha256_file(&staging.join(file))?);
    }
    fs::write(
        staging.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::remove_file(staging.join("staging-manifest.json"))?;
    fs::remove_file(staging.join("INCOMPLETE.json"))?;
    File::open(staging)?.sync_all()?;
    fs::rename(staging, &final_path)?;
    File::open(root)?.sync_all()?;
    validate_bundle(&final_path)?;
    Ok(final_path)
}

fn validate_json_lines_if_needed(path: &Path, name: &str) -> Result<(), RecordingError> {
    if name == "workspace.json" {
        let _: WorkspaceDocument = serde_json::from_slice(&fs::read(path)?)?;
        return Ok(());
    }
    for line in std::io::BufRead::lines(std::io::BufReader::new(File::open(path)?)) {
        let line = line?;
        if !line.trim().is_empty() {
            let _: serde_json::Value = serde_json::from_str(&line)?;
        }
    }
    Ok(())
}

fn create_buffered(path: &Path) -> Result<BufWriter<File>, RecordingError> {
    Ok(BufWriter::new(
        OpenOptions::new().create_new(true).write(true).open(path)?,
    ))
}

fn flush_sync(writer: &mut BufWriter<File>) -> Result<(), RecordingError> {
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn write_json_line(
    writer: &mut BufWriter<File>,
    value: &impl Serialize,
) -> Result<(), RecordingError> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, RecordingError> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn safe_relative(root: &Path, relative: &str) -> Result<PathBuf, RecordingError> {
    let candidate = Path::new(relative);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
    {
        return Err(RecordingError::UnsafePath(relative.to_owned()));
    }
    Ok(root.join(candidate))
}

#[derive(Debug, Error)]
pub enum RecordingError {
    #[error("recording name is blank or unsafe")]
    InvalidName,
    #[error("recording manifest is invalid")]
    InvalidManifest,
    #[error("recording already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("PCM does not contain complete interleaved frames")]
    MisalignedPcm,
    #[error("recording bundle is incomplete: {0}")]
    Incomplete(PathBuf),
    #[error("unsupported recording schema major {0}")]
    UnsupportedMajor(u16),
    #[error("unsafe bundle path {0}")]
    UnsafePath(String),
    #[error("bundle hash or chunk boundary mismatch for {0}")]
    HashOrBoundary(String),
    #[error("recording storage worker capacity must be non-zero")]
    InvalidWorkerCapacity,
    #[error("recording storage worker is unavailable")]
    WorkerUnavailable,
    #[error("recording storage worker panicked")]
    WorkerPanicked,
    #[error("recording is not marked incomplete: {0}")]
    NotIncomplete(PathBuf),
    #[error("recording recovery directory has no safe parent")]
    UnsafeRecoveryRoot,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> RecordingManifest {
        RecordingManifest {
            schema_major: 1,
            schema_minor: 0,
            recording_id: Uuid::from_u128(1),
            sample_rate: 48_000,
            channels: 2,
            build_sha: "test".to_owned(),
            device_runtime_id: "synthetic".to_owned(),
            device_fingerprint: BTreeMap::new(),
            chunks: Vec::new(),
            file_hashes: BTreeMap::new(),
        }
    }

    #[test]
    fn finalization_is_atomic_and_validates_hashes() {
        let root = tempfile::tempdir().unwrap();
        let mut writer = RecordingWriter::create(
            root.path(),
            "bundle",
            WorkspaceDocument::default(),
            manifest(),
        )
        .unwrap();
        writer.write_pcm(&[0.0, 0.0, 0.5, -0.5]).unwrap();
        writer
            .write_timeline(&TimelineEntry {
                sequence: 0,
                source_start: 0,
                source_end: 2,
                monotonic_ns: 10,
                dropped_frames_before: 0,
                discontinuity: None,
            })
            .unwrap();
        let bundle = writer.finalize().unwrap();
        assert!(!bundle.join("INCOMPLETE.json").exists());
        assert_eq!(validate_bundle(&bundle).unwrap().chunks[0].frames, 2);
    }

    #[test]
    fn incomplete_and_corrupt_bundles_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let writer = RecordingWriter::create(
            root.path(),
            "bundle",
            WorkspaceDocument::default(),
            manifest(),
        )
        .unwrap();
        assert_eq!(inspect_incomplete(root.path()).unwrap().len(), 1);
        drop(writer);

        let mut writer = RecordingWriter::create(
            root.path(),
            "final",
            WorkspaceDocument::default(),
            manifest(),
        )
        .unwrap();
        writer.write_pcm(&[0.0, 0.0]).unwrap();
        let bundle = writer.finalize().unwrap();
        fs::write(bundle.join("pcm/000000.f32le"), b"bad").unwrap();
        assert!(matches!(
            validate_bundle(bundle),
            Err(RecordingError::HashOrBoundary(_))
        ));
    }

    #[test]
    fn incomplete_bundle_is_recovered_only_after_boundary_validation() {
        let root = tempfile::tempdir().unwrap();
        let mut writer = RecordingWriter::create(
            root.path(),
            "interrupted",
            WorkspaceDocument::default(),
            manifest(),
        )
        .unwrap();
        writer.write_pcm(&[0.25, -0.25, 0.5, -0.5]).unwrap();
        drop(writer);
        let staging = inspect_incomplete(root.path()).unwrap().pop().unwrap();
        let recovered = recover_incomplete(&staging, "recovered").unwrap();
        let recovered_manifest = validate_bundle(recovered).unwrap();
        assert_eq!(recovered_manifest.chunks[0].frames, 2);
    }

    #[test]
    fn bounded_storage_worker_finalizes_analyzer_and_telemetry_journals() {
        let root = tempfile::tempdir().unwrap();
        let writer = RecordingWriter::create(
            root.path(),
            "worker",
            WorkspaceDocument::default(),
            manifest(),
        )
        .unwrap();
        let worker = RecordingStorageWorker::start(writer, 4).unwrap();
        worker.write_pcm(vec![0.1, -0.1]).unwrap();
        worker
            .write_analyzer(serde_json::json!({"sequence": 1, "meter": 0.1}))
            .unwrap();
        worker
            .write_telemetry(serde_json::json!({"sequence": 1, "payload": "meter"}))
            .unwrap();
        let bundle = worker.finalize().unwrap();
        let manifest = validate_bundle(bundle).unwrap();
        assert!(manifest.file_hashes.contains_key("analyzers.jsonl"));
        assert!(manifest.file_hashes.contains_key("telemetry.jsonl"));
    }
}
