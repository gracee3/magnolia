//! Audio Visualization Tile
//!
//! GPU-accelerated audio visualization with multiple display modes.
//! Each tile instance can be configured with different visualization types
//! and color schemes via the settings modal (maximized view).
//!
//! Uses SPSC ring buffer for minimal latency audio streaming.

#[cfg(feature = "tile-rendering")]
use nannou::prelude::*;
#[cfg(feature = "tile-rendering")]
use rustfft::num_complex::Complex;
#[cfg(feature = "tile-rendering")]
use rustfft::FftPlanner;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc as StdArc;
use std::sync::{Arc, Mutex};
#[cfg(feature = "tile-rendering")]
use talisman_core::{BindableAction, RenderContext, TileError, TileRenderer};
use talisman_signals::ring_buffer::RingBufferReceiver;
#[cfg(feature = "tile-rendering")]
use talisman_ui::{draw_text, FontId, TextAlignment};

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
    CyanReactive, // Default - cyan with brightness reactive to amplitude
    GreenScope,   // Classic oscilloscope green
    Rainbow,      // Frequency-mapped rainbow
    Monochrome,   // White/gray
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

    /// Ring buffer receiver for audio samples (SPSC, f32 stream)
    ring_rx: Option<RingBufferReceiver<f32>>,

    /// Number of channels in the stream
    channels: u16,

    /// Local buffer for visualization (mono samples)
    buffer: Vec<f32>,

    /// Left channel buffer (for stereo/Lissajous)
    left_buffer: Vec<f32>,

    /// Right channel buffer (for stereo/Lissajous)
    right_buffer: Vec<f32>,

    /// Frozen buffer snapshot
    frozen_buffer: Vec<f32>,

    /// FFT working buffer (complex)
    #[cfg(feature = "tile-rendering")]
    fft_buffer: Vec<Complex<f32>>,

    /// Spectrum magnitudes (half-size)
    #[cfg(feature = "tile-rendering")]
    spectrum: Vec<f32>,

    /// FFT planner and cached plan
    #[cfg(feature = "tile-rendering")]
    fft_planner: FftPlanner<f32>,
    #[cfg(feature = "tile-rendering")]
    fft_plan: Option<StdArc<dyn rustfft::Fft<f32>>>,

    /// Window function
    #[cfg(feature = "tile-rendering")]
    window: Vec<f32>,

    /// Current error state
    #[cfg(feature = "tile-rendering")]
    error: Option<TileError>,

    /// Fallback Arc<Mutex> buffer for when ring buffer isn't connected
    legacy_buffer: Arc<Mutex<Vec<f32>>>,

    /// Optional latency meter (microseconds)
    latency_us: Option<Arc<AtomicU64>>,
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
            channels: 2,
            buffer: vec![0.0; BUFFER_SIZE],
            left_buffer: vec![0.0; BUFFER_SIZE / 2],
            right_buffer: vec![0.0; BUFFER_SIZE / 2],
            frozen_buffer: Vec::new(),
            #[cfg(feature = "tile-rendering")]
            fft_buffer: vec![Complex::new(0.0, 0.0); BUFFER_SIZE],
            #[cfg(feature = "tile-rendering")]
            spectrum: vec![0.0; BUFFER_SIZE / 2],
            #[cfg(feature = "tile-rendering")]
            fft_planner: FftPlanner::new(),
            #[cfg(feature = "tile-rendering")]
            fft_plan: None,
            #[cfg(feature = "tile-rendering")]
            window: Vec::new(),
            #[cfg(feature = "tile-rendering")]
            error: Some(TileError::info("No audio connected")),
            legacy_buffer: Arc::new(Mutex::new(vec![0.0; BUFFER_SIZE])),
            latency_us: None,
        }
    }

    pub fn connect_audio_stream(&mut self, receiver: RingBufferReceiver<f32>, channels: u16) {
        self.ring_rx = Some(receiver);
        self.channels = channels;
        #[cfg(feature = "tile-rendering")]
        {
            self.error = None; // Clear error when connected
        }
        log::info!(
            "AudioVisTile {}: connected to audio stream ({} ch)",
            self.instance_id,
            channels
        );
    }

    pub fn is_connected(&self) -> bool {
        self.ring_rx.is_some()
    }

    pub fn get_legacy_buffer(&self) -> Arc<Mutex<Vec<f32>>> {
        self.legacy_buffer.clone()
    }

    pub fn connect_latency_meter(&mut self, latency_us: Arc<AtomicU64>) {
        self.latency_us = Some(latency_us);
    }

    #[cfg(feature = "tile-rendering")]
    pub fn set_error(&mut self, error: TileError) {
        self.error = Some(error);
    }

    #[cfg(feature = "tile-rendering")]
    fn get_color(&self, amplitude: f32) -> LinSrgba {
        match self.color_scheme {
            ColorScheme::CyanReactive => {
                let brightness = (0.5 + amplitude.abs() * self.sensitivity * 0.5).min(1.0);
                LinSrgba::new(0.0, brightness, brightness, 1.0)
            }
            ColorScheme::GreenScope => LinSrgba::new(0.2, 1.0, 0.3, 1.0),
            ColorScheme::Rainbow => {
                let hue = (amplitude.abs() * self.sensitivity).min(1.0);
                let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                LinSrgba::new(r, g, b, 1.0)
            }
            ColorScheme::Monochrome => LinSrgba::new(0.9, 0.9, 0.9, 1.0),
        }
    }

    fn get_current_buffer(&self) -> &[f32] {
        if self.is_frozen {
            &self.frozen_buffer
        } else if self.is_muted {
            &[]
        } else {
            &self.buffer
        }
    }

    fn poll_audio(&mut self) {
        if let Some(ref rx) = self.ring_rx {
            let mut frames_processed = 0;
            let max_frames = BUFFER_SIZE;

            while frames_processed < max_frames {
                match self.channels {
                    1 => {
                        if let Some(sample) = rx.try_recv() {
                            self.buffer.rotate_left(1);
                            let len = self.buffer.len();
                            self.buffer[len - 1] = sample;

                            self.left_buffer.rotate_left(1);
                            let len_l = self.left_buffer.len();
                            self.left_buffer[len_l - 1] = sample;

                            self.right_buffer.rotate_left(1);
                            let len_r = self.right_buffer.len();
                            self.right_buffer[len_r - 1] = sample;

                            frames_processed += 1;
                        } else {
                            break;
                        }
                    }
                    2 => {
                        if let Some(left) = rx.try_recv() {
                            let right = rx.try_recv().unwrap_or(0.0);
                            let mono = (left + right) * 0.5;

                            self.buffer.rotate_left(1);
                            let len = self.buffer.len();
                            self.buffer[len - 1] = mono;

                            self.left_buffer.rotate_left(1);
                            let len_l = self.left_buffer.len();
                            self.left_buffer[len_l - 1] = left;

                            self.right_buffer.rotate_left(1);
                            let len_r = self.right_buffer.len();
                            self.right_buffer[len_r - 1] = right;

                            frames_processed += 1;
                        } else {
                            break;
                        }
                    }
                    ch => {
                        let mut sum = 0.0;
                        let mut got_frame = false;

                        if let Some(s1) = rx.try_recv() {
                            sum += s1;
                            let mut s2 = 0.0;
                            for i in 1..ch {
                                let s = rx.try_recv().unwrap_or(0.0);
                                sum += s;
                                if i == 1 {
                                    s2 = s;
                                }
                            }
                            let mono = sum / ch as f32;
                            self.buffer.rotate_left(1);
                            let len = self.buffer.len();
                            self.buffer[len - 1] = mono;

                            self.left_buffer.rotate_left(1);
                            let len_l = self.left_buffer.len();
                            self.left_buffer[len_l - 1] = s1;

                            self.right_buffer.rotate_left(1);
                            let len_r = self.right_buffer.len();
                            self.right_buffer[len_r - 1] = s2;
                            got_frame = true;
                        }

                        if got_frame {
                            frames_processed += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        } else {
            if let Ok(buf) = self.legacy_buffer.lock() {
                let len = buf.len().min(BUFFER_SIZE);
                if len > 0 {
                    self.buffer[..len].copy_from_slice(&buf[..len]);
                }
            }
        }
    }

    #[cfg(feature = "tile-rendering")]
    fn ensure_fft(&mut self) {
        let n = self.buffer.len();
        if self.window.len() != n {
            self.window = (0..n)
                .map(|i| {
                    let x = i as f32 / (n as f32 - 1.0);
                    (0.5 - 0.5 * (2.0 * std::f32::consts::PI * x).cos()) as f32
                })
                .collect();
        }

        if self.fft_buffer.len() != n {
            self.fft_buffer = vec![Complex::new(0.0, 0.0); n];
        }

        if self.spectrum.len() != n / 2 {
            self.spectrum = vec![0.0; n / 2];
        }

        let needs_plan = self
            .fft_plan
            .as_ref()
            .map(|plan| plan.len() != n)
            .unwrap_or(true);
        if needs_plan {
            self.fft_plan = Some(self.fft_planner.plan_fft_forward(n));
        }
    }

    #[cfg(feature = "tile-rendering")]
    fn update_spectrum(&mut self) {
        self.ensure_fft();

        for (i, sample) in self.buffer.iter().enumerate() {
            let w = self.window.get(i).copied().unwrap_or(1.0);
            self.fft_buffer[i] = Complex::new(sample * w, 0.0);
        }

        if let Some(plan) = &self.fft_plan {
            plan.process(&mut self.fft_buffer);
        }

        let n = self.fft_buffer.len();
        for i in 0..(n / 2) {
            let mag = self.fft_buffer[i].norm();
            self.spectrum[i] = mag;
        }
    }
}

impl Default for AudioVisTile {
    fn default() -> Self {
        Self::new("audio_vis")
    }
}

#[cfg(feature = "tile-rendering")]
impl TileRenderer for AudioVisTile {
    fn id(&self) -> &str {
        &self.instance_id
    }
    fn name(&self) -> &str {
        "Audio Visualizer"
    }
    fn prefers_gpu(&self) -> bool {
        true
    }

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.95));

        let buffer = self.get_current_buffer();
        let avg_amp = buffer.iter().map(|s| s.abs()).sum::<f32>() / buffer.len().max(1) as f32;
        let color = self.get_color(avg_amp);

        let content_rect = rect.pad(5.0);

        match self.vis_type {
            VisualizationType::Oscilloscope => {
                let points: Vec<Point2> = buffer
                    .iter()
                    .enumerate()
                    .map(|(i, &sample)| {
                        let x = map_range(
                            i,
                            0,
                            buffer.len(),
                            content_rect.left(),
                            content_rect.right(),
                        );
                        let y =
                            content_rect.y() + sample * content_rect.h() * 0.4 * self.sensitivity;
                        pt2(x, y)
                    })
                    .collect();

                if !points.is_empty() {
                    draw.polyline().weight(2.0).points(points).color(color);
                }
            }
            VisualizationType::SpectrumBars | VisualizationType::SpectrumLine => {
                let spectrum = &self.spectrum;
                if spectrum.is_empty() {
                    return;
                }
                let max_mag = spectrum.iter().cloned().fold(0.0_f32, f32::max).max(1e-6);

                if self.vis_type == VisualizationType::SpectrumBars {
                    let bar_w = content_rect.w() / spectrum.len() as f32;
                    for (i, &mag) in spectrum.iter().enumerate() {
                        let norm = (mag / max_mag).min(1.0);
                        let h = norm * content_rect.h();
                        let x = content_rect.left() + i as f32 * bar_w + bar_w * 0.5;
                        draw.rect()
                            .x_y(x, content_rect.bottom() + h * 0.5)
                            .w_h(bar_w * 0.8, h)
                            .color(color);
                    }
                } else {
                    let points: Vec<Point2> = spectrum
                        .iter()
                        .enumerate()
                        .map(|(i, &mag)| {
                            let x = map_range(
                                i,
                                0,
                                spectrum.len(),
                                content_rect.left(),
                                content_rect.right(),
                            );
                            let norm = (mag / max_mag).min(1.0);
                            let y = content_rect.bottom() + norm * content_rect.h();
                            pt2(x, y)
                        })
                        .collect();
                    if !points.is_empty() {
                        draw.polyline().weight(2.0).points(points).color(color);
                    }
                }
            }
            VisualizationType::VuMeter => {
                let amp = buffer.iter().map(|s| s.abs()).sum::<f32>() / buffer.len().max(1) as f32;
                let norm = (amp * self.sensitivity).min(1.0);
                let bar_h = norm * content_rect.h();
                draw.rect()
                    .x_y(content_rect.x(), content_rect.bottom() + bar_h * 0.5)
                    .w_h(content_rect.w() * 0.2, bar_h)
                    .color(color);
            }
            VisualizationType::Lissajous => {
                let points: Vec<Point2> = self
                    .left_buffer
                    .iter()
                    .zip(self.right_buffer.iter())
                    .map(|(&l, &r)| {
                        let x = content_rect.x() + l * content_rect.w() * 0.4 * self.sensitivity;
                        let y = content_rect.y() + r * content_rect.h() * 0.4 * self.sensitivity;
                        pt2(x, y)
                    })
                    .collect();
                if !points.is_empty() {
                    draw.polyline().weight(2.0).points(points).color(color);
                }
            }
        }

        // Status indicators
        let status_y = rect.top() - 12.0;
        draw_text(
            draw,
            FontId::PlexSansBold,
            self.vis_type.label(),
            pt2(rect.x(), status_y),
            10.0,
            srgba(0.5, 0.5, 0.5, 1.0),
            TextAlignment::Center,
        );

        let mut indicator_x = rect.right() - 35.0;
        if self.is_muted {
            draw_text(
                draw,
                FontId::PlexSansBold,
                "MUTE",
                pt2(indicator_x, status_y),
                9.0,
                srgba(1.0, 0.3, 0.3, 0.8),
                TextAlignment::Center,
            );
            indicator_x -= 35.0;
        }
        if self.is_frozen {
            draw_text(
                draw,
                FontId::PlexSansBold,
                "FREEZE",
                pt2(indicator_x, status_y),
                9.0,
                srgba(0.3, 0.5, 1.0, 0.8),
                TextAlignment::Center,
            );
        }

        if let Some(latency) = &self.latency_us {
            let latency_ms = latency.load(Ordering::Relaxed) as f32 / 1000.0;
            let text = format!("{:.1}ms", latency_ms);
            draw_text(
                draw,
                FontId::PlexMonoRegular,
                &text,
                pt2(rect.left() + 32.0, status_y),
                9.0,
                srgba(0.6, 0.7, 0.9, 0.9),
                TextAlignment::Center,
            );
        }
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.98));

        draw_text(
            draw,
            FontId::PlexSansBold,
            "AUDIO VISUALIZER",
            pt2(rect.x(), rect.top() - 30.0),
            18.0,
            srgba(0.0, 1.0, 1.0, 1.0),
            TextAlignment::Center,
        );

        let subtitle = format!("{} | {}", self.vis_type.label(), self.color_scheme.label());
        draw_text(
            draw,
            FontId::PlexSansRegular,
            &subtitle,
            pt2(rect.x(), rect.top() - 50.0),
            12.0,
            srgba(0.5, 0.5, 0.5, 1.0),
            TextAlignment::Center,
        );

        if let Some(latency) = &self.latency_us {
            let latency_ms = latency.load(Ordering::Relaxed) as f32 / 1000.0;
            let text = format!("Latency: {:.1} ms", latency_ms);
            draw_text(
                draw,
                FontId::PlexMonoRegular,
                &text,
                pt2(rect.x(), rect.top() - 70.0),
                11.0,
                srgba(0.4, 0.6, 0.9, 1.0),
                TextAlignment::Center,
            );
        }

        let preview_rect = Rect::from_x_y_w_h(
            rect.x() + rect.w() * 0.15,
            rect.y() - 20.0,
            rect.w() * 0.65,
            rect.h() * 0.5,
        );

        draw.rect()
            .xy(preview_rect.xy())
            .wh(preview_rect.wh())
            .no_fill()
            .stroke(srgba(0.2, 0.3, 0.3, 1.0))
            .stroke_weight(1.0);

        draw_text(
            draw,
            FontId::PlexSansBold,
            "LIVE PREVIEW",
            pt2(preview_rect.x(), preview_rect.top() + 15.0),
            10.0,
            srgba(0.3, 0.3, 0.3, 1.0),
            TextAlignment::Center,
        );

        self.render_monitor(draw, preview_rect.pad(2.0), ctx);
        false
    }

    fn update(&mut self) {
        if !self.is_frozen && !self.is_muted {
            self.poll_audio();
        }

        #[cfg(feature = "tile-rendering")]
        {
            if matches!(
                self.vis_type,
                VisualizationType::SpectrumBars | VisualizationType::SpectrumLine
            ) {
                self.update_spectrum();
            }
        }
    }

    fn get_error(&self) -> Option<TileError> {
        #[cfg(feature = "tile-rendering")]
        {
            self.error.clone()
        }
        #[cfg(not(feature = "tile-rendering"))]
        {
            None
        }
    }

    fn clear_error(&mut self) {
        #[cfg(feature = "tile-rendering")]
        {
            self.error = None;
        }
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
            }
            "freeze" => {
                if !self.is_frozen {
                    self.frozen_buffer = self.buffer.clone();
                }
                self.is_frozen = !self.is_frozen;
                log::info!("Audio vis freeze: {}", self.is_frozen);
                true
            }
            "next_vis" => {
                self.vis_type = self.vis_type.next();
                log::info!("Audio vis type: {:?}", self.vis_type);
                true
            }
            _ => false,
        }
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);

    match (i as i32) % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}
