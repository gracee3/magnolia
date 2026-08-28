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
}

impl EdgeCounters {
    #[must_use]
    pub fn snapshot(&self) -> EdgeSnapshot {
        EdgeSnapshot {
            published: self.published.load(Ordering::Relaxed),
            consumed: self.consumed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            high_water: self.high_water.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeSnapshot {
    pub published: u64,
    pub consumed: u64,
    pub dropped: u64,
    pub high_water: u64,
}

pub struct BlockProducer {
    free: Consumer<AudioBlock>,
    ready: Producer<AudioBlock>,
    counters: Arc<EdgeCounters>,
    pending_drops: u64,
}

pub struct BlockConsumer {
    ready: Consumer<AudioBlock>,
    free: Producer<AudioBlock>,
    counters: Arc<EdgeCounters>,
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
        },
        BlockConsumer {
            ready: ready_rx,
            free: free_tx,
            counters: counters.clone(),
        },
        counters,
    )
}

impl BlockProducer {
    pub fn publish<F>(&mut self, index: BlockIndex, valid_frames: u32, fill: F) -> bool
    where
        F: FnOnce(&mut [f32]),
    {
        let Ok(mut block) = self.free.pop() else {
            self.pending_drops = self.pending_drops.saturating_add(1);
            self.counters.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        };
        fill(block.capacity_mut());
        let discontinuity = (self.pending_drops > 0).then_some(Discontinuity {
            dropped_blocks_before: self.pending_drops,
        });
        self.pending_drops = 0;
        block
            .commit(index, valid_frames, discontinuity)
            .expect("producer must honor the prepared block size");
        self.ready
            .push(block)
            .expect("a returned free block guarantees ready capacity");
        let published = self.counters.published.fetch_add(1, Ordering::Relaxed) + 1;
        let consumed = self.counters.consumed.load(Ordering::Relaxed);
        let depth = published.saturating_sub(consumed);
        self.counters.high_water.fetch_max(depth, Ordering::Relaxed);
        true
    }
}

impl BlockConsumer {
    pub fn consume<F, R>(&mut self, consume: F) -> Option<R>
    where
        F: FnOnce(&AudioBlock) -> R,
    {
        let Ok(block) = self.ready.pop() else {
            return None;
        };
        let result = consume(&block);
        self.free
            .push(block)
            .expect("consuming one ready block guarantees free capacity");
        self.counters.consumed.fetch_add(1, Ordering::Relaxed);
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_is_counted_and_marks_the_next_available_block() {
        let format = AudioFormat::new(48_000, 2, 4).unwrap();
        let (mut producer, mut consumer, counters) = block_channel(format, 1);
        assert!(producer.publish(BlockIndex(0), 4, |samples| samples.fill(0.5)));
        assert!(!producer.publish(BlockIndex(1), 4, |_| {}));
        assert_eq!(consumer.consume(|block| block.index()), Some(BlockIndex(0)));
        assert!(producer.publish(BlockIndex(2), 4, |_| {}));
        assert_eq!(
            consumer.consume(|block| block.discontinuity()),
            Some(Some(Discontinuity {
                dropped_blocks_before: 1
            }))
        );
        assert_eq!(counters.snapshot().dropped, 1);
        assert_eq!(counters.snapshot().high_water, 1);
    }
}
