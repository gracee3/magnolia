use crate::{ModuleSchema, Signal};
use async_trait::async_trait;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tokio::sync::mpsc;

/// Execution model for a module - determines how it runs
#[derive(Debug, Clone)]
pub enum ExecutionModel {
    /// Runs on tokio async runtime (lightweight, async IO)
    Async,
    /// Dedicated OS thread (heavy processing, blocking work)
    DedicatedThread,
    /// Thread pool for CPU-bound work (future: rayon integration)
    ThreadPool { threads: usize },
}

/// Priority level for module scheduling
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
    RealTime,
}

/// Runtime trait for modules that can be spawned and managed
/// This is the key abstraction for isolated module execution
#[async_trait]
pub trait ModuleRuntime: Send + Sync {
    /// Unique identifier for this module
    fn id(&self) -> &str;

    /// Human-readable name
    fn name(&self) -> &str;

    /// Schema describing ports and capabilities
    fn schema(&self) -> ModuleSchema;

    /// Execution model preference
    fn execution_model(&self) -> ExecutionModel {
        ExecutionModel::Async
    }

    /// Priority level
    fn priority(&self) -> Priority {
        Priority::Normal
    }

    /// Whether this module is currently enabled
    fn is_enabled(&self) -> bool;

    /// Enable or disable this module
    fn set_enabled(&mut self, enabled: bool);

    /// Run the module's main loop (async)
    /// This will be called in a separate thread/task with a tokio runtime
    async fn run(&mut self, inbox: mpsc::Receiver<Signal>, outbox: mpsc::Sender<RoutedSignal>);
}

/// Envelope for router-bound signals with source attribution
#[derive(Debug, Clone)]
pub struct RoutedSignal {
    pub source_id: String,
    pub signal: Signal,
}

/// Handle to a running module instance
pub struct ModuleHandle {
    pub id: String,
    pub thread: Option<JoinHandle<()>>,
    pub inbox: mpsc::Sender<Signal>,
    _shutdown_tx: mpsc::Sender<()>,
}

impl ModuleHandle {
    /// Send a signal to this module
    pub async fn send(&self, signal: Signal) -> Result<(), mpsc::error::SendError<Signal>> {
        self.inbox.send(signal).await
    }

    /// Try to send a signal without blocking
    pub fn try_send(&self, signal: Signal) -> Result<(), mpsc::error::TrySendError<Signal>> {
        self.inbox.try_send(signal)
    }

    /// Request shutdown of this module
    pub fn shutdown(&self) {
        let _ = self._shutdown_tx.try_send(());
    }
}

use crate::resources::buffer_pool::{AudioBufferPool, BlobBufferPool};
use crate::resources::gpu_map::{GpuBufferMap, GpuTextureMap, GpuTextureViewMap};

/// Manages the lifecycle of all module runtimes
pub struct ModuleHost {
    modules: HashMap<String, ModuleHandle>,
    router_tx: mpsc::Sender<RoutedSignal>,
    pub audio_pool: Arc<AudioBufferPool>,
    pub blob_pool: Arc<BlobBufferPool>,
    pub texture_map: Arc<GpuTextureMap>,
    pub buffer_map: Arc<GpuBufferMap>,
    pub view_map: Arc<GpuTextureViewMap>,
}

impl ModuleHost {
    /// Create a new module host
    pub fn new(router_tx: mpsc::Sender<RoutedSignal>) -> Self {
        Self {
            modules: HashMap::new(),
            router_tx,
            audio_pool: Arc::new(AudioBufferPool::new()),
            blob_pool: Arc::new(BlobBufferPool::new()),
            texture_map: Arc::new(GpuTextureMap::new()),
            buffer_map: Arc::new(GpuBufferMap::new()),
            view_map: Arc::new(GpuTextureViewMap::new()),
        }
    }

    /// Spawn a module in its own isolated thread with panic catching
    pub fn spawn<M>(&mut self, mut module: M, buffer_size: usize) -> Result<(), String>
    where
        M: ModuleRuntime + 'static,
    {
        let module_id = module.id().to_string();
        let module_name = module.name().to_string();

        if self.modules.contains_key(&module_id) {
            return Err(format!("Module {} already spawned", module_id));
        }

        // Create channels for this module
        let (inbox_tx, inbox_rx) = mpsc::channel::<Signal>(buffer_size);
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let outbox = self.router_tx.clone();

        // Spawn based on execution model
        let handle = match module.execution_model() {
            ExecutionModel::Async => {
                // Spawn on tokio runtime in a new thread to isolate panics
                let module_name_clone = module_name.clone();
                thread::spawn(move || {
                    let rt =
                        tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

                    let result = catch_unwind(AssertUnwindSafe(|| {
                        rt.block_on(async move {
                            tokio::select! {
                                _ = shutdown_rx.recv() => {
                                    log::info!("Module {} received shutdown signal", module_name_clone);
                                }
                            _ = module.run(inbox_rx, outbox) => {
                                log::info!("Module {} exited normally", module_name_clone);
                            }
                        }
                    });
                    }));

                    match result {
                        Ok(_) => log::info!("Module {} thread exited cleanly", module_name),
                        Err(e) => {
                            log::error!("Module {} panicked: {:?}", module_name, e);
                            // TODO: Auto-restart logic could go here
                        }
                    }
                })
            }
            ExecutionModel::DedicatedThread => {
                // Direct OS thread with its own runtime
                thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        rt.block_on(async {
                            module.run(inbox_rx, outbox).await;
                        });
                    }));

                    match result {
                        Ok(_) => log::info!("Module {} exited normally", module_name),
                        Err(e) => {
                            log::error!("Module {} panicked: {:?}", module_name, e);
                        }
                    }
                })
            }
            ExecutionModel::ThreadPool { .. } => {
                // For now, treat as dedicated thread
                // TODO: Implement rayon integration
                thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        rt.block_on(async {
                            module.run(inbox_rx, outbox).await;
                        });
                    }));

                    match result {
                        Ok(_) => log::info!("Module {} exited normally", module_name),
                        Err(e) => {
                            log::error!("Module {} panicked: {:?}", module_name, e);
                        }
                    }
                })
            }
        };

        let module_handle = ModuleHandle {
            id: module_id.clone(),
            thread: Some(handle),
            inbox: inbox_tx,
            _shutdown_tx: shutdown_tx,
        };

        self.modules.insert(module_id, module_handle);
        Ok(())
    }

    /// Get a handle to a module by ID
    pub fn get_module(&self, module_id: &str) -> Option<&ModuleHandle> {
        self.modules.get(module_id)
    }

    /// Get mutable reference to a module handle
    pub fn get_module_mut(&mut self, module_id: &str) -> Option<&mut ModuleHandle> {
        self.modules.get_mut(module_id)
    }

    /// List all module IDs
    pub fn list_modules(&self) -> Vec<&str> {
        self.modules.keys().map(|s| s.as_str()).collect()
    }

    /// Shutdown a specific module
    pub fn shutdown_module(&mut self, module_id: &str) -> Result<(), String> {
        if let Some(mut handle) = self.modules.remove(module_id) {
            handle.shutdown();
            if let Some(thread) = handle.thread.take() {
                let _ = thread.join();
            }
            Ok(())
        } else {
            Err(format!("Module {} not found", module_id))
        }
    }

    /// Shutdown all modules and wait for them to finish
    pub fn shutdown_all(&mut self) {
        log::info!("Shutting down {} modules", self.modules.len());

        // Send shutdown signals
        for (id, handle) in &self.modules {
            log::debug!("Sending shutdown to {}", id);
            handle.shutdown();
        }

        // Wait for all threads to finish
        for (id, mut handle) in self.modules.drain() {
            if let Some(thread) = handle.thread.take() {
                log::debug!("Waiting for {} to finish", id);
                let _ = thread.join();
            }
        }

        log::info!("All modules shut down");
    }
    /// Send a signal to a specific module (non-blocking)
    pub fn send_signal(&self, module_id: &str, signal: Signal) -> Result<(), String> {
        if let Some(handle) = self.modules.get(module_id) {
            handle.try_send(signal).map_err(|e| e.to_string())
        } else {
            Err(format!("Module {} not found", module_id))
        }
    }

    /// Get a direct sender to a module's inbox (for UI/Tiles)
    pub fn get_sender(&self, module_id: &str) -> Option<mpsc::Sender<Signal>> {
        self.modules.get(module_id).map(|h| h.inbox.clone())
    }
}

impl Drop for ModuleHost {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    struct TestModule {
        id: String,
        enabled: bool,
        ran: Arc<AtomicBool>,
    }

    impl TestModule {
        fn new(id: &str) -> (Self, Arc<AtomicBool>) {
            let ran = Arc::new(AtomicBool::new(false));
            (
                Self {
                    id: id.to_string(),
                    enabled: true,
                    ran: ran.clone(),
                },
                ran,
            )
        }
    }

    #[async_trait]
    impl ModuleRuntime for TestModule {
        fn id(&self) -> &str {
            &self.id
        }
        fn name(&self) -> &str {
            &self.id
        }
        fn schema(&self) -> ModuleSchema {
            ModuleSchema {
                id: self.id.clone(),
                name: self.id.clone(),
                description: "Test module".to_string(),
                ports: vec![],
                settings_schema: None,
            }
        }
        fn is_enabled(&self) -> bool {
            self.enabled
        }
        fn set_enabled(&mut self, enabled: bool) {
            self.enabled = enabled;
        }

        async fn run(
            &mut self,
            mut inbox: mpsc::Receiver<Signal>,
            _outbox: mpsc::Sender<RoutedSignal>,
        ) {
            self.ran.store(true, Ordering::SeqCst);
            // Simple echo loop
            while let Some(_signal) = inbox.recv().await {
                // Process signals
            }
        }
    }

    #[test]
    fn test_module_spawn() {
        let (router_tx, _router_rx) = mpsc::channel(10);
        let mut host = ModuleHost::new(router_tx);

        let (module, ran_flag) = TestModule::new("test_module");
        host.spawn(module, 10).unwrap();

        // Give it time to start
        thread::sleep(Duration::from_millis(100));

        assert!(ran_flag.load(Ordering::SeqCst), "Module should have run");
        assert!(host.get_module("test_module").is_some());
    }

    #[test]
    fn test_module_shutdown() {
        let (router_tx, _router_rx) = mpsc::channel(10);
        let mut host = ModuleHost::new(router_tx);

        let (module, _) = TestModule::new("test_module");
        host.spawn(module, 10).unwrap();

        thread::sleep(Duration::from_millis(50));
        host.shutdown_module("test_module").unwrap();

        assert!(host.get_module("test_module").is_none());
    }
}
