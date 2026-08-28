use crate::{
    client::{ConnectionPhase, WebSocketApplicationClient},
    model::{all_tiles, StudioState, WORKSPACES},
    workspace::{TilePalette, WorkspaceArea},
};
use leptos::{ev, prelude::*};
use magnolia_protocol::SemanticCommand;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;

#[derive(Debug, Clone, Copy)]
struct CommandDefinition {
    id: &'static str,
    label: &'static str,
    shortcut: &'static str,
    description: &'static str,
}

const COMMANDS: [CommandDefinition; 13] = [
    CommandDefinition {
        id: "workspace.load_demo",
        label: "Load synthetic cockpit",
        shortcut: "G",
        description: "Create the typed source, processor, sink, edges, tiles, and presets",
    },
    CommandDefinition {
        id: "layout.preview",
        label: "Preview split adjustment",
        shortcut: "L",
        description: "Create a disposable local layout draft without touching the document",
    },
    CommandDefinition {
        id: "layout.commit",
        label: "Commit layout draft",
        shortcut: "Shift+L",
        description: "Commit the local draft as a document-only semantic command",
    },
    CommandDefinition {
        id: "layout.discard",
        label: "Discard layout draft",
        shortcut: "",
        description: "Return to the committed authoritative workspace preset",
    },
    CommandDefinition {
        id: "history.undo",
        label: "Undo document command",
        shortcut: "U",
        description: "Undo through the authoritative application history",
    },
    CommandDefinition {
        id: "history.redo",
        label: "Redo document command",
        shortcut: "Shift+U",
        description: "Redo through the authoritative application history",
    },
    CommandDefinition {
        id: "receipt.retry",
        label: "Retry last envelope",
        shortcut: "R",
        description: "Prove receipt replay without re-executing the command",
    },
    CommandDefinition {
        id: "focus.next",
        label: "Focus next tile",
        shortcut: "F6",
        description: "Move visible browser focus to the next retained tile",
    },
    CommandDefinition {
        id: "workspace.capture",
        label: "Switch to Capture",
        shortcut: "Alt+1",
        description: "Change disposable presentation without recreating the runtime",
    },
    CommandDefinition {
        id: "workspace.transcribe",
        label: "Switch to Transcribe",
        shortcut: "Alt+2",
        description: "Change disposable presentation without recreating the runtime",
    },
    CommandDefinition {
        id: "workspace.patch",
        label: "Switch to Patch",
        shortcut: "Alt+3",
        description: "Change disposable presentation without recreating the runtime",
    },
    CommandDefinition {
        id: "workspace.diagnose",
        label: "Switch to Diagnose",
        shortcut: "Alt+4",
        description: "Change disposable presentation without recreating the runtime",
    },
    CommandDefinition {
        id: "workspace.perform",
        label: "Switch to Perform",
        shortcut: "Alt+5",
        description: "Change disposable presentation without recreating the runtime",
    },
];

#[component]
pub fn App() -> impl IntoView {
    let initialized = WebSocketApplicationClient::from_window()
        .and_then(|client| {
            WebSocketApplicationClient::client_id_from_window().map(|id| (client, id))
        })
        .map_err(|error| error.to_string())
        .and_then(|(client, client_id)| StudioState::new(client, client_id));
    match initialized {
        Ok(state) => {
            state.connect();
            view! { <StudioShell state /> }.into_any()
        }
        Err(error) => view! {
            <main class="startup-error" role="alert">
                <p class="eyebrow">"MAGNOLIA / SESSION ERROR"</p>
                <h1>"The native cockpit could not start"</h1>
                <p>{error}</p>
                <p>"Launch this page through magnolia-desktop so it receives a one-time fragment authority."</p>
            </main>
        }
        .into_any(),
    }
}

#[component]
fn StudioShell(state: StudioState) -> impl IntoView {
    let shortcut_state = state.clone();
    let shortcut_listener = window_event_listener(ev::keydown, move |event| {
        let typing = event_target_is_text_entry(&event);
        if let Some(command) = command_for_key(&event, typing) {
            event.prevent_default();
            dispatch_registry(&shortcut_state, command);
        }
    });
    let shortcut_cleanup = SendWrapper::new(move || shortcut_listener.remove());
    on_cleanup(move || shortcut_cleanup.take()());

    view! {
        <div class="studio-shell" data-testid="studio-shell">
            <Header state=state.clone() />
            <div class="studio-main">
                <ModuleRail state=state.clone() />
                <WorkspaceArea state=state.clone() />
                <TilePalette state=state.clone() />
            </div>
            <DiagnosticStrip state=state.clone() />
            <CommandPalette state />
        </div>
    }
}

#[component]
fn Header(state: StudioState) -> impl IntoView {
    view! {
        <header class="app-header">
            <div class="brand">
                <span class="brand-glyph" aria-hidden="true">"M"</span>
                <div>
                    <p>"MAGNOLIA"</p>
                    <span>"native mock runtime / Leptos cockpit"</span>
                </div>
            </div>
            <nav class="workspace-switcher" aria-label="Workspace presets">
                {WORKSPACES.into_iter().enumerate().map(|(index, workspace)| {
                    let command = match workspace {
                        "Capture" => "workspace.capture",
                        "Transcribe" => "workspace.transcribe",
                        "Patch" => "workspace.patch",
                        "Diagnose" => "workspace.diagnose",
                        _ => "workspace.perform",
                    };
                    let active_state = state.clone();
                    let current_state = state.clone();
                    let click_state = state.clone();
                    view! {
                        <button
                            type="button"
                            class:active=move || active_state.active_workspace.get() == workspace
                            aria-current=move || (current_state.active_workspace.get() == workspace).then_some("page")
                            data-testid=format!("workspace-{}", workspace.to_lowercase())
                            title=format!("Alt+{}", index + 1)
                            on:click=move |_| dispatch_registry(&click_state, command)
                        >{workspace}</button>
                    }
                }).collect_view()}
            </nav>
            <div class="header-actions">
                <ConnectionBadge state=state.clone() />
                <span
                    class="draft-badge"
                    data-testid="layout-draft"
                    hidden={
                        let state = state.clone();
                        move || state.layout_draft.get().is_none()
                    }
                >"LOCAL DRAFT"</span>
                <button
                    type="button"
                    class="button quiet"
                    data-testid="preview-layout"
                    on:click={
                        let state = state.clone();
                        move |_| dispatch_registry(&state, "layout.preview")
                    }
                >"Preview split"</button>
                <button
                    type="button"
                    class="button quiet"
                    data-testid="commit-layout"
                    disabled={
                        let state = state.clone();
                        move || state.layout_draft.get().is_none()
                    }
                    on:click={
                        let state = state.clone();
                        move |_| dispatch_registry(&state, "layout.commit")
                    }
                >"Commit"</button>
                <button
                    type="button"
                    class="button command-trigger"
                    aria-haspopup="dialog"
                    data-testid="open-command-palette"
                    on:click={
                        let state = state.clone();
                        move |_| state.palette_open.set(true)
                    }
                >
                    <span>"Commands"</span><kbd>"Ctrl K"</kbd>
                </button>
            </div>
        </header>
    }
}

#[component]
fn ConnectionBadge(state: StudioState) -> impl IntoView {
    let class_state = state.clone();
    let phase_state = state.clone();
    let label_state = state.clone();
    view! {
        <div
            class="connection-badge"
            class:connected=move || class_state.phase.get() == ConnectionPhase::Connected
            data-testid="connection-state"
            data-phase=move || phase_label(phase_state.phase.get())
            role="status"
        >
            <span class="health-dot" aria-hidden="true"></span>
            <div>
                <strong>{move || phase_label(label_state.phase.get())}</strong>
                <small>{move || state.protocol_version.get().map_or_else(
                    || "protocol pending".to_owned(),
                    |version| format!("protocol {}.{}", version.major, version.minor),
                )}</small>
            </div>
        </div>
    }
}

fn phase_label(phase: ConnectionPhase) -> &'static str {
    match phase {
        ConnectionPhase::Disconnected => "disconnected",
        ConnectionPhase::Connecting => "connecting",
        ConnectionPhase::Connected => "connected",
        ConnectionPhase::Reconnecting => "reconnecting",
        ConnectionPhase::Rejected => "rejected",
    }
}

#[component]
fn ModuleRail(state: StudioState) -> impl IntoView {
    let disable_state = state.clone();
    let modules_state = state.clone();
    view! {
        <aside class="module-rail" aria-label="Module and command palette">
            <div class="rail-heading">
                <p class="eyebrow">"PATCH BAY"</p>
                <h2>"Modules"</h2>
            </div>
            <button
                type="button"
                class="module-create"
                data-testid="rail-load-demo"
                disabled=move || disable_state.projection.get().is_some_and(|projection| !projection.workspace.graph.modules.is_empty())
                on:click={
                    let state = state.clone();
                    move |_| dispatch_registry(&state, "workspace.load_demo")
                }
            >
                <span aria-hidden="true">"＋"</span>
                <span><strong>"Synthetic chain"</strong><small>"source → processor → sink"</small></span>
            </button>
            <div class="module-list">
                {move || modules_state.projection.get().map(|projection| {
                    projection.modules.iter().cloned().map(|module| {
                        let select_state = modules_state.clone();
                        view! {
                            <button
                                type="button"
                                class="module-row"
                                data-module-id=module.module_id.to_string()
                                on:click=move |_| select_state.push_event(format!("Selected module {}", module.module_id))
                            >
                                <span class="module-kind">{module.module_type.to_string().replace("synthetic.", "")}</span>
                                <strong>{module.module_type.to_string()}</strong>
                                <small>{format!("{:?}", module.state)}</small>
                            </button>
                        }
                    }).collect_view()
                })}
            </div>
            <section class="rail-commands">
                <h3>"Document"</h3>
                <button type="button" on:click={
                    let state = state.clone();
                    move |_| dispatch_registry(&state, "history.undo")
                }>"Undo" <kbd>"U"</kbd></button>
                <button type="button" on:click={
                    let state = state.clone();
                    move |_| dispatch_registry(&state, "history.redo")
                }>"Redo" <kbd>"⇧ U"</kbd></button>
                <button type="button" data-testid="retry-receipt" on:click={
                    let state = state.clone();
                    move |_| dispatch_registry(&state, "receipt.retry")
                }>"Retry receipt" <kbd>"R"</kbd></button>
            </section>
        </aside>
    }
}

#[component]
fn DiagnosticStrip(state: StudioState) -> impl IntoView {
    let doc_state = state.clone();
    let target_state = state.clone();
    let active_state = state.clone();
    let projection_state = state.clone();
    let runtime_state = state.clone();
    let receipt_state = state.clone();
    let tile_state = state.clone();
    let control_state = state.clone();
    view! {
        <footer class="diagnostic-strip" aria-live="polite">
            <div class="revision-cluster">
                <span>"DOC" <strong data-testid="document-revision">{move || doc_state.projection.get().map_or(0, |projection| projection.document_revision.get())}</strong></span>
                <span>"TARGET" <strong data-testid="target-revision">{move || target_state.projection.get().map_or(0, |projection| projection.target_graph_revision.get())}</strong></span>
                <span>"ACTIVE" <strong data-testid="active-revision">{move || active_state.projection.get().map_or(0, |projection| projection.active_graph_revision.get())}</strong></span>
                <span>"PROJECTION" <strong data-testid="projection-revision">{move || projection_state.projection.get().map_or(0, |projection| projection.revision.get())}</strong></span>
                <span>"RUNTIME" <strong data-testid="runtime-epoch">{move || runtime_state.projection.get().map_or_else(|| "pending".to_owned(), |projection| projection.runtime_epoch.to_string())}</strong></span>
            </div>
            <div class="receipt-status" data-testid="receipt-status">
                {move || receipt_state.last_receipt.get().map_or_else(
                    || "No command receipt yet".to_owned(),
                    |receipt| format!(
                        "sequence {} · {:?} · operation {}",
                        receipt.request_sequence.get(),
                        receipt.outcome,
                        receipt.operation_id.map_or_else(|| "none".to_owned(), |id| id.to_string()),
                    ),
                )}
            </div>
            <div class="focus-status">
                <span>"FOCUS"</span>
                <strong data-testid="focused-tile">{move || tile_state.focused_tile.get().map_or_else(|| "none".to_owned(), |id| id.to_string())}</strong>
                <span>{move || control_state.focused_control.get().unwrap_or_default()}</span>
            </div>
            <p class="error-status" role="alert" data-testid="command-error">
                {move || state.error.get().unwrap_or_default()}
            </p>
        </footer>
    }
}

#[component]
fn CommandPalette(state: StudioState) -> impl IntoView {
    let input_ref = NodeRef::<leptos::html::Input>::new();
    let effect_state = state.clone();
    Effect::new(move || {
        if effect_state.palette_open.get() {
            if let Some(input) = input_ref.get() {
                let _ = input.focus();
                input.select();
            }
        }
    });
    let show_state = state.clone();
    let query_state = state.clone();
    let backdrop_state = state.clone();
    let close_state = state.clone();
    let input_state = state.clone();
    let results_state = state.clone();
    view! {
        <div
            class="palette-backdrop"
            role="presentation"
            hidden=move || !show_state.palette_open.get()
            on:click=move |_| backdrop_state.palette_open.set(false)
        >
            <section
                class="command-palette"
                role="dialog"
                aria-modal="true"
                aria-labelledby="command-palette-title"
                data-testid="command-palette"
                on:click=move |event| event.stop_propagation()
            >
                <header>
                    <div>
                        <p class="eyebrow">"SEMANTIC COMMANDS"</p>
                        <h2 id="command-palette-title">"Command palette"</h2>
                    </div>
                    <button
                        type="button"
                        aria-label="Close command palette"
                        on:click=move |_| close_state.palette_open.set(false)
                    >"×"</button>
                </header>
                <input
                    node_ref=input_ref
                    type="search"
                    placeholder="Search commands"
                    aria-label="Search commands"
                    data-testid="command-search"
                    prop:value=move || query_state.palette_query.get()
                    on:input=move |event| input_state.palette_query.set(event_target_value(&event))
                />
                <div class="command-results">
                    {move || {
                        let query = results_state.palette_query.get().to_lowercase();
                        COMMANDS.into_iter().filter(|command| {
                            query.is_empty()
                                || command.label.to_lowercase().contains(&query)
                                || command.id.contains(&query)
                        }).map(|command| {
                            let state = results_state.clone();
                            view! {
                                <button
                                    type="button"
                                    data-command-id=command.id
                                    on:click=move |_| {
                                        dispatch_registry(&state, command.id);
                                        state.palette_open.set(false);
                                    }
                                >
                                    <span><strong>{command.label}</strong><small>{command.description}</small></span>
                                    <kbd>{command.shortcut}</kbd>
                                </button>
                            }
                        }).collect_view()
                    }}
                </div>
            </section>
        </div>
    }
}

fn dispatch_registry(state: &StudioState, command: &str) {
    match command {
        "palette.toggle" => state.palette_open.update(|open| *open = !*open),
        "palette.close" => state.palette_open.set(false),
        "workspace.load_demo" => state.load_demo(),
        "layout.preview" => state.preview_layout_adjustment(),
        "layout.commit" => state.commit_layout_draft(),
        "layout.discard" => state.discard_layout_draft(),
        "history.undo" => state.dispatch(SemanticCommand::Undo),
        "history.redo" => state.dispatch(SemanticCommand::Redo),
        "receipt.retry" => state.retry_last(),
        "focus.next" => focus_next_tile(state),
        "workspace.capture" => state.switch_workspace("Capture"),
        "workspace.transcribe" => state.switch_workspace("Transcribe"),
        "workspace.patch" => state.switch_workspace("Patch"),
        "workspace.diagnose" => state.switch_workspace("Diagnose"),
        "workspace.perform" => state.switch_workspace("Perform"),
        _ => state
            .error
            .set(Some(format!("Unknown semantic command: {command}"))),
    }
    state.push_event(format!("Command {command}"));
}

fn focus_next_tile(state: &StudioState) {
    let visible: Vec<_> = all_tiles()
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| !state.hidden_tiles.get_untracked().contains(id))
        .collect();
    if visible.is_empty() {
        return;
    }
    let current = state.focused_tile.get_untracked();
    let next = current
        .and_then(|current| visible.iter().position(|id| *id == current))
        .map_or(visible[0], |index| visible[(index + 1) % visible.len()]);
    state.focused_tile.set(Some(next));
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        if let Ok(Some(element)) = document.query_selector(&format!("[data-tile-id=\"{next}\"]")) {
            if let Ok(element) = element.dyn_into::<web_sys::HtmlElement>() {
                let _ = element.focus();
            }
        }
    }
}

fn command_for_key(event: &web_sys::KeyboardEvent, typing: bool) -> Option<&'static str> {
    let key = event.key();
    if (event.ctrl_key() || event.meta_key()) && key.eq_ignore_ascii_case("k") {
        return Some("palette.toggle");
    }
    if event.key() == "Escape" {
        return Some("palette.close");
    }
    if typing {
        return None;
    }
    if event.alt_key() {
        return match key.as_str() {
            "1" => Some("workspace.capture"),
            "2" => Some("workspace.transcribe"),
            "3" => Some("workspace.patch"),
            "4" => Some("workspace.diagnose"),
            "5" => Some("workspace.perform"),
            _ => None,
        };
    }
    if event.shift_key() && key.eq_ignore_ascii_case("l") {
        return Some("layout.commit");
    }
    if event.shift_key() && key.eq_ignore_ascii_case("u") {
        return Some("history.redo");
    }
    match key.as_str() {
        "g" | "G" => Some("workspace.load_demo"),
        "l" | "L" => Some("layout.preview"),
        "u" | "U" => Some("history.undo"),
        "r" | "R" => Some("receipt.retry"),
        "F6" => Some("focus.next"),
        _ => None,
    }
}

fn event_target_is_text_entry(event: &web_sys::KeyboardEvent) -> bool {
    let Some(target) = event.target() else {
        return false;
    };
    target.dyn_ref::<web_sys::HtmlInputElement>().is_some()
        || target.dyn_ref::<web_sys::HtmlTextAreaElement>().is_some()
        || target.dyn_ref::<web_sys::HtmlSelectElement>().is_some()
        || target
            .dyn_ref::<web_sys::HtmlElement>()
            .is_some_and(web_sys::HtmlElement::is_content_editable)
}
