use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames_per_block: u32,
}

impl AudioFormat {
    pub fn new(
        sample_rate: u32,
        channels: u16,
        frames_per_block: u32,
    ) -> Result<Self, AudioFormatError> {
        if sample_rate == 0 {
            return Err(AudioFormatError::InvalidSampleRate);
        }
        if channels == 0 {
            return Err(AudioFormatError::MissingChannels);
        }
        if frames_per_block == 0 {
            return Err(AudioFormatError::EmptyBlock);
        }
        Ok(Self {
            sample_rate,
            channels,
            frames_per_block,
        })
    }

    #[must_use]
    pub fn samples_per_block(self) -> usize {
        self.frames_per_block as usize * self.channels as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockIndex(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discontinuity {
    pub dropped_blocks_before: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BlockProvenance {
    pub source_frame_position: u64,
    pub capture_monotonic_ns: u64,
    pub block_complete_monotonic_ns: u64,
    pub graph_monotonic_ns: u64,
    pub dropped_frames_before: u64,
    pub discontinuity: bool,
}

#[derive(Debug)]
pub struct AudioBlock {
    format: AudioFormat,
    index: BlockIndex,
    valid_frames: u32,
    discontinuity: Option<Discontinuity>,
    provenance: BlockProvenance,
    samples: Box<[f32]>,
}

impl AudioBlock {
    #[must_use]
    pub fn allocated(format: AudioFormat) -> Self {
        Self {
            format,
            index: BlockIndex(0),
            valid_frames: 0,
            discontinuity: None,
            provenance: BlockProvenance::default(),
            samples: vec![0.0; format.samples_per_block()].into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    #[must_use]
    pub fn index(&self) -> BlockIndex {
        self.index
    }

    #[must_use]
    pub fn valid_frames(&self) -> u32 {
        self.valid_frames
    }

    #[must_use]
    pub fn discontinuity(&self) -> Option<Discontinuity> {
        self.discontinuity
    }

    #[must_use]
    pub fn provenance(&self) -> BlockProvenance {
        self.provenance
    }

    #[must_use]
    pub fn samples(&self) -> &[f32] {
        let len = self.valid_frames as usize * self.format.channels as usize;
        &self.samples[..len]
    }

    #[must_use]
    pub fn capacity_mut(&mut self) -> &mut [f32] {
        &mut self.samples
    }

    pub fn commit(
        &mut self,
        index: BlockIndex,
        valid_frames: u32,
        discontinuity: Option<Discontinuity>,
    ) -> Result<(), AudioBlockError> {
        self.commit_with_provenance(
            index,
            valid_frames,
            discontinuity,
            BlockProvenance {
                source_frame_position: index.0.saturating_mul(u64::from(valid_frames)),
                ..BlockProvenance::default()
            },
        )
    }

    pub fn commit_with_provenance(
        &mut self,
        index: BlockIndex,
        valid_frames: u32,
        discontinuity: Option<Discontinuity>,
        provenance: BlockProvenance,
    ) -> Result<(), AudioBlockError> {
        if valid_frames > self.format.frames_per_block {
            return Err(AudioBlockError::TooManyFrames {
                valid_frames,
                capacity: self.format.frames_per_block,
            });
        }
        self.index = index;
        self.valid_frames = valid_frames;
        self.discontinuity = discontinuity;
        self.provenance = provenance;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AudioFormatError {
    #[error("audio sample rate must be non-zero")]
    InvalidSampleRate,
    #[error("audio channel count must be non-zero")]
    MissingChannels,
    #[error("audio block size must be non-zero")]
    EmptyBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AudioBlockError {
    #[error("audio block has {valid_frames} frames but capacity is {capacity}")]
    TooManyFrames { valid_frames: u32, capacity: u32 },
}
