use nannou::prelude::*;
use talisman_core::{Source, Sink, Signal, PatchBay, PluginManager, PluginModuleAdapter, ModuleRuntime, Patch};
use aphrodite::AphroditeSource;
use logos::LogosSource;
use kamea::{self, SigilConfig};
use text_tools::{WordCountSink, DevowelizerSink};
use nannou_egui::{self, Egui, egui};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use std::thread;

use std::collections::VecDeque;
use std::path::PathBuf;
use audio_input::{AudioInputSource, AudioInputSourceRT};
use text_tools::{SaveFileSink, OutputFormat};
use talisman_core::ring_buffer;

// Layout editor and visualizer modules
mod layout_editor;
mod patch_visualizer;
mod module_picker;
mod modal;
mod signal_handler;
mod layout;
mod tiles;
mod input;
mod theme;


use layout_editor::LayoutEditor;
use module_picker::ModulePicker;
use modal::ModalLayer;
use layout::Layout;
use tiles::{TileRegistry, RenderContext, GpuRenderer};
use input::KeyboardNav;




// --- MODEL ---
struct Model {
    // We use a non-blocking channel for the UI thread to receive updates
    receiver: std::sync::mpsc::Receiver<Signal>,
    router_tx: mpsc::Sender<Signal>, 
    
    // UI State
    egui: Egui,
    text_buffer: String,
    
    // Vis State
    current_intent: String,
    path_points: Vec<Point2>,
    astro_data: String,
    retinal_burn: bool,
    // Sink Results
    word_count: String,
    devowel_text: String,
    config: SigilConfig,
    
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
    // disabled_tiles removed
    show_patch_bay: bool,
    
    // Settings & Controls
    show_global_settings: bool,
    show_tile_settings: Option<String>,  // tile_id if showing settings for that tile
    show_layout_manager: bool,
    show_close_confirmation: bool,
    is_sleeping: bool,

    
    // Runtime State
    module_host: talisman_core::ModuleHost,
    plugin_manager: talisman_core::PluginManager,
    
    // Audio State
    audio_buffer: VecDeque<f32>, // Circular buffer for oscilloscope (legacy)
    /// Real-time audio ring buffer receiver (for new tile system)
    audio_stream_rx: Option<ring_buffer::RingBufferReceiver<talisman_core::AudioFrame>>,
    
    // Layout Editor State (Phase 5)
    layout_editor: LayoutEditor,
    module_picker: ModulePicker,
    
    // Modal Layer (centralized modal management)
    modal_layer: ModalLayer,
    
    // Tile System (Phase 6: Settings Architecture)
    tile_registry: TileRegistry,
    gpu_renderer: GpuRenderer,
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

const CLI_GREEN: &str = "\x1b[32m";
const CLI_RESET: &str = "\x1b[0m";

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
    let (tx_router, mut rx_router) = mpsc::channel::<Signal>(1000);
    
    // Clone for different uses
    let tx_ui_clone = tx_ui.clone();
    
    // 2. Create ModuleHost for isolated module execution
    let mut module_host = talisman_core::ModuleHost::new(tx_router.clone());
    
    // 3. Register and spawn modules
    log::info!("Spawning modules with isolated threads...");
    
    // Sources
    use talisman_core::SourceAdapter;
    let aphrodite = SourceAdapter::new(AphroditeSource::new(10));
    if let Err(e) = module_host.spawn(aphrodite, 100) {
        log::error!("Failed to spawn Aphrodite: {}", e);
    }
    
    let logos = SourceAdapter::new(LogosSource::new());
    if let Err(e) = module_host.spawn(logos, 100) {
        log::error!("Failed to spawn Logos: {}", e);
    }
    
    // Sinks
    use talisman_core::SinkAdapter;
    let word_count = SinkAdapter::new(WordCountSink::new(Some(tx_ui.clone())));
    if let Err(e) = module_host.spawn(word_count, 100) {
        log::error!("Failed to spawn WordCount: {}", e);
    }
    
    let devowelizer = SinkAdapter::new(DevowelizerSink::new(Some(tx_ui.clone())));
    if let Err(e) = module_host.spawn(devowelizer, 100) {
        log::error!("Failed to spawn Devowelizer: {}", e);
    }
    
    let wav_sink = SaveFileSink::new(PathBuf::from("recording.wav"));
    wav_sink.set_format(OutputFormat::Wav);
    let wav_adapter = SinkAdapter::new(wav_sink);
    if let Err(e) = module_host.spawn(wav_adapter, 100) {
        log::error!("Failed to spawn WAV Sink: {}", e);
    }
    
    // 4. Spawn Router Thread (signal fan-out to modules)
    let module_handles: Vec<_> = module_host.list_modules()
        .iter()
        .filter_map(|id| module_host.get_module(id).map(|h| ((*id).to_string(), h.inbox.clone())))
        .collect();
    
    thread::spawn(move || {
        let rt = Runtime::new().expect("Tokio runtime");
        rt.block_on(async move {
            println!("{}TALISMAN MODULE ROUTER ONLINE{}", CLI_GREEN, CLI_RESET);
            
            while let Some(signal) = rx_router.recv().await {
                // Send to UI (non-blocking)
                let _ = tx_ui_clone.send(signal.clone());
                
                // Fan out to all module inboxes in parallel (non-blocking)
                for (module_id, inbox) in &module_handles {
                    if let Err(e) = inbox.try_send(signal.clone()) {
                        log::debug!("Module {} inbox full or closed: {}", module_id, e);
                    }
                }
            }
            log::warn!("Router channel closed, shutting down...");
        });
    });

    // 4. Init Window ID & Egui
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

    // 5. Init State
    let config = SigilConfig {
        spacing: 50.0,
        stroke_weight: 4.0,
        grid_rows: 4,
        grid_cols: 4,
    };

    // 6. Init Clipboard (might fail on some systems)
    let clipboard = match arboard::Clipboard::new() {
        Ok(cb) => Some(cb),
        Err(e) => {
            log::warn!("Failed to init Clipboard: {}", e);
            None
        }
    };

    // 7. Init Patch Bay and register module schemas
    let mut patch_bay = PatchBay::new();
    
    // Load layout config early to access saved patches
    let layout = Layout::new(app.window_rect());

    // Register all module schemas
    let logos_source = LogosSource::new();
    let aphrodite_source = AphroditeSource::new(10);
    let word_count_sink = WordCountSink::new(None);
    let devowelizer_sink = DevowelizerSink::new(None);
    let kamea_sink = kamea::KameaSink::new();
    
    patch_bay.register_module(logos_source.schema());
    patch_bay.register_module(aphrodite_source.schema());
    patch_bay.register_module(word_count_sink.schema());
    patch_bay.register_module(devowelizer_sink.schema());
    patch_bay.register_module(kamea_sink.schema());
    
    // Init Audio Input (spawn it with ModuleHost would be better, but need to refactor first)
    let tx_router_for_audio = tx_router.clone();
    match AudioInputSource::new(1024) {
        Ok(src) => {
            patch_bay.register_module(src.schema());
             // Start source thread
             let mut src_clone = src; // Move semantics
             thread::spawn(move || {
                 let runtime = Runtime::new().unwrap();
                 runtime.block_on(async {
                     loop {
                         if let Some(signal) = src_clone.poll().await {
                             let _ = tx_router_for_audio.send(signal).await;
                         }
                     }
                 });
             });
        },
        Err(e) => log::error!("Failed to init AudioInputSource: {}", e),
    }
    
    // Init Real-Time Audio Input (SPSC ring buffer for new tile system)
    // This provides minimal latency audio streaming (~5-10ns per frame)
    let audio_stream_rx = match AudioInputSourceRT::new(4096) {
        Ok((_source, rx)) => {
            // Source is kept alive by thread ownership
            // We get the ring buffer receiver directly
            log::info!("AudioInputSourceRT initialized with ring buffer");
            
            // The source runs in the audio thread callback
            // We keep the receiver for the tile system
            // Note: source is moved here, stream will stop when it drops
            // We need to keep it alive - store it or leak it
            std::mem::forget(_source); // Keep audio stream alive
            
            Some(rx)
        },
        Err(e) => {
            log::warn!("Failed to init AudioInputSourceRT: {} - using legacy audio", e);
            None
        }
    };

    // Init SaveFileSink (WAV/Text/Image)
    let save_file_sink = SaveFileSink::default();
    patch_bay.register_module(save_file_sink.schema());
    // Note: Sinks usually need a runner thread or be polled. 
    // The daemon orchestrator handles sink dispatch. 
    // We need to add SaveFileSink to the dispatcher.
    // Wait, the orchestrator/dispatcher logic in main.rs handles this. 
    // I need to see how sinks are stored. They seem to be dropped after registration?
    // Ah, `patch_bay` stores schemas, but the *instances* need to be managed by the orchestrator.
    // The current daemon implementation seems to have dedicated threads/channels for sources, 
    // but sinks seem to be handled... how?
    // Looking at `logos` and `kamea`, they are just registered? 
    // Wait, if I look at `main.rs`, I don't see where sink instances are kept.
    // Ah, line 290: `let word_count_sink = WordCountSink::new(None);`
    // If I don't move them into a runner, they die.
    // I suspect the Previous Implementation might have abstracted this or missed it?
    // Let me check `orchestrator` loop. It probably sends signals to *named* sinks.
    // But how does it invoke the sink instance?
    // Most likely, the instances are just for schema registration in this demo, 
    // OR there is a map of Sinks somewhere?
    // The `Model` struct doesn't have a `sinks` map.
    // Let me check `main.rs` lines 430+ (update loop) to see how sinks are invoked.    

    
    // Register editor as a virtual source (it emits text from UI)
    use talisman_core::{ModuleSchema, Port, DataType, PortDirection};
    let editor_schema = ModuleSchema {
        id: "editor".to_string(),
        name: "Text Editor".to_string(),
        description: "GUI text editor for intent input".to_string(),
        ports: vec![
            Port {
                id: "text_out".to_string(),
                label: "Text Output".to_string(),
                data_type: DataType::Text,
                direction: PortDirection::Output,
            },
        ],
        settings_schema: None,
    };
    patch_bay.register_module(editor_schema);
    
    // Register astrology display as a sink
    let astro_display_schema = ModuleSchema {
        id: "astrology_display".to_string(),
        name: "Astrology Display".to_string(),
        description: "Dashboard view of celestial data".to_string(),
        ports: vec![
            Port {
                id: "astro_in".to_string(),
                label: "Astrology Input".to_string(),
                data_type: DataType::Astrology,
                direction: PortDirection::Input,
            },
        ],
        settings_schema: None,
    };
    patch_bay.register_module(astro_display_schema);
    
    // Establish default patches ONLY if no patches loaded from config
    if layout.config.patches.is_empty() {
        log::info!("No patches found in config, applying factory defaults.");
        // Editor → WordCount
        if let Err(e) = patch_bay.connect("editor", "text_out", "word_count", "text_in") {
            log::warn!("Failed to connect editor→word_count: {}", e);
        }
        // Editor → Devowelizer
        if let Err(e) = patch_bay.connect("editor", "text_out", "devowelizer", "text_in") {
            log::warn!("Failed to connect editor→devowelizer: {}", e);
        }
        // Editor → Kamea Sigil
        if let Err(e) = patch_bay.connect("editor", "text_out", "kamea_printer", "text_in") {
            log::warn!("Failed to connect editor→kamea: {}", e);
        }
        // Aphrodite → Kamea (astrology input)
        if let Err(e) = patch_bay.connect("aphrodite", "astro_out", "kamea_printer", "astro_in") {
            log::warn!("Failed to connect aphrodite→kamea: {}", e);
        }
    }
    // Aphrodite → Astrology Display
    if let Err(e) = patch_bay.connect("aphrodite", "astro_out", "astrology_display", "astro_in") {
        log::warn!("Failed to connect aphrodite→astrology_display: {}", e);
    }
    
    log::info!("Patch Bay initialized with {} modules, {} patches", 
        patch_bay.get_modules().len(), 
        patch_bay.get_patches().len());
    
    // Apply patches from layout config
    for patch in &layout.config.patches {
        if let Err(e) = patch_bay.connect(
            &patch.source_module, 
            &patch.source_port, 
            &patch.sink_module, 
            &patch.sink_port
        ) {
            log::warn!("Failed to apply patch from config: {}", e);
        }
    }

    // Sync initial enabled/disabled state from layout tiles
    for tile in &layout.config.tiles {
        if !tile.enabled {
            let module_id = tile_to_module(&tile.id);
            patch_bay.disable_module(&module_id);
            log::info!("Disabled module '{}' based on layout config", module_id);
        }
    }


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
    }

    let mut model = Model {
        receiver: rx_ui,
        router_tx: tx_router,
        egui,
        text_buffer: String::new(),
        current_intent: "AWAITING SIGNAL".to_string(),
        path_points: vec![],
        astro_data: "NO DATA".to_string(),
        retinal_burn: false,
        word_count: "0".to_string(),
        devowel_text: "".to_string(),
        config,
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
        show_close_confirmation: false,
        is_sleeping: initial_sleep_state,

        audio_buffer: VecDeque::with_capacity(2048),
        audio_stream_rx,
        module_host,
        plugin_manager,
        layout_editor: LayoutEditor::new(),
        module_picker: ModulePicker::new(),
        modal_layer: ModalLayer::new(),
        tile_registry: tiles::create_default_registry(),
        gpu_renderer: GpuRenderer::new(app),
        start_time: std::time::Instant::now(),
        frame_count: 0,
        keyboard_nav: KeyboardNav::new(),
    };
    
    // Apply saved tile settings from layout config
    apply_tile_settings(&model.tile_registry, &model.layout);
    
    // Connect audio stream to AudioVisTile if available
    if let Some(rx) = model.audio_stream_rx.take() {
        // Get the audio_vis tile from registry and connect the stream
        if let Some(tile) = model.tile_registry.get("audio_vis") {
            if let Ok(mut t) = tile.write() {
                // Downcast to AudioVisTile - this is tricky with trait objects
                // For now, we'll use a different approach - store the receiver in the model
                // and poll it in update, pushing to the tile's legacy buffer
                log::info!("AudioVisTile found - audio stream ready (using polling bridge)");
            }
        }
        // Put the receiver back
        model.audio_stream_rx = Some(rx);
    }
    
    model
}


/// Map tile ID to module ID for PatchBay
fn tile_to_module(tile_id: &str) -> String {
    match tile_id {
        "editor_pane" => "editor".to_string(),
        "wc_pane" => "word_count".to_string(),
        "dvwl_pane" => "devowelizer".to_string(),
        "astro_pane" => "astrology_display".to_string(),
        "sigil_pane" | "kamea_sigil" => "kamea_printer".to_string(),
        _ => tile_id.to_string(), // fallback: use tile_id as module_id
    }
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
    if model.show_close_confirmation {
        log::debug!("UPDATE: show_close_confirmation is TRUE");
    }
    
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
    
    // Pump audio from ring buffer to legacy audio_buffer for visualization
    // The new tile system polls this in update(), but we also feed the legacy buffer
    // for backward compatibility with render_tile()
    if let Some(ref rx) = model.audio_stream_rx {
        while let Some(frame) = rx.try_recv() {
            // Add to circular buffer (mono mix)
            let sample = (frame.left + frame.right) * 0.5;
            if model.audio_buffer.len() >= 2048 {
                model.audio_buffer.pop_front();
            }
            model.audio_buffer.push_back(sample);
        }
    }

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

    // 1. UPDATE GUI
    if model.show_close_confirmation {
        log::debug!("UPDATE: About to set_elapsed_time and begin_frame");
    }
    model.egui.set_elapsed_time(update.since_start);
    if model.show_close_confirmation {
        log::debug!("UPDATE: About to call begin_frame");
    }
    let ctx = model.egui.begin_frame();
    if model.show_close_confirmation {
        log::debug!("UPDATE: begin_frame completed");
    }
    
    // Position Egui Window via Layout
    // Position Egui Window via Layout
    // Find the tile assigned to 'editor' module
    let editor_tile = model.layout.config.tiles.iter().find(|t| t.module == "editor");
    
    if let Some(tile) = editor_tile {
        if let Some(rect) = model.layout.calculate_rect(tile) {
            // Custom Style for "Terminal-like" look
            let mut style = (*ctx.style()).clone();
            style.visuals.widgets.noninteractive.bg_fill = egui::Color32::TRANSPARENT; // Transparent Window BG
            style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 255)); 
            style.visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT; // Transparent Input BG
            style.visuals.selection.bg_fill = egui::Color32::from_rgb(0, 100, 100);
            style.visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 255, 255));
            ctx.set_style(style);

            // Only show editor if no other tile is maximized
            let block_egui = model.maximized_tile.is_some() && model.maximized_tile.as_ref() != Some(&tile.id);
            
            if !block_egui {
                egui::Window::new("source_editor")
                    .default_pos(egui::pos2(rect.left() + 10.0, model.layout.window_rect.top() - rect.top() + 10.0))
                    .fixed_pos(egui::pos2(
                        rect.left() + app.window_rect().w()/2.0, 
                        app.window_rect().h()/2.0 - rect.top()
                    ))
                    .fixed_size(egui::vec2(rect.w(), rect.h()))
                    .title_bar(false) 
                    .frame(egui::Frame {
                        fill: egui::Color32::TRANSPARENT, // Fully Transparent Frame
                        inner_margin: egui::Margin::same(10.0),
                        ..Default::default()
                    })
                    .show(&ctx, |ui| {
                        let response = ui.add(
                            egui::TextEdit::multiline(&mut model.text_buffer)
                                .desired_width(ui.available_width())
                                .desired_rows(20)
                                .frame(false) 
                                .text_color(egui::Color32::from_rgb(0, 255, 255))
                                .font(egui::FontId::monospace(14.0)) 
                        );
                        
                        if response.changed() {
                            let signal = Signal::Text(model.text_buffer.clone());
                            let _ = model.router_tx.try_send(signal);
                        }
                    });
            }
        }
    }

    // Kamea Buttons
    let kamea_tile = model.layout.config.tiles.iter().find(|t| t.module == "kamea_sigil");
    if let Some(tile) = kamea_tile {
         let is_max = model.maximized_tile.as_ref() == Some(&tile.id);
         let something_else_max = model.maximized_tile.is_some() && !is_max;

         // Hide buttons if something else is maximized, or if we are animating
         if !something_else_max && (model.maximized_tile.is_none() || model.anim_factor > 0.9) {
             if let Some(grid_rect) = model.layout.calculate_rect(tile) {
                 let rect = if is_max { app.window_rect() } else { grid_rect };
                 
                 let egui_params = egui::pos2(
                      rect.left() + app.window_rect().w()/2.0, 
                      app.window_rect().h()/2.0 - rect.top()
                 );
                 
                 egui::Area::new("kamea_buttons")
                    .fixed_pos(egui::pos2(egui_params.x + 10.0, egui_params.y + rect.h() - 40.0))
                    .show(&ctx, |ui| {
                         ui.horizontal(|ui| {
                             ui.style_mut().visuals.widgets.inactive.bg_fill = egui::Color32::BLACK;
                             ui.style_mut().visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 50, 50);
                             
                             if ui.button("BURN").clicked() {
                                 model.retinal_burn = !model.retinal_burn;
                             }
                             if ui.button("PATCH").clicked() {
                                 model.show_patch_bay = !model.show_patch_bay;
                             }
                             if ui.button("DESTROY").clicked() {
                                 std::process::exit(0);
                             }
                         });
                    });
             }
         }
    }

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
                     // logic duplicated from key_pressed for now
                     let content = if tile_id == "wc_pane" {
                         Some(model.word_count.clone())
                    } else if tile_id == "dvwl_pane" {
                         Some(model.devowel_text.clone())
                    } else if tile_id == "astro_pane" {
                         Some(model.astro_data.clone())
                    } else if tile_id == "editor_pane" {
                         Some(model.text_buffer.clone())
                    } else {
                         None
                    };
                    
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
                     if tile_id == "editor_pane" {
                         if let Some(cb) = &mut model.clipboard {
                             if let Ok(text) = cb.get_text() {
                                  model.text_buffer.push_str(&text);
                                  let _ = model.router_tx.try_send(Signal::Text(model.text_buffer.clone()));
                             }
                        }
                     }
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
                ui.checkbox(&mut model.retinal_burn, "Retinal Burn Mode (Inverted Colors)");
                
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

    // Close Confirmation Dialog (ESC outside edit mode)
    // NOTE: ESC detection moved to egui input to avoid nannou/egui frame collision
    if model.show_close_confirmation {
        log::debug!("Rendering close confirmation dialog...");
        let screen_rect = ctx.screen_rect();
        let width = 300.0;
        let height = 120.0;
        let x = screen_rect.center().x - width / 2.0;
        let y = screen_rect.center().y - height / 2.0;
        
        log::debug!("Dialog position: ({}, {}), size: {}x{}", x, y, width, height);

        egui::Window::new("Confirm Close")
            .fixed_pos(egui::pos2(x, y))
            .fixed_size(egui::vec2(width, height))
            .collapsible(false)
            .resizable(false)
            .frame(egui::Frame {
                fill: egui::Color32::from_rgba_unmultiplied(20, 20, 20, 250),
                stroke: egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 100, 100)),
                inner_margin: egui::Margin::same(20.0),
                ..Default::default()
            })
            .show(&ctx, |ui| {
                log::debug!("Inside dialog UI closure");
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Exit Talisman?")
                        .heading()
                        .color(egui::Color32::from_rgb(255, 200, 200)));
                    
                    ui.add_space(15.0);
                    
                    ui.horizontal(|ui| {
                        ui.add_space(30.0);
                        if ui.button(egui::RichText::new("  Quit  ").color(egui::Color32::from_rgb(255, 100, 100))).clicked() {
                            log::info!("User confirmed quit");
                            std::process::exit(0);
                        }
                        ui.add_space(20.0);
                        if ui.button("Cancel").clicked() {
                            model.show_close_confirmation = false;
                        }
                    });
                });
            });
        
        log::debug!("Dialog window created");
        
        // Handle Enter to confirm quit
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            log::info!("User confirmed quit via Enter");
            std::process::exit(0);
        }
        
        // Handle ESC to cancel dialog
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            log::info!("Close confirmation cancelled via ESC");
            model.show_close_confirmation = false;
        }
    } else {
        // ESC triggers close confirmation when:
        // - Not in edit mode
        // - Module picker is not visible
        // - No other modal is active
        let esc_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if esc_pressed 
            && !model.layout_editor.edit_mode 
            && !model.module_picker.visible 
            && model.context_menu.is_none()
        {
            log::info!("ESC pressed - showing close confirmation");
            model.show_close_confirmation = true;
        }
    }



    // 2. PROCESS SIGNALS from Orchestrator (using signal_handler module)
    signal_handler::process_signals(
        &model.receiver,
        &mut model.current_intent,
        &mut model.word_count,
        &mut model.devowel_text,
        &mut model.astro_data,
        &mut model.path_points,
        &mut model.config,
        &model.layout.config,
        &mut model.audio_buffer,
        |tile| model.layout.calculate_rect(tile),
    );
}

fn mouse_pressed(app: &App, model: &mut Model, button: MouseButton) {
    // 0. Intercept clicks for Egui
    if model.egui.ctx().wants_pointer_input() {
        return;
    }
    
    // Clear context menu if clicking away (and egui didn't want it)
    model.context_menu = None;

    // Phase 5: Edit mode mouse handling - CLICK-BASED (no drag)
    if model.layout_editor.edit_mode && button == MouseButton::Left {
        let mouse_pos = app.mouse.position();
        let (col_tracks, row_tracks) = model.layout.config.generate_tracks();
        let col_sizes = model.layout.resolve_tracks(&col_tracks, app.window_rect().w());
        let row_sizes = model.layout.resolve_tracks(&row_tracks, app.window_rect().h());
        
        // Get the grid cell under the mouse
        if let Some((col, row)) = model.layout_editor.get_grid_cell(
            mouse_pos,
            app.window_rect(),
            &col_sizes,
            &row_sizes
        ) {
            // Update cursor position to clicked cell
            model.layout_editor.cursor_cell = (col, row);
            
            use layout_editor::EditState;
            match &model.layout_editor.edit_state {
                EditState::Navigation => {
                    // Click on tile = select it
                    if let Some(tile) = layout_editor::LayoutEditor::get_tile_at_cell(&model.layout.config, col, row) {
                        model.layout_editor.select_tile(tile.id.clone());
                        log::info!("Selected tile: {}", tile.id);
                    }
                    // Empty cell click - could open module picker
                },
                EditState::TileSelected { .. } => {
                    // Click elsewhere deselects, or click on another tile selects it
                    if let Some(tile) = layout_editor::LayoutEditor::get_tile_at_cell(&model.layout.config, col, row) {
                        model.layout_editor.select_tile(tile.id.clone());
                        log::info!("Selected tile: {}", tile.id);
                    } else {
                        model.layout_editor.deselect();
                    }
                },
                EditState::MoveResize { tile_id, .. } => {
                    // Handle move/resize clicks
                    let tile_id = tile_id.clone();
                    if model.layout_editor.handle_move_resize_click((col, row), &mut model.layout.config) {
                        log::info!("Move/resize completed for tile: {}", tile_id);
                    }
                },

                EditState::Patching { tile_id, role } => {
                    // Complete patch if clicking on another tile
                    use layout_editor::PatchRole;
                    if *role != PatchRole::SelectingRole {
                        if let Some(target_tile) = layout_editor::LayoutEditor::get_tile_at_cell(&model.layout.config, col, row) {
                            if target_tile.id != *tile_id {
                                // Complete patch
                                let tile_to_module = |tid: &str| -> Option<String> {
                                    model.layout.config.tiles.iter()
                                        .find(|t| t.id == tid)
                                        .map(|t| t.module.clone())
                                };
                                if let Some(_patch) = model.layout_editor.complete_patch(
                                    &target_tile.id,
                                    &model.layout.config,
                                    tile_to_module
                                ) {
                                    log::info!("Patch created (ports to be determined)");
                                }
                            }
                        }
                    }
                },
            }
            return; // Handled edit mode click
        }
    }

    

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

fn mouse_moved(app: &App, model: &mut Model, pos: Point2) {
    if !model.layout_editor.edit_mode {
        return;
    }
    
    // Update hover cell for visual feedback
    let (col_tracks, row_tracks) = model.layout.config.generate_tracks();
    let col_sizes = model.layout.resolve_tracks(&col_tracks, app.window_rect().w());
    let row_sizes = model.layout.resolve_tracks(&row_tracks, app.window_rect().h());
    
    model.layout_editor.hover_cell = model.layout_editor.get_grid_cell(
        pos,
        app.window_rect(),
        &col_sizes,
        &row_sizes
    );
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
    // 3. Close confirmation is showing
    if model.egui.ctx().wants_keyboard_input() 
        || model.modal_layer.is_active() 
        || model.show_close_confirmation 
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
        // Ctrl key combinations (clipboard, etc.)
        match key {
            Key::C => {
                // COPY logic
                if let Some(selected) = model.keyboard_nav.selected_tile_id() {
                    let content = match selected {
                        "wc_pane" => Some(model.word_count.clone()),
                        "dvwl_pane" => Some(model.devowel_text.clone()),
                        "astro_pane" => Some(model.astro_data.clone()),
                        "editor_pane" => Some(model.text_buffer.clone()),
                        _ => None,
                    };
                    
                    if let Some(text) = content {
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
                if let Some(selected) = model.keyboard_nav.selected_tile_id() {
                    if selected == "editor_pane" {
                        if let Some(cb) = &mut model.clipboard {
                            match cb.get_text() {
                                Ok(text) => {
                                    model.text_buffer.push_str(&text);
                                    let _ = model.router_tx.try_send(Signal::Text(model.text_buffer.clone()));
                                },
                                Err(e) => log::error!("Clipboard Paste Failed: {}", e)
                            }
                        }
                    }
                }
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
                        // Navigate and auto-select tile at cursor
                        model.keyboard_nav.navigate(direction);
                        if let Some(tile_id) = model.keyboard_nav.select_tile_at_cursor(&model.layout.config) {
                            model.selected_tile = Some(tile_id.clone());
                            log::debug!("Selected tile: {}", tile_id);
                        } else {
                            model.selected_tile = None;
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
                                
                                if let Some((col, row, colspan, rowspan)) = tile_info {
                                    let new_colspan = (colspan as i32 + delta_col).max(1) as usize;
                                    let new_rowspan = (rowspan as i32 + delta_row).max(1) as usize;
                                    
                                    // Validate placement
                                    if model.layout_editor.is_placement_valid(
                                        &model.layout.config, &tile_id, col, row, new_colspan, new_rowspan
                                    ) {
                                        // Now mutate
                                        if let Some(tile) = model.layout.config.tiles.iter_mut().find(|t| t.id == tile_id) {
                                            tile.colspan = Some(new_colspan);
                                            tile.rowspan = Some(new_rowspan);
                                            model.layout_editor.pending_changes = true;
                                            log::debug!("Resized tile to {}x{}", new_colspan, new_rowspan);
                                        }
                                    }
                                }
                            },
                            LayoutSubState::Move { tile_id, .. } => {
                                // Arrow keys move the tile (as 1×1)
                                let tile_id = tile_id.clone();
                                model.keyboard_nav.navigate(direction);
                                let (new_col, new_row) = model.keyboard_nav.cursor;
                                
                                if model.layout_editor.is_placement_valid(
                                    &model.layout.config, &tile_id, new_col, new_row, 1, 1
                                ) {
                                    if let Some(tile) = model.layout.config.tiles.iter_mut().find(|t| t.id == tile_id) {
                                        tile.col = new_col;
                                        tile.row = new_row;
                                        tile.colspan = Some(1);
                                        tile.rowspan = Some(1);
                                        model.layout_editor.pending_changes = true;
                                        log::debug!("Moved tile to ({}, {})", new_col, new_row);
                                    }
                                }
                            },
                            LayoutSubState::Navigation => {
                                // Standard cursor navigation in layout mode
                                model.keyboard_nav.navigate(direction);
                                model.layout_editor.cursor_cell = model.keyboard_nav.cursor;
                                
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
                            model.layout_editor.edit_mode = true;
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
                        model.layout_editor.edit_mode = false;
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
                        model.layout_editor.edit_mode = false;
                        model.selected_tile = None;
                        log::debug!("Exited mode (layout/patch) back to normal");
                    },
                    EscapeResult::ExitRequested => {
                        // Deferred: exit confirmation dialog
                        // For now, just log
                        log::debug!("Exit requested (confirmation deferred)");
                    },
                }
                
                // Sync layout_editor state
                model.layout_editor.edit_mode = model.keyboard_nav.mode == InputMode::Layout;
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
    // Retinal Burn Mode: Invert Colors
    let (bg_color, fg_color, stroke_color) = if model.retinal_burn {
        (CYAN, BLACK, BLACK)
    } else {
        (BLACK, CYAN, GRAY)
    };
    
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
                    gpu: Some(&model.gpu_renderer),
                };
                
                model.tile_registry.render_monitor(&tile.module, &draw, rect.pad(5.0), &ctx);
                
                // Render error overlay if tile has an error
                if let Some(error) = model.tile_registry.get_error(&tile.module) {
                    tiles::render_error_overlay(&draw, rect, &error);
                }
            } else {
                // Fallback to legacy render_tile
                render_tile(draw.clone(), tile, rect, model, bc, fg_color, false);
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
                    egui_ctx: None, // Will be handled separately
                    tile_settings: Some(&tile.settings.config),
                    gpu: Some(&model.gpu_renderer),
                };
                
                // Try tile registry first
                if model.tile_registry.get(&tile.module).is_some() {
                    model.tile_registry.render_controls(&tile.module, &draw, rect, &ctx);
                } else {
                    // Fallback to legacy render_tile
                    render_tile(draw.clone(), tile, rect, model, CYAN.into_format().into_linear().into(), fg_color, true);
                }
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

    
    // Render layout editor overlay (Phase 5) - KEYBOARD-DRIVEN UI
    if model.layout_editor.edit_mode {
        let (col_tracks, row_tracks) = model.layout.config.generate_tracks();
        let col_sizes = model.layout.resolve_tracks(&col_tracks, app.window_rect().w());
        let row_sizes = model.layout.resolve_tracks(&row_tracks, app.window_rect().h());
        
        // Render grid overlay
        layout_editor::render_edit_overlay(&draw, app.window_rect(), &col_sizes, &row_sizes);
        
        // Render cell indicators (cursor, validity, selection)
        layout_editor::render_cell_indicators(
            &draw,
            app.window_rect(),
            &col_sizes,
            &row_sizes,
            &model.layout.config,
            &model.layout_editor,
        );
        
        // Render tile labels with module names
        layout_editor::render_tile_labels(
            &draw,
            app.window_rect(),
            &model.layout.config,
            &col_sizes,
            &row_sizes,
            model.layout_editor.selected_tile_id(),
        );
        
        // Render patch cables in edit mode
        layout_editor::render_patch_cables(
            &draw,
            app.window_rect(),
            &model.layout.config.patches,
            &model.layout.config,
            &col_sizes,
            &row_sizes,
            |tile_id| tile_to_module(tile_id),
        );
        
        // Show edit mode indicator
        draw.text("EDIT MODE")
            .xy(pt2(app.window_rect().left() + 60.0, app.window_rect().top() - 15.0))
            .color(rgba(0.0, 1.0, 1.0, 0.8))
            .font_size(12);
        
        // Show current state
        let state_text = match &model.layout_editor.edit_state {
            layout_editor::EditState::Navigation => "NAV",
            layout_editor::EditState::TileSelected { .. } => "SEL",
            layout_editor::EditState::MoveResize { .. } => "MOVE",
            layout_editor::EditState::Patching { .. } => "PATCH",
        };

        draw.text(state_text)
            .xy(pt2(app.window_rect().left() + 130.0, app.window_rect().top() - 15.0))
            .color(rgba(1.0, 0.8, 0.0, 0.8))
            .font_size(12);
    }
    
    // Render patch cables (always visible if not maximized and not in edit mode)
    if model.maximized_tile.is_none() && !model.layout.config.patches.is_empty() && !model.layout_editor.edit_mode {
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

// Helper to render tile content
fn render_tile(draw: Draw, tile: &TileConfig, rect: Rect, model: &Model, border_color: LinSrgba, fg_color: Srgb<u8>, drawing_maximized: bool) {
    let is_selected = model.selected_tile.as_ref() == Some(&tile.id);
    let is_disabled = !tile.enabled;
    
    // Visualize Borders - use dim red for disabled tiles
    let final_border = if is_disabled {
        LinSrgba::new(0.5, 0.2, 0.2, 0.8)
    } else if drawing_maximized { 
        CYAN.into_format().into_linear().into() 
    } else { 
        border_color 
    };
    
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .color(rgba(0.0, 0.0, 0.0, 0.0)) // Transparent BG
        .stroke(final_border) 
        .stroke_weight(if is_disabled { 5.0 } else if is_selected || drawing_maximized { 2.0 } else { 1.0 });
    
    // Show disabled indicator
    if is_disabled && !drawing_maximized {
        draw.text("[DISABLED]")
            .xy(pt2(rect.x(), rect.top() - 12.0))
            .color(Srgb::new(150u8, 50, 50))
            .font_size(10);
        
        // Dim overlay
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(rgba(0.0, 0.0, 0.0, 0.5));
    }
    
    if is_selected && !drawing_maximized && !is_disabled {
        draw.text("[CLIPBOARD ACTIVE]")
            .xy(pt2(rect.left() + 60.0, rect.bottom() + 10.0))
            .color(CYAN)
            .font_size(10);
    }
    
    // Skip content rendering for disabled tiles
    if is_disabled {
        return;
    }
    
    // Content Padding
    let content_rect = rect.pad(10.0);

    
    match tile.module.as_str() {
        "header" => {},
        "kamea_sigil" => {
             if !model.path_points.is_empty() {
                 let offset = content_rect.xy();
                 let points: Vec<Point2> = model.path_points.iter()
                    .map(|p| *p + offset)
                    .collect();
                draw.polyline()
                    .weight(model.config.stroke_weight)
                    .join_round()
                    .caps_round()
                    .points(points)
                    .color(fg_color);
             }
             // Label: Sanitize newlines
             let sanitized_intent = model.current_intent.replace('\n', " ").replace('\r', "");
             let truncated_intent = if sanitized_intent.len() > 50 && !drawing_maximized {
                 format!("{}...", &sanitized_intent[..50])
             } else {
                 sanitized_intent
             };
             draw.text(&truncated_intent)
                .xy(pt2(content_rect.x(), content_rect.top() - 10.0))
                .color(fg_color)
                .font_size(if drawing_maximized { 24 } else { 14 });
        },
        "word_count" => {
            let text = format!("WORD COUNT: {}", model.word_count);
            draw.text(&text)
                .xy(content_rect.xy())
                .color(YELLOW)
                .font_size(if drawing_maximized { 48 } else { 12 });
                
            if model.word_count.len() > 100 {
                 draw.rect().x(rect.right() - 5.0).y(rect.y()).w(4.0).h(rect.h() * 0.3).color(rgba(1.0, 1.0, 1.0, 0.3));
            }
        },
        "devowelizer" => {
            let raw = &model.devowel_text;
            let clean = raw.replace('\n', " ").replace('\r', "");
            let clean_text = clean.split_whitespace().collect::<Vec<&str>>().join(" ");
            
            let _text = format!("DEVOWELIZER: {}", clean_text);
            let display_text = if clean_text.len() > 200 && !drawing_maximized {
                 format!("{}...", &clean_text[..200])
            } else {
                 clean_text
            };
            
            draw.text(&format!("DVWL: {}", display_text))
                .xy(content_rect.xy())
                .color(MAGENTA)
                .font_size(if drawing_maximized { 32 } else { 12 })
                .w(content_rect.w());
                
            if raw.len() > 50 {
                 draw.rect().x(rect.right() - 5.0).y(rect.y()).w(4.0).h(rect.h() * 0.5).color(rgba(1.0, 0.0, 1.0, 0.5));
            }
        },
        "astrology" => {
            // Parse astro_data into components
            let parts: Vec<&str> = model.astro_data.split('|').collect();
            
            // Dashboard header
            draw.text("CELESTIAL STATUS")
                .xy(pt2(content_rect.x(), content_rect.top() - 15.0))
                .color(YELLOW)
                .font_size(if drawing_maximized { 18 } else { 11 });
            
            // Big three (Sun, Moon, Rising)
            if parts.len() >= 3 {
                let line_height = if drawing_maximized { 28.0 } else { 16.0 };
                let font_size = if drawing_maximized { 16 } else { 10 };
                let start_y = content_rect.y() + 10.0;
                
                // Sun
                draw.text(&format!("SUN {}", parts.get(0).unwrap_or(&"--")))
                    .xy(pt2(content_rect.x(), start_y))
                    .color(Srgb::new(255u8, 200, 50)) // Golden yellow
                    .font_size(font_size);
                
                // Moon
                draw.text(&format!("MOON {}", parts.get(1).unwrap_or(&"--")))
                    .xy(pt2(content_rect.x(), start_y - line_height))
                    .color(Srgb::new(200u8, 200, 255)) // Pale blue
                    .font_size(font_size);
                
                // Rising
                draw.text(&format!("ASC {}", parts.get(2).unwrap_or(&"--")))
                    .xy(pt2(content_rect.x(), start_y - line_height * 2.0))
                    .color(Srgb::new(255u8, 150, 150)) // Soft red
                    .font_size(font_size);
                
                // Planetary positions (remaining parts)
                if parts.len() > 3 && drawing_maximized {
                    for (i, planet) in parts.iter().skip(3).enumerate() {
                        draw.text(planet.trim())
                            .xy(pt2(content_rect.x(), start_y - line_height * (3.0 + i as f32)))
                            .color(GRAY)
                            .font_size(12);
                    }
                }
            } else {
                draw.text(&model.astro_data)
                    .xy(content_rect.xy())
                    .color(GRAY)
                    .font_size(if drawing_maximized { 16 } else { 11 });
            }
            
            // Status indicator
            let indicator_color = if model.astro_data.contains("NO DATA") {
                rgba(1.0, 0.3, 0.3, 0.6)
            } else {
                rgba(0.3, 1.0, 0.3, 0.6)
            };
            draw.rect()
                .x(rect.right() - 5.0)
                .y(rect.top() - 10.0)
                .w(6.0)
                .h(6.0)
                .color(indicator_color);
        },
        "editor" => {
             // Managed by Egui
        },
        "audio_input" => {
             // Oscilloscope Visualization
             let points: Vec<Point2> = model.audio_buffer.iter().enumerate().map(|(i, &sample)| {
                 let x = map_range(i, 0, 2048, content_rect.left(), content_rect.right());
                 let y = map_range(sample, -1.0, 1.0, content_rect.bottom(), content_rect.top());
                 pt2(x, y)
             }).collect();
             
             if !points.is_empty() {
                 draw.polyline()
                    .weight(2.0)
                    .points(points)
                    .color(SPRINGGREEN.into_format::<f32>().into_linear()); // Bright Green Scope
             }
                
             draw.text("OSCILLOSCOPE")
                .xy(pt2(content_rect.x(), content_rect.top() - 10.0))
                .color(SPRINGGREEN)
                .font_size(10);
        },
        _ => {}
    }
}