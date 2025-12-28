//! Kamea Tile - Generative sigil visualization
//!
//! Creates visual sigils from text input using SHA256 hash
//! and random walk on a grid (Digital Kamea method).
//!
//! Monitor mode: Displays the current sigil
//! Control mode: Settings for grid size, colors, stroke weight

use nannou::prelude::*;
use nannou_egui::egui;
use sha2::{Sha256, Digest};
use crate::{generate_path, SigilConfig};
use std::sync::{Arc, Mutex};
use talisman_core::{TileRenderer, RenderContext, BindableAction};

pub struct KameaTile {
    current_text: Arc<Mutex<String>>,
    path_points: Vec<Point2>,
    config: SigilConfig,
    last_text_hash: [u8; 32],
    show_grid_dots: bool,
    glow_intensity: f32,
    path_color: (f32, f32, f32), // RGB 0-1
}

impl KameaTile {
    pub fn new() -> Self {
        Self {
            current_text: Arc::new(Mutex::new(String::new())),
            path_points: Vec::new(),
            config: SigilConfig {
                spacing: 40.0,
                stroke_weight: 2.0,
                grid_rows: 4,
                grid_cols: 4,
            },
            last_text_hash: [0u8; 32],
            show_grid_dots: true,
            glow_intensity: 0.2,
            path_color: (0.0, 1.0, 1.0), // Cyan default
        }
    }
    
    /// Get the shared text buffer for external input
    pub fn get_text_buffer(&self) -> Arc<Mutex<String>> {
        self.current_text.clone()
    }
    
    /// Set the input text (triggers regeneration on next update)
    pub fn set_text(&self, text: &str) {
        if let Ok(mut t) = self.current_text.lock() {
            *t = text.to_string();
        }
    }
    
    fn regenerate_path(&mut self, text: &str) {
        // Hash the text
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let result = hasher.finalize();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&result);
        
        // Check if we need to regenerate
        if seed == self.last_text_hash && !self.path_points.is_empty() {
            return;
        }
        self.last_text_hash = seed;
        
        // Generate the path with current config
        self.path_points = generate_path(seed, self.config)
            .into_iter()
            .map(|(x, y)| pt2(x, y))
            .collect();
    }
    
    fn render_sigil(&self, draw: &Draw, rect: Rect) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.95));
        
        // Calculate scale to fit grid in rect
        let grid_size = self.config.grid_cols.max(self.config.grid_rows) as f32;
        let scale = (rect.w().min(rect.h()) * 0.8) / (grid_size * self.config.spacing);
        
        // Draw grid dots
        if self.show_grid_dots {
            let dot_color = srgba(0.2, 0.3, 0.4, 0.5);
            for row in 0..self.config.grid_rows {
                for col in 0..self.config.grid_cols {
                    let x = (col as f32 - (self.config.grid_cols as f32 - 1.0) / 2.0) * self.config.spacing * scale;
                    let y = (row as f32 - (self.config.grid_rows as f32 - 1.0) / 2.0) * self.config.spacing * scale;
                    draw.ellipse()
                        .xy(rect.xy() + vec2(x, y))
                        .radius(3.0)
                        .color(dot_color);
                }
            }
        }
        
        // Draw sigil path
        if self.path_points.len() >= 2 {
            let (r, g, b) = self.path_color;
            let path_color = srgb(r, g, b);
            
            for window in self.path_points.windows(2) {
                let offset = vec2(rect.x(), rect.y());
                let p0 = window[0] * scale + offset;
                let p1 = window[1] * scale + offset;
                
                // Glow effect (wider, transparent)
                if self.glow_intensity > 0.0 {
                    draw.line()
                        .start(p0)
                        .end(p1)
                        .weight(self.config.stroke_weight * 3.0)
                        .color(srgba(r, g, b, self.glow_intensity));
                }
                
                // Main line
                draw.line()
                    .start(p0)
                    .end(p1)
                    .weight(self.config.stroke_weight)
                    .color(path_color);
            }
            
            // Start marker - Circle ○
            if let Some(start) = self.path_points.first() {
                let offset = vec2(rect.x(), rect.y());
                let pos = *start * scale + offset;
                draw.ellipse()
                    .xy(pos)
                    .radius(8.0)
                    .no_fill()
                    .stroke_weight(2.0)
                    .stroke(srgb(0.0, 1.0, 0.5));
            }
            
            // End marker - Cross ×
            if let Some(end) = self.path_points.last() {
                let offset = vec2(rect.x(), rect.y());
                let pos = *end * scale + offset;
                let size = 6.0;
                draw.line()
                    .start(pos + vec2(-size, -size))
                    .end(pos + vec2(size, size))
                    .weight(2.0)
                    .color(srgb(1.0, 0.3, 0.3));
                draw.line()
                    .start(pos + vec2(size, -size))
                    .end(pos + vec2(-size, size))
                    .weight(2.0)
                    .color(srgb(1.0, 0.3, 0.3));
            }
        }
    }
}

impl Default for KameaTile {
    fn default() -> Self {
        Self::new()
    }
}

impl TileRenderer for KameaTile {
    fn id(&self) -> &str { "kamea" }
    
    fn name(&self) -> &str { "Kamea Sigil" }
    
    fn update(&mut self) {
        let text = self.current_text.lock()
            .map(|t| t.clone())
            .unwrap_or_default();
        
        if !text.is_empty() {
            self.regenerate_path(&text);
        }
    }
    
    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        self.render_sigil(draw, rect);
        
        // Label
        draw.text("KAMEA")
            .xy(pt2(rect.x(), rect.top() - 20.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(12);
        
        // Grid size indicator
        draw.text(&format!("{}×{}", self.config.grid_cols, self.config.grid_rows))
            .xy(pt2(rect.right() - 20.0, rect.top() - 20.0))
            .color(srgba(0.3, 0.3, 0.3, 0.8))
            .font_size(10);
    }
    
    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.98));
        
        // Title
        draw.text("KAMEA SIGIL SETTINGS")
            .xy(pt2(rect.x(), rect.top() - 30.0))
            .color(CYAN)
            .font_size(18);
        
        // Current intent preview
        let text = self.current_text.lock()
            .map(|t| if t.len() > 40 { format!("{}...", &t[..40]) } else { t.clone() })
            .unwrap_or_else(|_| "[No text]".to_string());
        draw.text(&format!("Intent: {}", text))
            .xy(pt2(rect.x(), rect.top() - 55.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(12);
        
        // Large sigil preview
        let preview_rect = Rect::from_x_y_w_h(
            rect.x() + rect.w() * 0.15,
            rect.y() - 20.0,
            rect.w() * 0.5,
            rect.h() * 0.5,
        );
        
        // Preview border
        draw.rect()
            .xy(preview_rect.xy())
            .wh(preview_rect.wh())
            .no_fill()
            .stroke(srgba(0.2, 0.3, 0.3, 1.0))
            .stroke_weight(1.0);
        
        self.render_sigil(draw, preview_rect.pad(5.0));
        
        // Egui controls
        let mut used_egui = false;
        if let Some(egui_ctx) = ctx.egui_ctx {
            used_egui = true;
            
            let panel_x = rect.left() + 40.0 + (rect.w() / 2.0);
            let panel_y = rect.top() - 80.0 + (rect.h() / 2.0);
            
            egui::Area::new(egui::Id::new("kamea_controls"))
                .fixed_pos(egui::pos2(panel_x, panel_y))
                .show(egui_ctx, |ui| {
                    ui.set_max_width(280.0);
                    
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgba_unmultiplied(10, 10, 15, 240))
                        .inner_margin(egui::Margin::same(15.0))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 100, 100)))
                        .show(ui, |ui| {
                            ui.heading(egui::RichText::new("Sigil Settings").color(egui::Color32::from_rgb(0, 255, 255)));
                            ui.add_space(10.0);
                            
                            // Grid size
                            ui.label(egui::RichText::new("Grid Size").color(egui::Color32::GRAY).small());
                            let mut grid_size = self.config.grid_rows;
                            ui.add(egui::Slider::new(&mut grid_size, 3..=9).text("×"));
                            
                            ui.add_space(8.0);
                            
                            // Stroke weight
                            ui.label(egui::RichText::new("Line Thickness").color(egui::Color32::GRAY).small());
                            let mut weight = self.config.stroke_weight;
                            ui.add(egui::Slider::new(&mut weight, 0.5..=8.0));
                            
                            ui.add_space(8.0);
                            
                            // Glow intensity
                            ui.label(egui::RichText::new("Glow Intensity").color(egui::Color32::GRAY).small());
                            let mut glow = self.glow_intensity;
                            ui.add(egui::Slider::new(&mut glow, 0.0..=0.5));
                            
                            ui.add_space(8.0);
                            
                            // Show grid dots
                            let mut dots = self.show_grid_dots;
                            ui.checkbox(&mut dots, "Show Grid Dots");
                            
                            ui.add_space(15.0);
                            ui.separator();
                            ui.add_space(10.0);
                            
                            // Keybindings
                            ui.label(egui::RichText::new("Keybindings").color(egui::Color32::from_rgb(0, 255, 255)));
                            for action in self.bindable_actions() {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&action.label).color(egui::Color32::LIGHT_GRAY));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mut key = String::from("_");
                                        ui.add(egui::TextEdit::singleline(&mut key).desired_width(30.0));
                                    });
                                });
                            }
                        });
                });
        }
        
        used_egui
    }
    
    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "grid_size": {
                    "type": "integer",
                    "minimum": 3,
                    "maximum": 9,
                    "default": 4,
                    "title": "Grid Size"
                },
                "stroke_weight": {
                    "type": "number",
                    "minimum": 0.5,
                    "maximum": 8.0,
                    "default": 2.0,
                    "title": "Line Thickness"
                },
                "glow_intensity": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 0.5,
                    "default": 0.2,
                    "title": "Glow Intensity"
                },
                "show_grid_dots": {
                    "type": "boolean",
                    "default": true,
                    "title": "Show Grid Dots"
                },
                "path_color": {
                    "type": "array",
                    "items": { "type": "number" },
                    "default": [0.0, 1.0, 1.0],
                    "title": "Path Color (RGB)"
                }
            }
        }))
    }
    
    fn apply_settings(&mut self, settings: &serde_json::Value) {
        if let Some(size) = settings.get("grid_size").and_then(|v| v.as_i64()) {
            let size = (size as usize).clamp(3, 9);
            self.config.grid_rows = size;
            self.config.grid_cols = size;
            // Force regeneration
            self.last_text_hash = [0u8; 32];
        }
        if let Some(weight) = settings.get("stroke_weight").and_then(|v| v.as_f64()) {
            self.config.stroke_weight = (weight as f32).clamp(0.5, 8.0);
        }
        if let Some(glow) = settings.get("glow_intensity").and_then(|v| v.as_f64()) {
            self.glow_intensity = (glow as f32).clamp(0.0, 0.5);
        }
        if let Some(dots) = settings.get("show_grid_dots").and_then(|v| v.as_bool()) {
            self.show_grid_dots = dots;
        }
        if let Some(color) = settings.get("path_color").and_then(|v| v.as_array()) {
            if color.len() >= 3 {
                self.path_color = (
                    color[0].as_f64().unwrap_or(0.0) as f32,
                    color[1].as_f64().unwrap_or(1.0) as f32,
                    color[2].as_f64().unwrap_or(1.0) as f32,
                );
            }
        }
    }
    
    fn get_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "grid_size": self.config.grid_rows,
            "stroke_weight": self.config.stroke_weight,
            "glow_intensity": self.glow_intensity,
            "show_grid_dots": self.show_grid_dots,
            "path_color": [self.path_color.0, self.path_color.1, self.path_color.2]
        })
    }
    
    fn bindable_actions(&self) -> Vec<BindableAction> {
        vec![
            BindableAction::new("toggle_dots", "Toggle Grid Dots", true),
            BindableAction::new("increase_grid", "Increase Grid Size", false),
            BindableAction::new("decrease_grid", "Decrease Grid Size", false),
            BindableAction::new("regenerate", "Regenerate Sigil", false),
        ]
    }
    
    fn execute_action(&mut self, action: &str) -> bool {
        match action {
            "toggle_dots" => {
                self.show_grid_dots = !self.show_grid_dots;
                true
            },
            "increase_grid" => {
                if self.config.grid_rows < 9 {
                    self.config.grid_rows += 1;
                    self.config.grid_cols += 1;
                    self.last_text_hash = [0u8; 32]; // Force regeneration
                    true
                } else {
                    false
                }
            },
            "decrease_grid" => {
                if self.config.grid_rows > 3 {
                    self.config.grid_rows -= 1;
                    self.config.grid_cols -= 1;
                    self.last_text_hash = [0u8; 32]; // Force regeneration
                    true
                } else {
                    false
                }
            },
            "regenerate" => {
                self.last_text_hash = [0u8; 32]; // Force regeneration
                true
            },
            _ => false,
        }
    }
    
    fn get_display_text(&self) -> Option<String> {
        self.current_text.lock().ok().map(|t| t.clone())
    }
}
