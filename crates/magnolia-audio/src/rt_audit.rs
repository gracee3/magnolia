use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

thread_local! {
    static CALLBACK_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);

pub struct CallbackScope;

impl CallbackScope {
    #[must_use]
    pub fn enter() -> Self {
        CALLBACK_ACTIVE.with(|active| active.set(true));
        Self
    }
}

impl Drop for CallbackScope {
    fn drop(&mut self) {
        CALLBACK_ACTIVE.with(|active| active.set(false));
    }
}

pub struct CallbackCountingAllocator;

const TIMING_BUCKET_NS: u64 = 10_000;
const TIMING_BUCKETS: usize = 256;

#[derive(Debug)]
pub struct CallbackTiming {
    callbacks: AtomicU64,
    maximum_ns: AtomicU64,
    buckets: [AtomicU64; TIMING_BUCKETS],
}

impl Default for CallbackTiming {
    fn default() -> Self {
        Self {
            callbacks: AtomicU64::new(0),
            maximum_ns: AtomicU64::new(0),
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl CallbackTiming {
    #[must_use]
    pub fn start(&self) -> CallbackTimer<'_> {
        CallbackTimer {
            timing: self,
            started: Instant::now(),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> CallbackTimingSnapshot {
        CallbackTimingSnapshot {
            callbacks: self.callbacks.load(Ordering::Relaxed),
            maximum_ns: self.maximum_ns.load(Ordering::Relaxed),
            p99_ns: self.percentile_ns(99, 100),
            p999_ns: self.percentile_ns(999, 1_000),
        }
    }

    fn percentile_ns(&self, numerator: u64, denominator: u64) -> u64 {
        let callbacks = self.callbacks.load(Ordering::Relaxed);
        if callbacks == 0 {
            return 0;
        }
        let target = callbacks
            .saturating_mul(numerator)
            .saturating_add(denominator - 1)
            / denominator;
        let mut cumulative = 0_u64;
        for (index, bucket) in self.buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(bucket.load(Ordering::Relaxed));
            if cumulative >= target {
                return (index as u64 + 1).saturating_mul(TIMING_BUCKET_NS);
            }
        }
        self.maximum_ns.load(Ordering::Relaxed)
    }
}

pub struct CallbackTimer<'a> {
    timing: &'a CallbackTiming,
    started: Instant,
}

impl Drop for CallbackTimer<'_> {
    fn drop(&mut self) {
        let elapsed_ns = self.started.elapsed().as_nanos() as u64;
        self.timing.callbacks.fetch_add(1, Ordering::Relaxed);
        self.timing
            .maximum_ns
            .fetch_max(elapsed_ns, Ordering::Relaxed);
        let bucket = (elapsed_ns / TIMING_BUCKET_NS).min(TIMING_BUCKETS as u64 - 1) as usize;
        self.timing.buckets[bucket].fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackTimingSnapshot {
    pub callbacks: u64,
    pub maximum_ns: u64,
    pub p99_ns: u64,
    pub p999_ns: u64,
}

unsafe impl GlobalAlloc for CallbackCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if callback_active() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if callback_active() {
            DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(pointer, layout) };
    }
}

fn callback_active() -> bool {
    CALLBACK_ACTIVE.with(Cell::get)
}

pub fn reset_callback_allocation_counts() {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    DEALLOCATIONS.store(0, Ordering::Relaxed);
}

#[must_use]
pub fn callback_allocation_counts() -> (u64, u64) {
    (
        ALLOCATIONS.load(Ordering::Relaxed),
        DEALLOCATIONS.load(Ordering::Relaxed),
    )
}
