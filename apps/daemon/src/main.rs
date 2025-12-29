use nannou::prelude::*;
use talisman_core::{Signal, PatchBay, PluginManager, PluginModuleAdapter, ModuleRuntime, RoutedSignal};
use talisman_core::{Source, Sink, Processor};
use talisman_core::adapters::{SourceAdapter, SinkAdapter, ProcessorAdapter};
// use nannou_egui removed
use tokio::sync::mpsc;

use audio_input::{AudioInputSource, AudioVizSink, AudioInputSettings, AudioInputTile};
use audio_input::tile::AudioVisTile;
use audio_dsp::{AudioDspProcessor, AudioDspState};
use audio_dsp::tile::AudioDspTile;
use audio_output::{AudioOutputSink, AudioOutputSettings, AudioOutputState};
use audio_output::tile::AudioOutputTile;
// use talisman_core::ring_buffer; // Removed usage


// Layout editor and visualizer modules
mod patch_visualizer;
mod layout;
mod tiles;
mod input;
mod ui;
mod theme;

use talisman_ui::{FontId, draw_text, TextAlignment};

use layout::Layout;
use tiles::{TileRegistry, RenderContext};
use input::{KeyboardNav, AppAction};
use ui::modals::{ModalStack, ModalState, PatchBayModalState};
use ui::fullscreen_modal::ModalAnim;




// --- MODEL ---
struct Model {
    // We use a non-blocking channel for the UI thread to receive updates
    _receiver: std::sync::mpsc::Receiver<Signal>,
    router_rx: mpsc::Receiver<RoutedSignal>,
    
    // UI State
    // egui removed
    
    // Layout and Interaction
    layout: Layout,
    selected_tile: Option<String>,
    // Animation for tile maximize/minimize
    anim_factor: f32, // 0.0 to 1.0
    is_closing: bool,
    clipboard: Option<arboard::Clipboard>,
    
    // Unified Modal Stack (keyboard-first navigation)
    modal_stack: ModalStack,
    
    // Patch Bay State
    patch_bay: PatchBay,
    
    // Global State
    is_sleeping: bool,
    
    // Runtime State
    module_host: talisman_core::ModuleHost,
    plugin_manager: talisman_core::PluginManager,
    
    // Tile System (Phase 6: Settings Architecture)
    tile_registry: TileRegistry,
    _compositor: tiles::Compositor,
    start_time: std::time::Instant,
    frame_count: u64,
    
    // Keyboard Navigation (keyboard-first UI)
    keyboard_nav: KeyboardNav,
    
    // Modal animation states (for fullscreen modals)
    modal_anims: std::collections::HashMap<ModalAnimKey, ModalAnim>,
}

/// Key for modal animation tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ModalAnimKey {
    GlobalSettings,
    PatchBay,
    LayoutManager,

    AddTilePicker,
}

fn make_unique_tile_id(layout: &talisman_core::LayoutConfig, base: &str) -> String {
    if !layout.tiles.iter().any(|t| t.id == base) {
        return base.to_string();
    }
    for i in 2..10_000usize {
        let candidate = format!("{}_{}", base, i);
        if !layout.tiles.iter().any(|t| t.id == candidate) {
            return candidate;
        }
    }
    format!("{}_{}", base, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
}


// Layout now imported from layout.rs module
use talisman_core::TileConfig;



fn main() {
    // Init Logger
    // Default: warn for everything, but silence wgpu warnings, info for our crates.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,wgpu_core=error,wgpu_hal=error,nannou=error,daemon=info,text_tools=info,aphrodite=info,logos=info,kamea=info")).init();
    
    nannou::app(model)
        .update(update)
        .run();
}

fn model(app: &App) -> Model {
    app.set_exit_on_escape(false);
    
    // 1. Setup Channels
    let (tx_ui, rx_ui) = std::sync::mpsc::channel::<Signal>();
    let (tx_router, rx_router) = mpsc::channel::<RoutedSignal>(1000);
    
    // Clone for different uses
    let _tx_ui_clone = tx_ui.clone();
    
    // 2. Create ModuleHost for isolated module execution
    let mut module_host = talisman_core::ModuleHost::new(tx_router.clone());
    
    // NOTE: No hardcoded module registration here!
    // Modules are discovered and loaded dynamically via PluginManager.
    // See plugin discovery section below.
    
    log::info!("ModuleHost initialized - modules will be loaded dynamically via PluginManager");
    
    // 3. Initialize Window & Egui
    let _window_id = app.new_window()
        .view(view)
        .raw_event(raw_window_event)
        .key_pressed(key_pressed)
        .mouse_pressed(mouse_pressed)
        .mouse_moved(mouse_moved)
        .size(900, 600)
        .title("TALISMAN // DIGITAL LAB")
        .build()
        .unwrap();

    let _window = app.window(_window_id).unwrap();
    // let egui = Egui::from_window(&window); removed

    // 4. Init Clipboard (might fail on some systems)
    let clipboard = match arboard::Clipboard::new() {
        Ok(cb) => Some(cb),
        Err(e) => {
            log::warn!("Failed to init Clipboard: {}", e);
            None
        }
    };

    // 5. Init Patch Bay (empty - modules register themselves when loaded)
    let mut patch_bay = PatchBay::new();
    
    // Load layout config
    let layout = Layout::new(app.window_rect());

    // Apply patches from layout config (after plugins register their schemas)
    // This will be re-applied after plugin loading
    
    let mut tile_registry = tiles::create_default_registry();
    
    // Audio device settings
    let audio_input_settings = AudioInputSettings::new();
    let audio_output_settings = AudioOutputSettings::new();

    // Audio input tile (device selection)
    tile_registry.register(AudioInputTile::new("audio_input", audio_input_settings.clone()));

    // Audio visualization tile (fed by AudioVizSink)
    let mut vis_tile = AudioVisTile::new("audio_viz");
    let vis_buffer = vis_tile.get_legacy_buffer();
    let vis_latency = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    vis_tile.connect_latency_meter(vis_latency.clone());
    tile_registry.register(vis_tile);

    // Audio output tile (fed by AudioOutputSink)
    let (audio_output_sink, audio_output_state) = match AudioOutputSink::new("audio_output", audio_output_settings.clone()) {
        Ok((sink, state)) => (Some(sink), state),
        Err(e) => {
            log::error!("Failed to initialize audio output: {}", e);
            // Create a dummy state to keep UI stable
            let state = std::sync::Arc::new(AudioOutputState::default());
            (None, state)
        }
    };

    tile_registry.register(AudioOutputTile::new("audio_output", audio_output_state.clone(), audio_output_settings.clone()));

    // Audio DSP tile (settings)
    let dsp_state = AudioDspState::new();
    tile_registry.register(AudioDspTile::new("audio_dsp", dsp_state.clone()));

    // Astro tile (astrological chart)
    tile_registry.register(aphrodite::tile::AstroTile::new());

    // Audio pipeline modules
    if let Ok(audio_input_source) = AudioInputSource::new("audio_input", audio_input_settings.clone()) {
        let schema = audio_input_source.schema();
        patch_bay.register_module(schema);
        if let Err(e) = module_host.spawn(SourceAdapter::new(audio_input_source), 100) {
            log::error!("Failed to spawn audio input source: {}", e);
        }
    } else {
        log::error!("Audio input source failed to initialize");
    }

    let audio_dsp = AudioDspProcessor::new("audio_dsp", dsp_state.clone());
    let dsp_schema = audio_dsp.schema();
    patch_bay.register_module(dsp_schema);
    if let Err(e) = module_host.spawn(ProcessorAdapter::new(audio_dsp), 100) {
        log::error!("Failed to spawn audio DSP: {}", e);
    }

    let audio_viz_sink = AudioVizSink::new("audio_viz", vis_buffer, vis_latency);
    let viz_schema = audio_viz_sink.schema();
    patch_bay.register_module(viz_schema);
    if let Err(e) = module_host.spawn(SinkAdapter::new(audio_viz_sink), 100) {
        log::error!("Failed to spawn audio viz sink: {}", e);
    }

    if let Some(output_sink) = audio_output_sink {
        let output_schema = output_sink.schema();
        patch_bay.register_module(output_schema);
        if let Err(e) = module_host.spawn(SinkAdapter::new(output_sink), 100) {
            log::error!("Failed to spawn audio output sink: {}", e);
        }
    }
    
    log::info!("Patch Bay initialized - modules will register via PluginManager");

    // Extract sleep state before moving layout into Model
    let initial_sleep_state = layout.config.is_sleeping;

    // Load and spawn plugins
    let mut plugin_manager = PluginManager::new();
    
    // Enable hot-reload (in dev mode)
    if let Err(e) = plugin_manager.enable_hot_reload() {
        log::warn!("Failed to enable hot-reload: {}", e);
    }
    
    // Load existing plugins
    log::info!("Discovering and loading plugins...");
    {
        // Safe to unwrap here as we are single threaded in init
        let mut loader = plugin_manager.loader.write().unwrap();
        if let Err(e) = unsafe { loader.discover().and_then(|_| loader.load_all()) } {
            log::error!("Failed to load plugins: {}", e);
        }
        
        // Spawn plugins
        for plugin in loader.drain_loaded() {
            let adapter = PluginModuleAdapter::new(plugin);
            let id = adapter.id().to_string();
            let name = adapter.name().to_string();
            let adapter_schema = adapter.schema(); // Clones ModuleSchema
            let settings_json = adapter_schema.settings_schema.clone(); // Option<Value>
            
            log::info!("Spawning plugin module: {}", id);
            
            // Register module schema in PatchBay
            patch_bay.register_module(adapter_schema);
            
            if let Err(e) = module_host.spawn(adapter, 100) {
                 log::error!("Failed to spawn plugin: {}", e);
            } else {
                 // Register Visual Tile wrapper to bridge settings UI
                 if let Some(sender) = module_host.get_sender(&id) {
                     let tile = tiles::SchemaTile::new(&id, &name, settings_json, sender);
                     tile_registry.register(tile);
                     log::info!("Registered SchemaTile for plugin: {}", id);
                 }
            }
        }
        
        // Broadcast GPU Context to all plugins
        // We do this after spawning to ensure they receive it in their inbox.
        let window = app.main_window();
        let device = window.device();
        let queue = window.queue();
        
        let gpu_signal = Signal::GpuContext {
            device: device as *const _ as usize,
            queue: queue as *const _ as usize,
        };
        
        // We need to send this to all active modules.
        // ModuleHost doesn't expose a list of IDs easily?
        // We know we just spawned them from `loader.loaded()`.
        // Let's iterate `plugin_manager.loader` again? No, drained.
        // We should have collected IDs or use `patch_bay`?
        // ModuleHost has internal map.
        // Let's just broadcast to known "kamea" for now or verify ModuleHost API.
        // talisman_core::ModuleHost::send_signal takes id.
        // Let's assume we can get IDs from PatchBay.
        
        for module in patch_bay.get_modules() {
             // Clone signal for each send
             let sig = match gpu_signal {
                 Signal::GpuContext { device, queue } => Signal::GpuContext { device, queue },
                 _ => Signal::Pulse,
             };
             let _ = module_host.send_signal(&module.id, sig);
        }
    }

    // Apply saved patches from layout config
    for patch in &layout.config.patches {
        if let Err(e) = patch_bay.connect(
            &patch.source_module,
            &patch.source_port,
            &patch.sink_module,
            &patch.sink_port,
        ) {
            log::warn!("Failed to apply patch {}: {}", patch.id, e);
        }
    }

    let model = Model {
        _receiver: rx_ui,
        router_rx: rx_router,
        // egui removed
        layout,
        selected_tile: None,
        anim_factor: 0.0,
        is_closing: false,
        clipboard,
        modal_stack: ModalStack::new(),
        patch_bay,
        is_sleeping: initial_sleep_state,

        module_host,
        plugin_manager,
        tile_registry,
        _compositor: tiles::Compositor::new(app),
        start_time: std::time::Instant::now(),
        frame_count: 0,
        keyboard_nav: KeyboardNav::new(),
        modal_anims: std::collections::HashMap::new(),
    };
    
    // Apply saved tile settings from layout config
    apply_tile_settings(&model.tile_registry, &model.layout);
    
    /*
    // Connect audio stream to AudioVisTile if available
    if let Some(rx) = model.audio_stream_rx.take() {
        // ... logic removed
    }
    */

    
    model
}




/// Apply saved settings from layout config to all tiles in registry
fn apply_tile_settings(registry: &tiles::TileRegistry, layout: &Layout) {
    for tile in &layout.config.tiles {
        // Apply config settings if present
        if tile.settings.config != serde_json::Value::Null {
            registry.apply_settings(&tile.module, &tile.settings.config);
            log::debug!("Applied settings to tile {}: {:?}", tile.id, tile.settings.config);
        }
    }
}

/// Save current tile settings from registry back to layout config
/// Call this when closing a maximized tile to persist any changes
fn save_tile_settings(registry: &tiles::TileRegistry, layout: &mut Layout, tile_id: &str) {
    // Find the tile in layout config
    if let Some(tile) = layout.config.tiles.iter_mut().find(|t| t.id == tile_id) {
        // Get settings from registry
        let settings = registry.get_settings(&tile.module);
        if settings != serde_json::Value::Null {
            tile.settings.config = settings;
            log::info!("Saved settings for tile {}", tile_id);
            
            // Save layout to disk
            layout.save();
        }
    }
}

/// Update animation state for fullscreen modals
fn update_modal_anims(model: &mut Model) {
    use ui::fullscreen_modal::ModalAnim;
    
    // Helper to sync animation state with modal presence
    let sync_anim = |anims: &mut std::collections::HashMap<ModalAnimKey, ModalAnim>, key: ModalAnimKey, is_open: bool| {
        if is_open {
            let anim = anims.entry(key).or_insert_with(ModalAnim::new);
            anim.update();
        } else if let Some(anim) = anims.get_mut(&key) {
            if anim.factor > 0.0 {
                anim.closing = true;
                if anim.update() {
                    // Animation complete, remove
                    anims.remove(&key);
                }
            }
        }
    };
    
    // Check each modal type
    let is_global_settings = model.modal_stack.is_global_settings_open();
    let is_patch_bay = model.modal_stack.is_patch_bay_open();
    let is_layout_manager = model.modal_stack.is_layout_manager_open();

    let is_add_tile_picker = model.modal_stack.get_add_tile_picker().is_some();
    
    sync_anim(&mut model.modal_anims, ModalAnimKey::GlobalSettings, is_global_settings);
    sync_anim(&mut model.modal_anims, ModalAnimKey::PatchBay, is_patch_bay);
    sync_anim(&mut model.modal_anims, ModalAnimKey::LayoutManager, is_layout_manager);

    sync_anim(&mut model.modal_anims, ModalAnimKey::AddTilePicker, is_add_tile_picker);
}


fn update(_app: &App, model: &mut Model, _update: Update) {
    // Update Layout dimensions
    model.layout.update(_app.window_rect());
    
    // Smooth Animation for tile maximize/minimize
    let maximized_tile = model.modal_stack.get_maximized_tile().map(|s| s.to_string());
    if model.is_closing {
        model.anim_factor = (model.anim_factor - 0.1).max(0.0);
        if model.anim_factor <= 0.0 {
            // Save tile settings before clearing (persist any changes made in control mode)
            if let Some(ref tile_id) = maximized_tile {
                save_tile_settings(&model.tile_registry, &mut model.layout, tile_id);
            }
            // Pop the maximized modal from stack
            model.modal_stack.close(&ModalState::Maximized { tile_id: maximized_tile.unwrap_or_default() });
            model.is_closing = false;
        }
    } else if maximized_tile.is_some() && model.anim_factor < 1.0 {
        model.anim_factor = (model.anim_factor + 0.1).min(1.0);
    }
    
    // Update modal animations for fullscreen modals
    update_modal_anims(model);
    
    // Update tile registry (extends to new tiles with render_monitor/render_controls)
    model.tile_registry.update_all();
    model.frame_count += 1;
    
    // (Audio tiles update independently; module runtime handles audio pipeline)

    // Handle Plugin Hot-Reload
    while let Ok(path) = model.plugin_manager.reload_rx.try_recv() {
        log::info!("Hot-reload trigger for: {}", path.display());
        match model.plugin_manager.reload_plugin(&path) {
            Ok(plugin) => {
                let adapter = PluginModuleAdapter::new(plugin);
                let id = adapter.id().to_string(); // Copy ID
                log::info!("Replacng module: {}", id);
                
                // Shutdown old module
                if let Err(e) = model.module_host.shutdown_module(&id) {
                    log::warn!("Error shutting down old module {}: {}", id, e);
                }
                
                // Determine execution model (Thread pool? Dedicated?)
                // Defaulting to dedicated for plugins.
                // We need to re-spawn.
                if let Err(e) = model.module_host.spawn(adapter, 100) {
                    log::error!("Failed to respawn refreshed plugin {}: {}", id, e);
                } else {
                    log::info!("Successfully hot-reloaded plugin: {}", id);
                }
            },
            Err(e) => {
                log::error!("Failed to reload plugin from {}: {}", path.display(), e);
            }
        }
    }

    // Process Router Signals (From Plugins)
    while let Ok(routed) = model.router_rx.try_recv() {
        // Handle host-level signals before routing
        if let Signal::Texture { handle, start_time: _ } = &routed.signal {
            log::info!(
                "Received texture handle {} ({}x{}) from plugin",
                handle.id,
                handle.width,
                handle.height
            );
            // Texture is already registered in view_map by the adapter (when enabled).
            // Compositor can lookup via handle.id.
        }
        
        // Route signals through PatchBay
        let outgoing = model.patch_bay.get_outgoing_patches(&routed.source_id);
        if outgoing.is_empty() {
            continue;
        }
        
        let is_audio_stream = matches!(&routed.signal, Signal::AudioStream { .. });
        if is_audio_stream && outgoing.len() > 1 {
            log::warn!(
                "AudioStream from {} has {} sinks; only first sink will receive it",
                routed.source_id,
                outgoing.len()
            );
        }
        
        let active_sinks: Vec<_> = outgoing
            .into_iter()
            .filter(|patch| !model.patch_bay.is_module_disabled(&patch.sink_module))
            .collect();
        if active_sinks.is_empty() {
            continue;
        }
        
        if is_audio_stream {
            if let Some(first) = active_sinks.first() {
                let _ = model.module_host.send_signal(&first.sink_module, routed.signal);
            }
            continue;
        }
        
        let mut remaining = active_sinks.len();
        let mut signal = Some(routed.signal);
        for patch in active_sinks {
            let payload = if remaining == 1 {
                signal.take().expect("signal payload already taken")
            } else {
                signal.as_ref().expect("signal payload missing").clone()
            };
            remaining -= 1;
            let _ = model.module_host.send_signal(&patch.sink_module, payload);
        }
    }

    // GUI update removed (egui removed)


    // (Close confirmation dialog removed - ESC is for navigation only, not exit)



    // (Legacy signal_handler::process_signals removed - tiles handle their own state via TileRegistry)
}

fn mouse_pressed(_app: &App, _model: &mut Model, _button: MouseButton) {
    // Keyboard-only navigation: mouse input disabled
    // All interaction is handled via keyboard in key_pressed()
    log::trace!("Mouse input disabled - use keyboard for navigation");
}





fn mouse_moved(_app: &App, _model: &mut Model, _pos: Point2) {
    // Mouse movement handling - keyboard-first navigation only
}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    // === INPUT ROUTING GUARD ===
    // Egui keyboard guard removed

    
    let ctrl = _app.keys.mods.ctrl();
    let shift = _app.keys.mods.shift();

    // === MAXIMIZED TILE INPUT ROUTING (tile-local controls) ===
    // If a tile is maximized AND it is the top modal, give it input.
    if key != Key::Escape && !ctrl {
        // Only route if Maximized is the top modal (not covered by PatchBay etc)
        // We use a scope to limit the borrow of modal_stack
        let max_tile_id = if let Some(crate::ui::modals::ModalState::Maximized { tile_id }) = model.modal_stack.top_mut() {
            Some(tile_id.clone())
        } else {
            None
        };

        if let Some(max_id) = max_tile_id {
            if let Some(tile_cfg) = model.layout.config.tiles.iter().find(|t| t.id == max_id) {
                let handled = model
                    .tile_registry
                    .handle_key(&tile_cfg.module, key, ctrl, shift);
                if handled {
                    return;
                }
            }
        }
    }
    
    // === MODAL INPUT ROUTING ===
    // Route input to active modals (Patch Bay, Global Settings)
    // Return early if consumed.
    if let Some(mut state) = model.modal_stack.get_patch_bay_state_mut() {
        if ui::patch_bay::handle_key(key, &mut state, &mut model.patch_bay) {
            return;
        }
        // If Escape was not consumed (returned false), close the modal
        if key == Key::Escape {
            model.modal_stack.pop();
            return;
        }
        // For other keys, if not consumed, we typically block or allow fallthrough? 
        // If handle_key returns true for all nav keys, we are good.
        // It returns true by default for unknown keys too to consume them.
    }
    
    if let Some(mut state) = model.modal_stack.get_global_settings_state_mut() {
        if ui::settings::handle_key(key, &mut state) {
            return;
        }
        if key == Key::Escape {
            model.modal_stack.pop();
            return;
        }
    }

    // === MODAL ESC HANDLING (Generic) ===
    // If we are here, no specific modal consumed Escape.
    if key == Key::Escape {
        // Check if a tile is maximized - close it first
        if model.modal_stack.get_maximized_tile().is_some() {
            model.modal_stack.pop();
            return;
        }

        // Pop any other modal from stack
        if model.modal_stack.pop().is_some() {
            return;
        }
        // Fall through to keyboard_nav ESC handling (deselect, exit mode, etc.)
    }
    
    // Update grid size for navigation
    let (grid_cols, grid_rows) = model.layout.config.resolve_grid();
    model.keyboard_nav.set_grid_size(grid_cols, grid_rows);

    // === ADD TILE PICKER INPUT (captures keys while open) ===
    if let Some((col, row, selected_idx)) = model.modal_stack.get_add_tile_picker() {
        // Keyboard-only modal: Up/Down choose, Enter confirm.
        let available = model.tile_registry.list_tiles();
        if available.is_empty() {
            return;
        }

        match key {
            Key::Up => {
                model.modal_stack.move_add_tile_picker_selection(-1, available.len());
                return;
            }
            Key::Down => {
                model.modal_stack.move_add_tile_picker_selection(1, available.len());
                return;
            }
            Key::Return => {
                let module_id = available.get(selected_idx).cloned();
                if let Some(module_id) = module_id {
                    let tile_id = make_unique_tile_id(&model.layout.config, &module_id);
                    model.layout.config.tiles.push(TileConfig {
                        id: tile_id.clone(),
                        col,
                        row,
                        colspan: Some(1),
                        rowspan: Some(1),
                        module: module_id,
                        enabled: true,
                        settings: Default::default(),
                    });
                    model.layout.save();
                    model.modal_stack.close(&ModalState::AddTilePicker { cursor_col: col, cursor_row: row, selected_idx: 0 });
                    // Select the new tile immediately
                    model.keyboard_nav.cursor = (col, row);
                    model.keyboard_nav.selection = input::SelectionState::TileSelected { tile_id: tile_id.clone() };
                    model.selected_tile = Some(tile_id);
                }
                return;
            }
            _ => {
                // Ignore other keys while picker is open
                return;
            }
        }
    }
    
    // If any other modal is active, block grid navigation
    if !model.modal_stack.is_empty() {
        return;
    }
    
    // Delegate to unified input controller
    let action = model.keyboard_nav.handle_key(
        key, 
        ctrl,
        shift,
        &mut model.layout.config,
        &model.tile_registry
    );
    
    // Handle App Actions (Side Effects)
    if let Some(action) = action {
        match action {
            AppAction::SaveLayout => {
                if let Err(e) = model.layout.config.resolve_conflicts(None) {
                    log::warn!("Unable to resolve layout conflicts before save: {}", e);
                }
                model.layout.save();
                log::info!("Layout saved");
            },
            AppAction::QuitApp => {
                log::info!("Quit requested via Ctrl+Q");
                std::process::exit(0);
            },
            AppAction::Copy { text } => {
                if let Some(cb) = &mut model.clipboard {
                    if let Err(e) = cb.set_text(text) {
                        log::error!("Clipboard Copy Failed: {}", e);
                    } else {
                        log::info!("Copied to Clipboard");
                    }
                }
            },
            AppAction::OpenGlobalSettings => {
                model.modal_stack.push(ModalState::GlobalSettings(ui::modals::GlobalSettingsState::default()));
            },

            AppAction::OpenAddTilePicker { col, row } => {
                model.modal_stack.open_add_tile_picker(col, row);
            },
            AppAction::OpenPatchBay => {
                if !model.modal_stack.is_patch_bay_open() {
                    model.modal_stack.push(ModalState::PatchBay(PatchBayModalState::default()));
                }
            },
            AppAction::OpenTileSettings { tile_id } => {
                model.modal_stack.push(ModalState::Maximized { tile_id });
                model.is_closing = false;
                model.anim_factor = 0.0;
            },
            AppAction::ToggleMaximize => {
                if let Some(selected) = &model.selected_tile {
                    let is_maximized = model.modal_stack.get_maximized_tile() == Some(selected.as_str());
                    if is_maximized {
                        model.is_closing = true;
                    } else {
                        model.modal_stack.push(ModalState::Maximized { tile_id: selected.clone() });
                        model.is_closing = false;
                        model.anim_factor = 0.0;
                    }
                }
            },
            AppAction::OpenLayoutManager => {
                model.modal_stack.push(ModalState::LayoutManager);
            },
        }
    }
    
    // Sync selected_tile with keyboard_nav (source of truth)
    model.selected_tile = model.keyboard_nav.selected_tile_id().map(|s| s.to_string());
}



fn raw_window_event(_app: &App, _model: &mut Model, _event: &nannou::winit::event::WindowEvent) {
    // egui event handling removed
}

fn draw_fullscreen_overlay(draw: &Draw, win_rect: Rect, title: &str) {
    // Semi-transparent black background
    draw.rect()
        .xy(win_rect.xy())
        .wh(win_rect.wh())
        .color(rgba(0.0, 0.0, 0.0, 0.95));

    // Title centered
    draw_text(
        draw,
        FontId::PlexSansBold,
        title,
        win_rect.xy(),
        32.0,
        CYAN.into(),
        TextAlignment::Center,
    );
        
    // Close hint
    draw_text(
        draw,
        FontId::PlexSansRegular,
        "[ESC] Close",
        pt2(win_rect.x(), win_rect.y() - 40.0),
        12.0,
        GRAY.into(),
        TextAlignment::Center,
    );
}

fn view(app: &App, model: &Model, frame: Frame) {
    // Color scheme (retinal burn mode removed)
    let (bg_color, _fg_color, stroke_color) = (BLACK, CYAN, GRAY);
    
    let draw = app.draw();
    draw.background().color(bg_color);

    // Get maximized tile from modal stack
    let maximized_tile = model.modal_stack.get_maximized_tile();

    // Draw Empty Cell Placeholders
    if maximized_tile.is_none() {
        let (cols, rows) = model.layout.config.resolve_grid();
        for c in 0..cols {
            for r in 0..rows {
                 if model.layout.get_tile_at(c, r).is_none() {
                     let temp_tile = TileConfig {
                         id: String::new(), col: c, row: r, colspan: Some(1), rowspan: Some(1),
                         module: String::new(), enabled: true, settings: Default::default()
                     };
                     if let Some(rect) = model.layout.calculate_rect(&temp_tile) {
                         draw.rect().xy(rect.xy()).wh(rect.wh()).color(rgba(0.05, 0.05, 0.05, 0.5)).stroke(stroke_color).stroke_weight(1.0);
                         draw_text(
                             &draw,
                             FontId::PlexSansRegular,
                             "+",
                             rect.xy(),
                             24.0,
                             stroke_color.into(),
                             TextAlignment::Center,
                         );
                     }
                 }
            }
        }
    }

    // Layout cursor highlight (supports selecting empty "+" cells)
    if maximized_tile.is_none() && model.keyboard_nav.mode == input::InputMode::Layout {
        let (col, row) = model.keyboard_nav.cursor;
        let temp_tile = TileConfig {
            id: String::new(),
            col,
            row,
            colspan: Some(1),
            rowspan: Some(1),
            module: String::new(),
            enabled: true,
            settings: Default::default(),
        };
        if let Some(rect) = model.layout.calculate_rect(&temp_tile) {
            draw.rect()
                .xy(rect.xy())
                .wh(rect.wh())
                .color(rgba(0.0, 0.0, 0.0, 0.0))
                .stroke(rgba(0.0, 1.0, 0.5, 0.9))
                .stroke_weight(2.0);
        }
    }


    // Iterate over all tiles and render (MONITOR MODE - Read-only feedback)
    for tile in &model.layout.config.tiles {
        if maximized_tile == Some(tile.id.as_str()) {
            continue;
        }
        
        if let Some(rect) = model.layout.calculate_rect(tile) {
            let bc = if model.selected_tile.as_ref() == Some(&tile.id) {
                LinSrgba::new(0.0, 1.0, 1.0, 0.5)
            } else {
                stroke_color.into_format::<f32>().into_linear().into()
            };
            
            // Try tile registry first
            if model.tile_registry.get(&tile.module).is_some() {
                // Draw border
                let is_selected = model.selected_tile.as_ref() == Some(&tile.id);
                draw.rect()
                    .xy(rect.xy())
                    .wh(rect.wh())
                    .color(rgba(0.0, 0.0, 0.0, 0.0))
                    .stroke(bc)
                    .stroke_weight(if is_selected { 2.0 } else { 1.0 });
                
                let ctx = RenderContext {
                    time: model.start_time,
                    frame_count: model.frame_count,
                    is_selected,
                    is_maximized: false,
                    tile_settings: Some(&tile.settings.config),
                };
                
                model.tile_registry.render_monitor(&tile.module, &draw, rect.pad(5.0), &ctx);
                
                // Render error overlay if tile has an error
                if let Some(error) = model.tile_registry.get_error(&tile.module) {
                    tiles::render_error_overlay(&draw, rect, &error);
                }
            }
        }
    }

    // Draw Maximized Tile on top (CONTROL MODE - Settings UI)
    if let Some(max_id) = maximized_tile {
        if let Some(tile) = model.layout.config.tiles.iter().find(|t| t.id == max_id) {
            if let Some(source_rect) = model.layout.calculate_rect(tile) {
                let target_rect = app.window_rect(); // Full Window maximize
                
                let t = model.anim_factor;
                // Cubic easing for a smoother feel
                let t_smooth = if t < 0.5 { 4.0 * t * t * t } else { (t - 1.0) * (2.0 * t - 2.0) * (2.0 * t - 2.0) + 1.0 };

                let cx = source_rect.x() * (1.0 - t_smooth) + target_rect.x() * t_smooth;
                let cy = source_rect.y() * (1.0 - t_smooth) + target_rect.y() * t_smooth;
                let w = source_rect.w() * (1.0 - t_smooth) + target_rect.w() * t_smooth;
                let h = source_rect.h() * (1.0 - t_smooth) + target_rect.h() * t_smooth;
                
                let rect = Rect::from_x_y_w_h(cx, cy, w, h);
                
                // Draw background for maximized view to hide underlying grid
                draw.rect().xy(rect.xy()).wh(rect.wh()).color(bg_color);
                
                // Try to use tile registry for render_controls
                // Create RenderContext for tile
                let ctx = RenderContext {
                    time: model.start_time,
                    frame_count: model.frame_count,
                    is_selected: true,
                    is_maximized: true,
                    tile_settings: Some(&tile.settings.config),
                };
                
                // Render via tile registry
                model.tile_registry.render_controls(&tile.module, &draw, rect, &ctx);
            }
        }
    }

    // Sleep visualization
    if model.is_sleeping {
        draw.rect()
            .xy(app.window_rect().xy())
            .wh(app.window_rect().wh())
            .color(rgba(0.0, 0.0, 0.1, 0.4));
            
        draw_text(
            draw,
            FontId::PlexMonoRegular,
            "Zzz",
            pt2(app.window_rect().right() - 30.0, app.window_rect().bottom() + 30.0),
            24.0,
            rgba(0.5, 0.5, 1.0, 0.5),
            TextAlignment::Right,
        );
    }

    // Mode indicator (bottom-left corner)
    if maximized_tile.is_none() {
        let mode_text = match model.keyboard_nav.mode {
            input::InputMode::Normal => "NORMAL",
            input::InputMode::Layout => match model.keyboard_nav.layout_state {
                input::LayoutSubState::Navigation => "LAYOUT",
                input::LayoutSubState::Resize { .. } => "RESIZE",
                input::LayoutSubState::Move { .. } => "MOVE",
            },
            input::InputMode::Patch => "PATCH",
        };
        
        let mode_color = match model.keyboard_nav.mode {
            input::InputMode::Normal => rgba(0.5, 0.5, 0.5, 0.8),
            input::InputMode::Layout => rgba(0.0, 1.0, 0.5, 0.8),
            input::InputMode::Patch => rgba(1.0, 0.5, 0.0, 0.8),
        };
        
        let win_rect = app.window_rect();
        draw_text(
            &draw,
            FontId::PlexSansBold,
            mode_text,
            pt2(win_rect.left() + 50.0, win_rect.bottom() + 20.0),
            14.0,
            mode_color.into(),
            TextAlignment::Left,
        );
        
        // Show keybind hints
        let hints = match model.keyboard_nav.mode {
            input::InputMode::Normal => "[L]ayout [P]atch [G]lobal [Tab]Cycle [Arrows]Nav [E]dit [Enter]Select",
            input::InputMode::Layout => "[E]dit [A]dd [D]elete [Space]Toggle [Enter]Confirm [ESC]Cancel",
            input::InputMode::Patch => "[Arrows]Select [Enter]Patch [ESC]Exit",
        };
        
        draw_text(
            &draw,
            FontId::PlexSansRegular,
            hints,
            pt2(win_rect.left() + 250.0, win_rect.bottom() + 20.0),
            10.0,
            rgba(0.4, 0.4, 0.4, 0.8).into(),
            TextAlignment::Left,
        );
    }
    
    // Render patch cables (always visible if not maximized)
    if maximized_tile.is_none() && !model.layout.config.patches.is_empty() {
        let mut tile_rects = Vec::new();
        for tile in &model.layout.config.tiles {
            if let Some(rect) = model.layout.calculate_rect(tile) {
                tile_rects.push((tile.module.clone(), rect));
            }
        }
        patch_visualizer::render_patches(&draw, &model.layout.config.patches, &tile_rects);
    }



    // Fullscreen Modals
    let win_rect = app.window_rect();
    if let Some(state) = model.modal_stack.get_global_settings_state() {
        // Create anim state if needed or use existing
        let anim = model.modal_anims.get(&ModalAnimKey::GlobalSettings).cloned().unwrap_or(ModalAnim::new());
        ui::settings::render(&draw, win_rect, state, &anim);
    } else if let Some(state) = model.modal_stack.get_patch_bay_state() {
         let anim = ModalAnim { factor: 1.0, closing: false }; // TODO: Integrated animation state
         ui::patch_bay::render(&draw, win_rect, state, &anim, &model.patch_bay);
    }
 else if model.modal_stack.is_layout_manager_open() {
         draw_fullscreen_overlay(&draw, win_rect, "LAYOUT MANAGER");
    }

    draw.to_frame(app, &frame).unwrap();
    // egui draw removed
}
