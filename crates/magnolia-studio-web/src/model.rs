use crate::client::{ConnectionPhase, WebSocketApplicationClient};
use leptos::prelude::*;
use magnolia_client::ApplicationClient;
use magnolia_domain::{
    synthetic, ClientId, Edge, EntityId, LayoutNode, LayoutPreset, ModuleInstance, ModuleTypeId,
    PortId, PortRef, RequestId, SplitAxis, TileBinding, WorkspaceEdit, WorkspaceEditBatch,
};
use magnolia_protocol::{
    CommandEnvelope, CommandReceipt, ProtocolVersion, ProtocolVersionRange, RequestSequence,
    RuntimeProjection, SemanticCommand, PROTOCOL_VERSION, SYNTHETIC_CAPTION_STREAM_ID,
    SYNTHETIC_DIAGNOSTICS_STREAM_ID, SYNTHETIC_METER_STREAM_ID, SYNTHETIC_SPECTRUM_STREAM_ID,
    SYNTHETIC_WAVEFORM_STREAM_ID,
};
use send_wrapper::SendWrapper;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
    sync::Arc,
};

const REQUEST_SEQUENCE_KEY: &str = "magnolia.request_sequence.v1";
const ACTIVE_WORKSPACE_KEY: &str = "magnolia.active_workspace.v1";

pub const SOURCE_MODULE: EntityId = EntityId::from_u128(0x10_001);
pub const PROCESSOR_MODULE: EntityId = EntityId::from_u128(0x10_002);
pub const SINK_MODULE: EntityId = EntityId::from_u128(0x10_003);
const SOURCE_PROCESSOR_EDGE: EntityId = EntityId::from_u128(0x11_001);
const PROCESSOR_SINK_EDGE: EntityId = EntityId::from_u128(0x11_002);

pub const SOURCE_TILE: EntityId = EntityId::from_u128(0x20_001);
pub const PROCESSOR_TILE: EntityId = EntityId::from_u128(0x20_002);
pub const SINK_TILE: EntityId = EntityId::from_u128(0x20_003);
pub const GRAPH_TILE: EntityId = EntityId::from_u128(0x20_004);
pub const CONTROLS_TILE: EntityId = EntityId::from_u128(0x20_005);
pub const STATUS_TILE: EntityId = EntityId::from_u128(0x20_006);
pub const DIAGNOSTICS_TILE: EntityId = EntityId::from_u128(0x20_007);
pub const METER_TILE: EntityId = EntityId::from_u128(0x20_008);
pub const WAVEFORM_TILE: EntityId = EntityId::from_u128(0x20_009);
pub const SPECTRUM_TILE: EntityId = EntityId::from_u128(0x20_00a);
pub const TRANSCRIPT_TILE: EntityId = EntityId::from_u128(0x20_00b);

pub const WORKSPACES: [&str; 5] = ["Capture", "Transcribe", "Patch", "Diagnose", "Perform"];

#[derive(Clone)]
pub struct StudioState {
    // Leptos' arena-backed views can be represented as Send even though CSR runs on one
    // browser thread. SendWrapper enforces that the browser adapter is only accessed on
    // the thread where it was created; ApplicationClient itself remains non-Send.
    pub client: SendWrapper<WebSocketApplicationClient>,
    pub client_id: ClientId,
    pub phase: RwSignal<ConnectionPhase>,
    pub protocol_version: RwSignal<Option<ProtocolVersion>>,
    pub projection: RwSignal<Option<Arc<RuntimeProjection>>>,
    pub last_receipt: RwSignal<Option<CommandReceipt>>,
    pub last_envelope: RwSignal<Option<CommandEnvelope>>,
    pub error: RwSignal<Option<String>>,
    pub active_workspace: RwSignal<String>,
    pub focused_tile: RwSignal<Option<EntityId>>,
    pub focused_control: RwSignal<Option<String>>,
    pub hidden_tiles: RwSignal<BTreeSet<EntityId>>,
    pub layout_draft: RwSignal<Option<(String, LayoutPreset)>>,
    pub palette_open: RwSignal<bool>,
    pub palette_query: RwSignal<String>,
    pub events: RwSignal<VecDeque<String>>,
    request_sequence: RwSignal<u64>,
}

impl StudioState {
    pub fn new(client: WebSocketApplicationClient, client_id: ClientId) -> Result<Self, String> {
        let phase = RwSignal::new(client.phase());
        let phase_observer = phase;
        client.observe_connection(Rc::new(move |current| phase_observer.set(current)));
        Ok(Self {
            client: SendWrapper::new(client),
            client_id,
            phase,
            protocol_version: RwSignal::new(None),
            projection: RwSignal::new(None),
            last_receipt: RwSignal::new(None),
            last_envelope: RwSignal::new(None),
            error: RwSignal::new(None),
            active_workspace: RwSignal::new(read_active_workspace()),
            focused_tile: RwSignal::new(None),
            focused_control: RwSignal::new(None),
            hidden_tiles: RwSignal::new(BTreeSet::new()),
            layout_draft: RwSignal::new(None),
            palette_open: RwSignal::new(false),
            palette_query: RwSignal::new(String::new()),
            events: RwSignal::new(VecDeque::new()),
            request_sequence: RwSignal::new(read_request_sequence()?),
        })
    }

    pub fn connect(&self) {
        let state = self.clone();
        leptos::task::spawn_local(async move {
            let request = magnolia_protocol::ConnectRequest {
                client_id: state.client_id,
                supported_versions: vec![ProtocolVersionRange {
                    major: PROTOCOL_VERSION.major,
                    minimum_minor: PROTOCOL_VERSION.minor,
                    maximum_minor: PROTOCOL_VERSION.minor,
                }],
            };
            match state.client.connect(request).await {
                Ok(magnolia_protocol::ConnectResponse::Accepted {
                    negotiated_version,
                    snapshot,
                }) => {
                    state.protocol_version.set(Some(negotiated_version));
                    let snapshot = Arc::new(*snapshot);
                    state.publish_projection(Arc::clone(&snapshot));
                    state.push_event("Authenticated control connection".to_owned());
                    state.watch_projections(snapshot.revision);
                }
                Ok(magnolia_protocol::ConnectResponse::Rejected { error }) => {
                    state.error.set(Some(format!("Protocol rejected: {error}")));
                }
                Err(error) => state.error.set(Some(error.to_string())),
            }
        });
    }

    fn watch_projections(&self, mut after: magnolia_domain::ProjectionRevision) {
        let state = self.clone();
        leptos::task::spawn_local(async move {
            loop {
                match state.client.wait_for_projection(after).await {
                    Ok(projection) => {
                        after = projection.revision;
                        state.publish_projection(projection);
                    }
                    Err(error) => {
                        state.error.set(Some(error.to_string()));
                        return;
                    }
                }
            }
        });
    }

    pub fn publish_projection(&self, projection: Arc<RuntimeProjection>) {
        self.push_event(format!(
            "Projection {} · document {} · target {} · active {}",
            projection.revision,
            projection.document_revision,
            projection.target_graph_revision,
            projection.active_graph_revision
        ));
        self.projection.set(Some(projection));
    }

    pub fn dispatch(&self, command: SemanticCommand) {
        let Some(projection) = self.projection.get_untracked() else {
            self.error.set(Some(
                "Cannot dispatch before the first projection".to_owned(),
            ));
            return;
        };
        let sequence = self.take_request_sequence();
        let envelope = CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            client_id: self.client_id,
            request_id: RequestId::new(),
            request_sequence: RequestSequence::new(sequence),
            expected_document_revision: projection.document_revision,
            command,
        };
        self.dispatch_envelope(envelope, true);
    }

    pub fn retry_last(&self) {
        if let Some(envelope) = self.last_envelope.get_untracked() {
            self.dispatch_envelope(envelope, false);
        }
    }

    fn dispatch_envelope(&self, envelope: CommandEnvelope, remember: bool) {
        if remember {
            self.last_envelope.set(Some(envelope.clone()));
        }
        let state = self.clone();
        leptos::task::spawn_local(async move {
            match state.client.dispatch(envelope).await {
                Ok(receipt) => {
                    state.push_event(format!(
                        "Receipt {} · {:?} · document {} · target {}",
                        receipt.request_sequence.get(),
                        receipt.outcome,
                        receipt.document_revision,
                        receipt.target_graph_revision
                    ));
                    if let magnolia_protocol::ReceiptOutcome::Rejected { error } = &receipt.outcome
                    {
                        state.error.set(Some(error.message.clone()));
                    } else {
                        state.error.set(None);
                    }
                    state.last_receipt.set(Some(receipt));
                }
                Err(error) => state.error.set(Some(error.to_string())),
            }
        });
    }

    fn take_request_sequence(&self) -> u64 {
        let current = self.request_sequence.get();
        let next = current.saturating_add(1);
        self.request_sequence.set(next);
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.session_storage() {
                let _ = storage.set_item(REQUEST_SEQUENCE_KEY, &next.to_string());
            }
        }
        current
    }

    pub fn load_demo(&self) {
        self.dispatch(SemanticCommand::ApplyWorkspaceEdit {
            batch: demo_workspace_batch(),
        });
    }

    pub fn preview_layout_adjustment(&self) {
        let Some(projection) = self.projection.get_untracked() else {
            return;
        };
        let name = self.active_workspace.get_untracked();
        let Some(mut preset) = projection.workspace.presets.get(&name).cloned() else {
            return;
        };
        rotate_first_split_ratio(&mut preset.root);
        self.layout_draft.set(Some((name.clone(), preset)));
        self.push_event(format!("Previewed local layout draft for {name}"));
    }

    pub fn commit_layout_draft(&self) {
        let Some((name, preset)) = self.layout_draft.get_untracked() else {
            self.error
                .set(Some("There is no local layout draft to commit".to_owned()));
            return;
        };
        self.dispatch(SemanticCommand::ApplyWorkspaceEdit {
            batch: WorkspaceEditBatch::new(vec![WorkspaceEdit::PutPreset { name, preset }]),
        });
        self.layout_draft.set(None);
    }

    pub fn discard_layout_draft(&self) {
        if let Some((name, _)) = self.layout_draft.get_untracked() {
            self.push_event(format!("Discarded local layout draft for {name}"));
        }
        self.layout_draft.set(None);
    }

    pub fn switch_workspace(&self, workspace: &str) {
        if !WORKSPACES.contains(&workspace) {
            self.error
                .set(Some(format!("Unknown workspace preset: {workspace}")));
            return;
        }
        self.layout_draft.set(None);
        self.active_workspace.set(workspace.to_owned());
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.session_storage() {
                let _ = storage.set_item(ACTIVE_WORKSPACE_KEY, workspace);
            }
        }
    }

    pub fn set_control(
        &self,
        module_id: EntityId,
        control_id: magnolia_domain::ControlId,
        value: serde_json::Value,
    ) {
        self.dispatch(SemanticCommand::SetControl {
            module_id,
            control_id,
            value,
        });
    }

    pub fn push_event(&self, message: String) {
        self.events.update(|events| {
            events.push_back(message);
            while events.len() > 96 {
                events.pop_front();
            }
        });
    }

    pub fn hide_tile(&self, tile_id: EntityId) {
        self.hidden_tiles.update(|tiles| {
            tiles.insert(tile_id);
        });
        self.push_event(format!("Closed presentation tile {tile_id}"));
    }

    pub fn show_tile(&self, tile_id: EntityId) {
        self.hidden_tiles.update(|tiles| {
            tiles.remove(&tile_id);
        });
        self.push_event(format!("Reopened presentation tile {tile_id}"));
    }
}

fn read_request_sequence() -> Result<u64, String> {
    let Some(window) = web_sys::window() else {
        return Err("browser window is absent".to_owned());
    };
    let storage = window
        .session_storage()
        .map_err(|_| "sessionStorage is unavailable".to_owned())?
        .ok_or_else(|| "sessionStorage is disabled".to_owned())?;
    Ok(storage
        .get_item(REQUEST_SEQUENCE_KEY)
        .map_err(|_| "could not read request sequence".to_owned())?
        .and_then(|value| value.parse().ok())
        .unwrap_or(1))
}

fn read_active_workspace() -> String {
    web_sys::window()
        .and_then(|window| window.session_storage().ok().flatten())
        .and_then(|storage| storage.get_item(ACTIVE_WORKSPACE_KEY).ok().flatten())
        .filter(|workspace| WORKSPACES.contains(&workspace.as_str()))
        .unwrap_or_else(|| "Patch".to_owned())
}

fn module(id: EntityId, module_type: &str, label: &str) -> ModuleInstance {
    ModuleInstance {
        id,
        module_type: ModuleTypeId::new(module_type).expect("static module type"),
        configuration: json!({
            "enabled": true,
            "gain": 1.0,
            "mode": "steady",
            "label": label,
        }),
    }
}

fn edge(id: EntityId, from: EntityId, to: EntityId, capacity: Option<u32>) -> Edge {
    Edge {
        id,
        from: PortRef {
            module_id: from,
            port_id: PortId::new("out").expect("static port"),
        },
        to: PortRef {
            module_id: to,
            port_id: PortId::new("in").expect("static port"),
        },
        capacity,
    }
}

fn binding(kind: &str, modules: &[EntityId], resources: &[EntityId]) -> TileBinding {
    TileBinding {
        module_ids: modules.to_vec(),
        resource_ids: resources.to_vec(),
        settings: json!({"kind": kind}),
    }
}

fn tile(tile_id: EntityId) -> LayoutNode {
    LayoutNode::Tile { tile_id }
}

fn tabs(children: Vec<LayoutNode>) -> LayoutNode {
    LayoutNode::Tabs {
        active: 0,
        children,
    }
}

fn split(axis: SplitAxis, ratio: u32, first: LayoutNode, second: LayoutNode) -> LayoutNode {
    LayoutNode::Split {
        axis,
        ratio_millionths: ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn demo_workspace_batch() -> WorkspaceEditBatch {
    let bindings = [
        (SOURCE_TILE, binding("source", &[SOURCE_MODULE], &[])),
        (
            PROCESSOR_TILE,
            binding("processor", &[PROCESSOR_MODULE], &[]),
        ),
        (SINK_TILE, binding("sink", &[SINK_MODULE], &[])),
        (
            GRAPH_TILE,
            binding(
                "graph",
                &[SOURCE_MODULE, PROCESSOR_MODULE, SINK_MODULE],
                &[],
            ),
        ),
        (
            CONTROLS_TILE,
            binding(
                "controls",
                &[SOURCE_MODULE, PROCESSOR_MODULE, SINK_MODULE],
                &[],
            ),
        ),
        (STATUS_TILE, binding("status", &[], &[])),
        (
            DIAGNOSTICS_TILE,
            binding("diagnostics", &[], &[SYNTHETIC_DIAGNOSTICS_STREAM_ID]),
        ),
        (
            METER_TILE,
            binding("meter", &[SOURCE_MODULE], &[SYNTHETIC_METER_STREAM_ID]),
        ),
        (
            WAVEFORM_TILE,
            binding(
                "waveform",
                &[SOURCE_MODULE],
                &[SYNTHETIC_WAVEFORM_STREAM_ID],
            ),
        ),
        (
            SPECTRUM_TILE,
            binding(
                "spectrum",
                &[PROCESSOR_MODULE],
                &[SYNTHETIC_SPECTRUM_STREAM_ID],
            ),
        ),
        (
            TRANSCRIPT_TILE,
            binding(
                "transcript",
                &[PROCESSOR_MODULE],
                &[SYNTHETIC_CAPTION_STREAM_ID],
            ),
        ),
    ];
    let presets = BTreeMap::from([
        (
            "Capture".to_owned(),
            LayoutPreset {
                root: split(
                    SplitAxis::Horizontal,
                    360_000,
                    tile(SOURCE_TILE),
                    tabs(vec![tile(METER_TILE), tile(WAVEFORM_TILE)]),
                ),
            },
        ),
        (
            "Transcribe".to_owned(),
            LayoutPreset {
                root: split(
                    SplitAxis::Horizontal,
                    620_000,
                    tile(TRANSCRIPT_TILE),
                    tabs(vec![tile(SPECTRUM_TILE), tile(PROCESSOR_TILE)]),
                ),
            },
        ),
        (
            "Patch".to_owned(),
            LayoutPreset {
                root: split(
                    SplitAxis::Horizontal,
                    610_000,
                    tile(GRAPH_TILE),
                    tabs(vec![
                        tile(SOURCE_TILE),
                        tile(PROCESSOR_TILE),
                        tile(SINK_TILE),
                        tile(CONTROLS_TILE),
                    ]),
                ),
            },
        ),
        (
            "Diagnose".to_owned(),
            LayoutPreset {
                root: split(
                    SplitAxis::Vertical,
                    430_000,
                    split(
                        SplitAxis::Horizontal,
                        500_000,
                        tile(STATUS_TILE),
                        tile(DIAGNOSTICS_TILE),
                    ),
                    tabs(vec![
                        tile(METER_TILE),
                        tile(WAVEFORM_TILE),
                        tile(SPECTRUM_TILE),
                    ]),
                ),
            },
        ),
        (
            "Perform".to_owned(),
            LayoutPreset {
                root: split(
                    SplitAxis::Horizontal,
                    670_000,
                    tabs(vec![
                        tile(METER_TILE),
                        tile(WAVEFORM_TILE),
                        tile(SPECTRUM_TILE),
                    ]),
                    tile(TRANSCRIPT_TILE),
                ),
            },
        ),
    ]);

    let mut edits = vec![
        WorkspaceEdit::AddModule {
            instance: module(SOURCE_MODULE, synthetic::SOURCE, "Synthetic source"),
        },
        WorkspaceEdit::AddModule {
            instance: module(
                PROCESSOR_MODULE,
                synthetic::PROCESSOR,
                "Synthetic processor",
            ),
        },
        WorkspaceEdit::AddModule {
            instance: module(SINK_MODULE, synthetic::SINK, "Synthetic sink"),
        },
        WorkspaceEdit::AddEdge {
            edge: edge(SOURCE_PROCESSOR_EDGE, SOURCE_MODULE, PROCESSOR_MODULE, None),
        },
        WorkspaceEdit::AddEdge {
            edge: edge(PROCESSOR_SINK_EDGE, PROCESSOR_MODULE, SINK_MODULE, Some(8)),
        },
    ];
    edits.extend(
        bindings
            .into_iter()
            .map(|(tile_id, binding)| WorkspaceEdit::BindTile { tile_id, binding }),
    );
    edits.extend(
        presets
            .into_iter()
            .map(|(name, preset)| WorkspaceEdit::PutPreset { name, preset }),
    );
    edits.push(WorkspaceEdit::SetPromotedSetting {
        key: "studio.shell".to_owned(),
        value: json!("phase-2"),
    });
    WorkspaceEditBatch::new(edits)
}

fn rotate_first_split_ratio(node: &mut LayoutNode) -> bool {
    match node {
        LayoutNode::Split {
            ratio_millionths, ..
        } => {
            *ratio_millionths = if *ratio_millionths >= 650_000 {
                420_000
            } else {
                ratio_millionths.saturating_add(50_000).min(900_000)
            };
            true
        }
        LayoutNode::Tabs { children, .. } => children.iter_mut().any(rotate_first_split_ratio),
        LayoutNode::Tile { .. } => false,
    }
}

pub fn tile_title(tile_id: EntityId) -> &'static str {
    match tile_id {
        SOURCE_TILE => "Synthetic source",
        PROCESSOR_TILE => "Synthetic processor",
        SINK_TILE => "Synthetic sink",
        GRAPH_TILE => "Graph patch",
        CONTROLS_TILE => "Module controls",
        STATUS_TILE => "Runtime status",
        DIAGNOSTICS_TILE => "Diagnostics",
        METER_TILE => "Level meter",
        WAVEFORM_TILE => "Waveform",
        SPECTRUM_TILE => "Spectrum",
        TRANSCRIPT_TILE => "Transcript",
        _ => "Tile",
    }
}

pub fn all_tiles() -> [(EntityId, &'static str); 11] {
    [
        (SOURCE_TILE, tile_title(SOURCE_TILE)),
        (PROCESSOR_TILE, tile_title(PROCESSOR_TILE)),
        (SINK_TILE, tile_title(SINK_TILE)),
        (GRAPH_TILE, tile_title(GRAPH_TILE)),
        (CONTROLS_TILE, tile_title(CONTROLS_TILE)),
        (STATUS_TILE, tile_title(STATUS_TILE)),
        (DIAGNOSTICS_TILE, tile_title(DIAGNOSTICS_TILE)),
        (METER_TILE, tile_title(METER_TILE)),
        (WAVEFORM_TILE, tile_title(WAVEFORM_TILE)),
        (SPECTRUM_TILE, tile_title(SPECTRUM_TILE)),
        (TRANSCRIPT_TILE, tile_title(TRANSCRIPT_TILE)),
    ]
}
