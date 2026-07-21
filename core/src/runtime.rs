use crate::{ModuleSchema, Signal};
use async_trait::async_trait;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
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

/// Lifecycle state for a supervised module instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

impl ModuleState {
    fn as_u8(self) -> u8 {
        match self {
            Self::Starting => 0,
            Self::Running => 1,
            Self::Stopping => 2,
            Self::Stopped => 3,
            Self::Failed => 4,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Running,
            2 => Self::Stopping,
            3 => Self::Stopped,
            4 => Self::Failed,
            _ => Self::Starting,
        }
    }
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
    pub source_port: String,
    pub schema_version: u32,
    pub signal: Signal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutedSignalError {
    UnsupportedSchemaVersion { received: u32, expected: u32 },
    MissingSourceId,
    MissingSourcePort,
}

impl RoutedSignal {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn new(
        source_id: impl Into<String>,
        source_port: impl Into<String>,
        signal: Signal,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            source_port: source_port.into(),
            schema_version: Self::SCHEMA_VERSION,
            signal,
        }
    }

    /// Validate metadata before a signal enters the patch graph.
    pub fn validate(&self) -> Result<(), RoutedSignalError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(RoutedSignalError::UnsupportedSchemaVersion {
                received: self.schema_version,
                expected: Self::SCHEMA_VERSION,
            });
        }
        if self.source_id.trim().is_empty() {
            return Err(RoutedSignalError::MissingSourceId);
        }
        if self.source_port.trim().is_empty() {
            return Err(RoutedSignalError::MissingSourcePort);
        }
        Ok(())
    }
}

/// Select the first output port for adapters that emit one logical stream.
pub fn default_output_port(schema: &ModuleSchema) -> String {
    schema
        .ports
        .iter()
        .find(|port| port.direction == crate::PortDirection::Output)
        .map(|port| port.id.clone())
        .unwrap_or_else(|| "default".to_string())
}

/// Outcome of a bounded host shutdown request.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ShutdownReport {
    pub completed: Vec<String>,
    pub timed_out: Vec<String>,
}

/// Counters for signals crossing the control-plane router boundary.
#[derive(Default)]
pub struct RoutingMetrics {
    pub received: AtomicU64,
    pub invalid_dropped: AtomicU64,
    pub unroutable: AtomicU64,
    pub disabled: AtomicU64,
    pub delivered: AtomicU64,
    pub send_failures: AtomicU64,
    pub fanout_clones: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoutingMetricsSnapshot {
    pub received: u64,
    pub invalid_dropped: u64,
    pub unroutable: u64,
    pub disabled: u64,
    pub delivered: u64,
    pub send_failures: u64,
    pub fanout_clones: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoutingResult {
    pub delivered: usize,
    pub dropped: bool,
}

impl RoutingMetrics {
    pub fn snapshot(&self) -> RoutingMetricsSnapshot {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        RoutingMetricsSnapshot {
            received: load(&self.received),
            invalid_dropped: load(&self.invalid_dropped),
            unroutable: load(&self.unroutable),
            disabled: load(&self.disabled),
            delivered: load(&self.delivered),
            send_failures: load(&self.send_failures),
            fanout_clones: load(&self.fanout_clones),
        }
    }
}

/// Handle to a running module instance
pub struct ModuleHandle {
    pub id: String,
    task: Option<ModuleTask>,
    pub inbox: mpsc::Sender<Signal>,
    _shutdown_tx: mpsc::Sender<()>,
    state: Arc<AtomicU8>,
}

enum ModuleTask {
    Async {
        task: tokio::task::JoinHandle<()>,
        state: Arc<AtomicU8>,
    },
    Thread {
        thread: JoinHandle<()>,
        state: Arc<AtomicU8>,
    },
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
        let current = self.state();
        if matches!(current, ModuleState::Stopping | ModuleState::Stopped) {
            return;
        }
        self.state
            .store(ModuleState::Stopping.as_u8(), Ordering::Release);
        let _ = self._shutdown_tx.try_send(());
    }

    /// Return the latest lifecycle state observed for this module.
    pub fn state(&self) -> ModuleState {
        ModuleState::from_u8(self.state.load(Ordering::Acquire))
    }
}

use crate::resources::buffer_pool::{AudioBufferPool, BlobBufferPool};
#[cfg(feature = "gpu-resources")]
use crate::resources::gpu_map::{GpuBufferMap, GpuTextureMap, GpuTextureViewMap};

/// Manages the lifecycle of all module runtimes
pub struct ModuleHost {
    modules: HashMap<String, ModuleHandle>,
    router_tx: mpsc::Sender<RoutedSignal>,
    runtime: Arc<tokio::runtime::Runtime>,
    routing_metrics: Arc<RoutingMetrics>,
    pub audio_pool: Arc<AudioBufferPool>,
    pub blob_pool: Arc<BlobBufferPool>,
    #[cfg(feature = "gpu-resources")]
    pub texture_map: Arc<GpuTextureMap>,
    #[cfg(feature = "gpu-resources")]
    pub buffer_map: Arc<GpuBufferMap>,
    #[cfg(feature = "gpu-resources")]
    pub view_map: Arc<GpuTextureViewMap>,
}

impl ModuleHost {
    /// Create a new module host
    pub fn new(router_tx: mpsc::Sender<RoutedSignal>) -> Self {
        Self {
            modules: HashMap::new(),
            router_tx,
            runtime: Arc::new(
                tokio::runtime::Runtime::new().expect("Failed to create Magnolia runtime"),
            ),
            routing_metrics: Arc::new(RoutingMetrics::default()),
            audio_pool: Arc::new(AudioBufferPool::new()),
            blob_pool: Arc::new(BlobBufferPool::new()),
            #[cfg(feature = "gpu-resources")]
            texture_map: Arc::new(GpuTextureMap::new()),
            #[cfg(feature = "gpu-resources")]
            buffer_map: Arc::new(GpuBufferMap::new()),
            #[cfg(feature = "gpu-resources")]
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
        let state = Arc::new(AtomicU8::new(ModuleState::Starting.as_u8()));

        // Spawn based on execution model
        let task = match module.execution_model() {
            ExecutionModel::Async => {
                // Async modules share one runtime so each module does not create
                // an OS thread and a Tokio scheduler of its own.
                let module_name_clone = module_name.clone();
                let runtime = self.runtime.clone();
                let state = state.clone();
                let task_state = state.clone();
                ModuleTask::Async {
                    task: runtime.spawn(async move {
                        task_state.store(ModuleState::Running.as_u8(), Ordering::Release);
                        tokio::select! {
                            _ = shutdown_rx.recv() => {
                                log::info!("Module {} received shutdown signal", module_name_clone);
                            }
                            _ = module.run(inbox_rx, outbox) => {
                                log::info!("Module {} exited normally", module_name_clone);
                            }
                        }
                        task_state.store(ModuleState::Stopped.as_u8(), Ordering::Release);
                    }),
                    state: state.clone(),
                }
            }
            ExecutionModel::DedicatedThread => {
                // Direct OS thread with its own runtime
                let state = state.clone();
                let thread_state = state.clone();
                ModuleTask::Thread {
                    thread: thread::spawn(move || {
                        thread_state.store(ModuleState::Running.as_u8(), Ordering::Release);
                        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            rt.block_on(async {
                                module.run(inbox_rx, outbox).await;
                            });
                        }));

                        match result {
                            Ok(_) => {
                                thread_state.store(ModuleState::Stopped.as_u8(), Ordering::Release);
                                log::info!("Module {} exited normally", module_name)
                            }
                            Err(e) => {
                                thread_state.store(ModuleState::Failed.as_u8(), Ordering::Release);
                                log::error!("Module {} panicked: {:?}", module_name, e);
                            }
                        }
                    }),
                    state: state.clone(),
                }
            }
            ExecutionModel::ThreadPool { .. } => {
                // For now, treat as dedicated thread
                // TODO: Implement rayon integration
                let state = state.clone();
                let thread_state = state.clone();
                ModuleTask::Thread {
                    thread: thread::spawn(move || {
                        thread_state.store(ModuleState::Running.as_u8(), Ordering::Release);
                        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            rt.block_on(async {
                                module.run(inbox_rx, outbox).await;
                            });
                        }));

                        match result {
                            Ok(_) => {
                                thread_state.store(ModuleState::Stopped.as_u8(), Ordering::Release);
                                log::info!("Module {} exited normally", module_name)
                            }
                            Err(e) => {
                                thread_state.store(ModuleState::Failed.as_u8(), Ordering::Release);
                                log::error!("Module {} panicked: {:?}", module_name, e);
                            }
                        }
                    }),
                    state: state.clone(),
                }
            }
        };

        let module_handle = ModuleHandle {
            id: module_id.clone(),
            task: Some(task),
            inbox: inbox_tx,
            _shutdown_tx: shutdown_tx,
            state,
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
        let report = self.shutdown_module_with_timeout(module_id, Duration::from_secs(5))?;
        if report.timed_out.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Module {module_id} did not stop before the deadline"
            ))
        }
    }

    /// Shutdown one module with an explicit deadline.
    pub fn shutdown_module_with_timeout(
        &mut self,
        module_id: &str,
        timeout: Duration,
    ) -> Result<ShutdownReport, String> {
        if let Some(mut handle) = self.modules.remove(module_id) {
            handle.shutdown();
            let mut report = ShutdownReport::default();
            if let Some(task) = handle.task.take() {
                if Self::join_task(&self.runtime, task, timeout) {
                    report.completed.push(module_id.to_string());
                } else {
                    report.timed_out.push(module_id.to_string());
                }
            }
            Ok(report)
        } else {
            Err(format!("Module {} not found", module_id))
        }
    }

    /// Shutdown all modules and wait for them to finish
    pub fn shutdown_all(&mut self) {
        let report = self.shutdown_all_with_timeout(Duration::from_secs(5));
        if !report.timed_out.is_empty() {
            log::error!("Modules exceeded shutdown deadline: {:?}", report.timed_out);
        }
    }

    /// Shutdown all modules, bounding each join by `timeout`.
    pub fn shutdown_all_with_timeout(&mut self, timeout: Duration) -> ShutdownReport {
        log::info!("Shutting down {} modules", self.modules.len());
        let mut report = ShutdownReport::default();

        // Send shutdown signals
        for (id, handle) in &self.modules {
            log::debug!("Sending shutdown to {}", id);
            handle.shutdown();
        }

        // Wait for all threads to finish
        let runtime = self.runtime.clone();
        for (id, mut handle) in self.modules.drain() {
            if let Some(task) = handle.task.take() {
                log::debug!("Waiting for {} to finish", id);
                if Self::join_task(&runtime, task, timeout) {
                    report.completed.push(id);
                } else {
                    report.timed_out.push(id);
                }
            }
        }

        log::info!("All modules shut down");
        report
    }

    fn join_task(runtime: &tokio::runtime::Runtime, task: ModuleTask, timeout: Duration) -> bool {
        match task {
            ModuleTask::Async { mut task, state } => {
                match runtime.block_on(async { tokio::time::timeout(timeout, &mut task).await }) {
                    Ok(Ok(())) => true,
                    Ok(Err(error)) => {
                        state.store(ModuleState::Failed.as_u8(), Ordering::Release);
                        log::error!("Async module task failed during shutdown: {error}");
                        true
                    }
                    Err(_) => {
                        task.abort();
                        state.store(ModuleState::Failed.as_u8(), Ordering::Release);
                        false
                    }
                }
            }
            ModuleTask::Thread { thread, state } => {
                let (done_tx, done_rx) = std_mpsc::sync_channel(1);
                thread::spawn(move || {
                    let _ = thread.join();
                    let _ = done_tx.send(());
                });
                if done_rx.recv_timeout(timeout).is_ok() {
                    true
                } else {
                    state.store(ModuleState::Failed.as_u8(), Ordering::Release);
                    false
                }
            }
        }
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

    pub fn routing_metrics(&self) -> Arc<RoutingMetrics> {
        self.routing_metrics.clone()
    }

    /// Route an envelope through the patch graph and deliver it to module inboxes.
    pub fn route_signal(&self, patch_bay: &crate::PatchBay, routed: RoutedSignal) -> RoutingResult {
        self.routing_metrics
            .received
            .fetch_add(1, Ordering::Relaxed);
        if let Err(error) = routed.validate() {
            self.routing_metrics
                .invalid_dropped
                .fetch_add(1, Ordering::Relaxed);
            log::warn!(
                "Dropping invalid routed signal from '{}': {:?}",
                routed.source_id,
                error
            );
            return RoutingResult {
                dropped: true,
                ..Default::default()
            };
        }
        let outgoing = patch_bay
            .get_outgoing_patches(&routed.source_id)
            .into_iter()
            .filter(|patch| {
                routed.source_port == "default" || patch.source_port == routed.source_port
            })
            .collect::<Vec<_>>();
        if outgoing.is_empty() {
            self.routing_metrics
                .unroutable
                .fetch_add(1, Ordering::Relaxed);
            return RoutingResult {
                dropped: true,
                ..Default::default()
            };
        }
        let active_sinks = outgoing
            .into_iter()
            .filter(|patch| {
                if patch_bay.is_module_disabled(&patch.sink_module) {
                    self.routing_metrics
                        .disabled
                        .fetch_add(1, Ordering::Relaxed);
                    false
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();
        let delivery_count = if matches!(&routed.signal, Signal::AudioStream { .. }) {
            active_sinks.len().min(1)
        } else {
            active_sinks.len()
        };
        let mut signal = Some(routed.signal);
        let mut delivered = 0;
        for (index, patch) in active_sinks.into_iter().take(delivery_count).enumerate() {
            let payload = if index + 1 == delivery_count {
                signal.take().expect("signal payload already taken")
            } else {
                self.routing_metrics
                    .fanout_clones
                    .fetch_add(1, Ordering::Relaxed);
                signal.as_ref().expect("signal payload missing").clone()
            };
            if self.send_signal(&patch.sink_module, payload).is_ok() {
                delivered += 1;
                self.routing_metrics
                    .delivered
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                self.routing_metrics
                    .send_failures
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        RoutingResult {
            delivered,
            dropped: delivered == 0,
        }
    }

    /// Return the lifecycle state of a registered module.
    pub fn module_state(&self, module_id: &str) -> Option<ModuleState> {
        self.modules.get(module_id).map(ModuleHandle::state)
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
        slow_shutdown: bool,
    }

    impl TestModule {
        fn new(id: &str) -> (Self, Arc<AtomicBool>) {
            let ran = Arc::new(AtomicBool::new(false));
            (
                Self {
                    id: id.to_string(),
                    enabled: true,
                    ran: ran.clone(),
                    slow_shutdown: false,
                },
                ran,
            )
        }

        fn slow_shutdown(id: &str) -> Self {
            Self {
                id: id.to_string(),
                enabled: true,
                ran: Arc::new(AtomicBool::new(false)),
                slow_shutdown: true,
            }
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
        fn execution_model(&self) -> ExecutionModel {
            if self.slow_shutdown {
                ExecutionModel::DedicatedThread
            } else {
                ExecutionModel::Async
            }
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
            if self.slow_shutdown {
                tokio::time::sleep(Duration::from_millis(100)).await;
                return;
            }
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
        assert_eq!(host.module_state("test_module"), Some(ModuleState::Running));
    }

    #[test]
    fn test_module_shutdown() {
        let (router_tx, _router_rx) = mpsc::channel(10);
        let mut host = ModuleHost::new(router_tx);

        let (module, _) = TestModule::new("test_module");
        host.spawn(module, 10).unwrap();

        thread::sleep(Duration::from_millis(50));
        assert_eq!(host.module_state("test_module"), Some(ModuleState::Running));
        host.shutdown_module("test_module").unwrap();

        assert!(host.get_module("test_module").is_none());
    }

    #[test]
    fn test_module_shutdown_deadline() {
        let (router_tx, _router_rx) = mpsc::channel(10);
        let mut host = ModuleHost::new(router_tx);
        host.spawn(TestModule::slow_shutdown("slow_module"), 10)
            .unwrap();

        thread::sleep(Duration::from_millis(10));
        let report = host.shutdown_all_with_timeout(Duration::from_millis(1));
        assert_eq!(report.completed, Vec::<String>::new());
        assert_eq!(report.timed_out, vec!["slow_module".to_string()]);
    }

    #[test]
    fn routed_signal_metadata_is_validated() {
        let routed = RoutedSignal::new("source", "audio_out", Signal::Pulse);
        assert_eq!(routed.validate(), Ok(()));

        let mut invalid = routed.clone();
        invalid.schema_version = 99;
        assert_eq!(
            invalid.validate(),
            Err(RoutedSignalError::UnsupportedSchemaVersion {
                received: 99,
                expected: RoutedSignal::SCHEMA_VERSION,
            })
        );

        invalid.schema_version = RoutedSignal::SCHEMA_VERSION;
        invalid.source_port.clear();
        assert_eq!(
            invalid.validate(),
            Err(RoutedSignalError::MissingSourcePort)
        );
    }

    #[test]
    fn routing_metrics_snapshot_is_consistent() {
        let metrics = RoutingMetrics::default();
        metrics.received.fetch_add(2, Ordering::Relaxed);
        metrics.invalid_dropped.fetch_add(1, Ordering::Relaxed);
        assert_eq!(
            metrics.snapshot(),
            RoutingMetricsSnapshot {
                received: 2,
                invalid_dropped: 1,
                ..RoutingMetricsSnapshot::default()
            }
        );
    }
}
