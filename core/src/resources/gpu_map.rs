use std::sync::RwLock;
use slab::Slab;
use std::sync::Arc;

/// A generic map for GPU resources managed by the host.
/// Maps opaque integer handles to actual wgpu definitions.
pub struct GpuResourceMap<T> {
    store: RwLock<Slab<Entry<T>>>,
}

struct Entry<T> {
    resource: T,
    generation: u32,
}

impl<T> GpuResourceMap<T> {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(Slab::new()),
        }
    }

    /// Insert a resource and return its ID and generation
    pub fn insert(&self, resource: T) -> (u64, u32) {
        let mut store = self.store.write().unwrap();
        let entry = store.vacant_entry();
        let id = entry.key();
        
        let generation = 0; // TODO: Implement proper generation tracking
        
        entry.insert(Entry {
            resource,
            generation,
        });

        (id as u64, generation)
    }

    /// Get a reference to the resource if valid
    /// Since wgpu resources are internal Arcs (Clone is handling ref count), 
    /// we can return T if T is Clone, or we need to return a reference.
    /// wgpu::Texture is NOT Clone in 0.17? It is not. wgpu::Texture is a Handle.
    /// Wait, wgpu 0.17 Texture is not Clone? `wgpu::Texture` is a struct wrapping an ID and a Context.
    /// It's usually RefCounted internally but the struct itself might not be Clone if it holds a unique reference?
    /// Actually, wgpu resources usually are NOT Clone to prevent accidental keeping alive?
    /// No, usually they are `Arc` internally strictly speaking, but the API exposes them as moved types.
    ///
    /// If T is not Clone, we can only access it via callback or reference.
    /// Let's use a callback pattern or `Arc<T>` if we own it.
    /// wrapping T in Arc<T> is safest for shared ownership if T is not Clone.
    ///
    /// But wait, we want the Host to OWN it. The Module just gets a Handle.
    /// When the module wants to USE it, it asks the Host (or the Compositor uses it).
    /// The Compositor acts as the Host-side consumer.
    /// So `get` is called by the Compositor.
    
    pub fn get_with<F, R>(&self, id: u64, generation: u32, f: F) -> Option<R>
    where F: FnOnce(&T) -> R 
    {
        let store = self.store.read().unwrap();
        let idx = id as usize;
        if let Some(entry) = store.get(idx) {
            if entry.generation == generation {
                return Some(f(&entry.resource));
            }
        }
        None
    }
    
    /// Remove resource
    pub fn remove(&self, id: u64) -> Option<T> {
        let mut store = self.store.write().unwrap();
        let idx = id as usize;
        if store.contains(idx) {
             let entry = store.remove(idx);
             return Some(entry.resource);
        }
        None
    }
}

// wgpu resources need to be wrapped or we rely on them being Send/Sync (which they are).
pub type GpuTextureMap = GpuResourceMap<wgpu::Texture>;
pub type GpuBufferMap = GpuResourceMap<wgpu::Buffer>;
pub type GpuTextureViewMap = GpuResourceMap<wgpu::TextureView>;
