use crate::{
    client::TelemetryObserverHandle,
    dense::{DenseCanvas, DenseKind},
    model::{
        all_tiles, tile_title, StudioState, CONTROLS_TILE, DIAGNOSTICS_TILE, GRAPH_TILE,
        METER_TILE, PROCESSOR_TILE, SINK_TILE, SOURCE_TILE, SPECTRUM_TILE, STATUS_TILE,
        TRANSCRIPT_TILE, WAVEFORM_TILE,
    },
};
use leptos::prelude::*;
use magnolia_client::ApplicationClient;
use magnolia_domain::{
    ControlKind, DeviceFingerprint, DeviceSelector, EntityId, LayoutNode, SplitAxis, WorkspaceEdit,
    WorkspaceEditBatch,
};
use magnolia_protocol::{
    ControlManifest, ModuleState, OperationState, SemanticCommand, SyntheticTelemetryPayload,
    TelemetrySubscription, SYNTHETIC_CAPTION_STREAM_ID, SYNTHETIC_DIAGNOSTICS_STREAM_ID,
};
use send_wrapper::SendWrapper;
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

#[component]
pub fn WorkspaceArea(state: StudioState) -> impl IntoView {
    let layout_state = state.clone();
    let layout = Memo::new(move |_| {
        if let Some((name, preset)) = layout_state.layout_draft.get() {
            if name == layout_state.active_workspace.get() {
                return Some(preset.root);
            }
        }
        let projection = layout_state.projection.get()?;
        projection
            .workspace
            .presets
            .get(&layout_state.active_workspace.get())
            .map(|preset| preset.root.clone())
    });
    view! {
        <main class="workspace-area" aria-label="Retained tile workspace" data-testid="workspace-area">
            {move || layout.get().map_or_else(
                || view! {
                    <section class="empty-workspace" data-testid="empty-workspace">
                        <p class="eyebrow">"NATIVE RUNTIME / EMPTY DOCUMENT"</p>
                        <h2>"Build the synthetic patch"</h2>
                        <p>
                            "Create the source, processor, sink, tile bindings, and retained workspace presets as one typed document command."
                        </p>
                        <button
                            type="button"
                            class="button primary"
                            data-testid="load-demo"
                            on:click={
                                let state = state.clone();
                                move |_| state.load_demo()
                            }
                        >"Load synthetic cockpit"</button>
                        <AudioControls state=state.clone() />
                    </section>
                }.into_any(),
                |node| view! {
                    <LayoutRenderer
                        state=state.clone()
                        node
                        visible=Signal::derive(|| true)
                    />
                }.into_any(),
            )}
        </main>
    }
}

#[component]
fn LayoutRenderer(state: StudioState, node: LayoutNode, visible: Signal<bool>) -> impl IntoView {
    match node {
        LayoutNode::Tile { tile_id } => view! {
            <TileSurface state tile_id visible />
        }
        .into_any(),
        LayoutNode::Split {
            axis,
            ratio_millionths,
            first,
            second,
        } => {
            let ratio = f64::from(ratio_millionths) / 10_000.0;
            let first_style = format!("flex-basis:{ratio:.2}%");
            let second_style = format!("flex-basis:{:.2}%", 100.0 - ratio);
            let axis_class = match axis {
                SplitAxis::Horizontal => "horizontal",
                SplitAxis::Vertical => "vertical",
            };
            view! {
                <div
                    class=format!("layout-split {axis_class}")
                    data-layout-kind="split"
                    data-split-axis=axis_class
                >
                    <div class="split-child" style=first_style>
                        <LayoutRenderer
                            state=state.clone()
                            node=*first
                            visible
                        />
                    </div>
                    <div class="split-divider" aria-hidden="true"></div>
                    <div class="split-child" style=second_style>
                        <LayoutRenderer state node=*second visible />
                    </div>
                </div>
            }
            .into_any()
        }
        LayoutNode::Tabs { active, children } => {
            let active = RwSignal::new(active.min(children.len().saturating_sub(1)));
            let titles: Vec<_> = children.iter().map(layout_title).collect();
            let tab_buttons = titles
                .into_iter()
                .enumerate()
                .map(|(index, title)| {
                    view! {
                        <button
                            type="button"
                            role="tab"
                            class:active=move || active.get() == index
                            aria-selected=move || (active.get() == index).to_string()
                            data-testid=format!("tab-{title}")
                            on:click=move |_| active.set(index)
                        >{title}</button>
                    }
                })
                .collect_view();
            let tab_children = children
                .into_iter()
                .enumerate()
                .map(|(index, child)| {
                    let child_visible =
                        Signal::derive(move || visible.get() && active.get() == index);
                    view! {
                        <div
                            class="tab-panel"
                            role="tabpanel"
                            hidden=move || active.get() != index
                        >
                            <LayoutRenderer
                                state=state.clone()
                                node=child
                                visible=child_visible
                            />
                        </div>
                    }
                })
                .collect_view();
            view! {
                <section class="layout-tabs" data-layout-kind="tabs">
                    <div class="tab-strip" role="tablist">{tab_buttons}</div>
                    <div class="tab-panels">{tab_children}</div>
                </section>
            }
            .into_any()
        }
    }
}

fn layout_title(node: &LayoutNode) -> &'static str {
    match node {
        LayoutNode::Tile { tile_id } => tile_title(*tile_id),
        LayoutNode::Tabs { .. } => "Tabs",
        LayoutNode::Split { .. } => "Split",
    }
}

#[component]
fn TileSurface(state: StudioState, tile_id: EntityId, visible: Signal<bool>) -> impl IntoView {
    let tile_visible_state = state.clone();
    let tile_visible = Signal::derive(move || {
        visible.get() && !tile_visible_state.hidden_tiles.get().contains(&tile_id)
    });
    let focused_state = state.clone();
    let focus_state = state.clone();
    let click_state = state.clone();
    let close_state = state.clone();
    let show_state = state.clone();
    let closed_state = state.clone();
    let reopen_state = state.clone();
    let body_state = state;
    view! {
        <div
            class="closed-tile"
            hidden=move || !closed_state.hidden_tiles.get().contains(&tile_id)
        >
            <span>{format!("{} closed", tile_title(tile_id))}</span>
            <button
                type="button"
                on:click=move |_| reopen_state.show_tile(tile_id)
            >"Reopen"</button>
        </div>
        <article
            class="tile-surface"
            class:focused=move || focused_state.focused_tile.get() == Some(tile_id)
            hidden=move || show_state.hidden_tiles.get().contains(&tile_id)
            tabindex="0"
            data-tile-id=tile_id.to_string()
            data-testid=format!("tile-{tile_id}")
            aria-label=tile_title(tile_id)
            on:focus=move |_| focus_state.focused_tile.set(Some(tile_id))
            on:click=move |_| click_state.focused_tile.set(Some(tile_id))
        >
            <header class="tile-header">
                <div>
                    <span class="tile-grip" aria-hidden="true">"⠿"</span>
                    <h2>{tile_title(tile_id)}</h2>
                </div>
                <button
                    type="button"
                    class="tile-close"
                    aria-label=format!("Close {}", tile_title(tile_id))
                    data-testid=format!("close-tile-{tile_id}")
                    on:click=move |event| {
                        event.stop_propagation();
                        close_state.hide_tile(tile_id);
                    }
                >"×"</button>
            </header>
            <div class="tile-body">
                <TileBody state=body_state tile_id visible=tile_visible />
            </div>
        </article>
    }
}

#[component]
fn TileBody(state: StudioState, tile_id: EntityId, visible: Signal<bool>) -> impl IntoView {
    match tile_id {
        SOURCE_TILE | PROCESSOR_TILE | SINK_TILE => {
            view! { <ModuleTile state tile_id /> }.into_any()
        }
        GRAPH_TILE => view! { <GraphTile state /> }.into_any(),
        CONTROLS_TILE => view! { <ControlsTile state /> }.into_any(),
        STATUS_TILE => view! { <StatusTile state /> }.into_any(),
        DIAGNOSTICS_TILE => view! { <DiagnosticsTile state visible /> }.into_any(),
        METER_TILE => view! {
            <DenseCanvas
                state
                stream_id=magnolia_protocol::SYNTHETIC_METER_STREAM_ID
                kind=DenseKind::Meter
                visible
            />
        }
        .into_any(),
        WAVEFORM_TILE => view! {
            <DenseCanvas
                state
                stream_id=magnolia_protocol::SYNTHETIC_WAVEFORM_STREAM_ID
                kind=DenseKind::Waveform
                visible
            />
        }
        .into_any(),
        SPECTRUM_TILE => view! {
            <DenseCanvas
                state
                stream_id=magnolia_protocol::SYNTHETIC_SPECTRUM_STREAM_ID
                kind=DenseKind::Spectrum
                visible
            />
        }
        .into_any(),
        TRANSCRIPT_TILE => view! { <TranscriptTile state visible /> }.into_any(),
        _ => view! { <p>"Unknown tile binding"</p> }.into_any(),
    }
}

#[component]
fn ModuleTile(state: StudioState, tile_id: EntityId) -> impl IntoView {
    let module_id = match tile_id {
        SOURCE_TILE => crate::model::SOURCE_MODULE,
        PROCESSOR_TILE => crate::model::PROCESSOR_MODULE,
        _ => crate::model::SINK_MODULE,
    };
    view! {
        {move || {
            let projection = state.projection.get();
            let status = projection.as_ref().and_then(|projection| {
                projection.modules.iter().find(|module| module.module_id == module_id)
            });
            let instance = projection.as_ref().and_then(|projection| {
                projection.workspace.graph.modules.get(&module_id)
            });
            view! {
                <div class="module-summary" data-module-id=module_id.to_string()>
                    <div class="module-orbit" aria-hidden="true">
                        <span></span><span></span><span></span>
                    </div>
                    <dl>
                        <dt>"Identity"</dt>
                        <dd><code>{module_id.to_string()}</code></dd>
                        <dt>"Type"</dt>
                        <dd>{instance.map_or_else(|| "Unavailable".to_owned(), |module| module.module_type.to_string())}</dd>
                        <dt>"Lifecycle"</dt>
                        <dd>{status.map_or_else(|| "Unknown".to_owned(), |status| format!("{:?}", status.state))}</dd>
                    </dl>
                </div>
            }
        }}
    }
}

#[component]
fn GraphTile(state: StudioState) -> impl IntoView {
    view! {
        <div class="graph-patch" data-testid="graph-patch">
            {move || {
                let projection = state.projection.get();
                projection.map(|projection| {
                    let modules = projection.workspace.graph.modules.values().cloned().collect::<Vec<_>>();
                    let edges = projection.workspace.graph.edges.values().cloned().collect::<Vec<_>>();
                    view! {
                        <div class="patch-nodes">
                            {modules.into_iter().map(|module| view! {
                                <article class="patch-node" data-module-id=module.id.to_string()>
                                    <span class="node-status"></span>
                                    <strong>{module.module_type.to_string()}</strong>
                                    <small>{module.id.to_string()}</small>
                                </article>
                            }).collect_view()}
                        </div>
                        <ol class="patch-edges" aria-label="Graph edges">
                            {edges.into_iter().map(|edge| view! {
                                <li>
                                    <code>{edge.from.module_id.to_string()}</code>
                                    <span aria-hidden="true">"→"</span>
                                    <code>{edge.to.module_id.to_string()}</code>
                                    <span>{edge.capacity.map_or_else(|| "same lane".to_owned(), |capacity| format!("bounded {capacity}"))}</span>
                                </li>
                            }).collect_view()}
                        </ol>
                    }
                })
            }}
        </div>
    }
}

#[component]
fn ControlsTile(state: StudioState) -> impl IntoView {
    let manifest_state = state.clone();
    let manifests = Memo::new(move |_| {
        manifest_state
            .projection
            .get()
            .map(|projection| {
                projection
                    .control_manifests
                    .values()
                    .flatten()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    view! {
        <div class="manifest-controls" data-testid="manifest-controls">
            {move || {
                manifests.get().into_iter().map(|manifest| {
                    view! { <ManifestControl state=state.clone() manifest /> }
                }).collect_view()
            }}
        </div>
    }
}

#[component]
fn ManifestControl(state: StudioState, manifest: ControlManifest) -> impl IntoView {
    let address = format!("{}.{}", manifest.module_id, manifest.control_id);
    let label = manifest.label.clone();
    let disabled = manifest.availability == magnolia_protocol::ControlAvailability::Unavailable;
    let control = match manifest.kind.clone() {
        ControlKind::Toggle => {
            let module_id = manifest.module_id;
            let control_id = manifest.control_id.clone();
            let dispatch = state.clone();
            view! {
                <input
                    type="checkbox"
                    prop:checked=manifest.value.as_bool().unwrap_or(false)
                    disabled=disabled
                    data-testid=format!("control-{}-{}", module_id, control_id)
                    on:change=move |event| dispatch.set_control(
                        module_id,
                        control_id.clone(),
                        Value::Bool(event_target_checked(&event)),
                    )
                />
            }
            .into_any()
        }
        ControlKind::Number => {
            let module_id = manifest.module_id;
            let control_id = manifest.control_id.clone();
            let dispatch = state.clone();
            view! {
                <input
                    type="number"
                    step="0.1"
                    prop:value=manifest.value.as_f64().unwrap_or(0.0).to_string()
                    disabled=disabled
                    data-testid=format!("control-{}-{}", module_id, control_id)
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<f64>() {
                            dispatch.set_control(
                                module_id,
                                control_id.clone(),
                                serde_json::json!(value),
                            );
                        }
                    }
                />
            }
            .into_any()
        }
        ControlKind::Choice { options } => {
            let module_id = manifest.module_id;
            let control_id = manifest.control_id.clone();
            let dispatch = state.clone();
            let current = manifest.value.as_str().unwrap_or_default().to_owned();
            view! {
                <select
                    disabled=disabled
                    data-testid=format!("control-{}-{}", module_id, control_id)
                    on:change=move |event| dispatch.set_control(
                        module_id,
                        control_id.clone(),
                        Value::String(event_target_value(&event)),
                    )
                >
                    {options.into_iter().map(|option| {
                        let selected = option == current;
                        let label = option.clone();
                        view! { <option value=option selected=selected>{label}</option> }
                    }).collect_view()}
                </select>
            }
            .into_any()
        }
        ControlKind::Text => {
            let module_id = manifest.module_id;
            let control_id = manifest.control_id.clone();
            let dispatch = state.clone();
            let focus = state.clone();
            let blur = state.clone();
            view! {
                <input
                    type="text"
                    prop:value=manifest.value.as_str().unwrap_or_default().to_owned()
                    disabled=disabled
                    data-testid=format!("control-{}-{}", module_id, control_id)
                    on:focus={
                        let address = address.clone();
                        move |_| focus.focused_control.set(Some(address.clone()))
                    }
                    on:blur=move |_| blur.focused_control.set(None)
                    on:change=move |event| dispatch.set_control(
                        module_id,
                        control_id.clone(),
                        Value::String(event_target_value(&event)),
                    )
                />
            }
            .into_any()
        }
        ControlKind::Trigger => {
            let module_id = manifest.module_id;
            let control_id = manifest.control_id.clone();
            let dispatch = state.clone();
            view! {
                <button
                    type="button"
                    disabled=disabled
                    on:click=move |_| dispatch.set_control(
                        module_id,
                        control_id.clone(),
                        Value::Null,
                    )
                >"Run"</button>
            }
            .into_any()
        }
    };
    view! {
        <label class="manifest-control" data-control-address=address>
            <span>{label}</span>
            {control}
            <small>{manifest.module_id.to_string()}</small>
        </label>
    }
}

#[component]
fn StatusTile(state: StudioState) -> impl IntoView {
    view! {
        <div class="status-grid" data-testid="runtime-status-tile">
            {move || state.projection.get().map(|projection| {
                let pending = projection.operations.iter().filter(|operation| operation.state == OperationState::Pending).count();
                let failed = projection.operations.iter().filter(|operation| operation.state == OperationState::Failed).count();
                let active_modules = projection.modules.iter().filter(|module| module.state == ModuleState::Active).count();
                view! {
                    <StatusDatum label="Runtime epoch" value=projection.runtime_epoch.to_string() test_id="runtime-identity" />
                    <StatusDatum label="Document" value=projection.document_revision.to_string() test_id="status-document-revision" />
                    <StatusDatum label="Target graph" value=projection.target_graph_revision.to_string() test_id="status-target-revision" />
                    <StatusDatum label="Active graph" value=projection.active_graph_revision.to_string() test_id="status-active-revision" />
                    <StatusDatum label="Pending operations" value=pending.to_string() test_id="status-pending-operations" />
                    <StatusDatum label="Failed operations" value=failed.to_string() test_id="status-failed-operations" />
                    <StatusDatum label="Active modules" value=active_modules.to_string() test_id="status-active-modules" />
                }
            })}
        </div>
    }
}

#[component]
fn StatusDatum(label: &'static str, value: String, test_id: &'static str) -> impl IntoView {
    view! {
        <div class="status-datum">
            <span>{label}</span>
            <strong data-testid=test_id>{value}</strong>
        </div>
    }
}

#[component]
fn DiagnosticsTile(state: StudioState, visible: Signal<bool>) -> impl IntoView {
    let telemetry_entries = RwSignal::new(VecDeque::<String>::new());
    let telemetry_dropped = RwSignal::new(0_u64);
    let observer = Rc::new(RefCell::new(None::<TelemetryObserverHandle>));
    let leased = Rc::new(Cell::new(false));
    let effect_observer = Rc::clone(&observer);
    let effect_leased = Rc::clone(&leased);
    let effect_state = state.clone();
    Effect::new(move || {
        if visible.get() && !effect_leased.replace(true) {
            let client = effect_state.client.clone();
            let callback = Rc::new(
                move |envelope: magnolia_protocol::TelemetryEnvelope,
                      payload: SyntheticTelemetryPayload| {
                    telemetry_dropped.set(envelope.cumulative_dropped);
                    if let SyntheticTelemetryPayload::Diagnostics {
                        entries,
                        lost_since_previous,
                    } = payload
                    {
                        telemetry_entries.update(|current| {
                            for entry in entries {
                                current
                                    .push_back(format!("{} · {}", entry.sequence, entry.message));
                            }
                            if lost_since_previous > 0 {
                                current
                                    .push_back(format!("{lost_since_previous} diagnostics lost"));
                            }
                            while current.len() > 32 {
                                current.pop_front();
                            }
                        });
                    }
                },
            );
            *effect_observer.borrow_mut() =
                Some(client.observe_telemetry(SYNTHETIC_DIAGNOSTICS_STREAM_ID, callback));
            leptos::task::spawn_local(async move {
                let _ = client
                    .subscribe_telemetry(TelemetrySubscription {
                        stream_id: SYNTHETIC_DIAGNOSTICS_STREAM_ID,
                        requested_rate_hz: 10,
                        capacity: 16,
                        delivery: magnolia_domain::DeliveryPolicy::DropOldest,
                    })
                    .await;
            });
        } else if !visible.get_untracked() && effect_leased.replace(false) {
            effect_observer.borrow_mut().take();
            let client = effect_state.client.clone();
            leptos::task::spawn_local(async move {
                let _ = client
                    .release_telemetry(SYNTHETIC_DIAGNOSTICS_STREAM_ID)
                    .await;
            });
        }
    });
    let cleanup_observer = Rc::clone(&observer);
    let cleanup_leased = Rc::clone(&leased);
    let cleanup_client = state.client.clone();
    let cleanup = SendWrapper::new(move || {
        cleanup_observer.borrow_mut().take();
        if cleanup_leased.replace(false) {
            let client = cleanup_client.clone();
            leptos::task::spawn_local(async move {
                let _ = client
                    .release_telemetry(SYNTHETIC_DIAGNOSTICS_STREAM_ID)
                    .await;
            });
        }
    });
    on_cleanup(move || cleanup.take()());

    view! {
        <div class="diagnostics-tile" data-testid="diagnostics-tile">
            <AudioControls state=state.clone() />
            <div class="diagnostic-counters">
                {move || state.projection.get().map(|projection| {
                    projection.diagnostics.counters.iter().map(|(name, value)| view! {
                        <div><span>{name.clone()}</span><strong>{value.to_string()}</strong></div>
                    }).collect_view()
                })}
                <div><span>"binary stream drops"</span><strong data-testid="diagnostics-drops">{move || telemetry_dropped.get()}</strong></div>
            </div>
            <ol class="event-list" data-testid="diagnostics-events">
                {move || telemetry_entries.get().into_iter().rev().map(|entry| view! { <li>{entry}</li> }).collect_view()}
            </ol>
            <h3>"Application history"</h3>
            <ol class="event-list" data-testid="application-events">
                {move || state.events.get().into_iter().rev().take(20).map(|entry| view! { <li>{entry}</li> }).collect_view()}
            </ol>
        </div>
    }
}

#[component]
fn AudioControls(state: StudioState) -> impl IntoView {
    let start = state.clone();
    let stop = state.clone();
    let capture_mute = state.clone();
    let monitor_enable = state.clone();
    let monitor_mute = state.clone();
    let gain_zero = state.clone();
    let gain_safe = state.clone();
    let projection = state.clone();
    let devices = state.clone();
    let follow_default = state.clone();
    view! {
        <section class="audio-controls" data-testid="audio-controls">
            <h3>"Native audio runtime"</h3>
            <div class="control-actions">
                <button type="button" on:click=move |_| start.dispatch(SemanticCommand::StartAudio)>"Start capture"</button>
                <button type="button" on:click=move |_| stop.dispatch(SemanticCommand::StopAudio)>"Stop"</button>
                <button type="button" on:click=move |_| {
                    let muted = capture_mute.projection.get_untracked().is_some_and(|value| value.audio.capture_muted);
                    capture_mute.dispatch(SemanticCommand::SetCaptureMuted { muted: !muted });
                }>"Capture mute"</button>
                <button type="button" on:click=move |_| {
                    let enabled = monitor_enable.projection.get_untracked().is_some_and(|value| value.audio.monitor_enabled);
                    monitor_enable.dispatch(SemanticCommand::SetMonitorEnabled { enabled: !enabled });
                }>"Monitor enable"</button>
                <button type="button" on:click=move |_| {
                    let muted = monitor_mute.projection.get_untracked().is_none_or(|value| value.audio.monitor_muted);
                    monitor_mute.dispatch(SemanticCommand::SetMonitorMuted { muted: !muted });
                }>"Monitor mute"</button>
                <button type="button" on:click=move |_| gain_zero.dispatch(SemanticCommand::SetMonitorGain { linear_millionths: 0 })>"Gain 0"</button>
                <button type="button" on:click=move |_| gain_safe.dispatch(SemanticCommand::SetMonitorGain { linear_millionths: 30_000 })>"Gain 3%"</button>
            </div>
            <div class="control-actions" data-testid="audio-input-devices">
                <button type="button" on:click=move |_| follow_default.dispatch(
                    SemanticCommand::ApplyWorkspaceEdit {
                        batch: WorkspaceEditBatch::new(vec![WorkspaceEdit::SetDeviceSelector {
                            key: "audio.input".to_owned(),
                            selector: DeviceSelector::FollowDefaultInput,
                        }]),
                    }
                )>"Follow default input"</button>
                {move || devices.projection.get().into_iter().flat_map(|projection| {
                    projection.audio.available_devices.iter().filter(|device| {
                        device.direction == magnolia_protocol::AudioDeviceDirection::Input
                    }).cloned().map(|device| {
                        let selection_state = devices.clone();
                        let label = if device.is_default {
                            format!("{} (default)", device.label)
                        } else {
                            device.label.clone()
                        };
                        view! {
                            <button type="button" on:click=move |_| selection_state.dispatch(
                                SemanticCommand::ApplyWorkspaceEdit {
                                    batch: WorkspaceEditBatch::new(vec![WorkspaceEdit::SetDeviceSelector {
                                        key: "audio.input".to_owned(),
                                        selector: DeviceSelector::Exact {
                                            fingerprint: DeviceFingerprint {
                                                node_name: device.node_name.clone(),
                                                device_api: device.device_api.clone(),
                                                object_path: device.object_path.clone(),
                                            },
                                        },
                                    }]),
                                }
                            )>{label}</button>
                        }
                    }).collect::<Vec<_>>()
                }).collect_view()}
            </div>
            <pre data-testid="audio-runtime-status">{move || projection.projection.get().map_or_else(
                || "audio projection pending".to_owned(),
                |value| format!(
                    "state={:?} format={:?} rate={:?} channels={:?}/{:?} quantum={:?} runtime_rev={} callbacks={} p99_ns={} p999_ns={} underruns={} drops={} discontinuities={} monitor={}/{}/{} error={}",
                    value.audio.state,
                    value.audio.sample_format,
                    value.audio.sample_rate,
                    value.audio.channels,
                    value.audio.channel_positions,
                    value.audio.quantum_frames,
                    value.audio.runtime_revision,
                    value.audio.callback_count,
                    value.audio.callback_p99_ns,
                    value.audio.callback_p999_ns,
                    value.audio.underruns,
                    value.audio.dropped_frames,
                    value.audio.discontinuities,
                    value.audio.monitor_enabled,
                    value.audio.monitor_muted,
                    value.audio.monitor_gain_millionths,
                    value.audio.last_error.clone().unwrap_or_else(|| "none".to_owned()),
                ),
            )}</pre>
        </section>
    }
}

#[component]
fn TranscriptTile(state: StudioState, visible: Signal<bool>) -> impl IntoView {
    let partial = RwSignal::new(None::<(EntityId, u64, String)>);
    let observer = Rc::new(RefCell::new(None::<TelemetryObserverHandle>));
    let leased = Rc::new(Cell::new(false));
    let effect_observer = Rc::clone(&observer);
    let effect_leased = Rc::clone(&leased);
    let effect_state = state.clone();
    Effect::new(move || {
        if visible.get() && !effect_leased.replace(true) {
            let client = effect_state.client.clone();
            let callback = Rc::new(
                move |_envelope: magnolia_protocol::TelemetryEnvelope,
                      payload: SyntheticTelemetryPayload| {
                    if let SyntheticTelemetryPayload::PartialCaption {
                        segment_id,
                        segment_revision,
                        text,
                    } = payload
                    {
                        let replace =
                            partial
                                .get_untracked()
                                .is_none_or(|(current_id, revision, _)| {
                                    current_id != segment_id || segment_revision >= revision
                                });
                        if replace {
                            partial.set(Some((segment_id, segment_revision, text)));
                        }
                    }
                },
            );
            *effect_observer.borrow_mut() =
                Some(client.observe_telemetry(SYNTHETIC_CAPTION_STREAM_ID, callback));
            leptos::task::spawn_local(async move {
                let _ = client
                    .subscribe_telemetry(TelemetrySubscription {
                        stream_id: SYNTHETIC_CAPTION_STREAM_ID,
                        requested_rate_hz: 12,
                        capacity: 4,
                        delivery: magnolia_domain::DeliveryPolicy::Latest,
                    })
                    .await;
            });
        } else if !visible.get_untracked() && effect_leased.replace(false) {
            effect_observer.borrow_mut().take();
            let client = effect_state.client.clone();
            leptos::task::spawn_local(async move {
                let _ = client.release_telemetry(SYNTHETIC_CAPTION_STREAM_ID).await;
            });
        }
    });
    let cleanup_observer = Rc::clone(&observer);
    let cleanup_leased = Rc::clone(&leased);
    let cleanup_client = state.client.clone();
    let cleanup = SendWrapper::new(move || {
        cleanup_observer.borrow_mut().take();
        if cleanup_leased.replace(false) {
            let client = cleanup_client.clone();
            leptos::task::spawn_local(async move {
                let _ = client.release_telemetry(SYNTHETIC_CAPTION_STREAM_ID).await;
            });
        }
    });
    on_cleanup(move || cleanup.take()());

    view! {
        <div class="transcript-tile" data-testid="transcript-tile">
            <div class="partial-caption" aria-live="polite" data-testid="partial-caption">
                <span class="caption-state">"PARTIAL"</span>
                {move || partial.get().map_or_else(|| "Waiting for synthetic speech…".to_owned(), |(_, _, text)| text)}
            </div>
            <ol class="transcript-finals" aria-label="Final transcript entries">
                {move || state.projection.get().map(|projection| {
                    projection.transcript.recent.iter().cloned().map(|segment| view! {
                        <li data-sequence=segment.sequence.to_string()>
                            <span>{format!("{:02}", segment.sequence)}</span>
                            <p>{segment.text}</p>
                        </li>
                    }).collect_view()
                })}
            </ol>
            <footer>
                <span data-testid="transcript-revision">{move || state.projection.get().map_or(0, |projection| projection.transcript.revision.get())}</span>
                <span data-testid="transcript-count">{move || state.projection.get().map_or(0, |projection| projection.transcript.final_segment_count)}</span>
            </footer>
        </div>
    }
}

#[component]
pub fn TilePalette(state: StudioState) -> impl IntoView {
    view! {
        <aside class="tile-palette" aria-label="Tile palette">
            <h2>"Tiles"</h2>
            <p>"Presentation surfaces; reopening never changes module lifecycle."</p>
            <div class="palette-list">
                {all_tiles().into_iter().map(|(tile_id, title)| {
                    let state = state.clone();
                    view! {
                        <button
                            type="button"
                            data-testid=format!("open-tile-{tile_id}")
                            on:click=move |_| state.show_tile(tile_id)
                        >{title}</button>
                    }
                }).collect_view()}
            </div>
        </aside>
    }
}
