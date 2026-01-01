//! System Monitor Tile - Real-time system metrics visualization
//!
//! Monitor mode: Text-based summary of CPU, RAM, and GPU
//! Control mode: Detailed line graphs for all metrics

use super::{RenderContext, TileRenderer};
use nannou::prelude::*;
// use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use std::fs;
use sysinfo::{System, Networks, Disks};
use nvml_wrapper::Nvml;
use magnolia_ui::{draw_text, FontId, TextAlignment};

const HISTORY_SIZE: usize = 60;
const REFRESH_INTERVAL: Duration = Duration::from_millis(1000); // 1Hz

pub struct SystemMonitorTile {
    sys: System,
    networks: Networks,
    _disks: Disks,
    nvml: Option<Nvml>,
    intel_gpu_path: Option<String>,
    
    last_refresh: Instant,
    
    // History buffers
    cpu_history: VecDeque<f32>,
    per_core_history: Vec<VecDeque<f32>>,
    mem_history: VecDeque<f32>,
    gpu_util_history: VecDeque<f32>,
    gpu_temp_history: VecDeque<f32>,
    net_up_history: VecDeque<f32>,
    net_down_history: VecDeque<f32>,
    
    // Current values for monitor mode
    current_cpu: f32,
    current_mem_gb: f32,
    current_gpu_util: f32,
    current_gpu_temp: f32,
}

impl SystemMonitorTile {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        
        let networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();
        let nvml = Nvml::init().ok();

        // Try to find Intel GPU busy percent path
        let intel_gpu_path = if fs::metadata("/sys/class/drm/card0/device/gpu_busy_percent").is_ok() {
            Some("/sys/class/drm/card0/device/gpu_busy_percent".to_string())
        } else if fs::metadata("/sys/class/drm/card1/device/gpu_busy_percent").is_ok() {
            Some("/sys/class/drm/card1/device/gpu_busy_percent".to_string())
        } else {
            None
        };

        let core_count = sys.cpus().len();

        Self {
            sys,
            networks,
            _disks: disks,
            nvml,
            intel_gpu_path,
            last_refresh: Instant::now() - REFRESH_INTERVAL,
            cpu_history: VecDeque::with_capacity(HISTORY_SIZE),
            per_core_history: vec![VecDeque::with_capacity(HISTORY_SIZE); core_count],
            mem_history: VecDeque::with_capacity(HISTORY_SIZE),
            gpu_util_history: VecDeque::with_capacity(HISTORY_SIZE),
            gpu_temp_history: VecDeque::with_capacity(HISTORY_SIZE),
            net_up_history: VecDeque::with_capacity(HISTORY_SIZE),
            net_down_history: VecDeque::with_capacity(HISTORY_SIZE),
            current_cpu: 0.0,
            current_mem_gb: 0.0,
            current_gpu_util: 0.0,
            current_gpu_temp: 0.0,
        }
    }

    fn refresh_metrics(&mut self) {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.networks.refresh(true);
        
        // CPU
        self.current_cpu = self.sys.global_cpu_usage();
        push_history(&mut self.cpu_history, self.current_cpu);
        
        for (i, cpu) in self.sys.cpus().iter().enumerate() {
            if i < self.per_core_history.len() {
                push_history(&mut self.per_core_history[i], cpu.cpu_usage());
            }
        }
        
        // Memory
        let used_mem = self.sys.used_memory() as f32 / (1024.0 * 1024.0 * 1024.0);
        self.current_mem_gb = used_mem;
        let total_mem = self.sys.total_memory() as f32 / (1024.0 * 1024.0 * 1024.0);
        push_history(&mut self.mem_history, (used_mem / total_mem) * 100.0);
        
        // GPU (NVIDIA)
        let mut gpu_found = false;
        if let Some(ref nvml) = self.nvml {
            if let Ok(device) = nvml.device_by_index(0) {
                if let Ok(util) = device.utilization_rates() {
                    self.current_gpu_util = util.gpu as f32;
                    push_history(&mut self.gpu_util_history, self.current_gpu_util);
                    gpu_found = true;
                }
                if let Ok(temp) = device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu) {
                    self.current_gpu_temp = temp as f32;
                    push_history(&mut self.gpu_temp_history, self.current_gpu_temp);
                }
            }
        }
        
        // GPU (Intel fallback)
        if !gpu_found {
            if let Some(ref path) = self.intel_gpu_path {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Ok(val) = content.trim().parse::<f32>() {
                        self.current_gpu_util = val;
                        push_history(&mut self.gpu_util_history, val);
                        gpu_found = true;
                    }
                }
            }
        }

        if !gpu_found {
            push_history(&mut self.gpu_util_history, 0.0);
        }
        
        // Network
        let mut total_received = 0.0;
        let mut total_transmitted = 0.0;
        for (_interface_name, data) in &self.networks {
            total_received += data.received() as f32;
            total_transmitted += data.transmitted() as f32;
        }
        // Convert to MB/s (assuming 1s interval)
        push_history(&mut self.net_down_history, total_received / (1024.0 * 1024.0));
        push_history(&mut self.net_up_history, total_transmitted / (1024.0 * 1024.0));
    }
}

fn push_history(history: &mut VecDeque<f32>, value: f32) {
    if history.len() >= HISTORY_SIZE {
        history.pop_front();
    }
    history.push_back(value);
}

impl TileRenderer for SystemMonitorTile {
    fn id(&self) -> &str { "system_monitor" }
    fn name(&self) -> &str { "System Monitor" }

    fn update(&mut self) {
        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh_metrics();
            self.last_refresh = Instant::now();
        }
    }

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.95));

        let font_size = (rect.h() * 0.15).min(16.0);
        let margin = 10.0;
        let x = rect.left() + margin;
        let mut y = rect.top() - margin;

        let stats = [
            format!("CPU: {:.1}%", self.current_cpu),
            format!("RAM: {:.1} GB", self.current_mem_gb),
            format!("GPU: {:.0}% / {:.0}°C", self.current_gpu_util, self.current_gpu_temp),
            format!("NET: ↓{:.1} ↑{:.1} MB/s", 
                self.net_down_history.back().cloned().unwrap_or(0.0),
                self.net_up_history.back().cloned().unwrap_or(0.0)
            ),
        ];

        for stat in stats {
            draw_text(
                draw,
                FontId::PlexMonoRegular,
                &stat,
                pt2(x, y),
                font_size,
                srgba(0.0, 1.0, 0.8, 1.0),
                TextAlignment::Left,
            );
            y -= font_size * 1.5;
        }
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) -> bool {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.01, 0.01, 0.02, 1.0));

        let padding = 20.0;
        let inner_rect = rect.pad(padding);
        let graph_spacing = 10.0;
        let num_main_graphs = 4;
        let main_graph_height = (inner_rect.h() / 2.0 - graph_spacing * (num_main_graphs as f32 - 1.0)) / num_main_graphs as f32;
        
        let mut current_y = inner_rect.top();

        // 1. CPU Global
        draw_graph(draw, pt2(inner_rect.left(), current_y), inner_rect.w(), main_graph_height, &self.cpu_history, "CPU Global (%)", srgba(0.0, 1.0, 0.5, 1.0), 100.0);
        current_y -= main_graph_height + graph_spacing;

        // 2. Memory
        draw_graph(draw, pt2(inner_rect.left(), current_y), inner_rect.w(), main_graph_height, &self.mem_history, "Memory (%)", srgba(0.2, 0.6, 1.0, 1.0), 100.0);
        current_y -= main_graph_height + graph_spacing;

        // 3. GPU
        draw_graph(draw, pt2(inner_rect.left(), current_y), inner_rect.w(), main_graph_height, &self.gpu_util_history, "GPU Utilization (%)", srgba(0.8, 0.2, 1.0, 1.0), 100.0);
        current_y -= main_graph_height + graph_spacing;

        // 4. Network
        draw_graph(draw, pt2(inner_rect.left(), current_y), inner_rect.w(), main_graph_height, &self.net_down_history, "Network Down (MB/s)", srgba(1.0, 0.8, 0.0, 1.0), 10.0);
        current_y -= main_graph_height + graph_spacing;

        // Per-core CPU (small grid at the bottom)
        let core_cols = 8;
        let core_rows = (self.per_core_history.len() as f32 / core_cols as f32).ceil() as usize;
        let core_graph_w = (inner_rect.w() - (core_cols - 1) as f32 * 5.0) / core_cols as f32;
        let core_graph_h = (inner_rect.h() / 2.0 - (core_rows - 1) as f32 * 5.0) / core_rows as f32;

        for (i, core_history) in self.per_core_history.iter().enumerate() {
            let col = i % core_cols;
            let row = i / core_cols;
            let x = inner_rect.left() + col as f32 * (core_graph_w + 5.0);
            let y = current_y - row as f32 * (core_graph_h + 5.0);
            
            draw_graph(draw, pt2(x, y), core_graph_w, core_graph_h, core_history, &format!("C{}", i), srgba(0.0, 0.8, 0.4, 0.6), 100.0);
        }

        false
    }
}

fn draw_graph(draw: &Draw, top_left: Point2, width: f32, height: f32, history: &VecDeque<f32>, label: &str, color: Srgba, max_val: f32) {
    let rect = Rect::from_corners(top_left, top_left + vec2(width, -height));
    
    // Label (only if height is enough)
    if height > 20.0 {
        draw_text(
            draw,
            FontId::PlexMonoRegular,
            label,
            top_left + pt2(5.0, -5.0),
            10.0,
            srgba(0.7, 0.7, 0.7, 1.0),
            TextAlignment::Left,
        );
    }

    // Background
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .color(srgba(0.1, 0.1, 0.1, 0.3));

    if history.is_empty() { return; }

    let step_x = width / (HISTORY_SIZE as f32 - 1.0);
    let points = history.iter().enumerate().map(|(i, &val)| {
        let x = rect.left() + i as f32 * step_x;
        let norm_val = (val / max_val).min(1.0);
        let y = rect.bottom() + norm_val * height;
        pt2(x, y)
    });

    draw.polyline()
        .weight(1.5)
        .points(points)
        .color(color);
}
