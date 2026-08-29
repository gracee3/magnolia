//! Portable Magnolia wire contracts.

use magnolia_domain::{
    ActiveGraphRevision, ClientId, ControlId, ControlKind, DeliveryPolicy, DocumentRevision,
    EntityId, ModuleTypeId, OperationId, ProjectionRevision, RequestId, RevisionOverflow,
    RuntimeEpochId, SchemaVersion, StreamTypeId, TargetGraphRevision, TranscriptRevision,
    WorkspaceDocument, WorkspaceEditBatch,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;
use uuid::Uuid;

mod transport;

pub use transport::*;

pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(1, 2);
pub const ASR_EVENT_SCHEMA_MAJOR: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersionRange {
    pub major: u16,
    pub minimum_minor: u16,
    pub maximum_minor: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestSequence(u64);

impl RequestSequence {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_next(self) -> Result<Self, RevisionOverflow> {
        self.0.checked_add(1).map(Self).ok_or(RevisionOverflow {
            kind: "request sequence",
            value: self.0,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectRequest {
    pub client_id: ClientId,
    pub supported_versions: Vec<ProtocolVersionRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConnectResponse {
    Accepted {
        negotiated_version: ProtocolVersion,
        snapshot: Box<RuntimeProjection>,
    },
    Rejected {
        error: HandshakeError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum HandshakeError {
    #[error("none of the requested protocol majors are supported")]
    UnsupportedMajor { supported_major: u16 },
    #[error("protocol major {major} has no compatible minor version")]
    NoCompatibleMinor { major: u16, supported_minor: u16 },
    #[error("at least one protocol range is required")]
    EmptyVersionSet,
    #[error("protocol range has minimum minor greater than maximum minor")]
    InvalidRange,
}

pub fn negotiate_protocol(
    ranges: &[ProtocolVersionRange],
) -> Result<ProtocolVersion, HandshakeError> {
    if ranges.is_empty() {
        return Err(HandshakeError::EmptyVersionSet);
    }
    if ranges
        .iter()
        .any(|range| range.minimum_minor > range.maximum_minor)
    {
        return Err(HandshakeError::InvalidRange);
    }
    let matching_major: Vec<_> = ranges
        .iter()
        .filter(|range| range.major == PROTOCOL_VERSION.major)
        .collect();
    if matching_major.is_empty() {
        return Err(HandshakeError::UnsupportedMajor {
            supported_major: PROTOCOL_VERSION.major,
        });
    }
    if let Some(minor) = matching_major
        .iter()
        .filter_map(|range| {
            let minor = PROTOCOL_VERSION.minor.min(range.maximum_minor);
            (minor >= range.minimum_minor).then_some(minor)
        })
        .max()
    {
        return Ok(ProtocolVersion::new(PROTOCOL_VERSION.major, minor));
    }
    Err(HandshakeError::NoCompatibleMinor {
        major: PROTOCOL_VERSION.major,
        supported_minor: PROTOCOL_VERSION.minor,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope {
    pub protocol_version: ProtocolVersion,
    pub client_id: ClientId,
    pub request_id: RequestId,
    pub request_sequence: RequestSequence,
    pub expected_document_revision: DocumentRevision,
    pub command: SemanticCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticCommand {
    ApplyWorkspaceEdit {
        batch: WorkspaceEditBatch,
    },
    SetControl {
        module_id: EntityId,
        control_id: ControlId,
        value: Value,
    },
    Undo,
    Redo,
    StartAudio,
    StopAudio,
    SetCaptureMuted {
        muted: bool,
    },
    SetMonitorEnabled {
        enabled: bool,
    },
    SetMonitorMuted {
        muted: bool,
    },
    SetMonitorGain {
        linear_millionths: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandReceipt {
    pub request_id: RequestId,
    pub request_sequence: RequestSequence,
    pub outcome: ReceiptOutcome,
    pub document_revision: DocumentRevision,
    pub target_graph_revision: TargetGraphRevision,
    pub operation_id: Option<OperationId>,
}

impl CommandReceipt {
    #[must_use]
    pub fn accepted(&self) -> bool {
        matches!(self.outcome, ReceiptOutcome::Accepted)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReceiptOutcome {
    Accepted,
    Rejected { error: CommandError },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandError {
    pub code: CommandErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandErrorCode {
    UnsupportedProtocolVersion,
    RevisionConflict,
    InvalidWorkspaceEdit,
    SequenceConflict,
    SequenceExpired,
    RequestIdConflict,
    NothingToUndo,
    NothingToRedo,
    PersistenceFailure,
    RevisionOverflow,
    UnknownControl,
    InvalidRuntimeControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeProjection {
    pub runtime_epoch: RuntimeEpochId,
    pub revision: ProjectionRevision,
    pub document_revision: DocumentRevision,
    pub target_graph_revision: TargetGraphRevision,
    pub active_graph_revision: ActiveGraphRevision,
    pub workspace: WorkspaceDocument,
    #[serde(default)]
    pub modules: Vec<ModuleStatus>,
    #[serde(default)]
    pub devices: Vec<DeviceStatus>,
    #[serde(default)]
    pub streams: Vec<StreamStatus>,
    #[serde(default)]
    pub operations: Vec<OperationStatus>,
    #[serde(default)]
    pub errors: Vec<RuntimeError>,
    #[serde(default)]
    pub control_manifests: BTreeMap<EntityId, Vec<ControlManifest>>,
    pub transcript: TranscriptSummary,
    pub diagnostics: DiagnosticsSummary,
    #[serde(default, skip_serializing_if = "AudioRuntimeProjection::is_empty")]
    pub audio: AudioRuntimeProjection,
    #[serde(default, skip_serializing_if = "AsrRuntimeProjection::is_empty")]
    pub asr: AsrRuntimeProjection,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsrRuntimeProjection {
    pub desired_running: bool,
    pub active: bool,
    pub session_id: Option<String>,
    pub provider: Option<String>,
    pub model_name: Option<String>,
    pub model_sha256: Option<String>,
    pub queue_depth: u32,
    pub skipped_frames: u64,
    pub discontinuities: u64,
    pub first_partial_latency_ms: Option<u64>,
    pub final_latency_ms: Option<u64>,
    pub real_time_factor_millionths: Option<u64>,
    pub last_error: Option<String>,
}

impl AsrRuntimeProjection {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioRuntimeProjection {
    pub desired_running: bool,
    pub capture_muted: bool,
    pub monitor_enabled: bool,
    pub monitor_muted: bool,
    pub monitor_gain_millionths: u32,
    pub input_selector_key: Option<String>,
    pub resolved_input_id: Option<String>,
    pub resolved_output_id: Option<String>,
    pub sample_format: Option<AudioSampleFormat>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channel_positions: Vec<String>,
    pub quantum_frames: Option<u32>,
    pub state: AudioRuntimeState,
    pub runtime_revision: u64,
    pub callback_count: u64,
    pub callback_p99_ns: u64,
    pub callback_p999_ns: u64,
    pub overruns: u64,
    pub underruns: u64,
    pub dropped_frames: u64,
    pub discontinuities: u64,
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_devices: Vec<AudioDeviceProjection>,
}

impl Default for AudioRuntimeProjection {
    fn default() -> Self {
        Self {
            desired_running: false,
            capture_muted: false,
            monitor_enabled: false,
            monitor_muted: true,
            monitor_gain_millionths: 0,
            input_selector_key: None,
            resolved_input_id: None,
            resolved_output_id: None,
            sample_format: None,
            sample_rate: None,
            channels: None,
            channel_positions: Vec::new(),
            quantum_frames: None,
            state: AudioRuntimeState::Stopped,
            runtime_revision: 0,
            callback_count: 0,
            callback_p99_ns: 0,
            callback_p999_ns: 0,
            overruns: 0,
            underruns: 0,
            dropped_frames: 0,
            discontinuities: 0,
            last_error: None,
            available_devices: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioDeviceProjection {
    pub runtime_id: String,
    pub label: String,
    pub direction: AudioDeviceDirection,
    pub node_name: String,
    pub device_api: String,
    pub object_path: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioDeviceDirection {
    Input,
    Output,
}

impl AudioRuntimeProjection {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRuntimeState {
    #[default]
    Stopped,
    Preparing,
    Running,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSampleFormat {
    F32Le,
    S16Le,
    S32Le,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleStatus {
    pub module_id: EntityId,
    pub module_type: ModuleTypeId,
    pub state: ModuleState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleState {
    Defined,
    Preparing,
    Active,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceStatus {
    pub device_id: EntityId,
    pub label: String,
    pub state: DeviceState,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Unresolved,
    Available,
    Active,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamStatus {
    pub stream_id: EntityId,
    pub stream_type: StreamTypeId,
    pub state: StreamState,
    pub cumulative_dropped: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamState {
    Inactive,
    Active,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationStatus {
    pub operation_id: OperationId,
    pub target_graph_revision: TargetGraphRevision,
    pub state: OperationState,
    pub error: Option<RuntimeError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Pending,
    Succeeded,
    Failed,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
    pub target_graph_revision: Option<TargetGraphRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlManifest {
    pub module_id: EntityId,
    pub control_id: ControlId,
    pub label: String,
    pub kind: ControlKind,
    pub value: Value,
    pub availability: ControlAvailability,
    pub disabled_reason: Option<String>,
    pub pending: bool,
    pub command: ControlCommandIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAvailability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlCommandIdentity {
    SetModuleControl {
        module_id: EntityId,
        control_id: ControlId,
    },
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSummary {
    pub revision: TranscriptRevision,
    pub final_segment_count: u64,
    #[serde(default)]
    pub recent: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSegment {
    pub session_id: EntityId,
    pub segment_id: EntityId,
    pub segment_revision: u64,
    pub sequence: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptPage {
    pub revision: TranscriptRevision,
    pub segments: Vec<TranscriptSegment>,
    pub next_cursor: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProvenance {
    pub provider: String,
    pub adapter_version: String,
    pub model_name: String,
    pub model_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WordAlignment {
    pub word: String,
    pub start_frame: u64,
    pub end_frame: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsrEventHeader {
    pub schema_major: u16,
    pub schema_minor: u16,
    pub session_id: Uuid,
    pub segment_id: Option<Uuid>,
    pub revision: u64,
    pub sequence: u64,
    pub runtime_monotonic_ns: u64,
    pub audio_start_frame: u64,
    pub audio_end_frame: u64,
    pub provenance: ModelProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AsrEventBody {
    SessionStart,
    PartialCreate {
        text: String,
    },
    PartialRevise {
        text: String,
    },
    AlignmentUpdate {
        words: Vec<WordAlignment>,
    },
    Final {
        text: String,
        words: Vec<WordAlignment>,
    },
    Warning {
        code: String,
        message: String,
    },
    Discontinuity {
        reason: DiscontinuityReason,
        lost_frames: u64,
    },
    Reset {
        reason: DiscontinuityReason,
    },
    SessionEnd {
        cancelled: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscontinuityReason {
    SourceLoss,
    Restart,
    Overflow,
    Recovery,
    Renegotiation,
    RecordedGap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsrEvent {
    pub header: AsrEventHeader,
    pub body: AsrEventBody,
}

impl AsrEvent {
    #[must_use]
    pub fn is_final(&self) -> bool {
        matches!(self.body, AsrEventBody::Final { .. })
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsSummary {
    #[serde(default)]
    pub counters: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetrySubscription {
    pub stream_id: EntityId,
    pub requested_rate_hz: u32,
    pub capacity: u32,
    pub delivery: DeliveryPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryLease {
    pub stream_id: EntityId,
    pub negotiated_rate_hz: u32,
    pub capacity: u32,
    pub delivery: DeliveryPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryEnvelope {
    pub protocol_version: ProtocolVersion,
    pub runtime_epoch: RuntimeEpochId,
    pub stream_id: EntityId,
    pub schema_version: SchemaVersion,
    pub clock: TelemetryClock,
    pub sequence: u64,
    pub source_start: u64,
    pub source_end: u64,
    pub emitted_monotonic_ns: u64,
    pub queue_depth: u32,
    pub cumulative_dropped: u64,
    pub discontinuity: bool,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryClock {
    AudioFrames,
    RuntimeMonotonic,
    External(String),
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("malformed JSON command: {0}")]
    Json(#[from] serde_json::Error),
    #[error("malformed postcard telemetry: {0}")]
    Postcard(#[from] postcard::Error),
}

pub fn decode_command_json(frame: &[u8]) -> Result<CommandEnvelope, FrameError> {
    Ok(serde_json::from_slice(frame)?)
}

pub fn encode_command_json(command: &CommandEnvelope) -> Result<Vec<u8>, FrameError> {
    Ok(serde_json::to_vec(command)?)
}

pub fn decode_telemetry_postcard(frame: &[u8]) -> Result<TelemetryEnvelope, FrameError> {
    Ok(postcard::from_bytes(frame)?)
}

pub fn encode_telemetry_postcard(telemetry: &TelemetryEnvelope) -> Result<Vec<u8>, FrameError> {
    Ok(postcard::to_allocvec(telemetry)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use magnolia_domain::{WorkspaceEdit, WorkspaceEditBatch};

    fn command() -> CommandEnvelope {
        CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(2),
            request_sequence: RequestSequence::new(7),
            expected_document_revision: DocumentRevision::new(3),
            command: SemanticCommand::ApplyWorkspaceEdit {
                batch: WorkspaceEditBatch::new(vec![WorkspaceEdit::RemovePromotedSetting {
                    key: "gain".to_owned(),
                }]),
            },
        }
    }

    fn telemetry() -> TelemetryEnvelope {
        TelemetryEnvelope {
            protocol_version: PROTOCOL_VERSION,
            runtime_epoch: RuntimeEpochId::from_u128(3),
            stream_id: EntityId::from_u128(4),
            schema_version: SchemaVersion::new(1, 0),
            clock: TelemetryClock::RuntimeMonotonic,
            sequence: 5,
            source_start: 10,
            source_end: 20,
            emitted_monotonic_ns: 30,
            queue_depth: 2,
            cumulative_dropped: 1,
            discontinuity: true,
            payload: vec![0xaa, 0xbb],
        }
    }

    #[test]
    fn negotiates_minor_and_rejects_unknown_major() {
        assert_eq!(
            negotiate_protocol(&[ProtocolVersionRange {
                major: 1,
                minimum_minor: 0,
                maximum_minor: 4,
            }]),
            Ok(PROTOCOL_VERSION)
        );
        assert!(matches!(
            negotiate_protocol(&[ProtocolVersionRange {
                major: 2,
                minimum_minor: 0,
                maximum_minor: 0,
            }]),
            Err(HandshakeError::UnsupportedMajor { .. })
        ));
    }

    #[test]
    fn malformed_and_unknown_json_fields_are_rejected() {
        assert!(decode_command_json(br#"{"protocol_version":1}"#).is_err());
        let mut value = serde_json::to_value(command()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("surprise".to_owned(), Value::Bool(true));
        assert!(decode_command_json(&serde_json::to_vec(&value).unwrap()).is_err());

        let mut nested = serde_json::to_value(command()).unwrap();
        nested["command"]
            .as_object_mut()
            .unwrap()
            .insert("surprise".to_owned(), Value::Bool(true));
        assert!(decode_command_json(&serde_json::to_vec(&nested).unwrap()).is_err());
    }

    #[test]
    fn command_json_matches_golden_fixture() {
        let actual = serde_json::to_string_pretty(&command()).unwrap();
        assert_eq!(
            actual.trim(),
            include_str!("../tests/fixtures/command-v1.json").trim()
        );
        assert_eq!(decode_command_json(actual.as_bytes()).unwrap(), command());
    }

    #[test]
    fn telemetry_postcard_matches_golden_fixture() {
        let encoded = encode_telemetry_postcard(&telemetry()).unwrap();
        assert_eq!(
            hex::encode(&encoded),
            include_str!("../tests/fixtures/telemetry-v1.hex").trim()
        );
        assert_eq!(decode_telemetry_postcard(&encoded).unwrap(), telemetry());
        assert!(decode_telemetry_postcard(&encoded[..encoded.len() - 1]).is_err());
    }
}
