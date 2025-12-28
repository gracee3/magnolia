use nannou::prelude::*;
use talisman_core::{Signal, PatchBay, PluginManager, PluginModuleAdapter, ModuleRuntime, RoutedSignal};
use talisman_core::{Source, Sink, Processor};
use talisman_core::adapters::{SourceAdapter, SinkAdapter, ProcessorAdapter};
use nannou_egui::{self, Egui, egui};
use tokio::sync::mpsc;

use audio_input::{AudioInputSource, AudioVizSink};
use audio_input::tile::AudioVisTile;
use audio_dsp::AudioDspProcessor;
use audio_output::{AudioOutputSink, AudioOutputState};
use audio_output::tile::AudioOutputTile;
// use talisman_core::ring_buffer; // Removed usage


// Layout editor and visualizer modules
mod patch_visualizer;
mod layout;
mod tiles;
mod input;
mod theme;



use layout::Layout;
use tiles::{TileRegistry, RenderContext};
use input::KeyboardNav;




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
    maximized_tile: Option<String>,
    last_click_time: std::time::Instant,
    // Animation
    anim_factor: f32, // 0.0 to 1.0
    is_closing: bool,
    clipboard: Option<arboard::Clipboard>,
    context_menu: Option<ContextMenuState>,
    
    // Patch Bay State
    patch_bay: PatchBay,
    show_patch_bay: bool,
    
    // Settings & Controls
    show_global_settings: bool,
    show_tile_settings: Option<String>,  // tile_id if showing settings for that tile
    show_layout_manager: bool,
    is_sleeping: bool,
    
    // Runtime State
    module_host: talisman_core::ModuleHost,
    plugin_manager: talisman_core::PluginManager,
    
    // Audio State - Managed by plugins
    // audio_stream_rx: Option<ring_buffer::RingBufferReceiver<talisman_core::AudioFrame>>, // Removed

    
    // Tile System (Phase 6: Settings Architecture)
    tile_registry: TileRegistry,
    _compositor: tiles::Compositor,
    start_time: std::time::Instant,
    frame_count: u64,
    
    // Keyboard Navigation (keyboard-first UI)
    keyboard_nav: KeyboardNav,
    
}


struct ContextMenuState {
    tile_id: String,
    position: Point2,
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
        .mouse_released(mouse_released)
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
    
    // Audio visualization tile (fed by AudioVizSink)
    let mut vis_tile = AudioVisTile::new("audio_viz");
    let vis_buffer = vis_tile.get_legacy_buffer();
    let vis_latency = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    vis_tile.connect_latency_meter(vis_latency.clone());
    tile_registry.register(vis_tile);

    // Audio output tile (fed by AudioOutputSink)
    let (audio_output_sink, audio_output_state) = match AudioOutputSink::new("audio_output") {
        Ok((sink, state)) => (Some(sink), state),
        Err(e) => {
            log::error!("Failed to initialize audio output: {}", e);
            // Create a dummy state to keep UI stable
            let state = std::sync::Arc::new(AudioOutputState::default());
            (None, state)
        }
    };

    tile_registry.register(AudioOutputTile::new("audio_output", audio_output_state.clone()));

    // Audio pipeline modules
    if let Ok(audio_input_source) = AudioInputSource::new("audio_input") {
        let schema = audio_input_source.schema();
        patch_bay.register_module(schema);
        if let Err(e) = module_host.spawn(SourceAdapter::new(audio_input_source), 100) {
            log::error!("Failed to spawn audio input source: {}", e);
        }
    } else {
        log::error!("Audio input source failed to initialize");
    }

    let audio_dsp = AudioDspProcessor::new("audio_dsp", 1.0);
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
            log::info!("Spawning plugin module: {}", adapter.id());
            
            // Register schema if possible? 
            // Currently adapter schema is basic.
            // We should register it in PatchBay too?
            // For now just spawn executing module.
            // Note: If we don't register in PatchBay, they won't show up in UI for patching.
            // TODO: Extract schema from adapter and register in PatchBay
            patch_bay.register_module(adapter.schema());
            
            if let Err(e) = module_host.spawn(adapter, 100) {
                 log::error!("Failed to spawn plugin: {}", e);
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
        maximized_tile: None,
        last_click_time: std::time::Instant::now(),
        anim_factor: 0.0,
        is_closing: false,
        clipboard,
        context_menu: None,
        patch_bay,
        show_patch_bay: false,
        show_global_settings: false,
        show_tile_settings: None,
        show_layout_manager: false,
        is_sleeping: initial_sleep_state,
        // audio_stream_rx, // Removed

        module_host,
        plugin_manager,
        tile_registry,
        _compositor: tiles::Compositor::new(app),
        start_time: std::time::Instant::now(),
        frame_count: 0,
        keyboard_nav: KeyboardNav::new(),
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


/// Map tile ID to module ID for PatchBay
/// Map tile ID to module ID for PatchBay
/// In the microkernel architecture, we prefer 1:1 mapping or explicit config.
/// For now, we default to using the tile_id as the module_id.
fn tile_to_module(tile_id: &str) -> String {
    tile_id.to_string()
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



fn update(app: &App, model: &mut Model, update: Update) {
    // Update Layout dimensions
    model.layout.update(app.window_rect());
    
    // Smooth Animation
    if model.is_closing {
        model.anim_factor = (model.anim_factor - 0.1).max(0.0);
        if model.anim_factor <= 0.0 {
            // Save tile settings before clearing (persist any changes made in control mode)
            if let Some(ref tile_id) = model.maximized_tile {
                save_tile_settings(&model.tile_registry, &mut model.layout, tile_id);
            }
            model.maximized_tile = None;
            model.is_closing = false;
        }
    } else if model.maximized_tile.is_some() && model.anim_factor < 1.0 {
        model.anim_factor = (model.anim_factor + 0.1).min(1.0);
    }
    
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

    // Context Menu
    if let Some(menu) = &model.context_menu {
        let win_w = app.window_rect().w();
        let win_h = app.window_rect().h();
        
        let egui_x = menu.position.x + win_w / 2.0;
        let egui_y = win_h / 2.0 - menu.position.y;
        
        let mut open = true;
        let tile_id = menu.tile_id.clone();
        
        egui::Window::new("context_menu")
            .fixed_pos(egui::pos2(egui_x, egui_y))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .min_width(140.0)
            .default_width(140.0)
            .frame(egui::Frame {
                fill: egui::Color32::BLACK,
                stroke: egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 255)),
                inner_margin: egui::Margin::same(10.0),
                ..Default::default()
            })
            .show(&ctx, |ui| {
                // Custom Style for Flat/Transparent buttons
                let mut style = (*ctx.style()).clone();
                // Normal State
                style.visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
                style.visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 255));
                style.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                style.visuals.widgets.inactive.rounding = egui::Rounding::ZERO;
                
                // Hovered State
                style.visuals.widgets.hovered.bg_fill = egui::Color32::TRANSPARENT;
                style.visuals.widgets.hovered.weak_bg_fill = egui::Color32::TRANSPARENT;
                style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 255));
                style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 255));
                style.visuals.widgets.hovered.rounding = egui::Rounding::ZERO;
                
                // Active/Clicked State
                style.visuals.widgets.active.bg_fill = egui::Color32::TRANSPARENT;
                style.visuals.widgets.active.weak_bg_fill = egui::Color32::TRANSPARENT;
                style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 255, 255)); // Thicker stroke for active
                style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 255));
                style.visuals.widgets.active.rounding = egui::Rounding::ZERO;
                
                ui.set_style(style);
                
                ui.label(egui::RichText::new(format!("TILE: {}", tile_id)).strong().color(egui::Color32::from_rgb(0, 255, 255)));
                ui.add(egui::Separator::default().spacing(10.0));
                
                let btn_size = egui::vec2(ui.available_width(), 20.0);

                let res = ui.add_sized(btn_size, egui::Button::new("SETTINGS"));
                if res.clicked() || res.secondary_clicked() {
                    model.show_tile_settings = Some(tile_id.clone());
                    log::info!("Opening settings for {}", tile_id);
                    open = false;
                }
                
                let res = ui.add_sized(btn_size, egui::Button::new("COPY"));
                if res.clicked() || res.secondary_clicked() {
                     // Get content from tile registry
                     let content = model.tile_registry.get_display_text(&tile_id);
                    
                    if let Some(text) = content {
                         if let Some(cb) = &mut model.clipboard {
                             let _ = cb.set_text(text);
                             log::info!("Copied via Menu");
                         }
                    }
                    open = false;
                }
                
                let res = ui.add_sized(btn_size, egui::Button::new("PASTE"));
                if res.clicked() || res.secondary_clicked() {
                     // Note: Paste functionality removed - tiles handle their own input
                     open = false;
                }
                
                ui.add(egui::Separator::default().spacing(10.0));
                
                // Toggle button text based on disabled state
                // Toggle button text based on disabled state
                // Use a block to avoid creating a double borrow if we did this inline, 
                // but actually we need to find the tile index first.
                let tile_idx = model.layout.config.tiles.iter().position(|t| t.id == tile_id);
                
                if let Some(idx) = tile_idx {
                    let is_disabled = !model.layout.config.tiles[idx].enabled;
                    let disable_text = if is_disabled { "ENABLE" } else { "DISABLE" };
                    
                    let res = ui.add_sized(btn_size, egui::Button::new(disable_text));
                    if res.clicked() || res.secondary_clicked() {
                         let module_id = tile_to_module(&tile_id);
                         
                         // Toggle state
                         let new_state = is_disabled; // if was disabled, new state is enabled (true)
                         model.layout.config.tiles[idx].enabled = new_state;

                         if new_state {
                             model.patch_bay.enable_module(&module_id);
                             log::info!("Enabled Tile/Module: {} / {}", tile_id, module_id);
                         } else {
                             model.patch_bay.disable_module(&module_id);
                             log::info!("Disabled Tile/Module: {} / {}", tile_id, module_id);
                         }
                         model.layout.save();
                         open = false;
                    }
                }

                
                let res = ui.add_sized(btn_size, egui::Button::new("REMOVE"));
                if res.clicked() || res.secondary_clicked() {
                    // Remove from layout config
                    model.layout.config.tiles.retain(|t| t.id != tile_id);
                    model.layout.save();
                    open = false;
                }
                
                ui.add(egui::Separator::default().spacing(10.0));
                let res = ui.add_sized(btn_size, egui::Button::new("PATCH BAY"));
                if res.clicked() || res.secondary_clicked() {
                    model.show_patch_bay = true;
                    log::info!("Opening Patch Bay");
                    open = false;
                }
                let res = ui.add_sized(btn_size, egui::Button::new("GLOBAL SETTINGS"));
                if res.clicked() || res.secondary_clicked() {
                    model.show_global_settings = true;
                    log::info!("Opening Global Settings");
                    open = false;
                }
                
                // Sleep toggle
                let sleep_text = if model.is_sleeping { "WAKE" } else { "SLEEP" };
                let res = ui.add_sized(btn_size, egui::Button::new(sleep_text));
                if res.clicked() || res.secondary_clicked() {
                    model.is_sleeping = !model.is_sleeping;
                    model.layout.config.is_sleeping = model.is_sleeping;
                    model.layout.save();
                    log::info!("Engine {}", if model.is_sleeping { "sleeping" } else { "awake" });
                    open = false;
                }
                
                ui.add(egui::Separator::default().spacing(10.0));
                
                let res = ui.add_sized(btn_size, egui::Button::new("EXIT DAEMON"));
                if res.clicked() || res.secondary_clicked() {
                    std::process::exit(0);
                }
            });
            
        if !open {
            model.context_menu = None;
        }
    }

    // Patch Bay Modal
    if model.show_patch_bay {
        let screen_rect = ctx.screen_rect();
        let width = 600.0;
        let height = 450.0;
        let x = screen_rect.center().x - width / 2.0;
        let y = screen_rect.center().y - height / 2.0;

        egui::Window::new("PATCH BAY")
            .fixed_pos(egui::pos2(x, y))
            .fixed_size(egui::vec2(width, height))
            .collapsible(false)
            .resizable(true)
            .frame(egui::Frame {
                fill: egui::Color32::from_rgba_unmultiplied(10, 10, 10, 250),
                stroke: egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 255, 255)),
                inner_margin: egui::Margin::same(15.0),
                ..Default::default()
            })
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("MODULE PATCH BAY")
                        .heading()
                        .color(egui::Color32::from_rgb(0, 255, 255)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            model.show_patch_bay = false;
                        }
                    });
                });
                
                ui.add(egui::Separator::default().spacing(10.0));
                
                // Module List
                ui.label(egui::RichText::new("REGISTERED MODULES")
                    .small()
                    .color(egui::Color32::GRAY));
                
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        let modules: Vec<_> = model.patch_bay.get_modules()
                            .iter()
                            .map(|m| (*m).clone())
                            .collect();
                        
                        for module in modules {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&module.name)
                                        .strong()
                                        .color(egui::Color32::from_rgb(0, 255, 255)));
                                    ui.label(egui::RichText::new(format!("({})", module.id))
                                        .small()
                                        .color(egui::Color32::GRAY));
                                });
                                
                                ui.label(egui::RichText::new(&module.description)
                                    .small()
                                    .color(egui::Color32::from_rgb(150, 150, 150)));
                                
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("PORTS:").small().color(egui::Color32::GRAY));
                                    for port in &module.ports {
                                        let (prefix, color) = match port.direction {
                                            talisman_core::PortDirection::Input => ("IN", egui::Color32::from_rgb(100, 200, 100)),
                                            talisman_core::PortDirection::Output => ("OUT", egui::Color32::from_rgb(200, 100, 100)),
                                        };
                                        ui.label(egui::RichText::new(format!("{} {} ({:?})", prefix, port.label, port.data_type))
                                            .small()
                                            .color(color));
                                    }
                                });
                            });
                            ui.add_space(5.0);
                        }
                    });
                
                ui.add(egui::Separator::default().spacing(10.0));
                
                // Connections
                let patches = model.patch_bay.get_patches();
                if patches.is_empty() {
                    ui.label(egui::RichText::new("No active patches")
                        .small()
                        .color(egui::Color32::GRAY));
                } else {
                    ui.label(egui::RichText::new(format!("ACTIVE PATCHES: {}", patches.len()))
                        .small()
                        .color(egui::Color32::GRAY));
                    for patch in patches {
                        ui.label(egui::RichText::new(format!(
                            "  {}:{} -> {}:{}",
                            patch.source_module, patch.source_port,
                            patch.sink_module, patch.sink_port
                        )).small().color(egui::Color32::YELLOW));
                    }
                }
            });
    }

    // Global Settings Modal
    if model.show_global_settings {
        let screen_rect = ctx.screen_rect();
        let width = 400.0;
        let height = 300.0;
        let x = screen_rect.center().x - width / 2.0;
        let y = screen_rect.center().y - height / 2.0;

        egui::Window::new("GLOBAL SETTINGS")
            .fixed_pos(egui::pos2(x, y))
            .fixed_size(egui::vec2(width, height))
            .collapsible(false)
            .resizable(true)
            .frame(egui::Frame {
                fill: egui::Color32::from_rgba_unmultiplied(10, 10, 10, 250),
                stroke: egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 255, 255)),
                inner_margin: egui::Margin::same(15.0),
                ..Default::default()
            })
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("TALISMAN CONFIGURATION")
                        .heading()
                        .color(egui::Color32::from_rgb(0, 255, 255)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            model.show_global_settings = false;
                        }
                    });
                });
                
                ui.add(egui::Separator::default().spacing(10.0));
                
                // Display options
                ui.label(egui::RichText::new("DISPLAY").small().color(egui::Color32::GRAY));
                // (Retinal burn mode removed)
                
                ui.add_space(10.0);
                
                // Engine state
                ui.label(egui::RichText::new("ENGINE").small().color(egui::Color32::GRAY));
                let status = if model.is_sleeping { "SLEEPING" } else { "ACTIVE" };
                let status_color = if model.is_sleeping { 
                    egui::Color32::from_rgb(255, 100, 100) 
                } else { 
                    egui::Color32::from_rgb(100, 255, 100) 
                };
                ui.horizontal(|ui| {
                    ui.label("Status:");
                    ui.label(egui::RichText::new(status).color(status_color));
                });
                
                if ui.button(if model.is_sleeping { "WAKE ENGINE" } else { "SLEEP ENGINE" }).clicked() {
                    model.is_sleeping = !model.is_sleeping;
                    model.layout.config.is_sleeping = model.is_sleeping;
                    model.layout.save();
                }
                
                if ui.button("OPEN LAYOUT MANAGER").clicked() {
                    model.show_layout_manager = true;
                    model.show_global_settings = false;
                }
                
                ui.add_space(10.0);
                
                // Module overview
                ui.label(egui::RichText::new("MODULES").small().color(egui::Color32::GRAY));
                ui.label(format!("Registered: {}", model.patch_bay.get_modules().len()));
                ui.label(format!("Active Patches: {}", model.patch_bay.get_patches().len()));
                ui.label(format!("Disabled: {}", model.layout.config.tiles.iter().filter(|t| !t.enabled).count()));
            });
    }

    // Tile Settings Modal
    if let Some(tile_id) = model.show_tile_settings.clone() {
        let module_id = tile_to_module(&tile_id);
        let module_info = model.patch_bay.get_module(&module_id).cloned();
        let screen_rect = ctx.screen_rect();
        let width = 400.0;
        let height = 350.0;
        let x = screen_rect.center().x - width / 2.0;
        let y = screen_rect.center().y - height / 2.0;

        egui::Window::new(format!("Settings: {}", tile_id))
            .fixed_pos(egui::pos2(x, y))
            .fixed_size(egui::vec2(width, height))
            .collapsible(false)
            .resizable(true)
            .frame(egui::Frame {
                fill: egui::Color32::from_rgba_unmultiplied(10, 10, 10, 250),
                stroke: egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 255, 255)),
                inner_margin: egui::Margin::same(15.0),
                ..Default::default()
            })
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("TILE: {}", tile_id.to_uppercase()))
                        .heading()
                        .color(egui::Color32::from_rgb(0, 255, 255)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            model.show_tile_settings = None;
                        }
                    });
                });
                
                ui.add(egui::Separator::default().spacing(10.0));
                
                // Module info
                if let Some(module) = module_info {
                    ui.label(egui::RichText::new("MODULE INFO").small().color(egui::Color32::GRAY));
                    ui.label(format!("Name: {}", module.name));
                    ui.label(format!("ID: {}", module.id));
                    ui.label(&module.description);
                    
                    ui.add_space(10.0);
                    
                    // Ports
                    ui.label(egui::RichText::new("PORTS").small().color(egui::Color32::GRAY));
                    for port in &module.ports {
                        let (icon, color) = match port.direction {
                            talisman_core::PortDirection::Input => ("◀ IN", egui::Color32::from_rgb(100, 200, 100)),
                            talisman_core::PortDirection::Output => ("▶ OUT", egui::Color32::from_rgb(200, 100, 100)),
                        };
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(icon).color(color));
                            ui.label(&port.label);
                            ui.label(egui::RichText::new(format!("({:?})", port.data_type)).small().color(egui::Color32::GRAY));
                        });
                    }
                    
                    ui.add_space(10.0);
                    
                    // Connections
                    ui.label(egui::RichText::new("CONNECTIONS").small().color(egui::Color32::GRAY));
                    let incoming = model.patch_bay.get_incoming_patches(&module.id);
                    let outgoing = model.patch_bay.get_outgoing_patches(&module.id);
                    
                    if incoming.is_empty() && outgoing.is_empty() {
                        ui.label("No connections");
                    } else {
                        for patch in incoming {
                            ui.label(egui::RichText::new(format!("← FROM: {}:{}", patch.source_module, patch.source_port))
                                .small()
                                .color(egui::Color32::from_rgb(100, 200, 100)));
                        }
                        for patch in outgoing {
                            ui.label(egui::RichText::new(format!("→ TO: {}:{}", patch.sink_module, patch.sink_port))
                                .small()
                                .color(egui::Color32::from_rgb(200, 100, 100)));
                        }
                    }
                } else {
                    ui.label(egui::RichText::new("Module not found in Patch Bay")
                        .color(egui::Color32::from_rgb(255, 100, 100)));
                }
                
                ui.add_space(10.0);
                
                // Enabled state
                ui.label(egui::RichText::new("STATE").small().color(egui::Color32::GRAY));
                // Find tile config to check enabled state
                let tile_config = model.layout.config.tiles.iter().find(|t| t.id == tile_id);
                let is_disabled = tile_config.map(|t| !t.enabled).unwrap_or(false);
                let state_text = if is_disabled { "DISABLED" } else { "ENABLED" };
                let state_color = if is_disabled { 
                    egui::Color32::from_rgb(255, 100, 100) 
                } else { 
                    egui::Color32::from_rgb(100, 255, 100) 
                };
                ui.label(egui::RichText::new(state_text).color(state_color));
            });
    }

    // Layout Manager Modal
    if model.show_layout_manager {
        let screen_rect = ctx.screen_rect();
        egui::Window::new("LAYOUT MANAGER")
            .fixed_pos(egui::pos2(screen_rect.center().x - 250.0, screen_rect.center().y - 300.0))
            .fixed_size(egui::vec2(500.0, 600.0))
            .resizable(true)
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("GRID MANAGER");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("CLOSE & SAVE").clicked() {
                            // Sync patches from PatchBay to Layout Config
                            model.layout.config.patches = model.patch_bay.get_patches().to_vec();
                            // Sync global state
                            model.layout.config.is_sleeping = model.is_sleeping;
                            model.layout.save();
                            model.show_layout_manager = false;
                        }
                    });
                });
                
                ui.separator();
                
                // 1. Grid Definition (Rows/Cols)
                ui.collapsing("Grid Dimensions", |ui| {
                    ui.label("Columns:");
                    let mut cols_to_remove = None;
                    for (i, col) in model.layout.config.columns.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("Col {}:", i));
                            ui.text_edit_singleline(col);
                            if ui.button("x").clicked() { cols_to_remove = Some(i); }
                        });
                    }
                    if let Some(i) = cols_to_remove { model.layout.config.columns.remove(i); }
                    if ui.button("+ Add Column").clicked() { model.layout.config.columns.push("1fr".to_string()); }

                    ui.label("Rows:");
                    let mut rows_to_remove = None;
                    for (i, row) in model.layout.config.rows.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("Row {}:", i));
                            ui.text_edit_singleline(row);
                            if ui.button("x").clicked() { rows_to_remove = Some(i); }
                        });
                    }
                    if let Some(i) = rows_to_remove { model.layout.config.rows.remove(i); }
                    if ui.button("+ Add Row").clicked() { model.layout.config.rows.push("1fr".to_string()); }
                });

                ui.separator();
                
                // 2. Active Tiles
                ui.label("Active Tiles:");
                egui::ScrollArea::vertical().max_height(350.0).show(ui, |ui| {
                    let mut tile_to_remove = None;
                    for (i, tile) in model.layout.config.tiles.iter_mut().enumerate() {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::from_rgb(0, 255, 255), &tile.id);
                                ui.label(format!("({})", tile.module));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("DELETE").clicked() { tile_to_remove = Some(i); }
                                });
                            });
                            
                            ui.horizontal(|ui| {
                                ui.label("Pos:");
                                ui.add(egui::DragValue::new(&mut tile.col).prefix("Col:"));
                                ui.add(egui::DragValue::new(&mut tile.row).prefix("Row:"));
                            });
                            
                            ui.horizontal(|ui| {
                                ui.label("Span:");
                                let mut cs = tile.colspan.unwrap_or(1);
                                let mut rs = tile.rowspan.unwrap_or(1);
                                ui.add(egui::DragValue::new(&mut cs).prefix("Col:"));
                                ui.add(egui::DragValue::new(&mut rs).prefix("Row:"));
                                tile.colspan = Some(cs);
                                tile.rowspan = Some(rs);
                            });
                            
                            ui.checkbox(&mut tile.enabled, "Enabled");
                        });
                    }
                    if let Some(i) = tile_to_remove { model.layout.config.tiles.remove(i); }
                });

                ui.separator();

                // 3. Add New Tile
                ui.menu_button("ADD NEW TILE...", |ui| {
                   // Clone module list to avoid borrow checker issues with patch_bay
                   let modules: Vec<_> = model.patch_bay.get_modules().iter().map(|m| (*m).clone()).collect();
                   for module in modules {
                       if ui.button(&module.name).clicked() {
                           // Add to layout
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
                });
            });
    }

    // (Close confirmation dialog removed - ESC is for navigation only, not exit)



    // (Legacy signal_handler::process_signals removed - tiles handle their own state via TileRegistry)
}

fn mouse_pressed(app: &App, model: &mut Model, button: MouseButton) {
    // 0. Intercept clicks for Egui
    if model.egui.ctx().wants_pointer_input() {
        return;
    }
    
    
    // Clear context menu if clicking away (and egui didn't want it)
    model.context_menu = None;

    // Mouse handling for tile selection (keyboard-first navigation)


    if button == MouseButton::Left {

        let mouse_pos = app.mouse.position();
        let now = std::time::Instant::now();
        let delta = now.duration_since(model.last_click_time);
        let is_double_click = delta.as_millis() < 300;
        model.last_click_time = now;

        let mut hit = false;
        for tile in &model.layout.config.tiles {
            if let Some(rect) = model.layout.calculate_rect(tile) {
                if rect.contains(mouse_pos) {
                    if is_double_click && model.selected_tile.as_ref() == Some(&tile.id) {
                        // Toggle Maximization
                        if model.maximized_tile.as_ref() == Some(&tile.id) {
                            model.is_closing = true;
                        } else {
                            model.maximized_tile = Some(tile.id.clone());
                            model.is_closing = false;
                            model.anim_factor = 0.0; // Reset animation
                        }
                    } else {
                        model.selected_tile = Some(tile.id.clone());
                    }
                    hit = true;
                    log::debug!("Clicked Tile: {}", tile.id);
                    break;
                }
            }
        }
        
        if !hit {
            // Check for empty cells to open Layout Manager
            if model.maximized_tile.is_none() {
                let cols = model.layout.config.columns.len();
                let rows = model.layout.config.rows.len();
                for c in 0..cols {
                    for r in 0..rows {
                         if model.layout.get_tile_at(c, r).is_none() {
                             let temp_tile = TileConfig {
                                 id: String::new(), col: c, row: r, colspan: Some(1), rowspan: Some(1),
                                 module: String::new(), enabled: true, settings: Default::default()
                             };
                             
                             if let Some(rect) = model.layout.calculate_rect(&temp_tile) {
                                  if rect.contains(mouse_pos) {
                                      model.show_layout_manager = true;
                                      return;
                                  }
                             }
                         }
                    }
                }
            }

            model.selected_tile = None;
            if model.maximized_tile.is_some() {
                 model.is_closing = true;
            }
        }
    } else if button == MouseButton::Right {
         let mouse_pos = app.mouse.position();
         
         for tile in &model.layout.config.tiles {
            if let Some(rect) = model.layout.calculate_rect(tile) {
                if rect.contains(mouse_pos) {
                    model.context_menu = Some(ContextMenuState {
                        tile_id: tile.id.clone(),
                        position: mouse_pos,
                    });
                     // Also select it
                    model.selected_tile = Some(tile.id.clone());
                    break;
                }
            }
         }
    }
}



fn mouse_released(_app: &App, _model: &mut Model, _button: MouseButton) {
    // No-op: All operations are click-based, not drag-based
}

fn mouse_moved(_app: &App, _model: &mut Model, _pos: Point2) {
    // Mouse movement handling - keyboard-first navigation only
}

/// Dispatch keybinds configured for the currently selected tile
/// Returns true if a keybind was executed
fn dispatch_tile_keybind(model: &mut Model, key: Key) -> bool {
    // Get selected tile
    let tile_id = match &model.selected_tile {
        Some(id) => id.clone(),
        None => return false,
    };
    
    // Find tile config
    let tile_config = model.layout.config.tiles.iter()
        .find(|t| t.id == tile_id);
    
    let (module, keybinds) = match tile_config {
        Some(t) => (t.module.clone(), t.settings.keybinds.clone()),
        None => return false,
    };
    
    if keybinds.is_empty() {
        return false;
    }
    
    // Convert key to string for matching
    let key_str = format!("{:?}", key).to_lowercase();
    
    // Find matching keybind
    for (action, bound_key) in &keybinds {
        if bound_key.to_lowercase() == key_str {
            log::info!("Executing keybind: {} -> {} on tile {}", bound_key, action, tile_id);
            return model.tile_registry.execute_action(&module, action);
        }
    }
    
    false
}


fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    // === INPUT ROUTING GUARD ===
    // Skip nannou key handling when:
    // 1. Egui wants keyboard input (e.g., TextEdit is focused)
    // 2. Modal layer is active (modals handle their own keys via egui)
    if model.egui.ctx().wants_keyboard_input() 
        
    {
        return;
    }
    
    let ctrl = _app.keys.mods.ctrl();
    
    // Update grid size for navigation
    let (grid_cols, grid_rows) = model.layout.config.resolve_grid();
    model.keyboard_nav.set_grid_size(grid_cols, grid_rows);
    
    // === TILE KEYBIND DISPATCH ===
    // If a tile is selected, try tile-specific keybinds first
    if !ctrl && model.keyboard_nav.has_selection() {
        if dispatch_tile_keybind(model, key) {
            return; // Keybind handled
        }
    }
    
    if ctrl {
        // Ctrl key combinations (clipboard, exit)
        match key {
            Key::Q => {
                // Ctrl+Q = Exit application (the ONLY keyboard exit)
                log::info!("Ctrl+Q pressed - exiting application");
                std::process::exit(0);
            },
            Key::C => {
                // COPY logic - get content from tile registry
                if let Some(selected) = model.keyboard_nav.selected_tile_id() {
                    if let Some(text) = model.tile_registry.get_display_text(selected) {
                        if let Some(cb) = &mut model.clipboard {
                            if let Err(e) = cb.set_text(text) {
                                log::error!("Clipboard Copy Failed: {}", e);
                            } else {
                                log::info!("Copied to Clipboard");
                            }
                        }
                    }
                }
            },
            Key::V => {
                // Note: Paste functionality removed - tiles handle their own input
            },
            _ => {}
        }
    } else {
        // Non-Ctrl keys - keyboard-first navigation
        use input::{InputMode, Direction, LayoutSubState, EscapeResult};
        
        match key {
            // === ARROW KEYS - Always navigate ===
            Key::Up | Key::Down | Key::Left | Key::Right => {
                let direction = match key {
                    Key::Up => Direction::Up,
                    Key::Down => Direction::Down,
                    Key::Left => Direction::Left,
                    Key::Right => Direction::Right,
                    _ => return,
                };
                
                match model.keyboard_nav.mode {
                    InputMode::Normal | InputMode::Patch => {
                        // Smart tile-to-tile navigation
                        if let Some(tile_id) = model.keyboard_nav.navigate_to_adjacent_tile(direction, &model.layout.config) {
                            model.selected_tile = Some(tile_id.clone());
                            log::debug!("Navigated to tile: {}", tile_id);
                        } else {
                            // No tile found, deselect
                            model.selected_tile = None;
                            log::debug!("No adjacent tile in that direction");
                        }
                    },
                    InputMode::Layout => {
                        match &model.keyboard_nav.layout_state {
                            LayoutSubState::Resize { tile_id, original_bounds: _ } => {
                                // Arrow keys resize the tile
                                let (delta_col, delta_row) = KeyboardNav::resize_direction(direction);
                                let tile_id = tile_id.clone();
                                
                                // First, read the current position and size
                                let tile_info = model.layout.config.tiles.iter()
                                    .find(|t| t.id == tile_id)
                                    .map(|t| (t.col, t.row, t.colspan.unwrap_or(1), t.rowspan.unwrap_or(1)));
                                
                                if let Some((_col, _row, colspan, rowspan)) = tile_info {
                                    let new_colspan = (colspan as i32 + delta_col).max(1) as usize;
                                    let new_rowspan = (rowspan as i32 + delta_row).max(1) as usize;
                                    
                                    // TODO: Re-implement collision detection for interactive layout editing
                                    // if model.layout_editor.is_placement_valid(...) { ... }
                                    //
                                    // For now, allow all resizes
                                    if let Some(tile) = model.layout.config.tiles.iter_mut().find(|t| t.id == tile_id) {
                                        tile.colspan = Some(new_colspan);
                                        tile.rowspan = Some(new_rowspan);
                                        log::debug!("Resized tile to {}x{}", new_colspan, new_rowspan);
                                    }
                                }
                            },
                            LayoutSubState::Move { tile_id, .. } => {
                                // Arrow keys move the tile (as 1×1)
                                let tile_id = tile_id.clone();
                                model.keyboard_nav.navigate(direction);
                                let (new_col, new_row) = model.keyboard_nav.cursor;
                                
                                // TODO: Re-implement collision detection for interactive layout editing
                                // if model.layout_editor.is_placement_valid(...) { ... }
                                //
                                // For now, allow all moves
                                if let Some(tile) = model.layout.config.tiles.iter_mut().find(|t| t.id == tile_id) {
                                    tile.col = new_col;
                                    tile.row = new_row;
                                    tile.colspan = Some(1);
                                    tile.rowspan = Some(1);
                                    log::debug!("Moved tile to ({}, {})", new_col, new_row);
                                }
                            },
                            LayoutSubState::Navigation => {
                                // Standard cursor navigation in layout mode
                                model.keyboard_nav.navigate(direction);
                                // Keyboard cursor is tracked in keyboard_nav
                                
                                // Auto-select tile if present
                                if let Some(tile_id) = model.keyboard_nav.select_tile_at_cursor(&model.layout.config) {
                                    model.selected_tile = Some(tile_id);
                                } else {
                                    model.keyboard_nav.deselect();
                                    model.selected_tile = None;
                                }
                            },
                        }
                    },
                }
            },
            
            // === E - Settings modal OR Layout mode ===
            Key::E => {
                match model.keyboard_nav.mode {
                    InputMode::Normal => {
                        if model.keyboard_nav.has_selection() {
                            // Tile selected → open settings modal (maximize tile)
                            if let Some(tile_id) = model.keyboard_nav.selected_tile_id() {
                                model.maximized_tile = Some(tile_id.to_string());
                                model.is_closing = false;
                                model.anim_factor = 0.0;
                                log::info!("Opening tile settings: {}", tile_id);
                            }
                        } else {
                            // No tile selected → enter layout mode
                            model.keyboard_nav.enter_layout_mode();
                            // TODO: Set edit mode flag when interactive editing is re-implemented
                            log::info!("Entered layout mode");
                        }
                    },
                    InputMode::Layout => {
                        if model.keyboard_nav.has_selection() {
                            // Enter resize mode
                            if model.keyboard_nav.enter_resize_mode(&model.layout.config) {
                                log::info!("Entered resize mode - arrows resize, Space to move, Enter to confirm");
                            }
                        }
                    },
                    InputMode::Patch => {
                        // E in patch mode could open port selection (deferred)
                        log::info!("Port selection mode (deferred)");
                    },
                }
            },
            
            // === P - Patch mode ===
            Key::P => {
                match model.keyboard_nav.mode {
                    InputMode::Normal => {
                        model.keyboard_nav.enter_patch_mode();
                        log::info!("Entered patch mode");
                    },
                    InputMode::Patch => {
                        // Already in patch mode - could toggle or no-op
                    },
                    InputMode::Layout => {
                        // Exit layout, enter patch
                        model.keyboard_nav.exit_layout_mode();
                        // Layout editing mode handled by keyboard_nav
                        model.keyboard_nav.enter_patch_mode();
                        log::info!("Switched from layout to patch mode");
                    },
                }
            },
            
            // === SPACE - Move mode toggle (in Layout resize) ===
            Key::Space => {
                if model.keyboard_nav.mode == InputMode::Layout {
                    match &model.keyboard_nav.layout_state {
                        LayoutSubState::Resize { .. } => {
                            // Toggle to move mode
                            if model.keyboard_nav.enter_move_mode(&model.layout.config) {
                                log::info!("Entered move mode - arrows move, Space again to resize, Enter to confirm");
                            }
                        },
                        LayoutSubState::Move { .. } => {
                            // Toggle back to resize mode
                            if model.keyboard_nav.enter_resize_mode(&model.layout.config) {
                                log::info!("Returned to resize mode");
                            }
                        },
                        _ => {}
                    }
                }
            },
            
            // === ENTER - Confirm / Select ===
            Key::Return => {
                match model.keyboard_nav.mode {
                    InputMode::Normal | InputMode::Patch => {
                        // If no tile selected, try to select at cursor
                        if !model.keyboard_nav.has_selection() {
                            if let Some(tile_id) = model.keyboard_nav.select_tile_at_cursor(&model.layout.config) {
                                model.selected_tile = Some(tile_id.clone());
                                log::info!("Selected tile: {}", tile_id);
                            }
                        }
                    },
                    InputMode::Layout => {
                        match &model.keyboard_nav.layout_state {
                            LayoutSubState::Resize { .. } | LayoutSubState::Move { .. } => {
                                // Confirm resize/move
                                model.keyboard_nav.exit_resize_move_mode();
                                model.layout.save();
                                log::info!("Confirmed resize/move, layout saved");
                            },
                            LayoutSubState::Navigation => {
                                // Select tile at cursor
                                if let Some(tile_id) = model.keyboard_nav.select_tile_at_cursor(&model.layout.config) {
                                    model.selected_tile = Some(tile_id.clone());
                                    log::info!("Selected tile: {}", tile_id);
                                }
                            },
                        }
                    },
                }
            },
            
            // === ESCAPE - Cascading exit ===
            Key::Escape => {
                let result = model.keyboard_nav.handle_escape();
                
                match result {
                    EscapeResult::Deselected => {
                        model.selected_tile = None;
                        log::debug!("Deselected tile");
                    },
                    EscapeResult::ExitedSubMode => {
                        // Reverted resize/move, restore original bounds
                        if let Some(bounds) = model.keyboard_nav.get_original_bounds() {
                            if let Some(tile_id) = model.keyboard_nav.selected_tile_id() {
                                if let Some(tile) = model.layout.config.tiles.iter_mut().find(|t| &t.id == tile_id) {
                                    tile.col = bounds.0;
                                    tile.row = bounds.1;
                                    tile.colspan = Some(bounds.2);
                                    tile.rowspan = Some(bounds.3);
                                }
                            }
                        }
                        log::debug!("Exited resize/move mode");
                    },
                    EscapeResult::ExitedMode => {
                        // Layout editing mode handled by keyboard_nav
                        model.selected_tile = None;
                        log::debug!("Exited mode (layout/patch) back to normal");
                    },
                    EscapeResult::NoAction => {
                        // ESC at root with no selection - nothing to do
                        // (Ctrl+Q is the only keyboard exit)
                    },
                }
                
                // Sync layout_editor state
                // Edit mode tracked in keyboard_nav.mode
            },
            
            _ => {}
        }
    }
    
    // Sync selected_tile with keyboard_nav (for backward compatibility)
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

    // Draw Empty Cell Placeholders
    if model.maximized_tile.is_none() {
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


    // Iterate over all tiles and render (MONITOR MODE - Read-only feedback)
    for tile in &model.layout.config.tiles {
        if model.maximized_tile.as_ref() == Some(&tile.id) {
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
    if let Some(max_id) = &model.maximized_tile {
        if let Some(tile) = model.layout.config.tiles.iter().find(|t| &t.id == max_id) {
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

    
    
    // [Future: Interactive layout editing visualization will go here]
    
    // Render patch cables (always visible if not maximized)
    if model.maximized_tile.is_none() && !model.layout.config.patches.is_empty() {
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
