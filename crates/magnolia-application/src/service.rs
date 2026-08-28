use crate::{ActivationRequest, PersistenceError, PersistencePort, RuntimeEvent, RuntimePort};
use magnolia_domain::{
    ActiveGraphRevision, ClientId, ControlKind, DescriptorRegistry, EntityId, OperationId,
    ProjectionRevision, RequestId, RuntimeEpochId, TargetGraphRevision, WorkspaceDocument,
    WorkspaceEdit, WorkspaceEditBatch,
};
use magnolia_protocol::{
    negotiate_protocol, CommandEnvelope, CommandError, CommandErrorCode, CommandReceipt,
    ConnectRequest, ConnectResponse, ControlAvailability, ControlCommandIdentity, ControlManifest,
    DiagnosticsSummary, ModuleState, ModuleStatus, OperationState, OperationStatus,
    ProtocolVersion, ReceiptOutcome, RequestSequence, RuntimeError, RuntimeProjection,
    SemanticCommand, TranscriptSummary,
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
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
    projection_changed: Condvar,
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
    undo: Vec<WorkspaceDocument>,
    redo: Vec<WorkspaceDocument>,
    clients: BTreeMap<ClientId, ClientLedger>,
    projection: Arc<RuntimeProjection>,
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
    #[error("projection wait timed out")]
    Timeout,
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
                    undo: Vec::new(),
                    redo: Vec::new(),
                    clients: BTreeMap::new(),
                    projection: Arc::new(initial),
                }),
                projection_changed: Condvar::new(),
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

    pub fn wait_for_projection(
        &self,
        after: ProjectionRevision,
        timeout: Duration,
    ) -> Result<Arc<RuntimeProjection>, ApplicationError> {
        let started = Instant::now();
        let mut inner = self.lock()?;
        loop {
            if inner.projection.revision > after {
                return Ok(Arc::clone(&inner.projection));
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return Err(ApplicationError::Timeout);
            }
            let remaining = timeout.saturating_sub(elapsed);
            let (next, timed_out) = self
                .shared
                .projection_changed
                .wait_timeout(inner, remaining)
                .map_err(|_| ApplicationError::Poisoned)?;
            inner = next;
            if timed_out.timed_out() && inner.projection.revision <= after {
                return Err(ApplicationError::Timeout);
            }
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
            self.shared.projection_changed.notify_all();
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
            }
            publish(&mut inner)?;
            report.handled += 1;
        }
        if report.handled > 0 {
            self.shared.projection_changed.notify_all();
        }
        Ok(report)
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

    let (mut candidate, history) = match prepare_candidate(inner, &envelope.command) {
        Ok(candidate) => candidate,
        Err((code, message)) => return Ok(rejected_receipt(inner, envelope, code, message)),
    };
    let next_document = inner
        .document
        .revision
        .checked_next()
        .map_err(|error| ApplicationError::RevisionOverflow(error.to_string()))?;
    let next_target = inner
        .target_revision
        .checked_next()
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
    for operation in inner.operations.values_mut() {
        if operation.state == OperationState::Pending {
            operation.state = OperationState::Superseded;
        }
    }
    inner.document = candidate;
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
    });
    publish(inner)?;

    Ok(CommandReceipt {
        request_id: envelope.request_id,
        request_sequence: envelope.request_sequence,
        outcome: ReceiptOutcome::Accepted,
        document_revision: next_document,
        target_graph_revision: next_target,
        operation_id: Some(operation_id),
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
            let mut configuration = instance
                .configuration
                .as_object()
                .cloned()
                .unwrap_or_else(Map::new);
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
        transcript: TranscriptSummary::default(),
        diagnostics: DiagnosticsSummary::default(),
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
    use magnolia_domain::{synthetic, DocumentRevision, ModuleInstance, ModuleTypeId};
    use magnolia_protocol::{ProtocolVersionRange, PROTOCOL_VERSION};

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
                service
                    .wait_for_projection(ProjectionRevision::ZERO, Duration::from_secs(1))
                    .unwrap()
            })
        };
        let second_waiter = {
            let service = service.clone();
            std::thread::spawn(move || {
                service
                    .wait_for_projection(ProjectionRevision::ZERO, Duration::from_secs(1))
                    .unwrap()
            })
        };
        assert!(service.dispatch(command(1, 0)).unwrap().accepted());
        let first = first_waiter.join().unwrap();
        let second = second_waiter.join().unwrap();
        assert_eq!(first.revision, second.revision);
        assert_eq!(first.document_revision, DocumentRevision::new(1));
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
}
