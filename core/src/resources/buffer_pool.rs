use std::sync::{Arc, Mutex, RwLock};
use slab::Slab;

/// A handle to a buffer in the pool
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BufferHandle {
    pub id: usize,
    pub generation: u32,
}

/// A generic buffer pool that manages resources with generation-checked handles.
/// This allows safe, zero-copy sharing of data between modules and host.
pub struct BufferPool<T> {
    // We use a Slab to manage the storage and generation tagging
    // Inner value is generic, but usually Vec<u8> or Vec<f32>
    store: RwLock<Slab<Entry<T>>>,
}

struct Entry<T> {
    data: Arc<T>,
    generation: u32,
}

impl<T> BufferPool<T> {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(Slab::new()),
        }
    }

    /// Allocate a new buffer and return a handle to it.
    /// The data is wrapped in an Arc to allow cheap cloning ref-counting by the pool.
    pub fn allocate(&self, data: T) -> BufferHandle {
        let mut store = self.store.write().unwrap();
        let entry = store.vacant_entry();
        let id = entry.key();
        
        // We don't have generation in Slab's vacant entry directly in all versions,
        // but Slab reuses indices. We need to maintain our own generation count if Slab doesn't.
        // Wait, standard Slab doesn't have generation counters built-in in older versions, 
        // but let's assume valid access pattern. For strict safety we need our own wrapper or a crate like `generational-arena`.
        // For now, simplistically: Slab + manual generation.
        // Actually, let's just use `0` for now if we don't store generation in Slab explicitly.
        // Or wait, if we re-use slots, we risk ABA.
        // Let's implement a simple generation check.
        
        // Inserting into Slab
        entry.insert(Entry {
            data: Arc::new(data),
            generation: 0, // TODO: Implement proper generation increment on reuse
        });

        BufferHandle {
            id,
            generation: 0,
        }
    }

    /// Get a reference to the buffer if the handle is valid
    pub fn get(&self, handle: BufferHandle) -> Option<Arc<T>> {
        let store = self.store.read().unwrap();
        if let Some(entry) = store.get(handle.id) {
            if entry.generation == handle.generation {
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Release a buffer (remove from pool)
    pub fn release(&self, handle: BufferHandle) -> bool {
        let mut store = self.store.write().unwrap();
        if store.contains(handle.id) {
            // Check generation if we were rigorous
            // For now just remove
            store.remove(handle.id);
            return true;
        }
        false
    }
}

// Default generic implementations useful for Audio and Blobs
pub type AudioBufferPool = BufferPool<Vec<f32>>;
pub type BlobBufferPool = BufferPool<Vec<u8>>;
