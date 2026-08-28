use crate::{ApplicationService, PersistencePort, RuntimePort};
use magnolia_client::{ApplicationClient, ClientError};
use magnolia_domain::{ProjectionRevision, TranscriptRevision};
use magnolia_protocol::{
    CommandEnvelope, CommandReceipt, ConnectRequest, ConnectResponse, RuntimeProjection,
    TelemetryLease, TelemetrySubscription, TranscriptPage,
};
use std::{sync::Arc, time::Duration};

pub struct InProcessApplicationClient<P: PersistencePort, R: RuntimePort> {
    service: ApplicationService<P, R>,
}

impl<P: PersistencePort, R: RuntimePort> Clone for InProcessApplicationClient<P, R> {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
        }
    }
}

impl<P: PersistencePort, R: RuntimePort> InProcessApplicationClient<P, R> {
    #[must_use]
    pub fn new(service: ApplicationService<P, R>) -> Self {
        Self { service }
    }

    #[must_use]
    pub fn service(&self) -> &ApplicationService<P, R> {
        &self.service
    }
}

impl<P: PersistencePort, R: RuntimePort> ApplicationClient for InProcessApplicationClient<P, R> {
    fn connect(&self, request: ConnectRequest) -> Result<ConnectResponse, ClientError> {
        self.service
            .connect(request)
            .map_err(|error| ClientError::Service(error.to_string()))
    }

    fn snapshot(&self) -> Result<Arc<RuntimeProjection>, ClientError> {
        self.service
            .snapshot_arc()
            .map_err(|error| ClientError::Service(error.to_string()))
    }

    fn wait_for_projection(
        &self,
        after: ProjectionRevision,
        timeout: Duration,
    ) -> Result<Arc<RuntimeProjection>, ClientError> {
        self.service
            .wait_for_projection(after, timeout)
            .map_err(|error| {
                if matches!(error, crate::ApplicationError::Timeout) {
                    ClientError::Timeout
                } else {
                    ClientError::Service(error.to_string())
                }
            })
    }

    fn dispatch(&self, command: CommandEnvelope) -> Result<CommandReceipt, ClientError> {
        self.service
            .dispatch(command)
            .map_err(|error| ClientError::Service(error.to_string()))
    }

    fn subscribe_telemetry(
        &self,
        _subscription: TelemetrySubscription,
    ) -> Result<TelemetryLease, ClientError> {
        Err(ClientError::Unsupported(
            "telemetry is deferred until the observation phase",
        ))
    }

    fn transcript_page(&self, _after: u64, _limit: u32) -> Result<TranscriptPage, ClientError> {
        Ok(TranscriptPage {
            revision: TranscriptRevision::ZERO,
            segments: Vec::new(),
            next_cursor: None,
        })
    }
}
