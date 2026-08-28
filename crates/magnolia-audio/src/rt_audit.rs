use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
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
