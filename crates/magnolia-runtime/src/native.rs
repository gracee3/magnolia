use magnolia_application::{ActivationRequest, RuntimeControl, RuntimeEvent, RuntimePort};
#[cfg(target_os = "linux")]
use magnolia_audio::{
    pipewire::PipeWireRegistryManager, CaptureConfiguration, CaptureState, DeviceDirection,
    NativeSampleFormat, OutputConfiguration, PipeWireCapture, PipeWireOutput,
};
use magnolia_domain::{native_audio, DeviceSelector, EntityId, WorkspaceGraph};
use magnolia_protocol::{
    AudioDeviceDirection, AudioDeviceProjection, AudioRuntimeProjection, AudioRuntimeState,
};
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
    #[cfg(target_os = "linux")]
    let mut output: Option<PipeWireOutput> = None;
    loop {
        let message = match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                #[cfg(target_os = "linux")]
                let before = audio.clone();
                #[cfg(target_os = "linux")]
                if let Some(registry) = registry.as_ref() {
                    update_registry_projection(&mut audio, registry);
                }
                #[cfg(target_os = "linux")]
                if let Some(capture) = capture.as_ref() {
                    apply_capture_snapshot(&mut audio, capture);
                }
                #[cfg(target_os = "linux")]
                if let Some(output) = output.as_ref() {
                    audio.underruns = output.snapshot().underruns;
                }
                #[cfg(target_os = "linux")]
                if audio != before {
                    audio.runtime_revision = audio.runtime_revision.saturating_add(1);
                    push_event(events, RuntimeEvent::AudioProjection(audio.clone()));
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match message {
            WorkerMessage::Activate(request) => {
                #[cfg(target_os = "linux")]
                if let Some(registry) = registry.as_ref() {
                    update_registry_projection(&mut audio, registry);
                }
                input_selector = request
                    .device_selectors
                    .get("audio.input")
                    .cloned()
                    .or_else(|| request.device_selectors.values().next().cloned());
                audio.input_selector_key =
                    input_selector.as_ref().map(|_| "audio.input".to_owned());
                // Compilation and validation stay on this control worker. A
                // failed candidate never changes the application last-good
                // revision.
                let event = if let Err(error) = compile_audio_graph(&request.graph) {
                    RuntimeEvent::ActivationFailed {
                        operation_id: request.operation_id,
                        target_graph_revision: request.target_graph_revision,
                        code: error.code.to_owned(),
                        message: error.message,
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
                        output.take();
                        #[cfg(target_os = "linux")]
                        capture.take();
                        audio.desired_running = false;
                        audio.monitor_enabled = false;
                        audio.monitor_muted = true;
                        audio.monitor_gain_millionths = 0;
                        audio.state = AudioRuntimeState::Stopped;
                        audio.last_error = None;
                    }
                    RuntimeControl::SetCaptureMuted(muted) => {
                        audio.capture_muted = muted;
                        #[cfg(target_os = "linux")]
                        if let Some(capture) = capture.as_ref() {
                            capture.set_muted(muted);
                        }
                    }
                    RuntimeControl::SetMonitorEnabled(enabled) => {
                        audio.monitor_enabled = enabled;
                        if !enabled {
                            #[cfg(target_os = "linux")]
                            output.take();
                            audio.monitor_muted = true;
                            audio.monitor_gain_millionths = 0;
                        } else {
                            #[cfg(target_os = "linux")]
                            start_monitor_output(
                                &mut audio,
                                registry.as_ref(),
                                capture.as_mut(),
                                &mut output,
                            );
                        }
                    }
                    RuntimeControl::SetMonitorMuted(muted) => {
                        audio.monitor_muted = muted;
                        #[cfg(target_os = "linux")]
                        if let Some(output) = output.as_ref() {
                            output.set_muted(muted);
                        }
                    }
                    RuntimeControl::SetMonitorGain(gain) => {
                        audio.monitor_gain_millionths = gain;
                        #[cfg(target_os = "linux")]
                        if let Some(output) = output.as_ref() {
                            output.set_gain_millionths(gain);
                        }
                    }
                }
                #[cfg(target_os = "linux")]
                if let Some(registry) = registry.as_ref() {
                    update_registry_projection(&mut audio, registry);
                }
                #[cfg(target_os = "linux")]
                if let Some(capture) = capture.as_ref() {
                    apply_capture_snapshot(&mut audio, capture);
                }
                #[cfg(target_os = "linux")]
                if let Some(output) = output.as_ref() {
                    audio.underruns = output.snapshot().underruns;
                }
                audio.runtime_revision = audio.runtime_revision.saturating_add(1);
                push_event(events, RuntimeEvent::AudioProjection(audio.clone()));
            }
            WorkerMessage::Shutdown => break,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphCompileError {
    code: &'static str,
    message: String,
}

fn compile_audio_graph(graph: &WorkspaceGraph) -> Result<(), GraphCompileError> {
    if graph.modules.is_empty() {
        return graph
            .edges
            .is_empty()
            .then_some(())
            .ok_or(GraphCompileError {
                code: "audio.graph.invalid_topology",
                message: "an empty audio graph cannot contain edges".to_owned(),
            });
    }

    let ordered_types = [
        native_audio::PIPEWIRE_INPUT,
        native_audio::FORMAT_CONVERT,
        native_audio::CHANNEL_MAP,
        native_audio::RESAMPLE,
        native_audio::CAPTURE_MUTE,
        native_audio::MONITOR,
    ];
    let mut ordered_ids: [Option<EntityId>; 6] = [None; 6];
    for module in graph.modules.values() {
        let Some(index) = ordered_types
            .iter()
            .position(|known| module.module_type.as_str() == *known)
        else {
            return Err(GraphCompileError {
                code: "audio.graph.unsupported_module",
                message: format!(
                    "module {} is not supported by the native audio compiler",
                    module.module_type
                ),
            });
        };
        if ordered_ids[index].replace(module.id).is_some() {
            return Err(GraphCompileError {
                code: "audio.graph.invalid_topology",
                message: format!(
                    "native audio path contains duplicate {} modules",
                    ordered_types[index]
                ),
            });
        }
    }

    for (index, module_type) in ordered_types[..5].iter().enumerate() {
        if ordered_ids[index].is_none() {
            return Err(GraphCompileError {
                code: "audio.graph.incomplete_path",
                message: format!("native audio path is missing required module {module_type}"),
            });
        }
    }
    let path_length = if ordered_ids[5].is_some() { 6 } else { 5 };
    if graph.modules.len() != path_length || graph.edges.len() != path_length - 1 {
        return Err(GraphCompileError {
            code: "audio.graph.invalid_topology",
            message: "native audio modules must form one bounded linear path".to_owned(),
        });
    }
    for index in 0..path_length - 1 {
        let (Some(from), Some(to)) = (ordered_ids[index], ordered_ids[index + 1]) else {
            return Err(GraphCompileError {
                code: "audio.graph.incomplete_path",
                message: "native audio path contains an unresolved slot".to_owned(),
            });
        };
        let connected = graph.edges.values().any(|edge| {
            edge.from.module_id == from
                && edge.from.port_id.as_str() == "out"
                && edge.to.module_id == to
                && edge.to.port_id.as_str() == "in"
        });
        if !connected {
            return Err(GraphCompileError {
                code: "audio.graph.invalid_topology",
                message: format!(
                    "native audio path must connect {} directly to {}",
                    ordered_types[index],
                    ordered_types[index + 1],
                ),
            });
        }
    }
    Ok(())
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

#[cfg(target_os = "linux")]
fn update_registry_projection(
    audio: &mut AudioRuntimeProjection,
    registry: &PipeWireRegistryManager,
) {
    let snapshot = registry.snapshot();
    audio.available_devices = snapshot
        .devices()
        .map(|device| AudioDeviceProjection {
            runtime_id: device.runtime_id.clone(),
            label: device.label.clone(),
            direction: match device.direction {
                DeviceDirection::Input => AudioDeviceDirection::Input,
                DeviceDirection::Output => AudioDeviceDirection::Output,
            },
            node_name: device.fingerprint.node_name.clone(),
            device_api: device.fingerprint.device_api.clone(),
            object_path: device.fingerprint.object_path.clone(),
            is_default: match device.direction {
                DeviceDirection::Input => {
                    snapshot.default_input_node_name()
                        == Some(device.fingerprint.node_name.as_str())
                }
                DeviceDirection::Output => {
                    snapshot.default_output_node_name()
                        == Some(device.fingerprint.node_name.as_str())
                }
            },
        })
        .collect();
}

#[cfg(target_os = "linux")]
fn start_monitor_output(
    audio: &mut AudioRuntimeProjection,
    registry: Option<&PipeWireRegistryManager>,
    capture: Option<&mut PipeWireCapture>,
    output: &mut Option<PipeWireOutput>,
) {
    let Some(registry) = registry else {
        audio.last_error = Some("PipeWire registry manager is unavailable".to_owned());
        return;
    };
    let snapshot = registry.snapshot();
    let Ok(device) = snapshot.default_output() else {
        audio.last_error = Some("PipeWire default output is unavailable".to_owned());
        return;
    };
    let Some(edge) = capture.and_then(PipeWireCapture::take_monitor_edge) else {
        audio.last_error = Some("capture graph edge is unavailable for monitoring".to_owned());
        return;
    };
    match PipeWireOutput::start(
        OutputConfiguration {
            target_node_name: device.fingerprint.node_name.clone(),
        },
        edge,
    ) {
        Ok(started) => {
            audio.resolved_output_id = Some(device.runtime_id.clone());
            audio.monitor_muted = true;
            audio.monitor_gain_millionths = 0;
            *output = Some(started);
        }
        Err(error) => audio.last_error = Some(error.to_string()),
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
    use magnolia_domain::{
        Edge, ModuleInstance, ModuleTypeId, OperationId, PortId, PortRef, TargetGraphRevision,
        WorkspaceGraph,
    };
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

    fn native_audio_graph(include_monitor: bool) -> WorkspaceGraph {
        let mut types = vec![
            native_audio::PIPEWIRE_INPUT,
            native_audio::FORMAT_CONVERT,
            native_audio::CHANNEL_MAP,
            native_audio::RESAMPLE,
            native_audio::CAPTURE_MUTE,
        ];
        if include_monitor {
            types.push(native_audio::MONITOR);
        }
        let mut graph = WorkspaceGraph::default();
        for (index, module_type) in types.iter().enumerate() {
            let id = EntityId::from_u128(index as u128 + 1);
            graph.modules.insert(
                id,
                ModuleInstance {
                    id,
                    module_type: ModuleTypeId::new(*module_type).unwrap(),
                    configuration: serde_json::json!({}),
                },
            );
        }
        for index in 0..types.len() - 1 {
            let id = EntityId::from_u128(index as u128 + 100);
            graph.edges.insert(
                id,
                Edge {
                    id,
                    from: PortRef {
                        module_id: EntityId::from_u128(index as u128 + 1),
                        port_id: PortId::new("out").unwrap(),
                    },
                    to: PortRef {
                        module_id: EntityId::from_u128(index as u128 + 2),
                        port_id: PortId::new("in").unwrap(),
                    },
                    capacity: None,
                },
            );
        }
        graph
    }

    #[test]
    fn native_audio_compiler_accepts_capture_with_optional_monitor() {
        assert_eq!(compile_audio_graph(&native_audio_graph(false)), Ok(()));
        assert_eq!(compile_audio_graph(&native_audio_graph(true)), Ok(()));
    }

    #[test]
    fn native_audio_compiler_rejects_out_of_order_edges() {
        let mut graph = native_audio_graph(false);
        let first = graph.edges.values_mut().next().unwrap();
        first.to.module_id = EntityId::from_u128(3);
        let error = compile_audio_graph(&graph).unwrap_err();
        assert_eq!(error.code, "audio.graph.invalid_topology");
    }
}
