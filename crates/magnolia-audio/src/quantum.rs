use thiserror::Error;

pub const MAGNOLIA_BLOCK_FRAMES: usize = 256;
pub const MAX_PIPEWIRE_QUANTUM_FRAMES: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscontinuityReason {
    Loss,
    Restart,
    Overflow,
    Recovery,
    Renegotiation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuantumBlockMeta {
    pub sequence: u64,
    pub source_frame_position: u64,
    pub monotonic_ns: u64,
    pub dropped_frames_before: u64,
    pub discontinuity: Option<DiscontinuityReason>,
}

pub struct QuantumAdapter {
    channels: usize,
    sample_rate: u32,
    scratch: Box<[f32]>,
    buffered_frames: usize,
    next_sequence: u64,
    next_source_frame: u64,
    next_monotonic_ns: u64,
    pending_dropped_frames: u64,
    pending_discontinuity: Option<DiscontinuityReason>,
}

impl QuantumAdapter {
    pub fn new(channels: usize, sample_rate: u32) -> Result<Self, QuantumError> {
        if !(1..=2).contains(&channels) {
            return Err(QuantumError::UnsupportedChannels(channels));
        }
        if sample_rate == 0 {
            return Err(QuantumError::ZeroRate);
        }
        Ok(Self {
            channels,
            sample_rate,
            scratch: vec![0.0; MAX_PIPEWIRE_QUANTUM_FRAMES * channels].into_boxed_slice(),
            buffered_frames: 0,
            next_sequence: 0,
            next_source_frame: 0,
            next_monotonic_ns: 0,
            pending_dropped_frames: 0,
            pending_discontinuity: None,
        })
    }

    pub fn mark_discontinuity(&mut self, reason: DiscontinuityReason, dropped_frames: u64) {
        self.pending_discontinuity = Some(reason);
        self.pending_dropped_frames = self.pending_dropped_frames.saturating_add(dropped_frames);
    }

    pub fn push(
        &mut self,
        samples: &[f32],
        source_frame_position: u64,
        monotonic_ns: u64,
        mut emit: impl FnMut(&[f32], QuantumBlockMeta),
    ) -> Result<usize, QuantumError> {
        if !samples.len().is_multiple_of(self.channels) {
            return Err(QuantumError::MisalignedSamples);
        }
        let frames = samples.len() / self.channels;
        if frames > MAX_PIPEWIRE_QUANTUM_FRAMES {
            return Err(QuantumError::QuantumTooLarge(frames));
        }
        if self.buffered_frames + frames > MAX_PIPEWIRE_QUANTUM_FRAMES {
            return Err(QuantumError::AdapterOverflow);
        }
        if self.buffered_frames == 0 {
            self.next_source_frame = source_frame_position;
            self.next_monotonic_ns = monotonic_ns;
        }
        let start = self.buffered_frames * self.channels;
        let end = start + samples.len();
        self.scratch[start..end].copy_from_slice(samples);
        self.buffered_frames += frames;

        let mut emitted = 0;
        while self.buffered_frames >= MAGNOLIA_BLOCK_FRAMES {
            let sample_count = MAGNOLIA_BLOCK_FRAMES * self.channels;
            emit(
                &self.scratch[..sample_count],
                QuantumBlockMeta {
                    sequence: self.next_sequence,
                    source_frame_position: self.next_source_frame,
                    monotonic_ns: self.next_monotonic_ns,
                    dropped_frames_before: self.pending_dropped_frames,
                    discontinuity: self.pending_discontinuity.take(),
                },
            );
            self.pending_dropped_frames = 0;
            self.next_sequence = self.next_sequence.saturating_add(1);
            self.next_source_frame = self
                .next_source_frame
                .saturating_add(MAGNOLIA_BLOCK_FRAMES as u64);
            self.next_monotonic_ns = self.next_monotonic_ns.saturating_add(
                (MAGNOLIA_BLOCK_FRAMES as u64 * 1_000_000_000) / u64::from(self.sample_rate),
            );
            self.buffered_frames -= MAGNOLIA_BLOCK_FRAMES;
            let remaining = self.buffered_frames * self.channels;
            self.scratch
                .copy_within(sample_count..sample_count + remaining, 0);
            emitted += 1;
        }
        Ok(emitted)
    }

    #[must_use]
    pub fn buffered_frames(&self) -> usize {
        self.buffered_frames
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum QuantumError {
    #[error("only mono and stereo PipeWire layouts are supported; received {0} channels")]
    UnsupportedChannels(usize),
    #[error("sample rate must be non-zero")]
    ZeroRate,
    #[error("PipeWire buffer does not contain complete interleaved frames")]
    MisalignedSamples,
    #[error("PipeWire quantum {0} exceeds the 8192-frame limit")]
    QuantumTooLarge(usize),
    #[error("quantum adapter capacity would be exceeded")]
    AdapterOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_combines_variable_quanta_into_fixed_blocks() {
        let mut adapter = QuantumAdapter::new(2, 48_000).unwrap();
        let first = vec![0.25; 128 * 2];
        assert_eq!(adapter.push(&first, 1_000, 2_000, |_, _| {}).unwrap(), 0);
        let second = vec![0.5; 640 * 2];
        let mut blocks = Vec::new();
        assert_eq!(
            adapter
                .push(&second, 1_128, 4_666_666, |samples, meta| {
                    blocks.push((samples[0], samples[256], meta));
                })
                .unwrap(),
            3
        );
        assert_eq!(blocks[0].0, 0.25);
        assert_eq!(blocks[0].1, 0.5);
        assert_eq!(blocks[0].2.source_frame_position, 1_000);
        assert_eq!(blocks[2].2.sequence, 2);
        assert_eq!(adapter.buffered_frames(), 0);
    }

    #[test]
    fn discontinuity_is_emitted_once_at_the_next_block_boundary() {
        let mut adapter = QuantumAdapter::new(1, 48_000).unwrap();
        adapter.mark_discontinuity(DiscontinuityReason::Recovery, 512);
        let samples = [0.0; MAGNOLIA_BLOCK_FRAMES];
        let mut meta = None;
        adapter
            .push(&samples, 90, 100, |_, value| meta = Some(value))
            .unwrap();
        let meta = meta.unwrap();
        assert_eq!(meta.discontinuity, Some(DiscontinuityReason::Recovery));
        assert_eq!(meta.dropped_frames_before, 512);
    }
}
