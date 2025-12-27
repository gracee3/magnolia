use nannou::prelude::*;
use talisman_core::{Source, Sink, Signal};
use aphrodite::AphroditeSource;
use logos::LogosSource;
use kamea::{self, SigilConfig};
use text_tools::{WordCountSink, DevowelizerSink};
use nannou_egui::{self, Egui, egui};
use tokio::runtime::Runtime;
use tokio::sync::mpsc; // Use Tokio mpsc for async-await support in orchestrator
use std::thread;

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
    clipboard: Option<arboard::Clipboard>,
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
    // Default: warn for everything, but silence wgpu warnings (buffer drops), debug for our crates.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,wgpu_core=error,wgpu_hal=error,nannou=error,daemon=debug,text_tools=debug,aphrodite=debug,logos=debug,kamea=debug")).init();
    
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
        clipboard,
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

    // Kamea Buttons
    let kamea_tile = model.layout.config.tiles.iter().find(|t| t.module == "kamea_sigil");
    if let Some(tile) = kamea_tile {
         if let Some(rect) = model.layout.calculate_rect(tile) {
             // Position buttons using an Area which respects coordinates better than Window logic sometimes
             // Egui coordinates: Top-Left is (0,0).
             let egui_params = egui::pos2(
                  rect.left() + app.window_rect().w()/2.0, 
                  app.window_rect().h()/2.0 - rect.top()
             );
             
             // Place buttons at the bottom of the tile
             egui::Area::new("kamea_buttons")
                .fixed_pos(egui::pos2(egui_params.x + 10.0, egui_params.y + rect.h() - 40.0))
                .show(&ctx, |ui| {
                     ui.horizontal(|ui| {
                         ui.style_mut().visuals.widgets.inactive.bg_fill = egui::Color32::BLACK; // Button BG
                         ui.style_mut().visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 50, 50);
                         
                         if ui.button("BURN").clicked() {
                             model.retinal_burn = !model.retinal_burn;
                         }
                         if ui.button("DESTROY").clicked() {
                             std::process::exit(0);
                         }
                     });
                });
         }
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
        // Hit test against tiles
        let mouse_pos = app.mouse.position(); // Relative to center?
        // App mouse position is (0,0) at center, +y up.
        // Our Layout calculation uses the same coordinate system.
        
        let mut hit = false;
        for tile in &model.layout.config.tiles {
            if let Some(rect) = model.layout.calculate_rect(tile) {
                if rect.contains(mouse_pos) {
                    model.selected_tile = Some(tile.id.clone());
                    hit = true;
                    log::debug!("Selected Tile: {}", tile.id);
                    break;
                }
            }
        }
        
        if !hit {
            model.selected_tile = None;
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

    // Iterate over all tiles in the config and render based on assigned module
    for tile in &model.layout.config.tiles {
        if let Some(rect) = model.layout.calculate_rect(tile) {
            
            // Determine border color based on Selection
            let is_selected = model.selected_tile.as_ref() == Some(&tile.id);
            
            // Unify color types to Alpha<Rgb>
            let border_color = if is_selected {
                rgba(0.0, 1.0, 1.0, 0.5) // Cyan Highlight
            } else {
                let s = stroke_color.into_format::<f32>();
                rgba(s.red, s.green, s.blue, 1.0)
            };
            
            // Visualize Borders
            draw.rect()
                .xy(rect.xy())
                .wh(rect.wh())
                .color(rgba(0.0, 0.0, 0.0, 0.0)) // Transparent BG
                .stroke(border_color) 
                .stroke_weight(if is_selected { 2.0 } else { 1.0 });
            
            if is_selected {
                draw.text("[CLIPBOARD ACTIVE]")
                    .xy(pt2(rect.left() + 60.0, rect.bottom() + 10.0))
                    .color(CYAN)
                    .font_size(10);
            }
            
            // Content Padding
            let content_rect = rect.pad(10.0);
            
            match tile.module.as_str() {
                "header" => {
                    // ...
                },
                "kamea_sigil" => {
                     // ... (Existing Sigil Code)
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
                     let truncated_intent = if sanitized_intent.len() > 50 {
                         format!("{}...", &sanitized_intent[..50])
                     } else {
                         sanitized_intent
                     };
                     draw.text(&truncated_intent)
                        .xy(pt2(content_rect.x(), content_rect.top() - 10.0))
                        .color(fg_color)
                        .font_size(14);
                },
                "word_count" => {
                    let text = format!("WORD COUNT: {}", model.word_count);
                    draw.text(&text)
                        .xy(content_rect.xy())
                        .color(YELLOW)
                        .font_size(12);
                        
                    // Simple Scrollbar placeholder if "needed" (simulation)
                    // If text length is huge, draw bar.
                    if model.word_count.len() > 100 {
                         draw.rect()
                            .x(rect.right() - 5.0)
                            .y(rect.y())
                            .w(4.0)
                            .h(rect.h() * 0.3)
                            .color(rgba(1.0, 1.0, 1.0, 0.3));
                    }
                },
                "devowelizer" => {
                    // Sanitize: No newlines, normalize spaces
                    // Regex replacement is heavy in view loop, maybe do it in Sink? 
                    // Sink already did regex? The Sink sends raw text? 
                    // Sink sends devoweled text. But user wants "sanitize the dvwl module so the text ignores newlines and normalizes spaces".
                    // Let's do a quick sanitization here.
                    let raw = &model.devowel_text;
                    let clean = raw.replace('\n', " ").replace('\r', "");
                    // Collapse spaces (simple)
                    let clean_text = clean.split_whitespace().collect::<Vec<&str>>().join(" ");
                    
                    let _text = format!("DEVOWELIZER: {}", clean_text);
                     // Truncate to avoid overflow for now as we don't have real scrolling view yet, just placeholder
                    let display_text = if clean_text.len() > 200 {
                         format!("{}...", &clean_text[..200])
                    } else {
                         clean_text
                    };
                    
                    draw.text(&format!("DVWL: {}", display_text))
                        .xy(content_rect.xy())
                        .color(MAGENTA)
                        .font_size(12)
                        .w(content_rect.w()); // Wrap width
                        
                    // Scrollbar Placeholder
                    if raw.len() > 50 {
                         draw.rect()
                            .x(rect.right() - 5.0)
                            .y(rect.y())
                            .w(4.0)
                            .h(rect.h() * 0.5)
                            .color(rgba(1.0, 0.0, 1.0, 0.5));
                    }
                },
                "astrology" => {
                    let text = format!("ASTROLOGY: {}", model.astro_data);
                    draw.text(&text)
                        .xy(content_rect.xy())
                        .color(GRAY)
                        .font_size(12);
                        
                    // Scrollbar Placeholder if data is wider than rect
                    if text.len() > 40 {
                         draw.rect()
                            .x(rect.right() - 5.0)
                            .y(rect.y())
                            .w(2.0)
                            .h(rect.h() * 0.4)
                            .color(rgba(0.5, 0.5, 0.5, 0.4));
                    }
                },
                "editor" => {
                    // Handled by Egui, but we can draw a placeholder back/border if needed.
                    // Egui is drawn on top.
                },
                _ => {}
            }
        }
    }

    draw.to_frame(app, &frame).unwrap();
    model.egui.draw_to_frame(&frame).unwrap();
}