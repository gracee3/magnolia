//! Portable Magnolia application-client boundary.

use magnolia_domain::ProjectionRevision;
use magnolia_protocol::{
    CommandEnvelope, CommandReceipt, ConnectRequest, ConnectResponse, RuntimeProjection,
    TelemetryLease, TelemetrySubscription, TranscriptPage,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use thiserror::Error;

mod scenario;

pub use scenario::*;

pub trait ApplicationClient: Send + Sync {
    fn connect(&self, request: ConnectRequest) -> Result<ConnectResponse, ClientError>;

    fn snapshot(&self) -> Result<Arc<RuntimeProjection>, ClientError>;

    fn wait_for_projection(
        &self,
        after: ProjectionRevision,
        timeout: Duration,
    ) -> Result<Arc<RuntimeProjection>, ClientError>;

    fn dispatch(&self, command: CommandEnvelope) -> Result<CommandReceipt, ClientError>;

    fn subscribe_telemetry(
        &self,
        subscription: TelemetrySubscription,
    ) -> Result<TelemetryLease, ClientError>;

    fn transcript_page(&self, after: u64, limit: u32) -> Result<TranscriptPage, ClientError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClientError {
    #[error("script has no remaining step for {0}")]
    ScriptExhausted(&'static str),
    #[error("script expected {expected}, received {actual}")]
    UnexpectedCall {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("script arguments did not match for {0}")]
    ArgumentMismatch(&'static str),
    #[error("projection wait timed out")]
    Timeout,
    #[error("operation is not available in this implementation: {0}")]
    Unsupported(&'static str),
    #[error("application client internal lock was poisoned")]
    Poisoned,
    #[error("application service error: {0}")]
    Service(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationCall {
    Connect(ConnectRequest),
    Snapshot,
    WaitForProjection {
        after: ProjectionRevision,
        timeout: Duration,
    },
    Dispatch(CommandEnvelope),
    SubscribeTelemetry(TelemetrySubscription),
    TranscriptPage {
        after: u64,
        limit: u32,
    },
}

#[derive(Debug, Clone)]
pub enum ScriptStep {
    Connect {
        expected: ConnectRequest,
        result: Result<ConnectResponse, ClientError>,
    },
    Snapshot {
        result: Result<Arc<RuntimeProjection>, ClientError>,
    },
    WaitForProjection {
        expected_after: ProjectionRevision,
        expected_timeout: Duration,
        result: Result<Arc<RuntimeProjection>, ClientError>,
    },
    Dispatch {
        expected: CommandEnvelope,
        result: Result<CommandReceipt, ClientError>,
    },
    SubscribeTelemetry {
        expected: TelemetrySubscription,
        result: Result<TelemetryLease, ClientError>,
    },
    TranscriptPage {
        expected_after: u64,
        expected_limit: u32,
        result: Result<TranscriptPage, ClientError>,
    },
}

impl ScriptStep {
    fn name(&self) -> &'static str {
        match self {
            Self::Connect { .. } => "connect",
            Self::Snapshot { .. } => "snapshot",
            Self::WaitForProjection { .. } => "wait_for_projection",
            Self::Dispatch { .. } => "dispatch",
            Self::SubscribeTelemetry { .. } => "subscribe_telemetry",
            Self::TranscriptPage { .. } => "transcript_page",
        }
    }
}

#[derive(Debug, Default)]
pub struct MockApplicationClient {
    script: Mutex<VecDeque<ScriptStep>>,
    calls: Mutex<Vec<ApplicationCall>>,
}

impl MockApplicationClient {
    #[must_use]
    pub fn scripted(steps: impl IntoIterator<Item = ScriptStep>) -> Self {
        Self {
            script: Mutex::new(steps.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Result<Vec<ApplicationCall>, ClientError> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .map_err(|_| ClientError::Poisoned)
    }

    pub fn assert_exhausted(&self) -> Result<(), ClientError> {
        let script = self.script.lock().map_err(|_| ClientError::Poisoned)?;
        if let Some(step) = script.front() {
            return Err(ClientError::UnexpectedCall {
                expected: step.name(),
                actual: "end of script",
            });
        }
        Ok(())
    }

    fn next(&self, actual: &'static str) -> Result<ScriptStep, ClientError> {
        let mut script = self.script.lock().map_err(|_| ClientError::Poisoned)?;
        let step = script
            .pop_front()
            .ok_or(ClientError::ScriptExhausted(actual))?;
        if step.name() != actual {
            return Err(ClientError::UnexpectedCall {
                expected: step.name(),
                actual,
            });
        }
        Ok(step)
    }

    fn record(&self, call: ApplicationCall) -> Result<(), ClientError> {
        self.calls
            .lock()
            .map_err(|_| ClientError::Poisoned)?
            .push(call);
        Ok(())
    }
}

impl ApplicationClient for MockApplicationClient {
    fn connect(&self, request: ConnectRequest) -> Result<ConnectResponse, ClientError> {
        self.record(ApplicationCall::Connect(request.clone()))?;
        match self.next("connect")? {
            ScriptStep::Connect { expected, result } if expected == request => result,
            ScriptStep::Connect { .. } => Err(ClientError::ArgumentMismatch("connect")),
            _ => unreachable!("step name checked"),
        }
    }

    fn snapshot(&self) -> Result<Arc<RuntimeProjection>, ClientError> {
        self.record(ApplicationCall::Snapshot)?;
        match self.next("snapshot")? {
            ScriptStep::Snapshot { result } => result,
            _ => unreachable!("step name checked"),
        }
    }

    fn wait_for_projection(
        &self,
        after: ProjectionRevision,
        timeout: Duration,
    ) -> Result<Arc<RuntimeProjection>, ClientError> {
        self.record(ApplicationCall::WaitForProjection { after, timeout })?;
        match self.next("wait_for_projection")? {
            ScriptStep::WaitForProjection {
                expected_after,
                expected_timeout,
                result,
            } if expected_after == after && expected_timeout == timeout => result,
            ScriptStep::WaitForProjection { .. } => {
                Err(ClientError::ArgumentMismatch("wait_for_projection"))
            }
            _ => unreachable!("step name checked"),
        }
    }

    fn dispatch(&self, command: CommandEnvelope) -> Result<CommandReceipt, ClientError> {
        self.record(ApplicationCall::Dispatch(command.clone()))?;
        match self.next("dispatch")? {
            ScriptStep::Dispatch { expected, result } if expected == command => result,
            ScriptStep::Dispatch { .. } => Err(ClientError::ArgumentMismatch("dispatch")),
            _ => unreachable!("step name checked"),
        }
    }

    fn subscribe_telemetry(
        &self,
        subscription: TelemetrySubscription,
    ) -> Result<TelemetryLease, ClientError> {
        self.record(ApplicationCall::SubscribeTelemetry(subscription.clone()))?;
        match self.next("subscribe_telemetry")? {
            ScriptStep::SubscribeTelemetry { expected, result } if expected == subscription => {
                result
            }
            ScriptStep::SubscribeTelemetry { .. } => {
                Err(ClientError::ArgumentMismatch("subscribe_telemetry"))
            }
            _ => unreachable!("step name checked"),
        }
    }

    fn transcript_page(&self, after: u64, limit: u32) -> Result<TranscriptPage, ClientError> {
        self.record(ApplicationCall::TranscriptPage { after, limit })?;
        match self.next("transcript_page")? {
            ScriptStep::TranscriptPage {
                expected_after,
                expected_limit,
                result,
            } if expected_after == after && expected_limit == limit => result,
            ScriptStep::TranscriptPage { .. } => {
                Err(ClientError::ArgumentMismatch("transcript_page"))
            }
            _ => unreachable!("step name checked"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use magnolia_domain::{ClientId, DocumentRevision, RequestId, TargetGraphRevision};
    use magnolia_protocol::{
        CommandError, CommandErrorCode, ReceiptOutcome, RequestSequence, SemanticCommand,
        PROTOCOL_VERSION,
    };

    #[test]
    fn scripted_client_checks_order_arguments_and_records_calls() {
        let command = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(2),
            request_sequence: RequestSequence::new(1),
            expected_document_revision: DocumentRevision::ZERO,
            command: SemanticCommand::Undo,
        };
        let receipt = CommandReceipt {
            request_id: command.request_id,
            request_sequence: command.request_sequence,
            outcome: ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::NothingToUndo,
                    message: "nothing to undo".to_owned(),
                },
            },
            document_revision: DocumentRevision::ZERO,
            target_graph_revision: TargetGraphRevision::ZERO,
            operation_id: None,
        };
        let client = MockApplicationClient::scripted([ScriptStep::Dispatch {
            expected: command.clone(),
            result: Ok(receipt.clone()),
        }]);

        assert_eq!(client.dispatch(command.clone()).unwrap(), receipt);
        assert_eq!(
            client.calls().unwrap(),
            vec![ApplicationCall::Dispatch(command)]
        );
        assert_eq!(client.assert_exhausted(), Ok(()));
    }
}
