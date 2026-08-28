use magnolia_application::{ActivationRequest, RuntimeControl, RuntimeEvent, RuntimePort};
#[cfg(target_os = "linux")]
use magnolia_audio::{
    pipewire::PipeWireRegistryManager, CaptureConfiguration, CaptureState, NativeSampleFormat,
    PipeWireCapture,
};
use magnolia_domain::DeviceSelector;
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
    let mut input_selector: Option<DeviceSelector> = None;
    #[cfg(target_os = "linux")]
    let registry = PipeWireRegistryManager::start().ok();
    #[cfg(target_os = "linux")]
    let mut capture: Option<PipeWireCapture> = None;
    loop {
        let message = match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                #[cfg(target_os = "linux")]
                if let Some(capture) = capture.as_ref() {
                    let before = audio.clone();
                    apply_capture_snapshot(&mut audio, capture);
                    if audio != before {
                        audio.runtime_revision = audio.runtime_revision.saturating_add(1);
                        push_event(events, RuntimeEvent::AudioProjection(audio.clone()));
                    }
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match message {
            WorkerMessage::Activate(request) => {
                input_selector = request
                    .device_selectors
                    .get("audio.input")
                    .cloned()
                    .or_else(|| request.device_selectors.values().next().cloned());
                audio.input_selector_key =
                    input_selector.as_ref().map(|_| "audio.input".to_owned());
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
                        #[cfg(target_os = "linux")]
                        if let Some(target_node_name) = resolve_desired_input(
                            &mut audio,
                            input_selector.as_ref(),
                            registry.as_ref(),
                        ) {
                            match PipeWireCapture::start(CaptureConfiguration { target_node_name })
                            {
                                Ok(started) => capture = Some(started),
                                Err(error) => {
                                    audio.state = AudioRuntimeState::Failed;
                                    audio.last_error = Some(error.to_string());
                                }
                            }
                        }
                        #[cfg(not(target_os = "linux"))]
                        resolve_desired_input(&mut audio, input_selector.as_ref());
                    }
                    RuntimeControl::StopAudio => {
                        #[cfg(target_os = "linux")]
                        capture.take();
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
                #[cfg(target_os = "linux")]
                if let Some(capture) = capture.as_ref() {
                    apply_capture_snapshot(&mut audio, capture);
                }
                audio.runtime_revision = audio.runtime_revision.saturating_add(1);
                push_event(events, RuntimeEvent::AudioProjection(audio.clone()));
            }
            WorkerMessage::Shutdown => break,
        }
    }
}

#[cfg(target_os = "linux")]
fn resolve_desired_input(
    audio: &mut AudioRuntimeProjection,
    selector: Option<&DeviceSelector>,
    registry: Option<&PipeWireRegistryManager>,
) -> Option<String> {
    let Some(selector) = selector else {
        audio.state = AudioRuntimeState::Degraded;
        audio.last_error = Some("no durable input device selector is configured".to_owned());
        return None;
    };
    if let Some(registry) = registry {
        match registry.snapshot().resolve_input(selector) {
            Ok(device) => {
                audio.resolved_input_id = Some(device.runtime_id.clone());
                audio.state = AudioRuntimeState::Preparing;
                audio.last_error = Some(
                    "PipeWire input resolved; capture stream preparation is pending".to_owned(),
                );
                return Some(device.fingerprint.node_name.clone());
            }
            Err(error) => {
                audio.resolved_input_id = None;
                audio.state = AudioRuntimeState::Degraded;
                audio.last_error = Some(error.to_string());
            }
        }
        return None;
    }
    audio.state = AudioRuntimeState::Failed;
    audio.last_error = Some("PipeWire registry manager is unavailable".to_owned());
    None
}

#[cfg(target_os = "linux")]
fn apply_capture_snapshot(audio: &mut AudioRuntimeProjection, capture: &PipeWireCapture) {
    let snapshot = capture.snapshot();
    audio.state = match snapshot.state {
        CaptureState::Running => AudioRuntimeState::Running,
        CaptureState::Preparing | CaptureState::Paused => AudioRuntimeState::Preparing,
        CaptureState::Failed => AudioRuntimeState::Degraded,
        CaptureState::Stopped => AudioRuntimeState::Stopped,
    };
    audio.sample_format = snapshot.sample_format.map(|format| match format {
        NativeSampleFormat::F32Le => magnolia_protocol::AudioSampleFormat::F32Le,
        NativeSampleFormat::S16Le => magnolia_protocol::AudioSampleFormat::S16Le,
        NativeSampleFormat::S32Le => magnolia_protocol::AudioSampleFormat::S32Le,
    });
    audio.sample_rate = (snapshot.sample_rate != 0).then_some(snapshot.sample_rate);
    audio.channels = u16::try_from(snapshot.channels)
        .ok()
        .filter(|value| *value != 0);
    audio.quantum_frames = (snapshot.quantum_frames != 0).then_some(snapshot.quantum_frames);
    audio.callback_count = snapshot.callbacks;
    audio.dropped_frames = snapshot.faults;
    if snapshot.state == CaptureState::Running {
        audio.last_error = None;
    }
}

#[cfg(not(target_os = "linux"))]
fn resolve_desired_input(audio: &mut AudioRuntimeProjection, selector: Option<&DeviceSelector>) {
    if selector.is_none() {
        audio.state = AudioRuntimeState::Degraded;
        audio.last_error = Some("no durable input device selector is configured".to_owned());
    } else {
        audio.state = AudioRuntimeState::Failed;
        audio.last_error = Some("native audio capture requires Linux PipeWire".to_owned());
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
            device_selectors: Default::default(),
        });
        assert!(matches!(
            next_event(&mut runtime),
            RuntimeEvent::ActivationSucceeded { .. }
        ));
    }
}
