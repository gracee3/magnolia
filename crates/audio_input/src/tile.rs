//! Audio Visualization Tile
//!
//! GPU-accelerated audio visualization with multiple display modes.
//! Each tile instance can be configured with different visualization types
//! and color schemes via the settings modal (maximized view).
//!
//! Uses SPSC ring buffer for minimal latency audio streaming.

use nannou::prelude::*;
use nannou_egui::egui;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use talisman_core::{AudioFrame, ring_buffer::RingBufferReceiver};
use talisman_core::{TileRenderer, RenderContext, BindableAction, TileError};

/// Available visualization types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisualizationType {
    Oscilloscope,
    SpectrumBars,
    SpectrumLine,
    VuMeter,
    Lissajous,
}

impl Default for VisualizationType {
    fn default() -> Self {
        Self::Oscilloscope
    }
}

impl VisualizationType {
    pub fn all() -> &'static [VisualizationType] {
        &[
            VisualizationType::Oscilloscope,
            VisualizationType::SpectrumBars,
            VisualizationType::SpectrumLine,
            VisualizationType::VuMeter,
            VisualizationType::Lissajous,
        ]
    }
    
    pub fn label(&self) -> &'static str {
        match self {
            VisualizationType::Oscilloscope => "Oscilloscope",
            VisualizationType::SpectrumBars => "Spectrum Bars",
            VisualizationType::SpectrumLine => "Spectrum Line",
            VisualizationType::VuMeter => "VU Meter",
            VisualizationType::Lissajous => "Lissajous",
        }
    }
    
    pub fn next(&self) -> Self {
        match self {
            VisualizationType::Oscilloscope => VisualizationType::SpectrumBars,
            VisualizationType::SpectrumBars => VisualizationType::SpectrumLine,
            VisualizationType::SpectrumLine => VisualizationType::VuMeter,
            VisualizationType::VuMeter => VisualizationType::Lissajous,
            VisualizationType::Lissajous => VisualizationType::Oscilloscope,
        }
    }
}

/// Color scheme options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorScheme {
    CyanReactive,    // Default - cyan with brightness reactive to amplitude
    GreenScope,      // Classic oscilloscope green
    Rainbow,         // Frequency-mapped rainbow
    Monochrome,      // White/gray
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::CyanReactive
    }
}

impl ColorScheme {
    pub fn all() -> &'static [ColorScheme] {
        &[
            ColorScheme::CyanReactive,
            ColorScheme::GreenScope,
            ColorScheme::Rainbow,
            ColorScheme::Monochrome,
        ]
    }
    
    pub fn label(&self) -> &'static str {
        match self {
            ColorScheme::CyanReactive => "Cyan Reactive",
            ColorScheme::GreenScope => "Green Scope",
            ColorScheme::Rainbow => "Rainbow",
            ColorScheme::Monochrome => "Monochrome",
        }
    }
}

/// Audio visualization tile with configurable display modes
pub struct AudioVisTile {
    /// Unique instance ID
    instance_id: String,
    
    /// Current visualization type
    vis_type: VisualizationType,
    
    /// Current color scheme
    color_scheme: ColorScheme,
    
    /// Sensitivity multiplier (0.1 - 5.0)
    sensitivity: f32,
    
    /// Whether audio is muted (visualization paused)
    is_muted: bool,
    
    /// Whether display is frozen (shows last captured frame)
    is_frozen: bool,
    
    /// Ring buffer receiver for audio frames (SPSC, minimal latency)
    ring_rx: Option<RingBufferReceiver<AudioFrame>>,
    
    /// Local buffer for visualization (mono samples)
    buffer: Vec<f32>,
    
    /// Left channel buffer (for stereo/Lissajous)
    left_buffer: Vec<f32>,
    
    /// Right channel buffer (for stereo/Lissajous)
    right_buffer: Vec<f32>,
    
    /// Frozen buffer snapshot
    frozen_buffer: Vec<f32>,
    
    /// Current error state
    error: Option<TileError>,
    
    /// Fallback Arc<Mutex> buffer for when ring buffer isn't connected
    legacy_buffer: Arc<Mutex<Vec<f32>>>,
}

const BUFFER_SIZE: usize = 2048;

impl AudioVisTile {
    pub fn new(id: &str) -> Self {
        Self {
            instance_id: id.to_string(),
            vis_type: VisualizationType::default(),
            color_scheme: ColorScheme::default(),
            sensitivity: 1.0,
            is_muted: false,
            is_frozen: false,
            ring_rx: None,
            buffer: vec![0.0; BUFFER_SIZE],
            left_buffer: vec![0.0; BUFFER_SIZE / 2],
            right_buffer: vec![0.0; BUFFER_SIZE / 2],
            frozen_buffer: Vec::new(),
            error: Some(TileError::info("No audio connected")),
            legacy_buffer: Arc::new(Mutex::new(vec![0.0; BUFFER_SIZE])),
        }
    }
    
    /// Connect a ring buffer receiver for real-time audio streaming
    /// 
    /// This uses the SPSC ring buffer for minimal latency (~5-10ns per frame)
    pub fn connect_audio_stream(&mut self, receiver: RingBufferReceiver<AudioFrame>) {
        self.ring_rx = Some(receiver);
        self.error = None; // Clear error when connected
        log::info!("AudioVisTile {}: connected to audio stream", self.instance_id);
    }
    
    /// Check if audio stream is connected
    pub fn is_connected(&self) -> bool {
        self.ring_rx.is_some()
    }
    
    /// Get the legacy shared buffer for fallback audio input
    pub fn get_legacy_buffer(&self) -> Arc<Mutex<Vec<f32>>> {
        self.legacy_buffer.clone()
    }
    
    /// Set error state
    pub fn set_error(&mut self, error: TileError) {
        self.error = Some(error);
    }
    
    /// Get current color based on scheme and amplitude
    fn get_color(&self, amplitude: f32) -> LinSrgba {
        match self.color_scheme {
            ColorScheme::CyanReactive => {
                let brightness = (0.5 + amplitude.abs() * self.sensitivity * 0.5).min(1.0);
                LinSrgba::new(0.0, brightness, brightness, 1.0)
            },
            ColorScheme::GreenScope => {
                LinSrgba::new(0.2, 1.0, 0.3, 1.0)
            },
            ColorScheme::Rainbow => {
                // Full saturation rainbow
                let hue = (amplitude.abs() * self.sensitivity).min(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                LinSrgba::new(r, g, b, 1.0)
            },
            ColorScheme::Monochrome => {
                LinSrgba::new(0.9, 0.9, 0.9, 1.0)
            },
        }
    }
    
    /// Get current buffer (live or frozen)
    fn get_current_buffer(&self) -> &[f32] {
        if self.is_frozen {
            &self.frozen_buffer
        } else if self.is_muted {
            &[]
        } else {
            &self.buffer
        }
    }
    
    /// Get stereo buffers for Lissajous display
    fn get_stereo_buffers(&self) -> (&[f32], &[f32]) {
        (&self.left_buffer, &self.right_buffer)
    }
    
    /// Poll audio from ring buffer and update local buffers
    fn poll_audio(&mut self) {
        if let Some(ref rx) = self.ring_rx {
            let mut samples_received = 0;
            
            // Drain ring buffer, keeping most recent samples
            while let Some(frame) = rx.try_recv() {
                // Shift buffers left
                if samples_received < BUFFER_SIZE {
                    let idx = samples_received % BUFFER_SIZE;
                    self.buffer[idx] = (frame.left + frame.right) * 0.5; // Mono mix
                    
                    if samples_received < BUFFER_SIZE / 2 {
                        self.left_buffer[samples_received] = frame.left;
                        self.right_buffer[samples_received] = frame.right;
                    }
                }
                samples_received += 1;
            }
            
            // If no samples received for a while, that could indicate a problem
            // (but we won't error immediately - audio might just be silent)
        } else {
            // Fall back to legacy buffer
            if let Ok(buf) = self.legacy_buffer.lock() {
                let len = buf.len().min(BUFFER_SIZE);
                self.buffer[..len].copy_from_slice(&buf[..len]);
            }
        }
    }
}

impl Default for AudioVisTile {
    fn default() -> Self {
        Self::new("audio_vis")
    }
}

impl TileRenderer for AudioVisTile {
    fn id(&self) -> &str { &self.instance_id }
    fn name(&self) -> &str { "Audio Visualizer" }
    fn prefers_gpu(&self) -> bool { true }
    
    fn render_monitor(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.95));
        
        let buffer = self.get_current_buffer();
        let avg_amp = buffer.iter().map(|s| s.abs()).sum::<f32>() / buffer.len().max(1) as f32;
        let color = self.get_color(avg_amp);
        
        // Render visualization using GPU renderer if available
        let content_rect = rect.pad(5.0);
        
        // Software rendering fallback (GPU rendering removed for now)
        let points: Vec<Point2> = buffer.iter().enumerate().map(|(i, &sample)| {
            let x = map_range(i, 0, buffer.len(), content_rect.left(), content_rect.right());
            let y = content_rect.y() + sample * content_rect.h() * 0.4 * self.sensitivity;
            pt2(x, y)
        }).collect();
        
        if !points.is_empty() {
            draw.polyline()
                .weight(2.0)
                .points(points)
                .color(color);
        }
        
        // Status indicators (monitor mode - read only)
        let status_y = rect.top() - 12.0;
        draw.text(self.vis_type.label())
            .xy(pt2(rect.x(), status_y))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(10);
        
        // State indicators
        let mut indicator_x = rect.right() - 35.0;
        if self.is_muted {
            draw.text("MUTE")
                .xy(pt2(indicator_x, status_y))
                .color(srgba(1.0, 0.3, 0.3, 0.8))
                .font_size(9);
            indicator_x -= 35.0;
        }
        if self.is_frozen {
            draw.text("FREEZE")
                .xy(pt2(indicator_x, status_y))
                .color(srgba(0.3, 0.5, 1.0, 0.8))
                .font_size(9);
        }
    }
    
    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        // Full background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.98));
        
        // Title
        draw.text("AUDIO VISUALIZER")
            .xy(pt2(rect.x(), rect.top() - 30.0))
            .color(CYAN)
            .font_size(18);
        
        // Subtitle with current settings
        let subtitle = format!("{} | {}", self.vis_type.label(), self.color_scheme.label());
        draw.text(&subtitle)
            .xy(pt2(rect.x(), rect.top() - 50.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(12);
        
        // Preview area (right side)
        let preview_rect = Rect::from_x_y_w_h(
            rect.x() + rect.w() * 0.15,
            rect.y() - 20.0,
            rect.w() * 0.65,
            rect.h() * 0.5,
        );
        
        // Preview border
        draw.rect()
            .xy(preview_rect.xy())
            .wh(preview_rect.wh())
            .no_fill()
            .stroke(srgba(0.2, 0.3, 0.3, 1.0))
            .stroke_weight(1.0);
        
        draw.text("LIVE PREVIEW")
            .xy(pt2(preview_rect.x(), preview_rect.top() + 15.0))
            .color(srgba(0.3, 0.3, 0.3, 1.0))
            .font_size(10);
        
        // Render live preview
        self.render_monitor(draw, preview_rect.pad(2.0), ctx);
        
        // Egui controls
        let mut used_egui = false;
        if let Some(egui_ctx) = ctx.egui_ctx {
            used_egui = true;
            
            let panel_x = rect.left() + 40.0 + (rect.w() / 2.0);
            let panel_y = rect.top() - 80.0 + (rect.h() / 2.0);
            
            egui::Area::new(egui::Id::new(format!("{}_controls", self.instance_id)))
                .fixed_pos(egui::pos2(panel_x, panel_y))
                .show(egui_ctx, |ui| {
                    ui.set_max_width(280.0);
                    
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgba_unmultiplied(10, 10, 15, 240))
                        .inner_margin(egui::Margin::same(15.0))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 100, 100)))
                        .show(ui, |ui| {
                            ui.heading(egui::RichText::new("Settings").color(egui::Color32::from_rgb(0, 255, 255)));
                            ui.add_space(10.0);
                            
                            // Visualization Type
                            ui.label(egui::RichText::new("Visualization").color(egui::Color32::GRAY).small());
                            egui::ComboBox::from_id_source("vis_type_select")
                                .selected_text(self.vis_type.label())
                                .width(200.0)
                                .show_ui(ui, |ui| {
                                    for vt in VisualizationType::all() {
                                        ui.selectable_label(self.vis_type == *vt, vt.label());
                                    }
                                });
                            
                            ui.add_space(8.0);
                            
                            // Color Scheme
                            ui.label(egui::RichText::new("Color Scheme").color(egui::Color32::GRAY).small());
                            egui::ComboBox::from_id_source("color_scheme_select")
                                .selected_text(self.color_scheme.label())
                                .width(200.0)
                                .show_ui(ui, |ui| {
                                    for cs in ColorScheme::all() {
                                        ui.selectable_label(self.color_scheme == *cs, cs.label());
                                    }
                                });
                            
                            ui.add_space(8.0);
                            
                            // Sensitivity
                            ui.label(egui::RichText::new("Sensitivity").color(egui::Color32::GRAY).small());
                            let mut sens = self.sensitivity;
                            ui.add(egui::Slider::new(&mut sens, 0.1..=5.0).show_value(true));
                            
                            ui.add_space(15.0);
                            ui.separator();
                            ui.add_space(10.0);
                            
                            // Keybindings section
                            ui.label(egui::RichText::new("Keybindings").color(egui::Color32::from_rgb(0, 255, 255)));
                            ui.add_space(5.0);
                            
                            for action in self.bindable_actions() {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&action.label).color(egui::Color32::LIGHT_GRAY));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mut key = String::from("_");
                                        ui.add(egui::TextEdit::singleline(&mut key).desired_width(30.0));
                                    });
                                });
                            }
                            
                            ui.add_space(15.0);
                            
                            // State toggles
                            ui.horizontal(|ui| {
                                let mut muted = self.is_muted;
                                if ui.checkbox(&mut muted, "Mute").changed() {
                                    // Note: would need interior mutability to actually change
                                }
                                let mut frozen = self.is_frozen;
                                if ui.checkbox(&mut frozen, "Freeze").changed() {
                                    // Note: would need interior mutability to actually change
                                }
                            });
                        });
                });
        }
        
        used_egui
    }
    
    fn update(&mut self) {
        // Poll audio from ring buffer (or legacy buffer)
        if !self.is_frozen && !self.is_muted {
            self.poll_audio();
        }
    }
    
    fn get_error(&self) -> Option<TileError> {
        self.error.clone()
    }
    
    fn clear_error(&mut self) {
        self.error = None;
    }
    
    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "vis_type": {
                    "type": "string",
                    "enum": ["Oscilloscope", "SpectrumBars", "SpectrumLine", "VuMeter", "Lissajous"],
                    "default": "Oscilloscope",
                    "title": "Visualization Type"
                },
                "color_scheme": {
                    "type": "string", 
                    "enum": ["CyanReactive", "GreenScope", "Rainbow", "Monochrome"],
                    "default": "CyanReactive",
                    "title": "Color Scheme"
                },
                "sensitivity": {
                    "type": "number",
                    "minimum": 0.1,
                    "maximum": 5.0,
                    "default": 1.0,
                    "title": "Sensitivity"
                }
            }
        }))
    }
    
    fn apply_settings(&mut self, settings: &serde_json::Value) {
        if let Some(vt) = settings.get("vis_type").and_then(|v| v.as_str()) {
            self.vis_type = match vt {
                "Oscilloscope" => VisualizationType::Oscilloscope,
                "SpectrumBars" => VisualizationType::SpectrumBars,
                "SpectrumLine" => VisualizationType::SpectrumLine,
                "VuMeter" => VisualizationType::VuMeter,
                "Lissajous" => VisualizationType::Lissajous,
                _ => VisualizationType::Oscilloscope,
            };
        }
        if let Some(cs) = settings.get("color_scheme").and_then(|v| v.as_str()) {
            self.color_scheme = match cs {
                "CyanReactive" => ColorScheme::CyanReactive,
                "GreenScope" => ColorScheme::GreenScope,
                "Rainbow" => ColorScheme::Rainbow,
                "Monochrome" => ColorScheme::Monochrome,
                _ => ColorScheme::CyanReactive,
            };
        }
        if let Some(s) = settings.get("sensitivity").and_then(|v| v.as_f64()) {
            self.sensitivity = (s as f32).clamp(0.1, 5.0);
        }
    }
    
    fn get_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "vis_type": format!("{:?}", self.vis_type),
            "color_scheme": format!("{:?}", self.color_scheme),
            "sensitivity": self.sensitivity
        })
    }
    
    fn bindable_actions(&self) -> Vec<BindableAction> {
        vec![
            BindableAction::new("mute", "Mute", true),
            BindableAction::new("freeze", "Freeze", true),
            BindableAction::new("next_vis", "Next Visualization", false),
        ]
    }
    
    fn execute_action(&mut self, action: &str) -> bool {
        match action {
            "mute" => { 
                self.is_muted = !self.is_muted; 
                log::info!("Audio vis mute: {}", self.is_muted);
                true 
            },
            "freeze" => {
                if !self.is_frozen {
                    // Capture current buffer
                    self.frozen_buffer = self.buffer.clone();
                }
                self.is_frozen = !self.is_frozen;
                log::info!("Audio vis freeze: {}", self.is_frozen);
                true
            },
            "next_vis" => {
                self.vis_type = self.vis_type.next();
                log::info!("Audio vis type: {:?}", self.vis_type);
                true
            },
            _ => false,
        }
    }
    
    fn get_display_text(&self) -> Option<String> {
        Some(format!("{} ({})", self.vis_type.label(), self.color_scheme.label()))
    }
}

/// Convert HSV to RGB
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    
    match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}
