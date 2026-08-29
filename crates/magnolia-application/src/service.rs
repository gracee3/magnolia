use crate::{
    ActivationRequest, PersistenceError, PersistencePort, RuntimeControl, RuntimeEvent, RuntimePort,
};
use event_listener::Event;
use magnolia_domain::{
    ActiveGraphRevision, ClientId, ControlKind, DescriptorRegistry, EntityId, OperationId,
    ProjectionRevision, RequestId, RuntimeEpochId, TargetGraphRevision, TranscriptRevision,
    WorkspaceDocument, WorkspaceEdit, WorkspaceEditBatch,
};
use magnolia_protocol::{
    negotiate_protocol, AudioRuntimeProjection, CommandEnvelope, CommandError, CommandErrorCode,
    CommandReceipt, ConnectRequest, ConnectResponse, ControlAvailability, ControlCommandIdentity,
    ControlManifest, DiagnosticsSummary, ModuleState, ModuleStatus, OperationState,
    OperationStatus, ProtocolVersion, ReceiptOutcome, RequestSequence, RuntimeError,
    RuntimeProjection, SemanticCommand, TranscriptPage, TranscriptSegment, TranscriptSummary,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};
use thiserror::Error;

const RECEIPT_WINDOW: usize = 1_024;

pub struct ApplicationService<P: PersistencePort, R: RuntimePort> {
    shared: Arc<Shared<P, R>>,
}

impl<P: PersistencePort, R: RuntimePort> Clone for ApplicationService<P, R> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

struct Shared<P: PersistencePort, R: RuntimePort> {
    inner: Mutex<Inner<P, R>>,
    projection_changed: Event,
}

struct Inner<P: PersistencePort, R: RuntimePort> {
    persistence: P,
    runtime: R,
    registry: DescriptorRegistry,
    epoch: RuntimeEpochId,
    document: WorkspaceDocument,
    target_revision: TargetGraphRevision,
    active_revision: ActiveGraphRevision,
    projection_revision: ProjectionRevision,
    operations: BTreeMap<OperationId, OperationStatus>,
    errors: Vec<RuntimeError>,
    transcript_revision: TranscriptRevision,
    transcript_segments: Vec<TranscriptSegment>,
    diagnostics: BTreeMap<String, u64>,
    undo: Vec<WorkspaceDocument>,
    redo: Vec<WorkspaceDocument>,
    clients: BTreeMap<ClientId, ClientLedger>,
    projection: Arc<RuntimeProjection>,
    audio: AudioRuntimeProjection,
}

#[derive(Default)]
struct ClientLedger {
    negotiated_version: Option<ProtocolVersion>,
    max_sequence: Option<RequestSequence>,
    receipts: BTreeMap<u64, CachedReceipt>,
    request_ids: BTreeMap<RequestId, u64>,
    insertion_order: VecDeque<u64>,
}

#[derive(Clone)]
struct CachedReceipt {
    envelope: CommandEnvelope,
    receipt: CommandReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpReport {
    pub handled: usize,
    pub ignored_stale: usize,
}

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("application lock was poisoned")]
    Poisoned,
    #[error("initial workspace is invalid: {0}")]
    InvalidInitialWorkspace(String),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("revision overflow: {0}")]
    RevisionOverflow(String),
    #[error("invalid observation update: {0}")]
    InvalidObservation(String),
}

impl<P: PersistencePort, R: RuntimePort> ApplicationService<P, R> {
    pub fn new(
        mut persistence: P,
        runtime: R,
        registry: DescriptorRegistry,
        epoch: RuntimeEpochId,
    ) -> Result<Self, ApplicationError> {
        let document = persistence.load()?.unwrap_or_default();
        document
            .validate(&registry)
            .map_err(|error| ApplicationError::InvalidInitialWorkspace(error.to_string()))?;
        let initial = RuntimeProjection {
            runtime_epoch: epoch,
            revision: ProjectionRevision::ZERO,
            document_revision: document.revision,
            target_graph_revision: TargetGraphRevision::ZERO,
            active_graph_revision: ActiveGraphRevision::ZERO,
            workspace: document.clone(),
            modules: document
                .graph
                .modules
                .values()
                .map(|module| ModuleStatus {
                    module_id: module.id,
                    module_type: module.module_type.clone(),
                    state: ModuleState::Defined,
                })
                .collect(),
            devices: Vec::new(),
            streams: Vec::new(),
            operations: Vec::new(),
            errors: Vec::new(),
            control_manifests: materialize_controls(&document, &registry, false),
            transcript: TranscriptSummary::default(),
            diagnostics: DiagnosticsSummary::default(),
            audio: AudioRuntimeProjection::default(),
            asr: Default::default(),
        };
        Ok(Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    persistence,
                    runtime,
                    registry,
                    epoch,
                    document,
                    target_revision: TargetGraphRevision::ZERO,
                    active_revision: ActiveGraphRevision::ZERO,
                    projection_revision: ProjectionRevision::ZERO,
                    operations: BTreeMap::new(),
                    errors: Vec::new(),
                    transcript_revision: TranscriptRevision::ZERO,
                    transcript_segments: Vec::new(),
                    diagnostics: BTreeMap::new(),
                    undo: Vec::new(),
                    redo: Vec::new(),
                    clients: BTreeMap::new(),
                    projection: Arc::new(initial),
                    audio: AudioRuntimeProjection::default(),
                }),
                projection_changed: Event::new(),
            }),
        })
    }

    pub fn connect(&self, request: ConnectRequest) -> Result<ConnectResponse, ApplicationError> {
        match negotiate_protocol(&request.supported_versions) {
            Ok(version) => {
                let mut inner = self.lock()?;
                inner
                    .clients
                    .entry(request.client_id)
                    .or_default()
                    .negotiated_version = Some(version);
                Ok(ConnectResponse::Accepted {
                    negotiated_version: version,
                    snapshot: Box::new((*inner.projection).clone()),
                })
            }
            Err(error) => Ok(ConnectResponse::Rejected { error }),
        }
    }

    pub fn snapshot(&self) -> Result<RuntimeProjection, ApplicationError> {
        Ok((*self.lock()?.projection).clone())
    }

    pub fn snapshot_arc(&self) -> Result<Arc<RuntimeProjection>, ApplicationError> {
        Ok(Arc::clone(&self.lock()?.projection))
    }

    pub async fn wait_for_projection(
        &self,
        after: ProjectionRevision,
    ) -> Result<Arc<RuntimeProjection>, ApplicationError> {
        loop {
            let listener = self.shared.projection_changed.listen();
            let projection = self.snapshot_arc()?;
            if projection.revision > after {
                return Ok(projection);
            }
            listener.await;
        }
    }

    pub fn dispatch(&self, envelope: CommandEnvelope) -> Result<CommandReceipt, ApplicationError> {
        let mut inner = self.lock()?;
        if let Some((receipt, consume_sequence)) = replay_or_reject(&inner, &envelope) {
            if consume_sequence {
                cache_receipt(&mut inner, envelope, receipt.clone());
            }
            return Ok(receipt);
        }

        let receipt = process_new_command(&mut inner, &envelope)?;
        cache_receipt(&mut inner, envelope, receipt.clone());
        if receipt.accepted() {
            self.shared.projection_changed.notify(usize::MAX);
        }
        Ok(receipt)
    }

    pub fn pump_runtime_events(&self) -> Result<PumpReport, ApplicationError> {
        let mut inner = self.lock()?;
        let mut report = PumpReport {
            handled: 0,
            ignored_stale: 0,
        };
        while let Some(event) = inner.runtime.poll_event() {
            if let RuntimeEvent::AudioProjection(audio) = event {
                inner.audio = audio;
                publish(&mut inner)?;
                report.handled += 1;
                continue;
            }
            let (operation_id, target_revision) = match &event {
                RuntimeEvent::ActivationSucceeded {
                    operation_id,
                    target_graph_revision,
                }
                | RuntimeEvent::ActivationFailed {
                    operation_id,
                    target_graph_revision,
                    ..
                } => (*operation_id, *target_graph_revision),
                RuntimeEvent::AudioProjection(_) => unreachable!("handled above"),
            };
            let is_current = target_revision == inner.target_revision
                && inner
                    .operations
                    .get(&operation_id)
                    .is_some_and(|operation| operation.state == OperationState::Pending);
            if !is_current {
                report.ignored_stale += 1;
                continue;
            }
            inner
                .projection_revision
                .checked_next()
                .map_err(|error| ApplicationError::RevisionOverflow(error.to_string()))?;

            match event {
                RuntimeEvent::ActivationSucceeded { .. } => {
                    inner.active_revision = ActiveGraphRevision::new(target_revision.get());
                    if let Some(operation) = inner.operations.get_mut(&operation_id) {
                        operation.state = OperationState::Succeeded;
                    }
                }
                RuntimeEvent::ActivationFailed { code, message, .. } => {
                    let error = RuntimeError {
                        code,
                        message,
                        target_graph_revision: Some(target_revision),
                    };
                    if let Some(operation) = inner.operations.get_mut(&operation_id) {
                        operation.state = OperationState::Failed;
                        operation.error = Some(error.clone());
                    }
                    inner.errors.push(error);
                }
                RuntimeEvent::AudioProjection(_) => unreachable!("handled above"),
            }
            publish(&mut inner)?;
            report.handled += 1;
        }
        if report.handled > 0 {
            self.shared.projection_changed.notify(usize::MAX);
        }
        Ok(report)
    }

    /// Append an authoritative final transcript segment to the process-local journal.
    ///
    /// Synthetic Phase 2 producers use the same application-owned path that later
    /// native ASR adapters will use. Partials remain telemetry and never enter the
    /// final journal.
    pub fn append_transcript(
        &self,
        segment: TranscriptSegment,
    ) -> Result<TranscriptRevision, ApplicationError> {
        let mut inner = self.lock()?;
        let next = inner
            .transcript_revision
            .checked_next()
            .map_err(|error| ApplicationError::RevisionOverflow(error.to_string()))?;
        if inner
            .transcript_segments
            .last()
            .is_some_and(|previous| segment.sequence <= previous.sequence)
        {
            return Err(ApplicationError::InvalidObservation(
                "final transcript sequences must increase monotonically".to_owned(),
            ));
        }
        inner.transcript_revision = next;
        inner.transcript_segments.push(segment);
        publish(&mut inner)?;
        drop(inner);
        self.shared.projection_changed.notify(usize::MAX);
        Ok(next)
    }

    pub fn transcript_page(
        &self,
        after: u64,
        limit: u32,
    ) -> Result<TranscriptPage, ApplicationError> {
        let inner = self.lock()?;
        let limit = usize::try_from(limit.clamp(1, 256)).unwrap_or(256);
        let mut matching = inner
            .transcript_segments
            .iter()
            .filter(|segment| segment.sequence > after);
        let segments: Vec<_> = matching.by_ref().take(limit).cloned().collect();
        let has_more = matching.next().is_some();
        let next_cursor = if has_more {
            segments.last().map(|segment| segment.sequence)
        } else {
            None
        };
        Ok(TranscriptPage {
            revision: inner.transcript_revision,
            segments,
            next_cursor,
        })
    }

    /// Replace a cumulative observation counter and publish only when it changes.
    pub fn set_diagnostic_counter(
        &self,
        name: impl Into<String>,
        value: u64,
    ) -> Result<bool, ApplicationError> {
        self.set_diagnostic_counters([(name.into(), value)])
    }

    /// Atomically replace a group of cumulative observation counters.
    ///
    /// Telemetry health is projected at a deliberately low cadence; publishing
    /// one immutable snapshot for the batch keeps observation traffic from
    /// crowding command receipts or document projections.
    pub fn set_diagnostic_counters(
        &self,
        counters: impl IntoIterator<Item = (String, u64)>,
    ) -> Result<bool, ApplicationError> {
        let counters: Vec<_> = counters.into_iter().collect();
        if counters.iter().any(|(name, _)| name.trim().is_empty()) {
            return Err(ApplicationError::InvalidObservation(
                "diagnostic counter name must not be blank".to_owned(),
            ));
        }
        let mut inner = self.lock()?;
        let changed = counters
            .iter()
            .any(|(name, value)| inner.diagnostics.get(name) != Some(value));
        if !changed {
            return Ok(false);
        }
        inner.diagnostics.extend(counters);
        publish(&mut inner)?;
        drop(inner);
        self.shared.projection_changed.notify(usize::MAX);
        Ok(true)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner<P, R>>, ApplicationError> {
        self.shared
            .inner
            .lock()
            .map_err(|_| ApplicationError::Poisoned)
    }
}

fn replay_or_reject<P: PersistencePort, R: RuntimePort>(
    inner: &Inner<P, R>,
    envelope: &CommandEnvelope,
) -> Option<(CommandReceipt, bool)> {
    let ledger = inner.clients.get(&envelope.client_id)?;
    let sequence = envelope.request_sequence.get();
    if let Some(cached) = ledger.receipts.get(&sequence) {
        if &cached.envelope == envelope {
            return Some((cached.receipt.clone(), false));
        }
        return Some((
            rejected_receipt(
                inner,
                envelope,
                CommandErrorCode::SequenceConflict,
                "request sequence conflicts with a cached command",
            ),
            false,
        ));
    }
    if ledger.request_ids.contains_key(&envelope.request_id) {
        return Some((
            rejected_receipt(
                inner,
                envelope,
                CommandErrorCode::RequestIdConflict,
                "request ID was already used with a different sequence",
            ),
            true,
        ));
    }
    if ledger
        .max_sequence
        .is_some_and(|maximum| envelope.request_sequence <= maximum)
    {
        return Some((
            rejected_receipt(
                inner,
                envelope,
                CommandErrorCode::SequenceExpired,
                "request sequence is older than the retained receipt window",
            ),
            false,
        ));
    }
    None
}

fn process_new_command<P: PersistencePort, R: RuntimePort>(
    inner: &mut Inner<P, R>,
    envelope: &CommandEnvelope,
) -> Result<CommandReceipt, ApplicationError> {
    let negotiated_version = inner
        .clients
        .get(&envelope.client_id)
        .and_then(|ledger| ledger.negotiated_version);
    if negotiated_version != Some(envelope.protocol_version) {
        return Ok(rejected_receipt(
            inner,
            envelope,
            CommandErrorCode::UnsupportedProtocolVersion,
            "command protocol version was not negotiated",
        ));
    }
    if envelope.expected_document_revision != inner.document.revision {
        return Ok(rejected_receipt(
            inner,
            envelope,
            CommandErrorCode::RevisionConflict,
            "expected document revision does not match authoritative revision",
        ));
    }

    let runtime_control = match envelope.command {
        SemanticCommand::StartAudio => Some(RuntimeControl::StartAudio),
        SemanticCommand::StopAudio => Some(RuntimeControl::StopAudio),
        SemanticCommand::SetCaptureMuted { muted } => Some(RuntimeControl::SetCaptureMuted(muted)),
        SemanticCommand::SetMonitorEnabled { enabled } => {
            Some(RuntimeControl::SetMonitorEnabled(enabled))
        }
        SemanticCommand::SetMonitorMuted { muted } => Some(RuntimeControl::SetMonitorMuted(muted)),
        SemanticCommand::SetMonitorGain { linear_millionths } => {
            if linear_millionths > 1_000_000 {
                return Ok(rejected_receipt(
                    inner,
                    envelope,
                    CommandErrorCode::InvalidRuntimeControl,
                    "monitor gain must be between zero and one million millionths",
                ));
            }
            Some(RuntimeControl::SetMonitorGain(linear_millionths))
        }
        _ => None,
    };
    if let Some(control) = runtime_control {
        inner.runtime.enqueue_control(control);
        publish(inner)?;
        return Ok(CommandReceipt {
            request_id: envelope.request_id,
            request_sequence: envelope.request_sequence,
            outcome: ReceiptOutcome::Accepted,
            document_revision: inner.document.revision,
            target_graph_revision: inner.target_revision,
            operation_id: None,
        });
    }

    let (mut candidate, history) = match prepare_candidate(inner, &envelope.command) {
        Ok(candidate) => candidate,
        Err((code, message)) => return Ok(rejected_receipt(inner, envelope, code, message)),
    };
    let runtime_changed = candidate.graph != inner.document.graph
        || candidate.device_selectors != inner.document.device_selectors;
    let next_document = inner
        .document
        .revision
        .checked_next()
        .map_err(|error| ApplicationError::RevisionOverflow(error.to_string()))?;
    let next_target = runtime_changed
        .then(|| inner.target_revision.checked_next())
        .transpose()
        .map_err(|error| ApplicationError::RevisionOverflow(error.to_string()))?;
    inner
        .projection_revision
        .checked_next()
        .map_err(|error| ApplicationError::RevisionOverflow(error.to_string()))?;
    candidate.revision = next_document;
    if let Err(error) = inner.persistence.save(&candidate) {
        return Ok(rejected_receipt(
            inner,
            envelope,
            CommandErrorCode::PersistenceFailure,
            error.message,
        ));
    }

    match history {
        HistoryChange::Apply => {
            inner.undo.push(inner.document.clone());
            inner.redo.clear();
        }
        HistoryChange::Undo => {
            inner.undo.pop();
            inner.redo.push(inner.document.clone());
        }
        HistoryChange::Redo => {
            inner.redo.pop();
            inner.undo.push(inner.document.clone());
        }
    }
    inner.document = candidate;
    let operation_id = if let Some(next_target) = next_target {
        for operation in inner.operations.values_mut() {
            if operation.state == OperationState::Pending {
                operation.state = OperationState::Superseded;
            }
        }
        inner.target_revision = next_target;
        let operation_id = OperationId::new();
        inner.operations.insert(
            operation_id,
            OperationStatus {
                operation_id,
                target_graph_revision: next_target,
                state: OperationState::Pending,
                error: None,
            },
        );
        inner.runtime.enqueue_activation(ActivationRequest {
            operation_id,
            target_graph_revision: next_target,
            graph: inner.document.graph.clone(),
            device_selectors: inner.document.device_selectors.clone(),
        });
        Some(operation_id)
    } else {
        None
    };
    publish(inner)?;

    Ok(CommandReceipt {
        request_id: envelope.request_id,
        request_sequence: envelope.request_sequence,
        outcome: ReceiptOutcome::Accepted,
        document_revision: next_document,
        target_graph_revision: inner.target_revision,
        operation_id,
    })
}

enum HistoryChange {
    Apply,
    Undo,
    Redo,
}

fn prepare_candidate<P: PersistencePort, R: RuntimePort>(
    inner: &Inner<P, R>,
    command: &SemanticCommand,
) -> Result<(WorkspaceDocument, HistoryChange), (CommandErrorCode, String)> {
    match command {
        SemanticCommand::ApplyWorkspaceEdit { batch } => inner
            .document
            .apply(batch, &inner.registry)
            .map(|document| (document, HistoryChange::Apply))
            .map_err(|error| (CommandErrorCode::InvalidWorkspaceEdit, error.to_string())),
        SemanticCommand::SetControl {
            module_id,
            control_id,
            value,
        } => {
            let instance = inner.document.graph.modules.get(module_id).ok_or_else(|| {
                (
                    CommandErrorCode::UnknownControl,
                    format!("module {module_id} does not exist"),
                )
            })?;
            let descriptor = inner.registry.get(&instance.module_type).ok_or_else(|| {
                (
                    CommandErrorCode::UnknownControl,
                    "module descriptor is unavailable".to_owned(),
                )
            })?;
            let control = descriptor
                .controls
                .iter()
                .find(|control| &control.id == control_id)
                .ok_or_else(|| {
                    (
                        CommandErrorCode::UnknownControl,
                        format!("control {control_id} does not exist"),
                    )
                })?;
            if !control_value_is_valid(&control.kind, value) {
                return Err((
                    CommandErrorCode::InvalidWorkspaceEdit,
                    format!("value is incompatible with control {control_id}"),
                ));
            }
            let mut configuration =
                instance.configuration.as_object().cloned().ok_or_else(|| {
                    (
                        CommandErrorCode::InvalidWorkspaceEdit,
                        format!("module {module_id} configuration must be an object"),
                    )
                })?;
            configuration.insert(control.setting_key.clone(), value.clone());
            inner
                .document
                .apply(
                    &WorkspaceEditBatch::new(vec![WorkspaceEdit::SetModuleConfiguration {
                        module_id: *module_id,
                        configuration: Value::Object(configuration),
                    }]),
                    &inner.registry,
                )
                .map(|document| (document, HistoryChange::Apply))
                .map_err(|error| (CommandErrorCode::InvalidWorkspaceEdit, error.to_string()))
        }
        SemanticCommand::Undo => inner
            .undo
            .last()
            .cloned()
            .map(|document| (document, HistoryChange::Undo))
            .ok_or_else(|| {
                (
                    CommandErrorCode::NothingToUndo,
                    "there is no durable edit to undo".to_owned(),
                )
            }),
        SemanticCommand::Redo => inner
            .redo
            .last()
            .cloned()
            .map(|document| (document, HistoryChange::Redo))
            .ok_or_else(|| {
                (
                    CommandErrorCode::NothingToRedo,
                    "there is no durable edit to redo".to_owned(),
                )
            }),
        SemanticCommand::StartAudio
        | SemanticCommand::StopAudio
        | SemanticCommand::SetCaptureMuted { .. }
        | SemanticCommand::SetMonitorEnabled { .. }
        | SemanticCommand::SetMonitorMuted { .. }
        | SemanticCommand::SetMonitorGain { .. } => Err((
            CommandErrorCode::InvalidRuntimeControl,
            "runtime command reached the durable command path".to_owned(),
        )),
    }
}

fn control_value_is_valid(kind: &ControlKind, value: &Value) -> bool {
    match kind {
        ControlKind::Toggle => value.is_boolean(),
        ControlKind::Number => value.is_number(),
        ControlKind::Choice { options } => value
            .as_str()
            .is_some_and(|value| options.iter().any(|option| option == value)),
        ControlKind::Text => value.is_string(),
        ControlKind::Trigger => value.is_null(),
    }
}

fn rejected_receipt<P: PersistencePort, R: RuntimePort>(
    inner: &Inner<P, R>,
    envelope: &CommandEnvelope,
    code: CommandErrorCode,
    message: impl Into<String>,
) -> CommandReceipt {
    CommandReceipt {
        request_id: envelope.request_id,
        request_sequence: envelope.request_sequence,
        outcome: ReceiptOutcome::Rejected {
            error: CommandError {
                code,
                message: message.into(),
            },
        },
        document_revision: inner.document.revision,
        target_graph_revision: inner.target_revision,
        operation_id: None,
    }
}

fn cache_receipt<P: PersistencePort, R: RuntimePort>(
    inner: &mut Inner<P, R>,
    envelope: CommandEnvelope,
    receipt: CommandReceipt,
) {
    let ledger = inner.clients.entry(envelope.client_id).or_default();
    let sequence = envelope.request_sequence.get();
    ledger.max_sequence = Some(envelope.request_sequence);
    ledger
        .request_ids
        .entry(envelope.request_id)
        .or_insert(sequence);
    ledger.insertion_order.push_back(sequence);
    ledger
        .receipts
        .insert(sequence, CachedReceipt { envelope, receipt });
    while ledger.insertion_order.len() > RECEIPT_WINDOW {
        if let Some(expired) = ledger.insertion_order.pop_front() {
            if let Some(cached) = ledger.receipts.remove(&expired) {
                let request_id = cached.envelope.request_id;
                if let Some((&replacement, _)) = ledger
                    .receipts
                    .iter()
                    .find(|(_, candidate)| candidate.envelope.request_id == request_id)
                {
                    ledger.request_ids.insert(request_id, replacement);
                } else {
                    ledger.request_ids.remove(&request_id);
                }
            }
        }
    }
}

fn publish<P: PersistencePort, R: RuntimePort>(
    inner: &mut Inner<P, R>,
) -> Result<(), ApplicationError> {
    inner.projection_revision = inner
        .projection_revision
        .checked_next()
        .map_err(|error| ApplicationError::RevisionOverflow(error.to_string()))?;
    let pending = inner
        .operations
        .values()
        .any(|operation| operation.state == OperationState::Pending);
    let current_error = inner
        .errors
        .iter()
        .rev()
        .find(|error| error.target_graph_revision == Some(inner.target_revision));
    let module_state = if current_error.is_some() {
        ModuleState::Failed
    } else if pending {
        ModuleState::Preparing
    } else if inner.active_revision.get() == inner.target_revision.get()
        && inner.active_revision != ActiveGraphRevision::ZERO
    {
        ModuleState::Active
    } else {
        ModuleState::Defined
    };
    inner.projection = Arc::new(RuntimeProjection {
        runtime_epoch: inner.epoch,
        revision: inner.projection_revision,
        document_revision: inner.document.revision,
        target_graph_revision: inner.target_revision,
        active_graph_revision: inner.active_revision,
        workspace: inner.document.clone(),
        modules: inner
            .document
            .graph
            .modules
            .values()
            .map(|module| ModuleStatus {
                module_id: module.id,
                module_type: module.module_type.clone(),
                state: module_state,
            })
            .collect(),
        devices: Vec::new(),
        streams: Vec::new(),
        operations: inner.operations.values().cloned().collect(),
        errors: inner.errors.clone(),
        control_manifests: materialize_controls(&inner.document, &inner.registry, pending),
        transcript: TranscriptSummary {
            revision: inner.transcript_revision,
            final_segment_count: inner.transcript_segments.len() as u64,
            recent: inner
                .transcript_segments
                .iter()
                .rev()
                .take(32)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
        },
        diagnostics: DiagnosticsSummary {
            counters: inner.diagnostics.clone(),
        },
        audio: inner.audio.clone(),
        asr: inner.projection.asr.clone(),
    });
    Ok(())
}

fn materialize_controls(
    document: &WorkspaceDocument,
    registry: &DescriptorRegistry,
    pending: bool,
) -> BTreeMap<EntityId, Vec<ControlManifest>> {
    document
        .graph
        .modules
        .values()
        .filter_map(|module| {
            let descriptor = registry.get(&module.module_type)?;
            let configuration = module.configuration.as_object();
            Some((
                module.id,
                descriptor
                    .controls
                    .iter()
                    .map(|control| ControlManifest {
                        module_id: module.id,
                        control_id: control.id.clone(),
                        label: control.label.clone(),
                        kind: control.kind.clone(),
                        value: configuration
                            .and_then(|values| values.get(&control.setting_key))
                            .cloned()
                            .unwrap_or_else(|| control.default_value.clone()),
                        availability: ControlAvailability::Available,
                        disabled_reason: None,
                        pending,
                        command: ControlCommandIdentity::SetModuleControl {
                            module_id: module.id,
                            control_id: control.id.clone(),
                        },
                    })
                    .collect(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryPersistence;
    use futures::{executor::block_on, FutureExt};
    use magnolia_domain::{
        synthetic, DeviceSelector, DocumentRevision, ModuleInstance, ModuleTypeId,
    };
    use magnolia_protocol::{ProtocolVersionRange, PROTOCOL_VERSION};
    use serde_json::Map;

    #[derive(Default)]
    struct TestRuntime {
        requests: Vec<ActivationRequest>,
        events: VecDeque<RuntimeEvent>,
    }

    impl RuntimePort for TestRuntime {
        fn enqueue_activation(&mut self, request: ActivationRequest) {
            self.requests.push(request);
        }

        fn poll_event(&mut self) -> Option<RuntimeEvent> {
            self.events.pop_front()
        }
    }

    fn service() -> ApplicationService<InMemoryPersistence, TestRuntime> {
        let service = ApplicationService::new(
            InMemoryPersistence::default(),
            TestRuntime::default(),
            synthetic::registry(),
            RuntimeEpochId::from_u128(1),
        )
        .unwrap();
        connect_client(&service);
        service
    }

    fn connect_client(service: &ApplicationService<InMemoryPersistence, TestRuntime>) {
        assert!(matches!(
            service
                .connect(ConnectRequest {
                    client_id: ClientId::from_u128(1),
                    supported_versions: vec![ProtocolVersionRange {
                        major: PROTOCOL_VERSION.major,
                        minimum_minor: PROTOCOL_VERSION.minor,
                        maximum_minor: PROTOCOL_VERSION.minor,
                    }],
                })
                .unwrap(),
            ConnectResponse::Accepted { .. }
        ));
    }

    fn command(sequence: u64, revision: u64) -> CommandEnvelope {
        CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(u128::from(sequence) + 10),
            request_sequence: RequestSequence::new(sequence),
            expected_document_revision: DocumentRevision::new(revision),
            command: SemanticCommand::ApplyWorkspaceEdit {
                batch: WorkspaceEditBatch::new(vec![WorkspaceEdit::AddModule {
                    instance: ModuleInstance {
                        id: EntityId::from_u128(u128::from(sequence) + 100),
                        module_type: ModuleTypeId::new(synthetic::SOURCE).unwrap(),
                        configuration: Value::Object(Map::new()),
                    },
                }]),
            },
        }
    }

    #[test]
    fn exact_retry_returns_original_receipt_without_second_save() {
        let persistence = InMemoryPersistence::default();
        let service = ApplicationService::new(
            persistence.clone(),
            TestRuntime::default(),
            synthetic::registry(),
            RuntimeEpochId::from_u128(1),
        )
        .unwrap();
        connect_client(&service);
        let command = command(1, 0);
        let first = service.dispatch(command.clone()).unwrap();
        let second = service.dispatch(command).unwrap();
        assert_eq!(first, second);
        assert_eq!(persistence.save_count().unwrap(), 1);
    }

    #[test]
    fn runtime_audio_controls_do_not_enter_persistence_or_document_history() {
        let persistence = InMemoryPersistence::default();
        let service = ApplicationService::new(
            persistence.clone(),
            TestRuntime::default(),
            synthetic::registry(),
            RuntimeEpochId::from_u128(1),
        )
        .unwrap();
        connect_client(&service);
        let envelope = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(90),
            request_sequence: RequestSequence::new(1),
            expected_document_revision: DocumentRevision::ZERO,
            command: SemanticCommand::SetCaptureMuted { muted: true },
        };
        let receipt = service.dispatch(envelope).unwrap();
        assert!(receipt.accepted());
        assert_eq!(receipt.document_revision, DocumentRevision::ZERO);
        assert_eq!(persistence.save_count().unwrap(), 0);

        let undo = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(91),
            request_sequence: RequestSequence::new(2),
            expected_document_revision: DocumentRevision::ZERO,
            command: SemanticCommand::Undo,
        };
        assert!(matches!(
            service.dispatch(undo).unwrap().outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::NothingToUndo,
                    ..
                }
            }
        ));
    }

    #[test]
    fn durable_device_selector_edits_persist_and_request_activation() {
        let service = service();
        let receipt = service
            .dispatch(CommandEnvelope {
                protocol_version: PROTOCOL_VERSION,
                client_id: ClientId::from_u128(1),
                request_id: RequestId::from_u128(92),
                request_sequence: RequestSequence::new(1),
                expected_document_revision: DocumentRevision::ZERO,
                command: SemanticCommand::ApplyWorkspaceEdit {
                    batch: WorkspaceEditBatch::new(vec![WorkspaceEdit::SetDeviceSelector {
                        key: "audio.input".to_owned(),
                        selector: DeviceSelector::FollowDefaultInput,
                    }]),
                },
            })
            .unwrap();
        assert!(receipt.accepted());
        assert_eq!(receipt.document_revision, DocumentRevision::new(1));
        assert_eq!(receipt.target_graph_revision, TargetGraphRevision::new(1));
        assert!(receipt.operation_id.is_some());

        let inner = service.shared.inner.lock().unwrap();
        assert_eq!(inner.runtime.requests.len(), 1);
        assert_eq!(
            inner.runtime.requests[0]
                .device_selectors
                .get("audio.input"),
            Some(&DeviceSelector::FollowDefaultInput)
        );
        assert_eq!(
            inner.document.device_selectors.get("audio.input"),
            Some(&DeviceSelector::FollowDefaultInput)
        );
    }

    #[test]
    fn conflicting_sequence_is_rejected_without_execution() {
        let service = service();
        let first = command(1, 0);
        assert!(service.dispatch(first.clone()).unwrap().accepted());
        let mut conflict = first;
        conflict.request_id = RequestId::from_u128(999);
        assert!(matches!(
            service.dispatch(conflict).unwrap().outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::SequenceConflict,
                    ..
                }
            }
        ));
    }

    #[test]
    fn persistence_failure_does_not_mutate_authoritative_state() {
        let persistence = InMemoryPersistence::default();
        persistence.fail_next_save("disk unavailable").unwrap();
        let service = ApplicationService::new(
            persistence,
            TestRuntime::default(),
            synthetic::registry(),
            RuntimeEpochId::from_u128(1),
        )
        .unwrap();
        connect_client(&service);
        let receipt = service.dispatch(command(1, 0)).unwrap();
        assert!(!receipt.accepted());
        let snapshot = service.snapshot().unwrap();
        assert_eq!(snapshot.document_revision, DocumentRevision::ZERO);
        assert!(snapshot.workspace.graph.modules.is_empty());
    }

    #[test]
    fn service_restores_a_valid_persisted_document() {
        let mut document = WorkspaceDocument {
            revision: DocumentRevision::new(7),
            ..WorkspaceDocument::default()
        };
        document.graph.modules.insert(
            EntityId::from_u128(77),
            ModuleInstance {
                id: EntityId::from_u128(77),
                module_type: ModuleTypeId::new(synthetic::SOURCE).unwrap(),
                configuration: Value::Object(Map::new()),
            },
        );
        let service = ApplicationService::new(
            InMemoryPersistence::with_document(document.clone()),
            TestRuntime::default(),
            synthetic::registry(),
            RuntimeEpochId::from_u128(1),
        )
        .unwrap();
        assert_eq!(service.snapshot().unwrap().workspace, document);
        assert_eq!(
            service.snapshot().unwrap().document_revision,
            DocumentRevision::new(7)
        );
    }

    #[test]
    fn concurrent_waiters_observe_the_same_non_consuming_projection() {
        let service = service();
        let first_waiter = {
            let service = service.clone();
            std::thread::spawn(move || {
                block_on(service.wait_for_projection(ProjectionRevision::ZERO)).unwrap()
            })
        };
        let second_waiter = {
            let service = service.clone();
            std::thread::spawn(move || {
                block_on(service.wait_for_projection(ProjectionRevision::ZERO)).unwrap()
            })
        };
        assert!(service.dispatch(command(1, 0)).unwrap().accepted());
        let first = first_waiter.join().unwrap();
        let second = second_waiter.join().unwrap();
        assert_eq!(first.revision, second.revision);
        assert_eq!(first.document_revision, DocumentRevision::new(1));
    }

    #[test]
    fn cancelled_projection_wait_unregisters_its_listener() {
        let service = service();

        assert!(service
            .wait_for_projection(ProjectionRevision::ZERO)
            .now_or_never()
            .is_none());
        assert_eq!(service.shared.projection_changed.total_listeners(), 0);
    }

    #[test]
    fn commands_require_a_successful_handshake() {
        let service = ApplicationService::new(
            InMemoryPersistence::default(),
            TestRuntime::default(),
            synthetic::registry(),
            RuntimeEpochId::from_u128(1),
        )
        .unwrap();
        assert!(matches!(
            service.dispatch(command(1, 0)).unwrap().outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::UnsupportedProtocolVersion,
                    ..
                }
            }
        ));
    }

    #[test]
    fn optimistic_revision_conflict_does_not_execute() {
        let service = service();
        let receipt = service.dispatch(command(1, 99)).unwrap();
        assert!(matches!(
            receipt.outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::RevisionConflict,
                    ..
                }
            }
        ));
        assert!(service
            .snapshot()
            .unwrap()
            .workspace
            .graph
            .modules
            .is_empty());
    }

    #[test]
    fn configuration_edits_cannot_bypass_the_descriptor_schema() {
        let service = service();
        assert!(service.dispatch(command(1, 0)).unwrap().accepted());
        let before = service.snapshot().unwrap();
        let module_id = EntityId::from_u128(101);
        let invalid = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(900),
            request_sequence: RequestSequence::new(2),
            expected_document_revision: DocumentRevision::new(1),
            command: SemanticCommand::ApplyWorkspaceEdit {
                batch: WorkspaceEditBatch::new(vec![WorkspaceEdit::SetModuleConfiguration {
                    module_id,
                    configuration: serde_json::json!({"enabled": "not-a-boolean"}),
                }]),
            },
        };

        assert!(matches!(
            service.dispatch(invalid).unwrap().outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::InvalidWorkspaceEdit,
                    ..
                }
            }
        ));
        let after = service.snapshot().unwrap();
        assert_eq!(after.document_revision, before.document_revision);
        assert_eq!(after.target_graph_revision, before.target_graph_revision);
        assert_eq!(after.operations, before.operations);
        assert_eq!(
            after.workspace.graph.modules[&module_id].configuration,
            Value::Object(Map::new())
        );
    }

    #[test]
    fn set_control_rejects_non_object_configuration_without_discarding_it() {
        let module_id = EntityId::from_u128(77);
        let module_type = ModuleTypeId::new(synthetic::SOURCE).unwrap();
        let base = synthetic::registry();
        let mut descriptor = base.get(&module_type).unwrap().clone();
        descriptor.configuration_schema = Value::Bool(true);
        let mut registry = DescriptorRegistry::new();
        registry.register(descriptor).unwrap();
        let mut document = WorkspaceDocument::default();
        document.graph.modules.insert(
            module_id,
            ModuleInstance {
                id: module_id,
                module_type,
                configuration: Value::String("preserve-me".to_owned()),
            },
        );
        let persistence = InMemoryPersistence::with_document(document);
        let service = ApplicationService::new(
            persistence.clone(),
            TestRuntime::default(),
            registry,
            RuntimeEpochId::from_u128(1),
        )
        .unwrap();
        connect_client(&service);
        let set_control = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(901),
            request_sequence: RequestSequence::new(1),
            expected_document_revision: DocumentRevision::ZERO,
            command: SemanticCommand::SetControl {
                module_id,
                control_id: magnolia_domain::ControlId::new("enabled").unwrap(),
                value: Value::Bool(false),
            },
        };

        assert!(matches!(
            service.dispatch(set_control).unwrap().outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::InvalidWorkspaceEdit,
                    ..
                }
            }
        ));
        let snapshot = service.snapshot().unwrap();
        assert_eq!(
            snapshot.workspace.graph.modules[&module_id].configuration,
            Value::String("preserve-me".to_owned())
        );
        assert_eq!(snapshot.document_revision, DocumentRevision::ZERO);
        assert_eq!(snapshot.target_graph_revision, TargetGraphRevision::ZERO);
        assert!(snapshot.operations.is_empty());
        assert_eq!(persistence.save_count().unwrap(), 0);
    }

    #[test]
    fn expired_receipts_and_reused_request_ids_never_execute() {
        let service = service();
        let mut first = None;
        for sequence in 0..=RECEIPT_WINDOW as u64 {
            let envelope = CommandEnvelope {
                protocol_version: PROTOCOL_VERSION,
                client_id: ClientId::from_u128(1),
                request_id: RequestId::from_u128(u128::from(sequence) + 10_000),
                request_sequence: RequestSequence::new(sequence),
                expected_document_revision: DocumentRevision::ZERO,
                command: SemanticCommand::Undo,
            };
            if sequence == 0 {
                first = Some(envelope.clone());
            }
            assert!(matches!(
                service.dispatch(envelope).unwrap().outcome,
                ReceiptOutcome::Rejected {
                    error: CommandError {
                        code: CommandErrorCode::NothingToUndo,
                        ..
                    }
                }
            ));
        }
        assert!(matches!(
            service.dispatch(first.unwrap()).unwrap().outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::SequenceExpired,
                    ..
                }
            }
        ));

        let mut reused_id = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(20_000),
            request_sequence: RequestSequence::new(RECEIPT_WINDOW as u64 + 2),
            expected_document_revision: DocumentRevision::ZERO,
            command: SemanticCommand::Undo,
        };
        let original = service.dispatch(reused_id.clone()).unwrap();
        assert!(!original.accepted());
        reused_id.request_sequence = RequestSequence::new(RECEIPT_WINDOW as u64 + 3);
        assert!(matches!(
            service.dispatch(reused_id.clone()).unwrap().outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::RequestIdConflict,
                    ..
                }
            }
        ));
        reused_id.request_id = RequestId::from_u128(30_000);
        assert!(matches!(
            service.dispatch(reused_id).unwrap().outcome,
            ReceiptOutcome::Rejected {
                error: CommandError {
                    code: CommandErrorCode::SequenceConflict,
                    ..
                }
            }
        ));
    }

    #[test]
    fn undo_redo_and_control_manifests_follow_authoritative_revisions() {
        let service = service();
        assert!(service.dispatch(command(1, 0)).unwrap().accepted());
        let module_id = EntityId::from_u128(101);
        let manifest = &service.snapshot().unwrap().control_manifests[&module_id][0];
        assert_eq!(manifest.value, serde_json::json!(true));
        assert!(manifest.pending);

        let set_control = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(800),
            request_sequence: RequestSequence::new(2),
            expected_document_revision: DocumentRevision::new(1),
            command: SemanticCommand::SetControl {
                module_id,
                control_id: magnolia_domain::ControlId::new("enabled").unwrap(),
                value: Value::Bool(false),
            },
        };
        assert!(service.dispatch(set_control).unwrap().accepted());
        assert_eq!(
            service.snapshot().unwrap().control_manifests[&module_id][0].value,
            Value::Bool(false)
        );

        let undo = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(801),
            request_sequence: RequestSequence::new(3),
            expected_document_revision: DocumentRevision::new(2),
            command: SemanticCommand::Undo,
        };
        assert!(service.dispatch(undo).unwrap().accepted());
        assert_eq!(
            service.snapshot().unwrap().control_manifests[&module_id][0].value,
            Value::Bool(true)
        );

        let redo = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: ClientId::from_u128(1),
            request_id: RequestId::from_u128(802),
            request_sequence: RequestSequence::new(4),
            expected_document_revision: DocumentRevision::new(3),
            command: SemanticCommand::Redo,
        };
        let receipt = service.dispatch(redo).unwrap();
        assert!(receipt.accepted());
        assert_eq!(receipt.document_revision, DocumentRevision::new(4));
        assert_eq!(
            service.snapshot().unwrap().control_manifests[&module_id][0].value,
            Value::Bool(false)
        );
    }

    #[test]
    fn final_transcript_journal_is_ordered_and_cursor_addressable() {
        let service = service();
        let session_id = EntityId::from_u128(500);
        for sequence in 1..=2 {
            service
                .append_transcript(TranscriptSegment {
                    session_id,
                    segment_id: EntityId::from_u128(500 + u128::from(sequence)),
                    segment_revision: 1,
                    sequence,
                    text: format!("segment {sequence}"),
                })
                .unwrap();
        }
        let first = service.transcript_page(0, 1).unwrap();
        assert_eq!(first.segments.len(), 1);
        assert_eq!(first.segments[0].sequence, 1);
        assert_eq!(first.next_cursor, Some(1));
        let second = service.transcript_page(1, 8).unwrap();
        assert_eq!(second.segments.len(), 1);
        assert_eq!(second.segments[0].sequence, 2);
        assert_eq!(second.next_cursor, None);
        let projection = service.snapshot().unwrap();
        assert_eq!(projection.transcript.final_segment_count, 2);
        assert_eq!(projection.transcript.recent.len(), 2);

        assert!(matches!(
            service.append_transcript(TranscriptSegment {
                session_id,
                segment_id: EntityId::from_u128(999),
                segment_revision: 1,
                sequence: 2,
                text: "duplicate".to_owned(),
            }),
            Err(ApplicationError::InvalidObservation(_))
        ));
    }

    #[test]
    fn diagnostic_counter_batches_publish_one_snapshot_only_when_changed() {
        let service = service();
        let before = service.snapshot().unwrap().revision;
        assert!(service
            .set_diagnostic_counters([
                ("telemetry.connections".to_owned(), 1),
                ("telemetry.dropped".to_owned(), 4),
            ])
            .unwrap());
        let after = service.snapshot().unwrap();
        assert_eq!(after.revision.get(), before.get() + 1);
        assert_eq!(after.diagnostics.counters["telemetry.connections"], 1);
        assert_eq!(after.diagnostics.counters["telemetry.dropped"], 4);
        assert!(!service
            .set_diagnostic_counters([
                ("telemetry.connections".to_owned(), 1),
                ("telemetry.dropped".to_owned(), 4),
            ])
            .unwrap());
        assert_eq!(service.snapshot().unwrap().revision, after.revision);
        assert!(matches!(
            service.set_diagnostic_counter(" ", 1),
            Err(ApplicationError::InvalidObservation(_))
        ));
    }
}
