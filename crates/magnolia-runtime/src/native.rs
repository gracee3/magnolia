use magnolia_application::{ActivationRequest, RuntimeControl, RuntimeEvent, RuntimePort};
use magnolia_protocol::{AudioRuntimeProjection, AudioRuntimeState};
use std::{
    collections::VecDeque,
    sync::{mpsc, Arc, Mutex, MutexGuard},
    thread::{self, JoinHandle},
};

enum WorkerMessage {
    Activate(ActivationRequest),
    Control(RuntimeControl),
    Shutdown,
}

pub struct NativeRuntime {
    sender: mpsc::Sender<WorkerMessage>,
    events: Arc<Mutex<VecDeque<RuntimeEvent>>>,
    worker: Option<JoinHandle<()>>,
}

impl NativeRuntime {
    #[must_use]
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let worker_events = Arc::clone(&events);
        let worker = thread::Builder::new()
            .name("magnolia-audio-control".to_owned())
            .spawn(move || run_worker(receiver, &worker_events))
            .ok();
        Self {
            sender,
            events,
            worker,
        }
    }

    fn events(&self) -> MutexGuard<'_, VecDeque<RuntimeEvent>> {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

impl Default for NativeRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimePort for NativeRuntime {
    fn enqueue_activation(&mut self, request: ActivationRequest) {
        if self
            .sender
            .send(WorkerMessage::Activate(request.clone()))
            .is_err()
        {
            self.events().push_back(RuntimeEvent::ActivationFailed {
                operation_id: request.operation_id,
                target_graph_revision: request.target_graph_revision,
                code: "audio.worker.unavailable".to_owned(),
                message: "native audio control worker is unavailable".to_owned(),
            });
        }
    }

    fn enqueue_control(&mut self, control: RuntimeControl) {
        if self.sender.send(WorkerMessage::Control(control)).is_err() {
            let projection = AudioRuntimeProjection {
                state: AudioRuntimeState::Failed,
                last_error: Some("native audio control worker is unavailable".to_owned()),
                ..AudioRuntimeProjection::default()
            };
            self.events()
                .push_back(RuntimeEvent::AudioProjection(projection));
        }
    }

    fn poll_event(&mut self) -> Option<RuntimeEvent> {
        self.events().pop_front()
    }
}

impl Drop for NativeRuntime {
    fn drop(&mut self) {
        let _ = self.sender.send(WorkerMessage::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_worker(
    receiver: mpsc::Receiver<WorkerMessage>,
    events: &Arc<Mutex<VecDeque<RuntimeEvent>>>,
) {
    let mut audio = AudioRuntimeProjection::default();
    while let Ok(message) = receiver.recv() {
        match message {
            WorkerMessage::Activate(request) => {
                // Graph compilation remains control-thread work. The initial
                // native slice accepts the empty graph and rejects every
                // module until its concrete audio compiler is registered.
                let unsupported = request.graph.modules.values().next();
                let event = if let Some(module) = unsupported {
                    RuntimeEvent::ActivationFailed {
                        operation_id: request.operation_id,
                        target_graph_revision: request.target_graph_revision,
                        code: "audio.graph.unsupported_module".to_owned(),
                        message: format!(
                            "module {} is not supported by the initial native audio compiler",
                            module.module_type
                        ),
                    }
                } else {
                    RuntimeEvent::ActivationSucceeded {
                        operation_id: request.operation_id,
                        target_graph_revision: request.target_graph_revision,
                    }
                };
                push_event(events, event);
            }
            WorkerMessage::Control(control) => {
                match control {
                    RuntimeControl::StartAudio => {
                        audio.desired_running = true;
                        audio.state = AudioRuntimeState::Degraded;
                        audio.last_error = Some(
                            "no resolved PipeWire default input metadata is available".to_owned(),
                        );
                    }
                    RuntimeControl::StopAudio => {
                        audio.desired_running = false;
                        audio.monitor_enabled = false;
                        audio.monitor_muted = true;
                        audio.monitor_gain_millionths = 0;
                        audio.state = AudioRuntimeState::Stopped;
                        audio.last_error = None;
                    }
                    RuntimeControl::SetCaptureMuted(muted) => audio.capture_muted = muted,
                    RuntimeControl::SetMonitorEnabled(enabled) => {
                        audio.monitor_enabled = enabled;
                        if !enabled {
                            audio.monitor_muted = true;
                            audio.monitor_gain_millionths = 0;
                        }
                    }
                    RuntimeControl::SetMonitorMuted(muted) => audio.monitor_muted = muted,
                    RuntimeControl::SetMonitorGain(gain) => {
                        audio.monitor_gain_millionths = gain;
                    }
                }
                audio.runtime_revision = audio.runtime_revision.saturating_add(1);
                push_event(events, RuntimeEvent::AudioProjection(audio.clone()));
            }
            WorkerMessage::Shutdown => break,
        }
    }
}

fn push_event(events: &Arc<Mutex<VecDeque<RuntimeEvent>>>, event: RuntimeEvent) {
    events
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push_back(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use magnolia_domain::{OperationId, TargetGraphRevision, WorkspaceGraph};
    use std::time::{Duration, Instant};

    fn next_event(runtime: &mut NativeRuntime) -> RuntimeEvent {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(event) = runtime.poll_event() {
                return event;
            }
            assert!(Instant::now() < deadline, "native worker event timed out");
            thread::yield_now();
        }
    }

    #[test]
    fn runtime_controls_are_ephemeral_and_stop_restores_safe_monitoring() {
        let mut runtime = NativeRuntime::new();
        runtime.enqueue_control(RuntimeControl::SetMonitorEnabled(true));
        runtime.enqueue_control(RuntimeControl::SetMonitorMuted(false));
        runtime.enqueue_control(RuntimeControl::SetMonitorGain(30_000));
        runtime.enqueue_control(RuntimeControl::StopAudio);
        for _ in 0..3 {
            let _ = next_event(&mut runtime);
        }
        let RuntimeEvent::AudioProjection(stopped) = next_event(&mut runtime) else {
            panic!("expected audio projection");
        };
        assert_eq!(stopped.state, AudioRuntimeState::Stopped);
        assert!(!stopped.monitor_enabled);
        assert!(stopped.monitor_muted);
        assert_eq!(stopped.monitor_gain_millionths, 0);
    }

    #[test]
    fn empty_graph_compiles_off_callback() {
        let mut runtime = NativeRuntime::new();
        runtime.enqueue_activation(ActivationRequest {
            operation_id: OperationId::from_u128(1),
            target_graph_revision: TargetGraphRevision::new(1),
            graph: WorkspaceGraph::default(),
        });
        assert!(matches!(
            next_event(&mut runtime),
            RuntimeEvent::ActivationSucceeded { .. }
        ));
    }
}
