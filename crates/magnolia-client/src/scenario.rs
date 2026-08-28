use crate::{ApplicationClient, ClientError};
use magnolia_domain::{ClientId, RequestId, WorkspaceEditBatch};
use magnolia_protocol::{
    CommandEnvelope, CommandError, CommandReceipt, ConnectRequest, ConnectResponse,
    ProtocolVersionRange, RequestSequence, RuntimeProjection, SemanticCommand, PROTOCOL_VERSION,
};
use std::sync::Arc;
use thiserror::Error;

/// Result of the adapter-neutral connect/edit/retry foundation scenario.
#[derive(Debug, Clone)]
pub struct FoundationScenarioResult {
    pub initial: RuntimeProjection,
    pub receipt: CommandReceipt,
    pub after_dispatch: Arc<RuntimeProjection>,
}

/// Runs the portable portion of the foundation scenario against any client adapter.
pub async fn run_foundation_edit_scenario<C: ApplicationClient>(
    client: &C,
    client_id: ClientId,
    request_id: RequestId,
    batch: WorkspaceEditBatch,
) -> Result<FoundationScenarioResult, FoundationScenarioError> {
    let initial = match client
        .connect(ConnectRequest {
            client_id,
            supported_versions: vec![ProtocolVersionRange {
                major: PROTOCOL_VERSION.major,
                minimum_minor: PROTOCOL_VERSION.minor,
                maximum_minor: PROTOCOL_VERSION.minor,
            }],
        })
        .await?
    {
        ConnectResponse::Accepted {
            negotiated_version,
            snapshot,
        } if negotiated_version == PROTOCOL_VERSION => *snapshot,
        ConnectResponse::Accepted {
            negotiated_version, ..
        } => {
            return Err(FoundationScenarioError::UnexpectedNegotiatedVersion {
                major: negotiated_version.major,
                minor: negotiated_version.minor,
            });
        }
        ConnectResponse::Rejected { error } => {
            return Err(FoundationScenarioError::HandshakeRejected(
                error.to_string(),
            ));
        }
    };
    let envelope = CommandEnvelope {
        protocol_version: PROTOCOL_VERSION,
        client_id,
        request_id,
        request_sequence: RequestSequence::new(1),
        expected_document_revision: initial.document_revision,
        command: SemanticCommand::ApplyWorkspaceEdit { batch },
    };
    let receipt = client.dispatch(envelope.clone()).await?;
    if let magnolia_protocol::ReceiptOutcome::Rejected { error } = &receipt.outcome {
        return Err(FoundationScenarioError::CommandRejected(error.clone()));
    }
    let retried = client.dispatch(envelope).await?;
    if retried != receipt {
        return Err(FoundationScenarioError::RetryMismatch);
    }
    let after_dispatch = client.snapshot().await?;
    if after_dispatch.document_revision != receipt.document_revision
        || after_dispatch.target_graph_revision != receipt.target_graph_revision
    {
        return Err(FoundationScenarioError::ProjectionMismatch);
    }
    Ok(FoundationScenarioResult {
        initial,
        receipt,
        after_dispatch,
    })
}

#[derive(Debug, Error)]
pub enum FoundationScenarioError {
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("handshake rejected: {0}")]
    HandshakeRejected(String),
    #[error("adapter negotiated unexpected protocol version {major}.{minor}")]
    UnexpectedNegotiatedVersion { major: u16, minor: u16 },
    #[error("foundation edit command was rejected: {0:?}")]
    CommandRejected(CommandError),
    #[error("in-window retry did not return the original receipt")]
    RetryMismatch,
    #[error("authoritative projection does not match the accepted receipt")]
    ProjectionMismatch,
}
