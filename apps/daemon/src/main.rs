use nannou::prelude::*;
use talisman_core::{Signal, PatchBay, PluginManager, PluginModuleAdapter, ModuleRuntime, RoutedSignal};
use talisman_core::{Source, Sink, Processor};
use talisman_core::adapters::{SourceAdapter, SinkAdapter, ProcessorAdapter};
use nannou_egui::{self, Egui, egui};
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



use layout::Layout;
use tiles::{TileRegistry, RenderContext};
use input::{KeyboardNav, AppAction};
use ui::modals::{ModalStack, ModalState};
use ui::fullscreen_modal::ModalAnim;




// --- MODEL ---
struct Model {
    // We use a non-blocking channel for the UI thread to receive updates
    _receiver: std::sync::mpsc::Receiver<Signal>,
    router_rx: mpsc::Receiver<RoutedSignal>,
    
    // UI State
    egui: Egui,
    
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
    let window_id = app.new_window()
        .view(view)
        .raw_event(raw_window_event)
        .key_pressed(key_pressed)
        .mouse_pressed(mouse_pressed)
        .mouse_moved(mouse_moved)
        .size(900, 600)
        .title("TALISMAN // DIGITAL LAB")
        .build()
        .unwrap();

    let window = app.window(window_id).unwrap();
    let egui = Egui::from_window(&window);

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
        egui,
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


fn update(app: &App, model: &mut Model, update: Update) {
    // Update Layout dimensions
    model.layout.update(app.window_rect());
    
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

    // 1. UPDATE GUI
    model.egui.set_elapsed_time(update.since_start);
    let ctx = model.egui.begin_frame();
    
    // (Legacy editor window removed - TextInputTile handles text input)
    // (Legacy kamea buttons removed - KameaTile handles its own controls)
    // (Context menu removed - keyboard-first navigation)

    // Patch Bay Modal (Fullscreen Style)
    if model.modal_stack.is_patch_bay_open() {
        let anim = model.modal_anims.get(&ModalAnimKey::PatchBay);
        let alpha = anim.map(|a| a.eased()).unwrap_or(1.0);
        let scale = 0.9 + 0.1 * alpha;
        
        let screen_rect = ctx.screen_rect();
        let margin = 40.0 * (1.0 + (1.0 - alpha));
        let modal_width = (screen_rect.width() - margin * 2.0) * scale;
        let modal_height = (screen_rect.height() - margin * 2.0) * scale;
        let modal_x = screen_rect.center().x - modal_width / 2.0;
        let modal_y = screen_rect.center().y - modal_height / 2.0;
        
        // Fullscreen dark backdrop
        egui::Area::new(egui::Id::new("patch_bay_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .show(&ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(rect, 0.0, 
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, (220.0 * alpha) as u8));
            });
        
        egui::Area::new(egui::Id::new("patch_bay_modal"))
            .fixed_pos(egui::pos2(modal_x, modal_y))
            .show(&ctx, |ui| {
                let frame = egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(8, 8, 12, (250.0 * alpha) as u8))
                    .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(0, 255, 255, (200.0 * alpha) as u8)))
                    .inner_margin(egui::Margin::same(20.0));
                
                frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(modal_width, modal_height));
                    
                    // Header
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("PATCH BAY")
                            .heading()
                            .size(24.0)
                            .color(egui::Color32::from_rgba_unmultiplied(0, 255, 255, (255.0 * alpha) as u8)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("[ESC] Close")
                                .small()
                                .color(egui::Color32::from_rgba_unmultiplied(100, 100, 100, (200.0 * alpha) as u8)));
                        });
                    });
                    
                    ui.add_space(10.0);
                    ui.add(egui::Separator::default().spacing(10.0));
                    ui.add_space(10.0);
                    
                    // Module List
                    ui.label(egui::RichText::new("REGISTERED MODULES")
                        .small()
                        .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                    
                    egui::ScrollArea::vertical()
                        .max_height(modal_height - 200.0)
                        .show(ui, |ui| {
                            let modules: Vec<_> = model.patch_bay.get_modules()
                                .iter()
                                .map(|m| (*m).clone())
                                .collect();
                            
                            for module in modules {
                                ui.add_space(8.0);
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 25, (200.0 * alpha) as u8))
                                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(50, 50, 60, (150.0 * alpha) as u8)))
                                    .inner_margin(egui::Margin::same(12.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new(&module.name)
                                                .strong()
                                                .color(egui::Color32::from_rgba_unmultiplied(0, 255, 255, (255.0 * alpha) as u8)));
                                            ui.label(egui::RichText::new(format!("({})", module.id))
                                                .small()
                                                .color(egui::Color32::from_rgba_unmultiplied(100, 100, 100, (200.0 * alpha) as u8)));
                                        });
                                        
                                        ui.label(egui::RichText::new(&module.description)
                                            .small()
                                            .color(egui::Color32::from_rgba_unmultiplied(150, 150, 150, (200.0 * alpha) as u8)));
                                        
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("PORTS:").small().color(egui::Color32::from_rgba_unmultiplied(80, 80, 80, (200.0 * alpha) as u8)));
                                            for port in &module.ports {
                                                let (prefix, r, g, b) = match port.direction {
                                                    talisman_core::PortDirection::Input => ("IN", 100, 200, 100),
                                                    talisman_core::PortDirection::Output => ("OUT", 200, 100, 100),
                                                };
                                                ui.label(egui::RichText::new(format!("{} {} ({:?})", prefix, port.label, port.data_type))
                                                    .small()
                                                    .color(egui::Color32::from_rgba_unmultiplied(r, g, b, (200.0 * alpha) as u8)));
                                            }
                                        });
                                    });
                            }
                        });
                    
                    ui.add_space(10.0);
                    ui.add(egui::Separator::default().spacing(10.0));
                    ui.add_space(5.0);
                    
                    // Connections
                    let patches = model.patch_bay.get_patches();
                    if patches.is_empty() {
                        ui.label(egui::RichText::new("No active patches")
                            .small()
                            .color(egui::Color32::from_rgba_unmultiplied(100, 100, 100, (200.0 * alpha) as u8)));
                    } else {
                        ui.label(egui::RichText::new(format!("ACTIVE PATCHES: {}", patches.len()))
                            .small()
                            .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                        for patch in patches {
                            ui.label(egui::RichText::new(format!(
                                "  {}:{} → {}:{}",
                                patch.source_module, patch.source_port,
                                patch.sink_module, patch.sink_port
                            )).small().color(egui::Color32::from_rgba_unmultiplied(255, 200, 50, (200.0 * alpha) as u8)));
                        }
                    }
                });
            });
    }

    // Global Settings Modal (Fullscreen Style)
    if model.modal_stack.is_global_settings_open() {
        let anim = model.modal_anims.get(&ModalAnimKey::GlobalSettings);
        let alpha = anim.map(|a| a.eased()).unwrap_or(1.0);
        let scale = 0.9 + 0.1 * alpha;
        
        let screen_rect = ctx.screen_rect();
        let margin = 60.0 * (1.0 + (1.0 - alpha));
        let modal_width = (screen_rect.width() - margin * 2.0).min(600.0) * scale;
        let modal_height = (screen_rect.height() - margin * 2.0).min(500.0) * scale;
        let modal_x = screen_rect.center().x - modal_width / 2.0;
        let modal_y = screen_rect.center().y - modal_height / 2.0;
        
        // Fullscreen dark backdrop
        egui::Area::new(egui::Id::new("global_settings_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .show(&ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(rect, 0.0, 
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, (220.0 * alpha) as u8));
            });
        
        egui::Area::new(egui::Id::new("global_settings_modal"))
            .fixed_pos(egui::pos2(modal_x, modal_y))
            .show(&ctx, |ui| {
                let frame = egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(8, 8, 12, (250.0 * alpha) as u8))
                    .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(0, 255, 255, (200.0 * alpha) as u8)))
                    .inner_margin(egui::Margin::same(25.0));
                
                frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(modal_width, modal_height));
                    
                    // Header
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("GLOBAL SETTINGS")
                            .heading()
                            .size(24.0)
                            .color(egui::Color32::from_rgba_unmultiplied(0, 255, 255, (255.0 * alpha) as u8)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("[ESC] Close")
                                .small()
                                .color(egui::Color32::from_rgba_unmultiplied(100, 100, 100, (200.0 * alpha) as u8)));
                        });
                    });
                    
                    ui.add_space(15.0);
                    ui.add(egui::Separator::default().spacing(10.0));
                    ui.add_space(15.0);
                    
                    // SYSTEM Section
                    ui.label(egui::RichText::new("SYSTEM")
                        .small()
                        .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                    ui.add_space(8.0);
                    
                    if ui.add(egui::Button::new(
                        egui::RichText::new("⏻  QUIT TALISMAN")
                            .color(egui::Color32::from_rgba_unmultiplied(255, 100, 100, (255.0 * alpha) as u8)))
                        .fill(egui::Color32::from_rgba_unmultiplied(40, 15, 15, (200.0 * alpha) as u8))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(100, 40, 40, (200.0 * alpha) as u8)))
                    ).clicked() {
                        std::process::exit(0);
                    }
                    
                    ui.add_space(20.0);
                    
                    // ENGINE Section
                    ui.label(egui::RichText::new("ENGINE")
                        .small()
                        .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                    ui.add_space(8.0);
                    
                    let status = if model.is_sleeping { "SLEEPING" } else { "ACTIVE" };
                    let (sr, sg, sb) = if model.is_sleeping { (255, 100, 100) } else { (100, 255, 100) };
                    
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Status:")
                            .color(egui::Color32::from_rgba_unmultiplied(180, 180, 180, (200.0 * alpha) as u8)));
                        ui.label(egui::RichText::new(status)
                            .strong()
                            .color(egui::Color32::from_rgba_unmultiplied(sr, sg, sb, (255.0 * alpha) as u8)));
                    });
                    
                    ui.add_space(5.0);
                    let btn_text = if model.is_sleeping { "▶  WAKE ENGINE" } else { "⏸  SLEEP ENGINE" };
                    if ui.add(egui::Button::new(
                        egui::RichText::new(btn_text)
                            .color(egui::Color32::from_rgba_unmultiplied(200, 200, 200, (255.0 * alpha) as u8)))
                        .fill(egui::Color32::from_rgba_unmultiplied(30, 30, 35, (200.0 * alpha) as u8))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(60, 60, 70, (200.0 * alpha) as u8)))
                    ).clicked() {
                        model.is_sleeping = !model.is_sleeping;
                        model.layout.config.is_sleeping = model.is_sleeping;
                        model.layout.save();
                    }
                    
                    ui.add_space(20.0);
                    
                    // LAYOUT Section
                    ui.label(egui::RichText::new("LAYOUT")
                        .small()
                        .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                    ui.add_space(8.0);
                    
                    if ui.add(egui::Button::new(
                        egui::RichText::new("⚙  OPEN LAYOUT MANAGER")
                            .color(egui::Color32::from_rgba_unmultiplied(0, 255, 255, (255.0 * alpha) as u8)))
                        .fill(egui::Color32::from_rgba_unmultiplied(0, 40, 40, (200.0 * alpha) as u8))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0, 100, 100, (200.0 * alpha) as u8)))
                    ).clicked() {
                        model.modal_stack.push(ModalState::LayoutManager);
                        model.modal_stack.close(&ModalState::GlobalSettings);
                    }
                    
                    ui.add_space(20.0);
                    
                    // STATS Section
                    ui.label(egui::RichText::new("STATS")
                        .small()
                        .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                    ui.add_space(8.0);
                    
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgba_unmultiplied(15, 15, 20, (200.0 * alpha) as u8))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Modules:")
                                    .color(egui::Color32::from_rgba_unmultiplied(150, 150, 150, (200.0 * alpha) as u8)));
                                ui.label(egui::RichText::new(format!("{}", model.patch_bay.get_modules().len()))
                                    .color(egui::Color32::from_rgba_unmultiplied(0, 255, 255, (255.0 * alpha) as u8)));
                                ui.add_space(20.0);
                                ui.label(egui::RichText::new("Patches:")
                                    .color(egui::Color32::from_rgba_unmultiplied(150, 150, 150, (200.0 * alpha) as u8)));
                                ui.label(egui::RichText::new(format!("{}", model.patch_bay.get_patches().len()))
                                    .color(egui::Color32::from_rgba_unmultiplied(0, 255, 255, (255.0 * alpha) as u8)));
                                ui.add_space(20.0);
                                ui.label(egui::RichText::new("FPS:")
                                    .color(egui::Color32::from_rgba_unmultiplied(150, 150, 150, (200.0 * alpha) as u8)));
                                ui.label(egui::RichText::new(format!("{:.0}", ctx.input(|i| i.predicted_dt).recip()))
                                    .color(egui::Color32::from_rgba_unmultiplied(100, 255, 100, (255.0 * alpha) as u8)));
                            });
                        });
                });
            });
    }



    // Layout Manager Modal (Fullscreen Style)
    if model.modal_stack.is_layout_manager_open() {
        let anim = model.modal_anims.get(&ModalAnimKey::LayoutManager);
        let alpha = anim.map(|a| a.eased()).unwrap_or(1.0);
        let scale = 0.9 + 0.1 * alpha;
        
        let screen_rect = ctx.screen_rect();
        let margin = 40.0 * (1.0 + (1.0 - alpha));
        let modal_width = (screen_rect.width() - margin * 2.0) * scale;
        let modal_height = (screen_rect.height() - margin * 2.0) * scale;
        let modal_x = screen_rect.center().x - modal_width / 2.0;
        let modal_y = screen_rect.center().y - modal_height / 2.0;
        
        // Fullscreen dark backdrop
        egui::Area::new(egui::Id::new("layout_manager_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .show(&ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(rect, 0.0, 
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, (220.0 * alpha) as u8));
            });
        
        egui::Area::new(egui::Id::new("layout_manager_modal"))
            .fixed_pos(egui::pos2(modal_x, modal_y))
            .show(&ctx, |ui| {
                let frame = egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(8, 8, 12, (250.0 * alpha) as u8))
                    .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(0, 255, 128, (200.0 * alpha) as u8)))
                    .inner_margin(egui::Margin::same(20.0));
                
                frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(modal_width, modal_height));
                    
                    // Header
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("LAYOUT MANAGER")
                            .heading()
                            .size(24.0)
                            .color(egui::Color32::from_rgba_unmultiplied(0, 255, 128, (255.0 * alpha) as u8)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(
                                egui::RichText::new("✓  SAVE & CLOSE")
                                    .color(egui::Color32::from_rgba_unmultiplied(100, 255, 100, (255.0 * alpha) as u8)))
                                .fill(egui::Color32::from_rgba_unmultiplied(15, 40, 15, (200.0 * alpha) as u8))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(40, 100, 40, (200.0 * alpha) as u8)))
                            ).clicked() {
                                model.layout.config.patches = model.patch_bay.get_patches().to_vec();
                                model.layout.config.is_sleeping = model.is_sleeping;
                                if let Err(e) = model.layout.config.resolve_conflicts(None) {
                                    log::warn!("Unable to resolve layout conflicts before saving: {}", e);
                                }
                                model.layout.save();
                                model.modal_stack.close(&ModalState::LayoutManager);
                            }
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("[ESC] Cancel")
                                .small()
                                .color(egui::Color32::from_rgba_unmultiplied(100, 100, 100, (200.0 * alpha) as u8)));
                        });
                    });
                    
                    ui.add_space(10.0);
                    ui.add(egui::Separator::default().spacing(10.0));
                    ui.add_space(10.0);
                    
                    egui::ScrollArea::vertical().max_height(modal_height - 100.0).show(ui, |ui| {
                        // Grid Dimensions
                        ui.label(egui::RichText::new("GRID DIMENSIONS")
                            .small()
                            .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                        ui.add_space(8.0);
                        
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgba_unmultiplied(15, 15, 20, (200.0 * alpha) as u8))
                            .inner_margin(egui::Margin::same(12.0))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Columns:")
                                    .color(egui::Color32::from_rgba_unmultiplied(180, 180, 180, (255.0 * alpha) as u8)));
                                let mut cols_to_remove = None;
                                for (i, col) in model.layout.config.columns.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(format!("Col {}:", i))
                                            .small()
                                            .color(egui::Color32::from_rgba_unmultiplied(120, 120, 120, (200.0 * alpha) as u8)));
                                        ui.text_edit_singleline(col);
                                        if ui.small_button("✕").clicked() { cols_to_remove = Some(i); }
                                    });
                                }
                                if let Some(i) = cols_to_remove { model.layout.config.columns.remove(i); }
                                if ui.small_button("+ Add Column").clicked() { 
                                    model.layout.config.columns.push("1fr".to_string()); 
                                }
                                
                                ui.add_space(10.0);
                                
                                ui.label(egui::RichText::new("Rows:")
                                    .color(egui::Color32::from_rgba_unmultiplied(180, 180, 180, (255.0 * alpha) as u8)));
                                let mut rows_to_remove = None;
                                for (i, row) in model.layout.config.rows.iter_mut().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(format!("Row {}:", i))
                                            .small()
                                            .color(egui::Color32::from_rgba_unmultiplied(120, 120, 120, (200.0 * alpha) as u8)));
                                        ui.text_edit_singleline(row);
                                        if ui.small_button("✕").clicked() { rows_to_remove = Some(i); }
                                    });
                                }
                                if let Some(i) = rows_to_remove { model.layout.config.rows.remove(i); }
                                if ui.small_button("+ Add Row").clicked() { 
                                    model.layout.config.rows.push("1fr".to_string()); 
                                }
                            });
                        
                        ui.add_space(15.0);
                        
                        // Active Tiles
                        ui.label(egui::RichText::new("ACTIVE TILES")
                            .small()
                            .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                        ui.add_space(8.0);
                        
                        let mut tile_to_remove = None;
                        for (i, tile) in model.layout.config.tiles.iter_mut().enumerate() {
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 25, (200.0 * alpha) as u8))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(50, 50, 60, (150.0 * alpha) as u8)))
                                .inner_margin(egui::Margin::same(10.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(&tile.id)
                                            .strong()
                                            .color(egui::Color32::from_rgba_unmultiplied(0, 255, 255, (255.0 * alpha) as u8)));
                                        ui.label(egui::RichText::new(format!("({})", tile.module))
                                            .small()
                                            .color(egui::Color32::from_rgba_unmultiplied(100, 100, 100, (200.0 * alpha) as u8)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.add(egui::Button::new(
                                                egui::RichText::new("DELETE")
                                                    .small()
                                                    .color(egui::Color32::from_rgba_unmultiplied(255, 100, 100, (255.0 * alpha) as u8)))
                                                .fill(egui::Color32::TRANSPARENT)
                                            ).clicked() { 
                                                tile_to_remove = Some(i); 
                                            }
                                        });
                                    });
                                    
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Pos:")
                                            .small()
                                            .color(egui::Color32::from_rgba_unmultiplied(120, 120, 120, (200.0 * alpha) as u8)));
                                        ui.add(egui::DragValue::new(&mut tile.col).prefix("Col:"));
                                        ui.add(egui::DragValue::new(&mut tile.row).prefix("Row:"));
                                        ui.add_space(20.0);
                                        ui.label(egui::RichText::new("Span:")
                                            .small()
                                            .color(egui::Color32::from_rgba_unmultiplied(120, 120, 120, (200.0 * alpha) as u8)));
                                        let mut cs = tile.colspan.unwrap_or(1);
                                        let mut rs = tile.rowspan.unwrap_or(1);
                                        ui.add(egui::DragValue::new(&mut cs).prefix("W:"));
                                        ui.add(egui::DragValue::new(&mut rs).prefix("H:"));
                                        tile.colspan = Some(cs);
                                        tile.rowspan = Some(rs);
                                    });
                                    
                                    ui.checkbox(&mut tile.enabled, "Enabled");
                                });
                            ui.add_space(5.0);
                        }
                        if let Some(i) = tile_to_remove { model.layout.config.tiles.remove(i); }
                        
                        ui.add_space(15.0);
                        
                        // Add New Tile
                        ui.menu_button(
                            egui::RichText::new("+ ADD NEW TILE")
                                .color(egui::Color32::from_rgba_unmultiplied(0, 255, 128, (255.0 * alpha) as u8)), 
                            |ui| {
                                let modules: Vec<_> = model.patch_bay.get_modules().iter().map(|m| (*m).clone()).collect();
                                for module in modules {
                                    if ui.button(&module.name).clicked() {
                                        model.layout.config.tiles.push(TileConfig {
                                            id: format!("{}_new", module.id),
                                            col: 0,
                                            row: 0,
                                            colspan: Some(1),
                                            rowspan: Some(1),
                                            module: module.id.clone(),
                                            enabled: true,
                                            settings: Default::default(),
                                        });
                                        ui.close_menu();
                                    }
                                }
                            }
                        );
                    });
                });
            });
    }

    // Add Tile Picker Modal (Fullscreen Style, keyboard-driven)
    if let Some((col, row, selected_idx)) = model.modal_stack.get_add_tile_picker() {
        let anim = model.modal_anims.get(&ModalAnimKey::AddTilePicker);
        let alpha = anim.map(|a| a.eased()).unwrap_or(1.0);
        let scale = 0.9 + 0.1 * alpha;
        
        let screen_rect = ctx.screen_rect();
        let margin = 80.0 * (1.0 + (1.0 - alpha));
        let modal_width = (screen_rect.width() - margin * 2.0).min(500.0) * scale;
        let modal_height = (screen_rect.height() - margin * 2.0).min(450.0) * scale;
        let modal_x = screen_rect.center().x - modal_width / 2.0;
        let modal_y = screen_rect.center().y - modal_height / 2.0;

        let available = model.tile_registry.list_tiles();
        
        // Fullscreen dark backdrop
        egui::Area::new(egui::Id::new("add_tile_picker_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .show(&ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(rect, 0.0, 
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, (220.0 * alpha) as u8));
            });
        
        egui::Area::new(egui::Id::new("add_tile_picker_modal"))
            .fixed_pos(egui::pos2(modal_x, modal_y))
            .show(&ctx, |ui| {
                let frame = egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(8, 8, 12, (250.0 * alpha) as u8))
                    .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(0, 255, 128, (200.0 * alpha) as u8)))
                    .inner_margin(egui::Margin::same(25.0));
                
                frame.show(ui, |ui| {
                    ui.set_min_size(egui::vec2(modal_width, modal_height));
                    
                    // Header
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("ADD TILE @ ({}, {})", col, row))
                            .heading()
                            .size(24.0)
                            .color(egui::Color32::from_rgba_unmultiplied(0, 255, 128, (255.0 * alpha) as u8)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("[ESC] Cancel")
                                .small()
                                .color(egui::Color32::from_rgba_unmultiplied(100, 100, 100, (200.0 * alpha) as u8)));
                        });
                    });
                    
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("↑/↓ Navigate   Enter: Place   Esc: Cancel")
                        .small()
                        .color(egui::Color32::from_rgba_unmultiplied(100, 100, 100, (200.0 * alpha) as u8)));
                    
                    ui.add_space(10.0);
                    ui.add(egui::Separator::default().spacing(10.0));
                    ui.add_space(10.0);

                    if available.is_empty() {
                        ui.label(egui::RichText::new("No tile modules registered")
                            .color(egui::Color32::from_rgba_unmultiplied(255, 100, 100, (255.0 * alpha) as u8)));
                    } else {
                        ui.label(egui::RichText::new("SELECT MODULE")
                            .small()
                            .color(egui::Color32::from_rgba_unmultiplied(100, 100, 110, (200.0 * alpha) as u8)));
                        ui.add_space(10.0);
                        
                        egui::ScrollArea::vertical().max_height(modal_height - 160.0).show(ui, |ui| {
                            for (i, module_id) in available.iter().enumerate() {
                                let is_selected = i == selected_idx;
                                
                                let (bg_color, text_color) = if is_selected {
                                    (
                                        egui::Color32::from_rgba_unmultiplied(0, 60, 40, (200.0 * alpha) as u8),
                                        egui::Color32::from_rgba_unmultiplied(0, 255, 128, (255.0 * alpha) as u8)
                                    )
                                } else {
                                    (
                                        egui::Color32::TRANSPARENT,
                                        egui::Color32::from_rgba_unmultiplied(160, 160, 160, (200.0 * alpha) as u8)
                                    )
                                };
                                
                                egui::Frame::none()
                                    .fill(bg_color)
                                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                    .show(ui, |ui| {
                                        let prefix = if is_selected { "▶" } else { " " };
                                        ui.label(egui::RichText::new(format!("{} {}", prefix, module_id))
                                            .size(16.0)
                                            .color(text_color));
                                    });
                            }
                        });
                    }
                });
            });
    }

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
    // Skip nannou key handling when egui wants keyboard input (e.g., TextEdit is focused)
    if model.egui.ctx().wants_keyboard_input() {
        return;
    }
    
    let ctrl = _app.keys.mods.ctrl();
    
    // === MODAL ESC HANDLING (highest priority) ===
    // ESC closes the top modal before any other input processing
    if key == Key::Escape {
        // Check if a tile is maximized - close it first
        if model.modal_stack.get_maximized_tile().is_some() {
            model.is_closing = true;
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

    // === MODAL NAVIGATION ===
    if key == Key::Escape && !model.modal_stack.is_empty() {
        model.modal_stack.pop();
        return;
    }

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
                model.modal_stack.push(ModalState::GlobalSettings);
            },

            AppAction::OpenAddTilePicker { col, row } => {
                model.modal_stack.open_add_tile_picker(col, row);
            },
            AppAction::OpenPatchBay => {
                if !model.modal_stack.is_patch_bay_open() {
                    model.modal_stack.push(ModalState::PatchBay);
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
        }
    }
    
    // Sync selected_tile with keyboard_nav (source of truth)
    model.selected_tile = model.keyboard_nav.selected_tile_id().map(|s| s.to_string());
}



fn raw_window_event(_app: &App, model: &mut Model, event: &nannou::winit::event::WindowEvent) {
    model.egui.handle_raw_event(event);
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
                         draw.text("+")
                             .xy(rect.xy())
                             .color(stroke_color)
                             .font_size(24);
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
                    egui_ctx: None,
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
                    egui_ctx: None,
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
            
        draw.text("Zzz")
             .xy(pt2(app.window_rect().right() - 30.0, app.window_rect().bottom() + 30.0))
             .color(rgba(0.5, 0.5, 1.0, 0.5))
             .font_size(24);
    }

    // Mode indicator (bottom-left corner)
    if maximized_tile.is_none() {
        let mode_text = match model.keyboard_nav.mode {
            input::InputMode::Normal => "NORMAL",
            input::InputMode::Layout => match &model.keyboard_nav.layout_state {
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
        draw.text(mode_text)
            .xy(pt2(win_rect.left() + 50.0, win_rect.bottom() + 20.0))
            .color(mode_color)
            .font_size(14);
        
        // Show keybind hints
        let hints = match model.keyboard_nav.mode {
            input::InputMode::Normal => "[L]ayout [P]atch [G]lobal [Tab]Cycle [Arrows]Nav [E]dit [Enter]Select",
            input::InputMode::Layout => "[E]dit [A]dd [D]elete [Space]Toggle [Enter]Confirm [ESC]Cancel",
            input::InputMode::Patch => "[Arrows]Select [Enter]Patch [ESC]Exit",
        };
        
        draw.text(hints)
            .xy(pt2(win_rect.left() + 250.0, win_rect.bottom() + 20.0))
            .color(rgba(0.4, 0.4, 0.4, 0.8))
            .font_size(10);
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


    draw.to_frame(app, &frame).unwrap();
    model.egui.draw_to_frame(&frame).unwrap();
}
