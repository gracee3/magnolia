use crate::{AsrEvent, AsrEventBody};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Append-only finalized transcript journal. A successful append is durable.
pub struct TranscriptJournal {
    path: PathBuf,
    writer: BufWriter<File>,
    last_sequence: Option<u64>,
}

impl TranscriptJournal {
    pub fn open(path: impl AsRef<Path>) -> Result<(Self, Vec<AsrEvent>), JournalError> {
        let path = path.as_ref().to_path_buf();
        let recovered = if path.exists() {
            read_events(&path)?
        } else {
            Vec::new()
        };
        validate_final_order(&recovered)?;
        let last_sequence = recovered.last().map(|event| event.header.sequence);
        let writer = BufWriter::new(OpenOptions::new().create(true).append(true).open(&path)?);
        Ok((
            Self {
                path,
                writer,
                last_sequence,
            },
            recovered,
        ))
    }

    pub fn append_final(&mut self, event: &AsrEvent) -> Result<(), JournalError> {
        if !matches!(event.body, AsrEventBody::Final { .. }) {
            return Err(JournalError::NotFinal);
        }
        if self
            .last_sequence
            .is_some_and(|sequence| event.header.sequence <= sequence)
        {
            return Err(JournalError::NonIncreasingSequence(event.header.sequence));
        }
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        self.writer.get_ref().sync_data()?;
        self.last_sequence = Some(event.header.sequence);
        Ok(())
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn read_events(path: &Path) -> Result<Vec<AsrEvent>, JournalError> {
    BufReader::new(File::open(path)?)
        .lines()
        .filter_map(|line| match line {
            Ok(line) if line.trim().is_empty() => None,
            other => Some(other),
        })
        .map(|line| Ok(serde_json::from_str(&line?)?))
        .collect()
}

fn validate_final_order(events: &[AsrEvent]) -> Result<(), JournalError> {
    let mut previous = None;
    for event in events {
        if !event.is_final() {
            return Err(JournalError::NotFinal);
        }
        if previous.is_some_and(|sequence| event.header.sequence <= sequence) {
            return Err(JournalError::NonIncreasingSequence(event.header.sequence));
        }
        previous = Some(event.header.sequence);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error("only finalized ASR events may enter the durable journal")]
    NotFinal,
    #[error("final ASR sequence {0} does not increase")]
    NonIncreasingSequence(u64),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
