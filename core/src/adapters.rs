use tokio::sync::mpsc;
use async_trait::async_trait;
use crate::{Signal, ModuleSchema, Source, Sink, Processor, ModuleRuntime, ExecutionModel, Priority, RoutedSignal};

/// Adapter to run a Source as a ModuleRuntime
pub struct SourceAdapter<S: Source + 'static> {
    source: S,
    schema: ModuleSchema,
}

impl<S: Source + 'static> SourceAdapter<S> {
    pub fn new(source: S) -> Self {
        let schema = source.schema();
        Self { source, schema }
    }
}

#[async_trait]
impl<S: Source + 'static> ModuleRuntime for SourceAdapter<S> {
    fn id(&self) -> &str {
        &self.schema.id
    }
    
    fn name(&self) -> &str {
        self.source.name()
    }
    
    fn schema(&self) -> ModuleSchema {
        self.schema.clone()
    }
    
    fn execution_model(&self) -> ExecutionModel {
        ExecutionModel::Async
    }
    
    fn priority(&self) -> Priority {
        Priority::Normal
    }
    
    fn is_enabled(&self) -> bool {
        self.source.is_enabled()
    }
    
    fn set_enabled(&mut self, enabled: bool) {
        self.source.set_enabled(enabled);
    }
    
    async fn run(&mut self, _inbox: mpsc::Receiver<Signal>, outbox: mpsc::Sender<RoutedSignal>) {
        // Sources don't receive signals, they only emit
        // Clean async/await now that run() is async!
        loop {
            match self.source.poll().await {
                Some(signal) => {
                    let routed = RoutedSignal {
                        source_id: self.schema.id.clone(),
                        signal,
                    };
                    if outbox.send(routed).await.is_err() {
                        log::warn!("Source {} outbox closed, shutting down", self.name());
                        break;
                    }
                }
                None => {
                    log::info!("Source {} poll returned None, shutting down", self.name());
                    break;
                }
            }
        }
    }
}

/// Adapter to run a Sink as a ModuleRuntime
pub struct SinkAdapter<S: Sink + 'static> {
    sink: S,
    schema: ModuleSchema,
}

impl<S: Sink + 'static> SinkAdapter<S> {
    pub fn new(sink: S) -> Self {
        let schema = sink.schema();
        Self { sink, schema }
    }
}

#[async_trait]
impl<S: Sink + 'static> ModuleRuntime for SinkAdapter<S> {
    fn id(&self) -> &str {
        &self.schema.id
    }
    
    fn name(&self) -> &str {
        self.sink.name()
    }
    
    fn schema(&self) -> ModuleSchema {
        self.schema.clone()
    }
    
    fn execution_model(&self) -> ExecutionModel {
        ExecutionModel::Async
    }
    
    fn priority(&self) -> Priority {
        Priority::Normal
    }
    
    fn is_enabled(&self) -> bool {
        self.sink.is_enabled()
    }
    
    fn set_enabled(&mut self, enabled: bool) {
        self.sink.set_enabled(enabled);
    }
    
    async fn run(&mut self, mut inbox: mpsc::Receiver<Signal>, _outbox: mpsc::Sender<RoutedSignal>) {
        // Sinks consume signals but don't emit (except via internal channels)
        // Clean async/await - no more runtime nesting!
        while let Some(signal) = inbox.recv().await {
            if !self.is_enabled() {
                continue;
            }
            
            if let Err(e) = self.sink.consume(signal).await {
                log::error!("Sink {} error: {}", self.name(), e);
            }
        }
        log::info!("Sink {} inbox closed, shutting down", self.name());
    }
}

/// Adapter to run a Processor as a ModuleRuntime
pub struct ProcessorAdapter<P: Processor + 'static> {
    processor: P,
    schema: ModuleSchema,
}

impl<P: Processor + 'static> ProcessorAdapter<P> {
    pub fn new(processor: P) -> Self {
        let schema = processor.schema();
        Self { processor, schema }
    }
}

#[async_trait]
impl<P: Processor + 'static> ModuleRuntime for ProcessorAdapter<P> {
    fn id(&self) -> &str {
        &self.schema.id
    }
    
    fn name(&self) -> &str {
        self.processor.name()
    }
    
    fn schema(&self) -> ModuleSchema {
        self.schema.clone()
    }
    
    fn execution_model(&self) -> ExecutionModel {
        ExecutionModel::Async
    }
    
    fn priority(&self) -> Priority {
        Priority::Normal
    }
    
    fn is_enabled(&self) -> bool {
        self.processor.is_enabled()
    }
    
    fn set_enabled(&mut self, enabled: bool) {
        self.processor.set_enabled(enabled);
    }
    
    async fn run(&mut self, mut inbox: mpsc::Receiver<Signal>, outbox: mpsc::Sender<RoutedSignal>) {
        while let Some(signal) = inbox.recv().await {
            if !self.is_enabled() {
                continue;
            }
            
            match self.processor.process(signal).await {
                Ok(Some(output)) => {
                    let routed = RoutedSignal {
                        source_id: self.schema.id.clone(),
                        signal: output,
                    };
                    if outbox.send(routed).await.is_err() {
                        log::warn!("Processor {} outbox closed, shutting down", self.name());
                        break;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    log::error!("Processor {} error: {}", self.name(), e);
                }
            }
        }
        log::info!("Processor {} inbox closed, shutting down", self.name());
    }
}
