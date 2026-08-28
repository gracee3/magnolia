use magnolia_domain::{DeviceSelector, OperationId, TargetGraphRevision, WorkspaceGraph};
use magnolia_protocol::AudioRuntimeProjection;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRequest {
    pub operation_id: OperationId,
    pub target_graph_revision: TargetGraphRevision,
    pub graph: WorkspaceGraph,
    pub device_selectors: BTreeMap<String, DeviceSelector>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    ActivationSucceeded {
        operation_id: OperationId,
        target_graph_revision: TargetGraphRevision,
    },
    ActivationFailed {
        operation_id: OperationId,
        target_graph_revision: TargetGraphRevision,
        code: String,
        message: String,
    },
    AudioProjection(AudioRuntimeProjection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeControl {
    StartAudio,
    StopAudio,
    SetCaptureMuted(bool),
    SetMonitorEnabled(bool),
    SetMonitorMuted(bool),
    SetMonitorGain(u32),
}

pub trait RuntimePort: Send + 'static {
    fn enqueue_activation(&mut self, request: ActivationRequest);
    fn enqueue_control(&mut self, _control: RuntimeControl) {}
    fn poll_event(&mut self) -> Option<RuntimeEvent>;
}
