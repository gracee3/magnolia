//! Native runtime adapters for Magnolia.

use magnolia_application::{ActivationRequest, RuntimeEvent, RuntimePort};
use magnolia_domain::TargetGraphRevision;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};
use thiserror::Error;

mod activation;
pub use activation::{activation_channel, ActivationBoundary, ActivationController};

#[derive(Debug, Default)]
struct MockState {
    pending: VecDeque<ActivationRequest>,
    observed: Vec<ActivationRequest>,
    events: VecDeque<RuntimeEvent>,
}

/// Deterministic runtime adapter controlled explicitly by tests or a shell proof.
#[derive(Debug, Clone, Default)]
pub struct MockRuntime {
    state: Arc<Mutex<MockState>>,
}

impl MockRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn pending_requests(&self) -> Vec<ActivationRequest> {
        self.lock().pending.iter().cloned().collect()
    }

    #[must_use]
    pub fn observed_requests(&self) -> Vec<ActivationRequest> {
        self.lock().observed.clone()
    }

    pub fn complete_next_success(&self) -> Result<ActivationRequest, MockRuntimeError> {
        let mut state = self.lock();
        let request = state
            .pending
            .pop_front()
            .ok_or(MockRuntimeError::NoPendingActivation)?;
        state.events.push_back(RuntimeEvent::ActivationSucceeded {
            operation_id: request.operation_id,
            target_graph_revision: request.target_graph_revision,
        });
        Ok(request)
    }

    pub fn complete_next_failure(
        &self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<ActivationRequest, MockRuntimeError> {
        let mut state = self.lock();
        let request = state
            .pending
            .pop_front()
            .ok_or(MockRuntimeError::NoPendingActivation)?;
        state.events.push_back(RuntimeEvent::ActivationFailed {
            operation_id: request.operation_id,
            target_graph_revision: request.target_graph_revision,
            code: code.into(),
            message: message.into(),
        });
        Ok(request)
    }

    pub fn complete_target_success(
        &self,
        target: TargetGraphRevision,
    ) -> Result<ActivationRequest, MockRuntimeError> {
        let mut state = self.lock();
        let position = state
            .pending
            .iter()
            .position(|request| request.target_graph_revision == target)
            .ok_or(MockRuntimeError::TargetNotPending(target))?;
        let request = state
            .pending
            .remove(position)
            .expect("position came from the same queue");
        state.events.push_back(RuntimeEvent::ActivationSucceeded {
            operation_id: request.operation_id,
            target_graph_revision: request.target_graph_revision,
        });
        Ok(request)
    }

    fn lock(&self) -> MutexGuard<'_, MockState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl RuntimePort for MockRuntime {
    fn enqueue_activation(&mut self, request: ActivationRequest) {
        let mut state = self.lock();
        state.observed.push(request.clone());
        state.pending.push_back(request);
    }

    fn poll_event(&mut self) -> Option<RuntimeEvent> {
        self.lock().events.pop_front()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MockRuntimeError {
    #[error("mock runtime has no pending activation")]
    NoPendingActivation,
    #[error("mock runtime has no pending activation for target revision {0}")]
    TargetNotPending(TargetGraphRevision),
}

#[cfg(test)]
mod tests {
    use super::*;
    use magnolia_application::RuntimePort;
    use magnolia_domain::WorkspaceGraph;

    #[test]
    fn completions_are_explicit_and_fifo_by_default() {
        let runtime = MockRuntime::new();
        let mut port = runtime.clone();
        let request = ActivationRequest {
            operation_id: magnolia_domain::OperationId::from_u128(1),
            target_graph_revision: TargetGraphRevision::new(2),
            graph: WorkspaceGraph::default(),
        };
        port.enqueue_activation(request.clone());
        assert_eq!(runtime.pending_requests(), vec![request.clone()]);
        assert_eq!(runtime.complete_next_success().unwrap(), request);
        assert!(matches!(
            port.poll_event(),
            Some(RuntimeEvent::ActivationSucceeded { .. })
        ));
        assert!(runtime.pending_requests().is_empty());
    }
}
