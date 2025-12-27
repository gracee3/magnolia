use nannou::prelude::*;
use talisman_core::{Source, Signal};
use aphrodite::AphroditeSource;
use logos::LogosSource;
use kamea::{self, SigilConfig}; // We might need to expose generate_path in kamea lib
use tokio::runtime::Runtime;
use std::sync::mpsc;
use std::thread;

// --- MODEL ---
struct Model {
    receiver: mpsc::Receiver<Signal>,
    current_intent: String,
    path_points: Vec<Point2>,
    astro_data: String,
    config: SigilConfig,
}

const CLI_GREEN: &str = "\x1b[32m";
const CLI_CYAN: &str = "\x1b[36m";
const CLI_RESET: &str = "\x1b[0m";

fn main() {
    nannou::app(model)
        .update(update)
        .simple_window(view)
        .run();
}

fn model(_app: &App) -> Model {
    // 1. Setup Channel: Orchestrator -> UI
    let (tx, rx) = mpsc::channel();

    // 2. Spawn Orchestrator (The Patch Bay)
    thread::spawn(move || {
        let rt = Runtime::new().expect("Failed to create Tokio runtime");
        rt.block_on(async move {
            println!("{}TALISMAN ORCHESTRATOR ONLINE{}", CLI_GREEN, CLI_RESET);
            
            // --- MODULES ---
            let mut sources: Vec<Box<dyn Source>> = vec![
                Box::new(AphroditeSource::new(10)), // Poll Astro every 10s
                Box::new(LogosSource::new()),       // Stdin
            ];

            // --- EVENT LOOP ---
            loop {
                // We poll cleanly. In a real system we'd use select_all or FuturesUnordered
                // For now, simple round-robin poll with small sleep to prevent busy loop
                let mut activity = false;
                for source in &mut sources {
                    if let Some(signal) = source.poll().await {
                        activity = true;
                        // ROUTING: Hardcoded "Everything to UI"
                        if let Err(e) = tx.send(signal) {
                            eprintln!("UI Channel closed: {}", e);
                            return;
                        }
                    }
                }
                
                if !activity {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        });
    });

    // 3. Init State
    let config = SigilConfig {
        spacing: 50.0,
        stroke_weight: 4.0,
        grid_rows: 4,
        grid_cols: 4,
    };

    Model {
        receiver: rx,
        current_intent: "AWAITING SIGNAL".to_string(),
        path_points: vec![],
        astro_data: "NO DATA".to_string(),
        config,
    }
}

fn update(_app: &App, model: &mut Model, _update: Update) {
    // NON-BLOCKING CHECK FOR SIGNALS
    while let Ok(signal) = model.receiver.try_recv() {
        match signal {
            Signal::Text(text) => {
                println!("{}RECEIVED INTENT: {}{}", CLI_CYAN, text, CLI_RESET);
                model.current_intent = text.clone();
                // Generate Sigil (Using generic Kamea logic if possible)
                // For now, we mock the seed logic or use kamea if it exposes it.
                // Assuming we can just make up a seed from text hash
                let mut hasher = sha2::Sha256::new();
                use sha2::Digest;
                hasher.update(text.as_bytes());
                let result = hasher.finalize();
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&result);

                // Re-calc grid mapping based on length
                let size = if text.len() > 10 { 5 } else { 4 };
                model.config.grid_rows = size;
                model.config.grid_cols = size;
                model.config.spacing = 600.0 / (size as f32);

                model.path_points = kamea::generate_path(seed, model.config)
                    .into_iter()
                    .map(|(x, y)| pt2(x, y))
                    .collect();
            },
            Signal::Astrology { sun_sign, moon_sign, .. } => {
                model.astro_data = format!("Sun: {} | Moon: {}", sun_sign, moon_sign);
            },
            Signal::Pulse => {}, // Heartbeat
            _ => {}
        }
    }
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    draw.background().color(BLACK);

    // Header
    draw.text("TALISMAN // PATCH BAY")
        .xy(pt2(0.0, 360.0))
        .color(WHITE)
        .font_size(16);

    // Astro Data
    draw.text(&model.astro_data)
        .xy(pt2(0.0, 340.0))
        .color(GRAY)
        .font_size(12);

    // Intent
    draw.text(&model.current_intent)
        .xy(pt2(0.0, -350.0))
        .color(CYAN)
        .font_size(14);

    // Sigil
    if !model.path_points.is_empty() {
        draw.polyline()
            .weight(model.config.stroke_weight)
            .join_round()
            .caps_round()
            .points(model.path_points.clone())
            .color(CYAN);
            
         // Dots for start/end
         if let Some(p) = model.path_points.first() {
             draw.ellipse().xy(*p).radius(5.0).color(CYAN);
         }
         if let Some(p) = model.path_points.last() {
             draw.rect().xy(*p).w_h(10.0, 10.0).color(CYAN);
         }
    }

    draw.to_frame(app, &frame).unwrap();
}