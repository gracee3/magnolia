use crate::{AsrEvent, AsrEventBody, WordAlignment};
use std::collections::BTreeMap;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReducedSegment {
    pub id: Uuid,
    pub revision: u64,
    pub sequence: u64,
    pub text: String,
    pub words: Vec<WordAlignment>,
    pub finalised: bool,
}

#[derive(Debug, Default)]
pub struct TranscriptReducer {
    session_id: Option<Uuid>,
    segments: BTreeMap<Uuid, ReducedSegment>,
    last_sequence: Option<u64>,
}

impl TranscriptReducer {
    pub fn apply(&mut self, event: &AsrEvent) -> Result<(), ReducerError> {
        if event.header.schema_major != crate::ASR_EVENT_SCHEMA_MAJOR {
            return Err(ReducerError::UnsupportedMajor(event.header.schema_major));
        }
        if self
            .last_sequence
            .is_some_and(|sequence| event.header.sequence <= sequence)
        {
            return Err(ReducerError::StaleSequence(event.header.sequence));
        }
        if self
            .session_id
            .is_some_and(|session| session != event.header.session_id)
        {
            return Err(ReducerError::WrongSession);
        }
        match &event.body {
            AsrEventBody::SessionStart => {
                if self.session_id.is_some() {
                    return Err(ReducerError::SessionAlreadyStarted);
                }
                self.session_id = Some(event.header.session_id);
            }
            AsrEventBody::PartialCreate { text } => {
                let id = segment_id(event)?;
                if self.segments.contains_key(&id) {
                    return Err(ReducerError::SegmentAlreadyExists(id));
                }
                self.segments.insert(
                    id,
                    ReducedSegment {
                        id,
                        revision: event.header.revision,
                        sequence: event.header.sequence,
                        text: text.clone(),
                        words: Vec::new(),
                        finalised: false,
                    },
                );
            }
            AsrEventBody::PartialRevise { text } => {
                let segment = self.mutable_segment(event)?;
                segment.text.clone_from(text);
            }
            AsrEventBody::AlignmentUpdate { words } => {
                let segment = self.mutable_segment(event)?;
                segment.words.clone_from(words);
            }
            AsrEventBody::Final { text, words } => {
                let id = segment_id(event)?;
                match self.segments.get_mut(&id) {
                    Some(segment) if segment.finalised => {
                        return Err(ReducerError::FinalConflict(id));
                    }
                    Some(segment) if event.header.revision <= segment.revision => {
                        return Err(ReducerError::StaleRevision {
                            id,
                            revision: event.header.revision,
                        });
                    }
                    Some(segment) => {
                        segment.revision = event.header.revision;
                        segment.sequence = event.header.sequence;
                        segment.text.clone_from(text);
                        segment.words.clone_from(words);
                        segment.finalised = true;
                    }
                    None => {
                        self.segments.insert(
                            id,
                            ReducedSegment {
                                id,
                                revision: event.header.revision,
                                sequence: event.header.sequence,
                                text: text.clone(),
                                words: words.clone(),
                                finalised: true,
                            },
                        );
                    }
                }
            }
            AsrEventBody::SessionEnd { .. } => {
                if self.session_id != Some(event.header.session_id) {
                    return Err(ReducerError::WrongSession);
                }
            }
            AsrEventBody::Reset { .. } => {
                self.segments.retain(|_, segment| segment.finalised);
            }
            AsrEventBody::Warning { .. } | AsrEventBody::Discontinuity { .. } => {}
        }
        self.last_sequence = Some(event.header.sequence);
        Ok(())
    }

    fn mutable_segment(&mut self, event: &AsrEvent) -> Result<&mut ReducedSegment, ReducerError> {
        let id = segment_id(event)?;
        let segment = self
            .segments
            .get_mut(&id)
            .ok_or(ReducerError::UnknownSegment(id))?;
        if segment.finalised {
            return Err(ReducerError::FinalConflict(id));
        }
        if event.header.revision <= segment.revision {
            return Err(ReducerError::StaleRevision {
                id,
                revision: event.header.revision,
            });
        }
        segment.revision = event.header.revision;
        segment.sequence = event.header.sequence;
        Ok(segment)
    }

    #[must_use]
    pub fn finalised(&self) -> Vec<&ReducedSegment> {
        let mut values = self
            .segments
            .values()
            .filter(|segment| segment.finalised)
            .collect::<Vec<_>>();
        values.sort_by_key(|segment| segment.sequence);
        values
    }
}

fn segment_id(event: &AsrEvent) -> Result<Uuid, ReducerError> {
    event.header.segment_id.ok_or(ReducerError::MissingSegment)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReducerError {
    #[error("unsupported ASR event schema major {0}")]
    UnsupportedMajor(u16),
    #[error("ASR event sequence {0} is stale or conflicting")]
    StaleSequence(u64),
    #[error("ASR session is already started")]
    SessionAlreadyStarted,
    #[error("ASR event belongs to the wrong session")]
    WrongSession,
    #[error("ASR event requires a segment id")]
    MissingSegment,
    #[error("ASR segment {0} already exists")]
    SegmentAlreadyExists(Uuid),
    #[error("ASR segment {0} is unknown")]
    UnknownSegment(Uuid),
    #[error("ASR segment {id} revision {revision} is stale")]
    StaleRevision { id: Uuid, revision: u64 },
    #[error("ASR segment {0} is already final")]
    FinalConflict(Uuid),
}
