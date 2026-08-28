use crate::{AudioBlock, AudioFormat, BlockIndex, Discontinuity};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Debug, Default)]
pub struct EdgeCounters {
    published: AtomicU64,
    consumed: AtomicU64,
    dropped: AtomicU64,
    high_water: AtomicU64,
    faults: AtomicU64,
}

impl EdgeCounters {
    #[must_use]
    pub fn snapshot(&self) -> EdgeSnapshot {
        EdgeSnapshot {
            published: self.published.load(Ordering::Relaxed),
            consumed: self.consumed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            high_water: self.high_water.load(Ordering::Relaxed),
            faults: self.faults.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeSnapshot {
    pub published: u64,
    pub consumed: u64,
    pub dropped: u64,
    pub high_water: u64,
    pub faults: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    Published,
    DroppedNoFreeBlock,
    InvalidFrameCount,
    RingFault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumeOutcome<R> {
    Consumed(R),
    Empty,
    RingFault,
}

pub struct BlockProducer {
    free: Consumer<AudioBlock>,
    ready: Producer<AudioBlock>,
    counters: Arc<EdgeCounters>,
    pending_drops: u64,
    held: Option<AudioBlock>,
    held_ready: bool,
}

pub struct BlockConsumer {
    ready: Consumer<AudioBlock>,
    free: Producer<AudioBlock>,
    counters: Arc<EdgeCounters>,
    held: Option<AudioBlock>,
}

#[must_use]
pub fn block_channel(
    format: AudioFormat,
    capacity: usize,
) -> (BlockProducer, BlockConsumer, Arc<EdgeCounters>) {
    assert!(capacity > 0, "block channel capacity must be non-zero");
    let (mut free_tx, free_rx) = RingBuffer::new(capacity);
    for _ in 0..capacity {
        free_tx
            .push(AudioBlock::allocated(format))
            .expect("free ring was created with exact capacity");
    }
    let (ready_tx, ready_rx) = RingBuffer::new(capacity);
    let counters = Arc::new(EdgeCounters::default());
    (
        BlockProducer {
            free: free_rx,
            ready: ready_tx,
            counters: counters.clone(),
            pending_drops: 0,
            held: None,
            held_ready: false,
        },
        BlockConsumer {
            ready: ready_rx,
            free: free_tx,
            counters: counters.clone(),
            held: None,
        },
        counters,
    )
}

impl BlockProducer {
    pub fn publish<F>(&mut self, index: BlockIndex, valid_frames: u32, fill: F) -> PublishOutcome
    where
        F: FnOnce(&mut [f32]),
    {
        if self.held_ready {
            if let Some(block) = self.held.take() {
                if let Err(rtrb::PushError::Full(block)) = self.ready.push(block) {
                    self.held = Some(block);
                    self.counters.faults.fetch_add(1, Ordering::Relaxed);
                    return PublishOutcome::RingFault;
                }
                self.held_ready = false;
                self.record_published();
            }
        }
        let mut block = if let Some(block) = self.held.take() {
            block
        } else if let Ok(block) = self.free.pop() {
            block
        } else {
            self.pending_drops = self.pending_drops.saturating_add(1);
            self.counters.dropped.fetch_add(1, Ordering::Relaxed);
            return PublishOutcome::DroppedNoFreeBlock;
        };
        if valid_frames > block.format().frames_per_block {
            self.held = Some(block);
            self.held_ready = false;
            self.counters.faults.fetch_add(1, Ordering::Relaxed);
            return PublishOutcome::InvalidFrameCount;
        }
        fill(block.capacity_mut());
        let discontinuity = (self.pending_drops > 0).then_some(Discontinuity {
            dropped_blocks_before: self.pending_drops,
        });
        self.pending_drops = 0;
        if block.commit(index, valid_frames, discontinuity).is_err() {
            self.held = Some(block);
            self.held_ready = false;
            self.counters.faults.fetch_add(1, Ordering::Relaxed);
            return PublishOutcome::RingFault;
        }
        if let Err(rtrb::PushError::Full(block)) = self.ready.push(block) {
            self.held = Some(block);
            self.held_ready = true;
            self.counters.faults.fetch_add(1, Ordering::Relaxed);
            return PublishOutcome::RingFault;
        }
        self.record_published();
        PublishOutcome::Published
    }

    fn record_published(&self) {
        let published = self.counters.published.fetch_add(1, Ordering::Relaxed) + 1;
        let consumed = self.counters.consumed.load(Ordering::Relaxed);
        let depth = published.saturating_sub(consumed);
        self.counters.high_water.fetch_max(depth, Ordering::Relaxed);
    }
}

impl BlockConsumer {
    pub fn consume<F, R>(&mut self, consume: F) -> ConsumeOutcome<R>
    where
        F: FnOnce(&AudioBlock) -> R,
    {
        if let Some(block) = self.held.take() {
            if let Err(rtrb::PushError::Full(block)) = self.free.push(block) {
                self.held = Some(block);
                self.counters.faults.fetch_add(1, Ordering::Relaxed);
                return ConsumeOutcome::RingFault;
            }
        }
        let Ok(block) = self.ready.pop() else {
            return ConsumeOutcome::Empty;
        };
        let result = consume(&block);
        if let Err(rtrb::PushError::Full(block)) = self.free.push(block) {
            self.held = Some(block);
            self.counters.faults.fetch_add(1, Ordering::Relaxed);
            return ConsumeOutcome::RingFault;
        }
        self.counters.consumed.fetch_add(1, Ordering::Relaxed);
        ConsumeOutcome::Consumed(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_is_counted_and_marks_the_next_available_block() {
        let format = AudioFormat::new(48_000, 2, 4).unwrap();
        let (mut producer, mut consumer, counters) = block_channel(format, 1);
        assert_eq!(
            producer.publish(BlockIndex(0), 4, |samples| samples.fill(0.5)),
            PublishOutcome::Published
        );
        assert_eq!(
            producer.publish(BlockIndex(1), 4, |_| {}),
            PublishOutcome::DroppedNoFreeBlock
        );
        assert_eq!(
            consumer.consume(|block| block.index()),
            ConsumeOutcome::Consumed(BlockIndex(0))
        );
        assert_eq!(
            producer.publish(BlockIndex(2), 4, |_| {}),
            PublishOutcome::Published
        );
        assert_eq!(
            consumer.consume(|block| block.discontinuity()),
            ConsumeOutcome::Consumed(Some(Discontinuity {
                dropped_blocks_before: 1
            }))
        );
        assert_eq!(counters.snapshot().dropped, 1);
        assert_eq!(counters.snapshot().high_water, 1);
    }

    #[test]
    fn invalid_frame_count_is_bounded_and_pool_remains_usable() {
        let format = AudioFormat::new(48_000, 2, 4).unwrap();
        let (mut producer, mut consumer, counters) = block_channel(format, 1);
        assert_eq!(
            producer.publish(BlockIndex(0), 5, |_| panic!("fill must not run")),
            PublishOutcome::InvalidFrameCount
        );
        assert_eq!(
            producer.publish(BlockIndex(1), 4, |_| {}),
            PublishOutcome::Published
        );
        assert!(matches!(
            consumer.consume(|block| block.index()),
            ConsumeOutcome::Consumed(BlockIndex(1))
        ));
        assert_eq!(counters.snapshot().faults, 1);
    }

    #[test]
    fn impossible_ready_ring_state_reports_fault_without_panicking() {
        let format = AudioFormat::new(48_000, 1, 4).unwrap();
        let (mut producer, _consumer, counters) = block_channel(format, 1);
        producer.ready.push(AudioBlock::allocated(format)).unwrap();
        assert_eq!(
            producer.publish(BlockIndex(0), 4, |_| {}),
            PublishOutcome::RingFault
        );
        assert_eq!(counters.snapshot().faults, 1);
        assert!(producer.held.is_some());
    }

    #[test]
    fn impossible_free_ring_state_rotates_held_block_without_panicking() {
        let format = AudioFormat::new(48_000, 1, 4).unwrap();
        let (mut producer, mut consumer, counters) = block_channel(format, 1);
        assert_eq!(
            producer.publish(BlockIndex(0), 4, |_| {}),
            PublishOutcome::Published
        );
        consumer.free.push(AudioBlock::allocated(format)).unwrap();

        assert_eq!(
            consumer.consume(|block| block.index()),
            ConsumeOutcome::RingFault
        );
        assert!(consumer.held.is_some());
        assert_eq!(counters.snapshot().faults, 1);

        assert_eq!(
            producer.publish(BlockIndex(1), 4, |_| {}),
            PublishOutcome::Published
        );
        assert_eq!(
            consumer.consume(|block| block.index()),
            ConsumeOutcome::RingFault
        );
        assert_eq!(
            consumer.held.as_ref().map(AudioBlock::index),
            Some(BlockIndex(1))
        );
        assert_eq!(counters.snapshot().faults, 2);
    }
}
