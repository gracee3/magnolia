use nannou::prelude::*;
use talisman_core::{Source, Sink, Signal};
use aphrodite::AphroditeSource;
use logos::LogosSource;
use kamea::{self, SigilConfig};
use text_tools::{WordCountSink, DevowelizerSink};
use nannou_egui::{self, Egui, egui};
use tokio::runtime::Runtime;
use std::sync::mpsc;
use std::thread;

// --- MODEL ---
struct Model {
    receiver: mpsc::Receiver<Signal>,
    orchestrator_tx: mpsc::Sender<Signal>, // Channel to send GUI events to Orchestrator (or Sinks)
    
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
}

const CLI_GREEN: &str = "\x1b[32m";

const CLI_RESET: &str = "\x1b[0m";

fn main() {
    nannou::app(model)
        .update(update)
        .run();
}

fn model(app: &App) -> Model {
    // 1. Setup Channels
    let (tx_to_ui, rx_from_orch) = mpsc::channel();
    let (tx_to_orch, rx_from_ui) = mpsc::channel::<Signal>();

    // 2. Clone sender for Orchestrator thread
    let tx_to_ui_clone = tx_to_ui.clone();
    
    // 3. Spawn Orchestrator (The Patch Bay)
    thread::spawn(move || {
        let rt = Runtime::new().expect("Failed to create Tokio runtime");
        rt.block_on(async move {
            println!("{}TALISMAN ORCHESTRATOR ONLINE{}", CLI_GREEN, CLI_RESET);
            
            // --- MODULES ---
            let mut sources: Vec<Box<dyn Source>> = vec![
                Box::new(AphroditeSource::new(10)),
                Box::new(LogosSource::new()),
            ];
            
            // We need a channel for Sinks to talk back to Orchestrator/UI
            // Actually, we can just reuse the UI channel 'tx_to_ui_clone' for simplicity?
            // Yes, because Computed signals go to UI.
            
            // We need to wrap it because `Sender` receives a value, it doesn't await. 
            // Sinks are async trait, but the send is blocking or sync. 
            // We used `mpsc` which is sync.
            // Let's create new clones for them.
            
            let sinks: Vec<Box<dyn Sink>> = vec![
                Box::new(WordCountSink::new(Some(tx_to_ui_clone.clone()))),
                Box::new(DevowelizerSink::new(Some(tx_to_ui_clone.clone()))),
            ];

            // --- EVENT LOOP ---
            loop {
                // A. Check Sources
                for source in &mut sources {
                    if let Some(signal) = source.poll().await {
                         // broadcast to UI
                         let _ = tx_to_ui_clone.send(signal.clone());
                         // broadcast to sinks
                         for sink in &sinks {
                             let _ = sink.consume(signal.clone()).await;
                         }
                    }
                }
                
                // B. Check GUI Inputs (acting as a Source)
                while let Ok(signal) = rx_from_ui.try_recv() {
                    // broadcast to UI local loop (for sigil gen) - though it originated there, we might want loopback
                    // actually, for this architecture, let's say the Orchestrator is the hub.
                    // So we bounce it back to the UI channel.
                    let _ = tx_to_ui_clone.send(signal.clone());
                    
                    // forward to sinks
                    for sink in &sinks {
                        let _ = sink.consume(signal.clone()).await;
                    }
                }
                
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
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
        receiver: rx_from_orch,
        orchestrator_tx: tx_to_orch,
        egui,
        text_buffer: String::new(),
        current_intent: "AWAITING SIGNAL".to_string(),
        path_points: vec![],
        astro_data: "NO DATA".to_string(),
        word_count: "0".to_string(),
        devowel_text: "".to_string(),
        config,
    }
}

fn update(_app: &App, model: &mut Model, update: Update) {
    // 1. UPDATE GUI
    model.egui.set_elapsed_time(update.since_start);
    let ctx = model.egui.begin_frame();

    egui::Window::new("Source: Text Editor").show(&ctx, |ui| {
        ui.label("Type your intent below:");
        let response = ui.add(egui::TextEdit::multiline(&mut model.text_buffer).desired_width(300.0));
        
        if response.changed() {
            // Act as a Source: Emit Signal
            // We strip newlines to simple single intents for now, or just send the whole block
            // Taking the last line or just the whole buffer? Let's send the whole buffer.
            let signal = Signal::Text(model.text_buffer.clone());
            let _ = model.orchestrator_tx.send(signal);
        }
    });

    // 2. PROCESS SIGNALS from Orchestrator
    while let Ok(signal) = model.receiver.try_recv() {
        match signal {
            Signal::Text(text) => {
                // If it came from us (GUI), we might ignore updating text_buffer to avoid loop, 
                // but we usually want to update the Sigil.
                // If it came from Stdin (Logos), we might mistakenly overwrite GUI? 
                // Let's just update the Intent Display and Sigil.
                model.current_intent = text.clone();
                
                // Sigil Logic
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
                model.config.spacing = 300.0 / (size as f32);

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

    // Sigil Area (Right side)
    let draw = draw.x(200.0); // Offset to the right

    // Header
    draw.text("TALISMAN // PATCH BAY")
        .xy(pt2(0.0, 300.0))
        .color(WHITE)
        .font_size(16);

    // Astro Data
    draw.text(&model.astro_data)
        .xy(pt2(0.0, 280.0))
        .color(GRAY)
        .font_size(12);

    // Intent
    draw.text(&model.current_intent)
        .xy(pt2(0.0, -250.0))
        .color(CYAN)
        .font_size(14);
        
    // Computed Results
    draw.text(&format!("WORDS: {}", model.word_count))
        .xy(pt2(-200.0, -280.0)) // Bottom Left
        .color(YELLOW)
        .font_size(12);
        
    draw.text(&format!("DVWL: {}", model.devowel_text))
        .xy(pt2(200.0, -280.0)) // Bottom Right
        .color(MAGENTA)
        .font_size(12);

    // Sigil
    if !model.path_points.is_empty() {
        draw.polyline()
            .weight(model.config.stroke_weight)
            .join_round()
            .caps_round()
            .points(model.path_points.clone())
            .color(CYAN);
    }

    draw.to_frame(app, &frame).unwrap();
    model.egui.draw_to_frame(&frame).unwrap();
}