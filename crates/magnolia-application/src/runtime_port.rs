use magnolia_domain::{OperationId, TargetGraphRevision, WorkspaceGraph};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRequest {
    pub operation_id: OperationId,
    pub target_graph_revision: TargetGraphRevision,
    pub graph: WorkspaceGraph,
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
}

pub trait RuntimePort: Send + 'static {
    fn enqueue_activation(&mut self, request: ActivationRequest);
    fn poll_event(&mut self) -> Option<RuntimeEvent>;
}
