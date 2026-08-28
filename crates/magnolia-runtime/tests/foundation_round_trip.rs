use magnolia_application::{ApplicationService, InMemoryPersistence, InProcessApplicationClient};
use magnolia_client::{run_foundation_edit_scenario, ApplicationClient};
use magnolia_domain::{
    synthetic, ActiveGraphRevision, ClientId, DocumentRevision, Edge, EntityId, ModuleInstance,
    ModuleTypeId, PortId, PortRef, ProjectionRevision, RequestId, RuntimeEpochId,
    TargetGraphRevision, WorkspaceEdit, WorkspaceEditBatch,
};
use magnolia_protocol::{
    CommandEnvelope, OperationState, ReceiptOutcome, RequestSequence, SemanticCommand,
    PROTOCOL_VERSION,
};
use magnolia_runtime::MockRuntime;
use serde_json::json;

fn envelope(sequence: u64, revision: u64, edits: Vec<WorkspaceEdit>) -> CommandEnvelope {
    CommandEnvelope {
        protocol_version: PROTOCOL_VERSION,
        client_id: ClientId::from_u128(1),
        request_id: RequestId::from_u128(u128::from(sequence) + 100),
        request_sequence: RequestSequence::new(sequence),
        expected_document_revision: DocumentRevision::new(revision),
        command: SemanticCommand::ApplyWorkspaceEdit {
            batch: WorkspaceEditBatch::new(edits),
        },
    }
}

#[test]
fn portable_foundation_round_trip_preserves_last_good_and_ignores_stale_results() {
    let persistence = InMemoryPersistence::default();
    let runtime = MockRuntime::new();
    let service = ApplicationService::new(
        persistence.clone(),
        runtime.clone(),
        synthetic::registry(),
        RuntimeEpochId::from_u128(9),
    )
    .unwrap();
    let client = InProcessApplicationClient::new(service.clone());

    // 1-3. Reusable client scenario: connect, edit, persist/project, exact retry.
    let source = EntityId::from_u128(10);
    let sink = EntityId::from_u128(11);
    let edge = EntityId::from_u128(12);
    let scenario = run_foundation_edit_scenario(
        &client,
        ClientId::from_u128(1),
        RequestId::from_u128(101),
        WorkspaceEditBatch::new(vec![
            WorkspaceEdit::AddModule {
                instance: ModuleInstance {
                    id: source,
                    module_type: ModuleTypeId::new(synthetic::SOURCE).unwrap(),
                    configuration: json!({"enabled": true}),
                },
            },
            WorkspaceEdit::AddModule {
                instance: ModuleInstance {
                    id: sink,
                    module_type: ModuleTypeId::new(synthetic::SINK).unwrap(),
                    configuration: json!({}),
                },
            },
            WorkspaceEdit::AddEdge {
                edge: Edge {
                    id: edge,
                    from: PortRef {
                        module_id: source,
                        port_id: PortId::new("out").unwrap(),
                    },
                    to: PortRef {
                        module_id: sink,
                        port_id: PortId::new("in").unwrap(),
                    },
                    capacity: Some(8),
                },
            },
        ]),
    )
    .unwrap();
    let first_receipt = scenario.receipt;
    assert_eq!(scenario.initial.revision, ProjectionRevision::ZERO);
    assert_eq!(scenario.initial.document_revision, DocumentRevision::ZERO);
    assert!(first_receipt.accepted());
    assert_eq!(first_receipt.document_revision, DocumentRevision::new(1));
    assert_eq!(
        first_receipt.target_graph_revision,
        TargetGraphRevision::new(1)
    );
    assert!(first_receipt.operation_id.is_some());
    assert_eq!(
        persistence.latest().unwrap().unwrap().revision,
        DocumentRevision::new(1)
    );
    assert_eq!(runtime.pending_requests().len(), 1);
    assert_eq!(persistence.save_count().unwrap(), 1);

    // 4. Pump a deterministic success into the authoritative projection.
    runtime.complete_next_success().unwrap();
    assert_eq!(service.pump_runtime_events().unwrap().handled, 1);
    let first_active = client.snapshot().unwrap();
    assert_eq!(
        first_active.active_graph_revision,
        ActiveGraphRevision::new(1)
    );
    assert_eq!(
        first_active.target_graph_revision,
        TargetGraphRevision::new(1)
    );

    // 5. A later failure advances target state but retains revision 1 as last-good.
    let second_receipt = client
        .dispatch(envelope(
            2,
            1,
            vec![WorkspaceEdit::SetModuleConfiguration {
                module_id: source,
                configuration: json!({"enabled": false}),
            }],
        ))
        .unwrap();
    assert!(second_receipt.accepted());
    runtime
        .complete_next_failure("synthetic.prepare", "injected activation failure")
        .unwrap();
    assert_eq!(service.pump_runtime_events().unwrap().handled, 1);
    let failed = client.snapshot().unwrap();
    assert_eq!(failed.target_graph_revision, TargetGraphRevision::new(2));
    assert_eq!(failed.active_graph_revision, ActiveGraphRevision::new(1));
    assert_eq!(failed.errors.last().unwrap().code, "synthetic.prepare");

    // 6. Supersede target 3 with target 4; target 3 completion is ignored.
    assert!(client
        .dispatch(envelope(
            3,
            2,
            vec![WorkspaceEdit::SetPromotedSetting {
                key: "synthetic.a".to_owned(),
                value: json!(1),
            }],
        ))
        .unwrap()
        .accepted());
    assert!(client
        .dispatch(envelope(
            4,
            3,
            vec![WorkspaceEdit::SetPromotedSetting {
                key: "synthetic.b".to_owned(),
                value: json!(2),
            }],
        ))
        .unwrap()
        .accepted());
    let before_stale = client.snapshot().unwrap();
    runtime
        .complete_target_success(TargetGraphRevision::new(3))
        .unwrap();
    let stale_report = service.pump_runtime_events().unwrap();
    assert_eq!(stale_report.ignored_stale, 1);
    let after_stale = client.snapshot().unwrap();
    assert_eq!(after_stale.revision, before_stale.revision);
    assert_eq!(
        after_stale.active_graph_revision,
        ActiveGraphRevision::new(1)
    );
    assert!(after_stale.operations.iter().any(|operation| {
        operation.target_graph_revision == TargetGraphRevision::new(3)
            && operation.state == OperationState::Superseded
    }));

    runtime
        .complete_target_success(TargetGraphRevision::new(4))
        .unwrap();
    assert_eq!(service.pump_runtime_events().unwrap().handled, 1);
    let final_projection = client.snapshot().unwrap();
    assert_eq!(
        final_projection.target_graph_revision,
        TargetGraphRevision::new(4)
    );
    assert_eq!(
        final_projection.active_graph_revision,
        ActiveGraphRevision::new(4)
    );
    assert!(matches!(first_receipt.outcome, ReceiptOutcome::Accepted));
}
