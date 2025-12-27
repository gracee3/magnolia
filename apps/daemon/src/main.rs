use nannou::prelude::*;
use talisman_core::{Source, Sink, Signal, PatchBay};
use aphrodite::AphroditeSource;
use logos::LogosSource;
use kamea::{self, SigilConfig};
use text_tools::{WordCountSink, DevowelizerSink};
use nannou_egui::{self, Egui, egui};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use std::thread;
use std::collections::HashSet;

// --- MODEL ---
struct Model {
    // We use a non-blocking channel for the UI thread to receive updates
    receiver: std::sync::mpsc::Receiver<Signal>,
    orchestrator_tx: mpsc::Sender<Signal>, 
    
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
    disabled_tiles: HashSet<String>,
    show_patch_bay: bool,
}

struct ContextMenuState {
    tile_id: String,
    position: Point2,
}

// --- LAYOUT ENGINE ---
use talisman_core::{LayoutConfig, TileConfig};
use std::fs;

struct Layout {
    window_rect: Rect,
    config: LayoutConfig,
    // Add cache for resolved rects? For now, recalculate is cheap.
}

impl Layout {
    fn new(win_rect: Rect) -> Self {
        // Load config
        // Try multiple paths (repo root vs crate dir)
        let paths = ["configs/layout.toml", "../../configs/layout.toml"];
        let mut content = None;
        for p in &paths {
            if let Ok(c) = fs::read_to_string(p) {
                content = Some(c);
                break;
            }
        }
        
        let content = content.unwrap_or_else(|| {
                println!("Warning: Could not load layout.toml from {:?}, using default.", paths);
                r#"
                columns = ["250px", "1fr"]
                rows = ["40px", "1fr", "30px"]
                
                [[tiles]]
                id = "header"
                col = 0
                row = 0
                colspan = 2
                module = "header"
                "# .to_string()
            });
            
        let config: LayoutConfig = toml::from_str(&content).expect("Failed to parse layout.toml");
        
        Self { 
            window_rect: win_rect,
            config,
        }
    }
    
    fn update(&mut self, win_rect: Rect) {
        self.window_rect = win_rect;
    }
    
    
    // Helper to calculate Grid Rect directly from Col/Row
    fn calculate_rect(&self, tile: &TileConfig) -> Option<Rect> {
        let cols = self.resolve_tracks(&self.config.columns, self.window_rect.w());
        let rows = self.resolve_tracks(&self.config.rows, self.window_rect.h());
        
        let start_x = cols.iter().take(tile.col).sum::<f32>();
        let width = cols.iter().skip(tile.col).take(tile.colspan.unwrap_or(1)).sum::<f32>();
        
        // Nannou Y is bottom-to-top, but Grid is usually Top-to-Bottom.
        // Let's assume Row 0 is Top.
        // total_h = self.window_rect.h()
        // row 0 height = rows[0]
        // y_top = self.window_rect.top()
        // row 0 y = y_top - rows[0]/2 ? Nannou coords are center based?
        // Let's map 0..H to window.top()..window.bottom().
        
        let start_y_from_top = rows.iter().take(tile.row).sum::<f32>();
        let height = rows.iter().skip(tile.row).take(tile.rowspan.unwrap_or(1)).sum::<f32>();
        
        // Nannou Coordinate Conversion
        // Left = self.window_rect.left() + start_x
        // Top = self.window_rect.top() - start_y_from_top
        // Center X = Left + w/2
        // Center Y = Top - h/2
        
        let cx = self.window_rect.left() + start_x + width / 2.0;
        let cy = self.window_rect.top() - start_y_from_top - height / 2.0;
        
        Some(Rect::from_x_y_w_h(cx, cy, width, height))
    }
    
    fn resolve_tracks(&self, tracks: &[String], total_size: f32) -> Vec<f32> {
        let mut resolved = vec![0.0; tracks.len()];
        let mut used_px = 0.0;
        let mut total_fr = 0.0;
        
        // First pass: PX and FR sum
        for (i, track) in tracks.iter().enumerate() {
            if track.ends_with("px") {
                let val = track.trim_end_matches("px").parse::<f32>().unwrap_or(0.0);
                resolved[i] = val;
                used_px += val;
            } else if track.ends_with("%") {
                let val = track.trim_end_matches("%").parse::<f32>().unwrap_or(0.0);
                let px = (val / 100.0) * total_size;
                resolved[i] = px;
                used_px += px;
            } else if track.ends_with("fr") {
                let val = track.trim_end_matches("fr").parse::<f32>().unwrap_or(1.0);
                total_fr += val;
            } else {
                 if track.contains("fr") {
                      let val = track.replace("fr","").parse::<f32>().unwrap_or(1.0);
                      total_fr += val;
                 } else if track.contains("%") {
                      let val = track.replace("%","").parse::<f32>().unwrap_or(0.0);
                      let px = (val / 100.0) * total_size;
                      resolved[i] = px;
                      used_px += px;
                 } else {
                      let val = track.replace("px","").parse::<f32>().unwrap_or(0.0);
                      resolved[i] = val;
                      used_px += val;
                 }
            }
        }
        
        let remaining = (total_size - used_px).max(0.0);
        
        // Second pass: Resolve FR
        if total_fr > 0.0 {
            for (i, track) in tracks.iter().enumerate() {
                 let is_fr = track.contains("fr"); // Loose check
                 if is_fr {
                      let val = track.trim_end_matches("fr").parse::<f32>().unwrap_or(1.0);
                      resolved[i] = (val / total_fr) * remaining;
                 }
            }
        }
        
        resolved
    }
}


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
    // 1. Setup Channels (High Perf Core)
    let (tx_ui, rx_ui) = std::sync::mpsc::channel::<Signal>();
    let (tx_orch, mut rx_orch) = mpsc::channel::<Signal>(100);
    
    let tx_ui_for_orch = tx_ui.clone();
    let tx_ui_for_sinks = tx_ui.clone();
    let tx_orch_for_sources = tx_orch.clone();
    
    thread::spawn(move || {
        let rt = Runtime::new().expect("Tokio");
        rt.block_on(async move {
            println!("{}TALISMAN HIGH-PERF CORE ONLINE{}", CLI_GREEN, CLI_RESET);
            
            // 1. Sinks (Consumer Layer)
            let sinks: Vec<Box<dyn Sink>> = vec![
                Box::new(WordCountSink::new(Some(tx_ui_for_sinks.clone()))),
                Box::new(DevowelizerSink::new(Some(tx_ui_for_sinks.clone()))),
            ];
            
            // 2. Spawn Sources (Producer Layer)
            spawn_source(Box::new(AphroditeSource::new(10)), tx_orch_for_sources.clone());
            spawn_source(Box::new(LogosSource::new()), tx_orch_for_sources.clone());
            
            // 3. Event Loop (Router)
            // No sleep! Just await content.
            while let Some(signal) = rx_orch.recv().await {
                // Route to UI
                let _ = tx_ui_for_orch.send(signal.clone());
                
                // Route to Sinks
                for sink in &sinks {
                    let _ = sink.consume(signal.clone()).await;
                }
            }
        });
    });

    // 4. Init Window ID & Egui
    let window_id = app.new_window()
        .view(view)
        .raw_event(raw_window_event)
        .key_pressed(key_pressed)
        .mouse_pressed(mouse_pressed)
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
    
    log::info!("Patch Bay initialized with {} modules", patch_bay.get_modules().len());

    Model {
        receiver: rx_ui,
        orchestrator_tx: tx_orch,
        egui,
        text_buffer: String::new(),
        current_intent: "AWAITING SIGNAL".to_string(),
        path_points: vec![],
        astro_data: "NO DATA".to_string(),
        retinal_burn: false,
        word_count: "0".to_string(),
        devowel_text: "".to_string(),
        config,
        layout: Layout::new(app.window_rect()),
        selected_tile: None,
        maximized_tile: None,
        last_click_time: std::time::Instant::now(),
        anim_factor: 0.0,
        is_closing: false,
        clipboard,
        context_menu: None,
        patch_bay,
        disabled_tiles: HashSet::new(),
        show_patch_bay: false,
    }
}

// Helper to spawn a source into a generic loop
fn spawn_source(mut source: Box<dyn Source>, tx: mpsc::Sender<Signal>) {
    tokio::spawn(async move {
        loop {
            // Poll the source
            if let Some(signal) = source.poll().await {
                 if tx.send(signal).await.is_err() {
                     break; // Channel closed
                 }
            } else {
                // Source exhausted (unlikely for our sources, but possible)
                // For now, if poll returns None, we assume it's done. 
                // But Logos might return None on EOF. Aphrodite never returns None (unless error).
                // Let's just loop.
                // Wait, if poll returns None, we should stop? 
                // Logos returns None on EOF (ctrl-D).
                // Sleep briefly to avoid busy loop if source is broken (returns None repeatedly)
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    });
}

fn update(app: &App, model: &mut Model, update: Update) {
    // Update Layout dimensions
    model.layout.update(app.window_rect());
    
    // Smooth Animation
    if model.is_closing {
        model.anim_factor = (model.anim_factor - 0.1).max(0.0);
        if model.anim_factor <= 0.0 {
            model.maximized_tile = None;
            model.is_closing = false;
        }
    } else if model.maximized_tile.is_some() && model.anim_factor < 1.0 {
        model.anim_factor = (model.anim_factor + 0.1).min(1.0);
    }

    // 1. UPDATE GUI
    model.egui.set_elapsed_time(update.since_start);
    let ctx = model.egui.begin_frame();
    
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
                            let _ = model.orchestrator_tx.try_send(signal);
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

                if ui.add_sized(btn_size, egui::Button::new("SETTINGS")).clicked() {
                    log::info!("Settings clicked for {}", tile_id);
                    open = false;
                }
                
                if ui.add_sized(btn_size, egui::Button::new("COPY")).clicked() {
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
                
                if ui.add_sized(btn_size, egui::Button::new("PASTE")).clicked() {
                     if tile_id == "editor_pane" {
                         if let Some(cb) = &mut model.clipboard {
                             if let Ok(text) = cb.get_text() {
                                  model.text_buffer.push_str(&text);
                                  let _ = model.orchestrator_tx.try_send(Signal::Text(model.text_buffer.clone()));
                             }
                        }
                     }
                     open = false;
                }
                
                ui.add(egui::Separator::default().spacing(10.0));
                
                // Toggle button text based on disabled state
                let is_disabled = model.disabled_tiles.contains(&tile_id);
                let disable_text = if is_disabled { "ENABLE" } else { "DISABLE" };
                
                if ui.add_sized(btn_size, egui::Button::new(disable_text)).clicked() {
                    if is_disabled {
                        model.disabled_tiles.remove(&tile_id);
                        log::info!("Enabled Tile: {}", tile_id);
                    } else {
                        model.disabled_tiles.insert(tile_id.clone());
                        log::info!("Disabled Tile: {}", tile_id);
                    }
                    open = false;
                }
                
                if ui.add_sized(btn_size, egui::Button::new("REMOVE")).clicked() {
                    // Remove from layout config
                    model.layout.config.tiles.retain(|t| t.id != tile_id);
                    open = false;
                }
                
                ui.add(egui::Separator::default().spacing(10.0));
                 if ui.add_sized(btn_size, egui::Button::new("PATCH BAY")).clicked() {
                    model.show_patch_bay = true;
                    log::info!("Opening Patch Bay");
                    open = false;
                }
                if ui.add_sized(btn_size, egui::Button::new("GLOBAL SETTINGS")).clicked() {
                    log::info!("Global Settings");
                    open = false;
                }
                 if ui.add_sized(btn_size, egui::Button::new("SLEEP")).clicked() {
                    log::info!("Sleep Engine");
                    // maybe toggle a global pause?
                     open = false;
                }
                
                ui.add(egui::Separator::default().spacing(10.0));
                
                if ui.add_sized(btn_size, egui::Button::new("EXIT DAEMON")).clicked() {
                    std::process::exit(0);
                }
            });
            
        if !open {
            model.context_menu = None;
        }
    }

    // Patch Bay Modal
    if model.show_patch_bay {
        egui::Window::new("PATCH BAY")
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_size(egui::vec2(600.0, 450.0))
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
                                            talisman_core::PortDirection::Input => ("◀", egui::Color32::from_rgb(100, 200, 100)),
                                            talisman_core::PortDirection::Output => ("▶", egui::Color32::from_rgb(200, 100, 100)),
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
                            "  {}:{} → {}:{}",
                            patch.source_module, patch.source_port,
                            patch.sink_module, patch.sink_port
                        )).small().color(egui::Color32::YELLOW));
                    }
                }
            });
    }

    // 2. PROCESS SIGNALS from Orchestrator (High speed!)
    while let Ok(signal) = model.receiver.try_recv() {
        match signal {
            Signal::Text(text) => {
                model.current_intent = text.clone();
                
                let mut hasher = sha2::Sha256::new();
                use sha2::Digest;
                hasher.update(text.as_bytes());
                let result = hasher.finalize();
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&result);

                let len_factor = text.len().min(100) as usize; 
                let size = if len_factor > 10 { 5 } else { 4 };
                model.config.grid_rows = size;
                model.config.grid_cols = size;
                
                // Find tile for kamea
                let sigil_tile = model.layout.config.tiles.iter().find(|t| t.module == "kamea_sigil");
                if let Some(tile) = sigil_tile {
                     if let Some(rect) = model.layout.calculate_rect(tile) {
                         model.config.spacing = rect.w() / (size as f32 * 2.0); 
                     } else {
                         model.config.spacing = 30.0;
                     }
                } else {
                     model.config.spacing = 30.0;
                }

                model.path_points = kamea::generate_path(seed, model.config)
                    .into_iter()
                    .map(|(x, y)| pt2(x, y))
                    .collect();
            },
            Signal::Computed { source, content } => {
                if source == "word_count" {
                    model.word_count = content;
                } else if source == "devowelizer" {
                    model.devowel_text = content;
                }
            },
            Signal::Astrology { sun_sign, moon_sign, .. } => {
                model.astro_data = format!("Sun: {} | Moon: {}", sun_sign, moon_sign);
            },
            _ => {}
        }
    }
}

fn mouse_pressed(app: &App, model: &mut Model, button: MouseButton) {
    if button == MouseButton::Left {
        // Close Context Menu if clicking elsewhere
        model.context_menu = None;

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

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    let ctrl = _app.keys.mods.ctrl();
    
    if ctrl {
        match key {
            Key::C => {
                // COPY logic
                if let Some(selected) = &model.selected_tile {
                    // Map selected ID to content
                    // Note: This relies on known IDs. 
                    let content = if selected == "wc_pane" {
                         Some(model.word_count.clone())
                    } else if selected == "dvwl_pane" {
                         Some(model.devowel_text.clone())
                    } else if selected == "astro_pane" {
                         Some(model.astro_data.clone())
                    } else if selected == "editor_pane" {
                         // Editor copy handled by Egui natively if focused?
                         // If we clicked the tile but Egui isn't focused, we might want to copy buffer?
                         Some(model.text_buffer.clone())
                    } else if selected == "sigil_pane" {
                         // Can't copy vector graphics to text clipboard easily
                         None 
                    } else {
                         None
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
                 // PASTE Logic
                 if let Some(selected) = &model.selected_tile {
                     if selected == "editor_pane" {
                          if let Some(cb) = &mut model.clipboard {
                               match cb.get_text() {
                                   Ok(text) => {
                                        model.text_buffer.push_str(&text);
                                        // Trigger Signal
                                        let _ = model.orchestrator_tx.try_send(Signal::Text(model.text_buffer.clone()));
                                   },
                                   Err(e) => log::error!("Clipboard Paste Failed: {}", e)
                               }
                          }
                     }
                 }
            },
            _ => {}
        }
    }
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

    // Iterate over all tiles and render
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
            render_tile(draw.clone(), tile, rect, model, bc, fg_color, false);
        }
    }

    // Draw Maximized Tile on top
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
                
                render_tile(draw.clone(), tile, rect, model, CYAN.into_format().into_linear().into(), fg_color, true);
            }
        }
    }

    draw.to_frame(app, &frame).unwrap();
    model.egui.draw_to_frame(&frame).unwrap();
}

// Helper to render tile content
fn render_tile(draw: Draw, tile: &TileConfig, rect: Rect, model: &Model, border_color: LinSrgba, fg_color: Srgb<u8>, drawing_maximized: bool) {
    let is_selected = model.selected_tile.as_ref() == Some(&tile.id);
    let is_disabled = model.disabled_tiles.contains(&tile.id);
    
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
        .stroke_weight(if is_selected || drawing_maximized { 2.0 } else { 1.0 });
    
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
            let text = format!("ASTROLOGY: {}", model.astro_data);
            draw.text(&text)
                .xy(content_rect.xy())
                .color(GRAY)
                .font_size(if drawing_maximized { 32 } else { 12 });
                
            if text.len() > 40 {
                 draw.rect().x(rect.right() - 5.0).y(rect.y()).w(2.0).h(rect.h() * 0.4).color(rgba(0.5, 0.5, 0.5, 0.4));
            }
        },
        "editor" => {
             // Managed by Egui
        },
        _ => {}
    }
}