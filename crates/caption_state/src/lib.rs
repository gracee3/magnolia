//! Stable/provisional caption state for TUI and native views.

use serde::{Deserialize, Serialize};
use speech_to_text::SttEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptionSegment {
    pub segment_id: u64,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptionState {
    pub committed: Vec<CaptionSegment>,
    pub provisional: Option<CaptionSegment>,
    pub last_sequence: u64,
}

impl CaptionState {
    pub fn apply(&mut self, event: SttEvent) {
        let sequence = match &event {
            SttEvent::Partial { sequence, .. } | SttEvent::Final { sequence, .. } => *sequence,
            _ => return,
        };
        if sequence <= self.last_sequence {
            return;
        }
        self.last_sequence = sequence;
        match event {
            SttEvent::Partial {
                segment_id,
                text,
                audio_end_ms,
                ..
            } => {
                self.provisional = Some(CaptionSegment {
                    segment_id,
                    text,
                    start_ms: 0,
                    end_ms: Some(audio_end_ms),
                });
            }
            SttEvent::Final {
                segment_id,
                text,
                start_ms,
                end_ms,
                ..
            } => {
                self.committed.push(CaptionSegment {
                    segment_id,
                    text,
                    start_ms,
                    end_ms: Some(end_ms),
                });
                self.provisional = None;
            }
            _ => {}
        }
    }

    pub fn display_text(&self) -> String {
        let mut parts: Vec<&str> = self.committed.iter().map(|s| s.text.as_str()).collect();
        if let Some(segment) = &self.provisional {
            parts.push(segment.text.as_str());
        }
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partials_replace_and_final_commits() {
        let mut state = CaptionState::default();
        state.apply(SttEvent::Partial {
            session_id: "s".into(),
            segment_id: 1,
            text: "the planets".into(),
            audio_end_ms: 500,
            sequence: 1,
        });
        state.apply(SttEvent::Partial {
            session_id: "s".into(),
            segment_id: 1,
            text: "the planets are".into(),
            audio_end_ms: 700,
            sequence: 2,
        });
        assert_eq!(state.display_text(), "the planets are");
        state.apply(SttEvent::Final {
            session_id: "s".into(),
            segment_id: 1,
            text: "The planets are".into(),
            start_ms: 0,
            end_ms: 900,
            sequence: 3,
        });
        assert_eq!(state.display_text(), "The planets are");
        assert!(state.provisional.is_none());
    }

    #[test]
    fn stale_partials_are_ignored() {
        let mut state = CaptionState::default();
        state.apply(SttEvent::Partial {
            session_id: "s".into(),
            segment_id: 1,
            text: "new".into(),
            audio_end_ms: 1,
            sequence: 2,
        });
        state.apply(SttEvent::Partial {
            session_id: "s".into(),
            segment_id: 1,
            text: "old".into(),
            audio_end_ms: 1,
            sequence: 1,
        });
        assert_eq!(state.display_text(), "new");
    }
}
