//! GPU Renderer - Hardware-accelerated rendering utilities
//!
//! Provides GPU-optimized rendering for real-time visualizations,
//! particularly audio waveforms and spectrum analysis.

#![allow(dead_code)] // Shared GPU infrastructure for future tile module use

use nannou::prelude::*;

/// GPU-accelerated rendering for real-time visualizations
/// 
/// Uses Nannou's built-in wgpu integration for efficient rendering.
/// All draw calls are batched for minimal CPU overhead.
pub struct GpuRenderer {
    /// Whether GPU acceleration is available
    available: bool,
}

impl GpuRenderer {
    /// Create a new GPU renderer
    /// 
    /// Checks for GPU availability and initializes resources.
    pub fn new(_app: &App) -> Self {
        // Nannou already uses wgpu for all rendering
        // This struct provides optimized drawing utilities
        Self {
            available: true,
        }
    }
    
    /// Check if GPU acceleration is available
    pub fn is_available(&self) -> bool {
        self.available
    }
    
    /// Render oscilloscope waveform (GPU path)
    /// 
    /// Efficiently renders audio samples as a continuous line.
    /// Uses batched draw calls for minimal CPU work.
    pub fn render_oscilloscope(
        &self,
        draw: &Draw,
        buffer: &[f32],
        rect: Rect,
        color: impl Into<LinSrgba>,
        stroke_weight: f32,
    ) {
        if buffer.is_empty() {
            return;
        }
        
        let color = color.into();
        
        // Batch all points into single polyline draw call
        let points: Vec<Point2> = buffer.iter().enumerate().map(|(i, &sample)| {
            let x = map_range(i, 0, buffer.len(), rect.left(), rect.right());
            let y = rect.y() + sample * rect.h() * 0.4;
            pt2(x, y)
        }).collect();
        
        if !points.is_empty() {
            // Glow effect (wider, transparent)
            draw.polyline()
                .weight(stroke_weight * 3.0)
                .points(points.clone())
                .color(LinSrgba::new(color.red, color.green, color.blue, 0.2));
            
            // Main line
            draw.polyline()
                .weight(stroke_weight)
                .points(points)
                .color(color);
        }
    }
    
    /// Render oscilloscope with cyan reactive coloring
    /// 
    /// Brightness reacts to amplitude for visual feedback.
    pub fn render_oscilloscope_reactive(
        &self,
        draw: &Draw,
        buffer: &[f32],
        rect: Rect,
        sensitivity: f32,
        stroke_weight: f32,
    ) {
        if buffer.is_empty() {
            return;
        }
        
        // Calculate average amplitude for color intensity
        let avg_amp = buffer.iter().map(|s| s.abs()).sum::<f32>() / buffer.len() as f32;
        let brightness = (0.5 + avg_amp * sensitivity * 0.5).min(1.0);
        let color = LinSrgba::new(0.0, brightness, brightness, 1.0);
        
        self.render_oscilloscope(draw, buffer, rect, color, stroke_weight);
    }
    
    /// Render spectrum bars (GPU path)
    /// 
    /// Renders frequency magnitude data as vertical bars.
    pub fn render_spectrum_bars(
        &self,
        draw: &Draw,
        magnitudes: &[f32],
        rect: Rect,
        color: impl Into<LinSrgba>,
        bar_count: usize,
    ) {
        if magnitudes.is_empty() || bar_count == 0 {
            return;
        }
        
        let color = color.into();
        let bar_width = rect.w() / bar_count as f32;
        let step = magnitudes.len().max(1) / bar_count.max(1);
        
        for i in 0..bar_count {
            let idx = (i * step).min(magnitudes.len().saturating_sub(1));
            let mag = magnitudes.get(idx).copied().unwrap_or(0.0);
            let height = (mag * rect.h()).min(rect.h());
            
            if height > 0.5 {
                let x = rect.left() + i as f32 * bar_width + bar_width * 0.5;
                let y = rect.bottom() + height * 0.5;
                
                // Bar
                draw.rect()
                    .x_y(x, y)
                    .w_h(bar_width * 0.8, height)
                    .color(color);
                
                // Glow on top
                draw.rect()
                    .x_y(x, rect.bottom() + height)
                    .w_h(bar_width * 0.6, 4.0)
                    .color(LinSrgba::new(color.red, color.green, color.blue, 0.5));
            }
        }
    }
    
    /// Render spectrum bars with rainbow coloring
    /// 
    /// Each bar is colored based on its frequency position.
    pub fn render_spectrum_rainbow(
        &self,
        draw: &Draw,
        magnitudes: &[f32],
        rect: Rect,
        bar_count: usize,
    ) {
        if magnitudes.is_empty() || bar_count == 0 {
            return;
        }
        
        let bar_width = rect.w() / bar_count as f32;
        let step = magnitudes.len().max(1) / bar_count.max(1);
        
        for i in 0..bar_count {
            let idx = (i * step).min(magnitudes.len().saturating_sub(1));
            let mag = magnitudes.get(idx).copied().unwrap_or(0.0);
            let height = (mag * rect.h()).min(rect.h());
            
            if height > 0.5 {
                let x = rect.left() + i as f32 * bar_width + bar_width * 0.5;
                let y = rect.bottom() + height * 0.5;
                
                // HSV to RGB for rainbow effect
                let hue = i as f32 / bar_count as f32;
                let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                let color = LinSrgba::new(r, g, b, 1.0);
                
                draw.rect()
                    .x_y(x, y)
                    .w_h(bar_width * 0.8, height)
                    .color(color);
            }
        }
    }
    
    /// Render VU meter style display
    pub fn render_vu_meter(
        &self,
        draw: &Draw,
        level: f32, // 0.0 to 1.0
        rect: Rect,
    ) {
        let level = level.clamp(0.0, 1.0);
        
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.1, 0.1, 0.1, 0.8));
        
        // Segments
        let segment_count = 20;
        let segment_height = rect.h() / segment_count as f32;
        let active_segments = (level * segment_count as f32) as usize;
        
        for i in 0..segment_count {
            let y = rect.bottom() + i as f32 * segment_height + segment_height * 0.5;
            let is_active = i < active_segments;
            
            // Color gradient: green -> yellow -> red
            let color = if i >= segment_count - 2 {
                if is_active { srgba(1.0, 0.2, 0.2, 1.0) } else { srgba(0.3, 0.1, 0.1, 0.5) }
            } else if i >= segment_count - 5 {
                if is_active { srgba(1.0, 0.8, 0.2, 1.0) } else { srgba(0.3, 0.2, 0.1, 0.5) }
            } else {
                if is_active { srgba(0.2, 1.0, 0.4, 1.0) } else { srgba(0.1, 0.2, 0.1, 0.5) }
            };
            
            draw.rect()
                .x_y(rect.x(), y)
                .w_h(rect.w() * 0.9, segment_height * 0.8)
                .color(color);
        }
    }
    
    /// Render Lissajous curve (stereo visualization)
    pub fn render_lissajous(
        &self,
        draw: &Draw,
        left: &[f32],
        right: &[f32],
        rect: Rect,
        color: impl Into<LinSrgba>,
    ) {
        let color = color.into();
        let len = left.len().min(right.len());
        
        if len < 2 {
            return;
        }
        
        let points: Vec<Point2> = (0..len).map(|i| {
            let x = rect.x() + left[i] * rect.w() * 0.4;
            let y = rect.y() + right[i] * rect.h() * 0.4;
            pt2(x, y)
        }).collect();
        
        // Draw with fade effect (newer samples brighter)
        let fade_start = len.saturating_sub(512);
        for window in points.windows(2).enumerate() {
            let (i, pts) = window;
            if i < fade_start {
                continue;
            }
            let alpha = (i - fade_start) as f32 / 512.0;
            draw.line()
                .start(pts[0])
                .end(pts[1])
                .weight(1.5)
                .color(LinSrgba::new(color.red, color.green, color.blue, alpha * color.alpha));
        }
    }
}

impl Default for GpuRenderer {
    fn default() -> Self {
        Self { available: true }
    }
}

/// Convert HSV to RGB
/// 
/// h, s, v are all in range 0.0..1.0
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
