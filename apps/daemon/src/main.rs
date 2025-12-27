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
    // Sink Results
    word_count: String,
    devowel_text: String,
    config: SigilConfig,
    
    // Layout
    layout: Layout,
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
    
    // Resolve a specific tile by ID
    fn get_rect(&self, tile_id: &str) -> Option<Rect> {
        let tile = self.config.tiles.iter().find(|t| t.id == tile_id)?;
        self.calculate_rect(tile)
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
            } else if track.ends_with("fr") {
                let val = track.trim_end_matches("fr").parse::<f32>().unwrap_or(1.0);
                total_fr += val;
            } else {
                 // Assume px if number, or Fr? 
                 // Let's assume px default or 1fr default?
                 // Let's assume "1fr" if strictly "1fr", otherwise try parse as px.
                 // Actually common CSS is "250px", "1fr".
                 if track.contains("fr") {
                      let val = track.replace("fr","").parse::<f32>().unwrap_or(1.0);
                      total_fr += val;
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

    Model {
        receiver: rx_ui,
        orchestrator_tx: tx_orch,
        egui,
        text_buffer: String::new(),
        current_intent: "AWAITING SIGNAL".to_string(),
        path_points: vec![],
        astro_data: "NO DATA".to_string(),
        word_count: "0".to_string(),
        devowel_text: "".to_string(),
        config,
        layout: Layout::new(app.window_rect()),
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
    if let Some(rect) = model.layout.get_rect("sidebar") {
        egui::Window::new("Source: Text Editor")
            .default_pos(egui::pos2(rect.left() + 10.0, model.layout.window_rect.top() - rect.top() + 10.0)) // Nannou Top is +Y, Egui Top is 0
            // Nannou: Y grows Up. Egui: Y grows Down?
            // Nannou: Center (0,0). 
            // rect.left() is OK (-X).
            // Nannou Top is +H/2. Egui Y=0 is Top.
            // So Egui Y = (Window.Top - Rect.Top) ? or just convert coords.
            // Let's use simple math:
            // Egui X = rect.x + W/2 + OFFSET? 
            // Actually, we can just use the rect size. Egui pos is tricky with Nannou coords.
            // Let's simplistic mapping: 
            // Nannou Window Top Left = (-W/2, +H/2).
            // Egui Top Left = (0, 0).
            // Egui X = Nannou X + Window.W/2.
            // Egui Y = Window.H/2 - Nannou Y.
            .fixed_pos(egui::pos2(
                rect.left() + app.window_rect().w()/2.0, 
                app.window_rect().h()/2.0 - rect.top()
            ))
            .fixed_size(egui::vec2(rect.w(), rect.h()))
            .show(&ctx, |ui| {
                ui.label("Type your intent below:");
                let response = ui.add(egui::TextEdit::multiline(&mut model.text_buffer).desired_width(ui.available_width()));
                
                if response.changed() {
                    let signal = Signal::Text(model.text_buffer.clone());
                    let _ = model.orchestrator_tx.try_send(signal);
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
                
                if let Some(main_rect) = model.layout.get_rect("main") {
                     model.config.spacing = main_rect.w() / (size as f32 * 2.0); 
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

fn raw_window_event(_app: &App, model: &mut Model, event: &nannou::winit::event::WindowEvent) {
    model.egui.handle_raw_event(event);
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    draw.background().color(BLACK);

    // Grid Lines (Debug)
    // ...

    // HEADER
    if let Some(header) = model.layout.get_rect("header") {
        draw.text("TALISMAN // PATCH BAY")
            .xy(header.xy())
            .color(WHITE)
            .font_size(16);
    }

    // MAIN CONTENT (Sigil)
    if let Some(main) = model.layout.get_rect("main") {
        if !model.path_points.is_empty() {
            let offset = main.xy();
            let translated_points: Vec<Point2> = model.path_points.iter()
                .map(|p| *p + offset) 
                .collect();
                
            draw.polyline()
                .weight(model.config.stroke_weight)
                .join_round()
                .caps_round()
                .points(translated_points)
                .color(CYAN);
        }
        
        // INTENT
        draw.text(&model.current_intent)
            .xy(pt2(main.x(), main.top() - 20.0))
            .color(CYAN)
            .font_size(14);
    }

    // FOOTER (Sinks / Status)
    if let Some(footer) = model.layout.get_rect("footer") {
        // Astro (Left side of footer)
        draw.text(&model.astro_data)
            .xy(pt2(footer.left() + 150.0, footer.y()))
            .color(GRAY)
            .font_size(12);

        // Computed (Right side of footer)
        draw.text(&format!("WORDS: {} | DVWL: {}", model.word_count, model.devowel_text))
            .xy(pt2(footer.right() - 200.0, footer.y()))
            .color(YELLOW)
            .font_size(12);
    }

    draw.to_frame(app, &frame).unwrap();
    model.egui.draw_to_frame(&frame).unwrap();
}