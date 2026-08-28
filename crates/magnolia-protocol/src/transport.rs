//! Transport-neutral messages used by the loopback desktop shell.

use crate::{
    CommandEnvelope, CommandReceipt, ConnectRequest, ConnectResponse, ProtocolVersion,
    RuntimeProjection, TelemetryEnvelope, TelemetryLease, TelemetrySubscription, TranscriptPage,
};
use magnolia_domain::{EntityId, ProjectionRevision, RequestId, RuntimeEpochId};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const SYNTHETIC_METER_STREAM_ID: EntityId = EntityId::from_u128(0x2_001);
pub const SYNTHETIC_WAVEFORM_STREAM_ID: EntityId = EntityId::from_u128(0x2_002);
pub const SYNTHETIC_SPECTRUM_STREAM_ID: EntityId = EntityId::from_u128(0x2_003);
pub const SYNTHETIC_DIAGNOSTICS_STREAM_ID: EntityId = EntityId::from_u128(0x2_004);
pub const SYNTHETIC_CAPTION_STREAM_ID: EntityId = EntityId::from_u128(0x2_005);
pub const SYNTHETIC_CAPTION_SESSION_ID: EntityId = EntityId::from_u128(0x2_100);

/// Authentication material carried only in the first message on a connection.
///
/// The launch token is exchanged once for a resumable, process-local session
/// identifier. The token exists in the initial URL fragment only; neither
/// value belongs in HTTP request targets, logs, projections, or durable
/// workspace state.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SessionCredential {
    LaunchToken(String),
    SessionId(String),
}

impl SessionCredential {
    #[must_use]
    pub fn expose(&self) -> &str {
        match self {
            Self::LaunchToken(value) | Self::SessionId(value) => value,
        }
    }
}

impl fmt::Debug for SessionCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LaunchToken(_) => formatter.write_str("LaunchToken([redacted])"),
            Self::SessionId(_) => formatter.write_str("SessionId([redacted])"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectCursor {
    pub runtime_epoch: Option<RuntimeEpochId>,
    pub projection_revision: ProjectionRevision,
    pub transcript_after: u64,
}

impl Default for ReconnectCursor {
    fn default() -> Self {
        Self {
            runtime_epoch: None,
            projection_revision: ProjectionRevision::ZERO,
            transcript_after: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlClientMessage {
    Authenticate {
        credential: SessionCredential,
        connect: ConnectRequest,
        cursor: ReconnectCursor,
    },
    Command {
        command: CommandEnvelope,
    },
    SubscribeTelemetry {
        request_id: RequestId,
        subscription: TelemetrySubscription,
    },
    ReleaseTelemetry {
        request_id: RequestId,
        stream_id: EntityId,
    },
    TranscriptPage {
        request_id: RequestId,
        after: u64,
        limit: u32,
    },
    Ping {
        nonce: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlServerMessage {
    Connected {
        session_id: String,
        resumed: bool,
        response: ConnectResponse,
        transcript: TranscriptPage,
    },
    Receipt {
        receipt: CommandReceipt,
    },
    Projection {
        projection: Box<RuntimeProjection>,
    },
    TelemetryLease {
        request_id: RequestId,
        lease: TelemetryLease,
    },
    TelemetryReleased {
        request_id: RequestId,
        stream_id: EntityId,
    },
    TranscriptPage {
        request_id: RequestId,
        page: TranscriptPage,
    },
    Error {
        request_id: Option<RequestId>,
        code: TransportErrorCode,
        message: String,
        fatal: bool,
    },
    Pong {
        nonce: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportErrorCode {
    AuthenticationRequired,
    InvalidCredential,
    ExpiredCredential,
    OriginRejected,
    MalformedMessage,
    ProtocolRejected,
    NotConnected,
    RequestConflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TelemetryClientMessage {
    Authenticate {
        session_id: String,
        protocol_version: ProtocolVersion,
        runtime_epoch: Option<RuntimeEpochId>,
    },
    Ping {
        nonce: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TelemetryServerMessage {
    Ready {
        runtime_epoch: RuntimeEpochId,
    },
    Error {
        code: TransportErrorCode,
        message: String,
        fatal: bool,
    },
    Pong {
        nonce: u64,
    },
}

/// Payload carried inside [`TelemetryEnvelope::payload`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntheticTelemetryPayload {
    Meter {
        level_milli: u16,
        peak_milli: u16,
    },
    Waveform {
        samples: Vec<i16>,
    },
    Spectrum {
        bins: Vec<u16>,
    },
    Diagnostics {
        entries: Vec<DiagnosticTelemetryEntry>,
        lost_since_previous: u64,
    },
    PartialCaption {
        segment_id: EntityId,
        segment_revision: u64,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticTelemetryEntry {
    pub sequence: u64,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

pub fn encode_synthetic_payload(
    payload: &SyntheticTelemetryPayload,
) -> Result<Vec<u8>, crate::FrameError> {
    Ok(postcard::to_allocvec(payload)?)
}

pub fn decode_synthetic_payload(
    envelope: &TelemetryEnvelope,
) -> Result<SyntheticTelemetryPayload, crate::FrameError> {
    Ok(postcard::from_bytes(&envelope.payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProtocolVersionRange, PROTOCOL_VERSION};
    use magnolia_domain::{ClientId, TranscriptRevision};

    #[test]
    fn credentials_are_redacted_from_debug_output() {
        let credential = SessionCredential::LaunchToken("secret-value".to_owned());
        let output = format!("{credential:?}");
        assert!(!output.contains("secret-value"));
        assert!(output.contains("redacted"));
    }

    #[test]
    fn control_handshake_round_trips_without_transport_dependencies() {
        let message = ControlClientMessage::Authenticate {
            credential: SessionCredential::SessionId("session".to_owned()),
            connect: ConnectRequest {
                client_id: ClientId::from_u128(1),
                supported_versions: vec![ProtocolVersionRange {
                    major: PROTOCOL_VERSION.major,
                    minimum_minor: PROTOCOL_VERSION.minor,
                    maximum_minor: PROTOCOL_VERSION.minor,
                }],
            },
            cursor: ReconnectCursor::default(),
        };
        let json = serde_json::to_vec(&message).unwrap();
        assert_eq!(
            serde_json::from_slice::<ControlClientMessage>(&json).unwrap(),
            message
        );

        let page = TranscriptPage {
            revision: TranscriptRevision::ZERO,
            segments: Vec::new(),
            next_cursor: None,
        };
        assert!(page.segments.is_empty());
    }

    #[test]
    fn synthetic_payload_round_trips_through_postcard() {
        let payload = SyntheticTelemetryPayload::Waveform {
            samples: vec![-32, 0, 32],
        };
        let encoded = encode_synthetic_payload(&payload).unwrap();
        let envelope = TelemetryEnvelope {
            protocol_version: PROTOCOL_VERSION,
            runtime_epoch: RuntimeEpochId::from_u128(2),
            stream_id: EntityId::from_u128(3),
            schema_version: magnolia_domain::SchemaVersion::new(1, 0),
            clock: crate::TelemetryClock::RuntimeMonotonic,
            sequence: 1,
            source_start: 0,
            source_end: 1,
            emitted_monotonic_ns: 10,
            queue_depth: 0,
            cumulative_dropped: 0,
            discontinuity: false,
            payload: encoded,
        };
        assert_eq!(decode_synthetic_payload(&envelope).unwrap(), payload);
    }
}
