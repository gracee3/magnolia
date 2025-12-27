use nannou::prelude::*;
use std::io::{self, Write};
use std::sync::OnceLock;
use std::time::Instant;
use sysinfo::System;

use kamea::{self, SigilConfig};

// --- GLOBAL CONTEXT ---
static SIGIL_CONTEXT: OnceLock<SigilContext> = OnceLock::new();

#[derive(Debug)]
struct SigilContext {
    config: SigilConfig,
    _intent: String,
    _hash_hex: String,
    seed_bytes: [u8; 32],
}

// --- CONSTANTS ---
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";

// --- ENTRY POINT ---
fn main() {
    let ctx = match boot_sequence() {
        Ok(c) => c,
        Err(_) => return,
    };
    let _ = SIGIL_CONTEXT.set(ctx);

    nannou::app(model)
        .update(update)
        .exit(bsod_exit)
        .run();
}

// --- BOOT & CONFIG ---
fn boot_sequence() -> io::Result<SigilContext> {
    let mut stdout = io::stdout();
    let sys = System::new_all();
    let pid = std::process::id();
    
    writeln!(stdout, "{}SIGIL KERNEL INITIALIZED. PID: {}{}", GREEN, pid, RESET)?;
    if let Some(cpu) = sys.cpus().first() {
        log_tech(&mut stdout, "CPU_BRAND", cpu.brand())?;
    }
    
    // 1. HARDWARE ENGAGEMENT
    log_tech(&mut stdout, "HARDWARE_CTRL", "ENGAGING CAPS_LOCK MECHANISM")?;
    std::panic::catch_unwind(|| {
        logos::engage_hardware();
    }).ok();

    // 2. INTENT
    print!("\n{}AWAITING INTENT STREAM > {}", CYAN, RESET);
    stdout.flush()?;
    
    let mut input = String::new();
    let start_read = Instant::now();
    io::stdin().read_line(&mut input)?;
    let read_duration = start_read.elapsed();

    // Disengage Caps Lock
    std::panic::catch_unwind(|| {
        logos::disengage_hardware();
    }).ok();
    
    let intent = input.trim().to_uppercase(); 
    if intent.is_empty() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "Null Intent")); }

    log_tech(&mut stdout, "INPUT_LATENCY", &format!("{} ns", read_duration.as_nanos()))?;
    log_tech(&mut stdout, "SEMANTIC_LOCK", &format!("INTENT DETECTED: {}", intent))?;

    // 3. ASTRO SALTING
    log_tech(&mut stdout, "MODULE_LOAD", "APHRODITE_CORE_V2")?;
    let salt = aphrodite::get_astro_salt();
    log_tech(&mut stdout, "ASTRO_CORE", &format!("SALT: {}", salt))?;

    // 4. KAMEA SELECTION
    let sphere = logos::determine_sphere(&intent);
    let size = sphere as usize;
    log_tech(&mut stdout, "KAMEA_SELECT", &format!("{:?} ({}x{})", sphere, size, size))?;

    // 5. CRYPTO HASHING
    let (hash_hex, seed_bytes) = logos::semantic_hash(&intent, &salt);
    
    log_tech(&mut stdout, "SEMANTIC_HASH", &hash_hex[..16])?;
    log_tech(&mut stdout, "GENERATOR", "TRACING KAMEA GRID PATH...")?;

    let config = SigilConfig {
        spacing: 600.0 / (size as f32),
        stroke_weight: 8.0,
        grid_rows: size,
        grid_cols: size,
    };

    Ok(SigilContext {
        config,
        _intent: intent,
        _hash_hex: hash_hex, 
        seed_bytes,
    })
}

fn log_tech(w: &mut io::Stdout, key: &str, val: &str) -> io::Result<()> {
    writeln!(w, "[{}] {}: {}", chrono::Utc::now().format("%H:%M:%S%.6f"), key, val)
}

// --- KAMEA MODEL ---

struct Model {
    path_points: Vec<Point2>,
    inverted: bool,
    config: SigilConfig,
}

fn model(app: &App) -> Model {
    let ctx = SIGIL_CONTEXT.get().expect("Init");
    
    app.new_window()
        .title("SIGIL // KAMEA TRACE")
        .size(800, 800)
        .view(view)
        .key_pressed(key_pressed)
        .build()
        .unwrap();

    let points_raw = kamea::generate_path(ctx.seed_bytes, ctx.config);
    let points: Vec<Point2> = points_raw.into_iter().map(|(x, y)| pt2(x, y)).collect();

    println!(">> KAMEA PATH GENERATED. LENGTH: {}", points.len());

    Model {
        path_points: points,
        inverted: false,
        config: ctx.config,
    }
}

fn update(_app: &App, _model: &mut Model, _update: Update) {
    // STATIC SEAL.
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();

    let (bg, fg) = if model.inverted {
        (WHITE, BLACK)
    } else {
        (BLACK, nannou::prelude::CYAN)
    };

    draw.background().color(bg);

    // 1. DRAW PATH (The Sigil)
    draw.polyline()
        .weight(model.config.stroke_weight)
        .join_round()
        .caps_round()
        .points(model.path_points.clone())
        .color(fg);

    // 2. START TERMINAL (Circle)
    if let Some(start) = model.path_points.first() {
        draw.ellipse()
            .xy(*start)
            .radius(12.0)
            .stroke(fg)
            .stroke_weight(4.0)
            .no_fill();
    }

    // 3. END TERMINAL (Crossbar)
    if let Some(end) = model.path_points.last() {
        let size = 15.0;
        
        // Vertical
        draw.line()
            .start(pt2(end.x, end.y - size))
            .end(pt2(end.x, end.y + size))
            .weight(6.0)
            .color(fg);
            
        // Horizontal
        draw.line()
            .start(pt2(end.x - size, end.y))
            .end(pt2(end.x + size, end.y))
            .weight(6.0)
            .color(fg);
    }

    draw.to_frame(app, &frame).unwrap();
}

fn key_pressed(app: &App, model: &mut Model, key: Key) {
    match key {
        Key::Space => {
            model.inverted = !model.inverted;
        },
        Key::Escape => {
            app.quit();
        },
        _ => {}
    }
}

fn bsod_exit(_app: &App, _model: Model) {
    println!("\n{}SIGNAL INTERRUPT DETECTED. COMMENCING CLEANUP.{}", RED, RESET);
    println!("[{}] MEMORY_ZEROING_START", chrono::Utc::now().format("%H:%M:%S%.6f"));
    
    // Simulate real memory scrubbing
    let pool_size = 4096 * 64;
    let mut dummy_mem = vec![0u8; pool_size];
    for i in &mut dummy_mem { *i = 0xFF; }
    for i in &mut dummy_mem { *i = 0x00; }
    
    println!("[{}] MEMORY_ZEROING_COMPLETE.", chrono::Utc::now().format("%H:%M:%S%.6f"));
    println!("{}[SYSTEM HALT]{}", RED, RESET);
}