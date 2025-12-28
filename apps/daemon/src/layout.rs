//! Layout Engine - Grid-based layout system for tile positioning
//!
//! Handles layout configuration loading, track resolution (px/fr/%),
//! and tile rect calculation.

use nannou::prelude::*;
use talisman_core::{LayoutConfig, TileConfig};
use std::fs;

pub struct Layout {
    pub window_rect: Rect,
    pub config: LayoutConfig,
}

impl Layout {
    pub fn new(win_rect: Rect) -> Self {
        // Load config from multiple potential paths
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
            id = "clock"
            col = 0
            row = 0
            colspan = 1
            module = "clock"
            
            [[tiles]]
            id = "astro"
            col = 1
            row = 0
            colspan = 1
            module = "astro"
            
            [[tiles]]
            id = "main"
            col = 0
            row = 1
            colspan = 2
            module = "text_input"
            "# .to_string()
        });
            
        let config: LayoutConfig = toml::from_str(&content).expect("Failed to parse layout.toml");
        
        Self { 
            window_rect: win_rect,
            config,
        }
    }
    
    pub fn update(&mut self, win_rect: Rect) {
        self.window_rect = win_rect;
    }

    pub fn save(&self) {
        let config = self.config.clone();
        std::thread::spawn(move || {
            match toml::to_string_pretty(&config) {
                Ok(c) => {
                    if let Err(e) = std::fs::write("configs/layout.toml", c) {
                        log::error!("Failed to save layout.toml: {}", e);
                    } else {
                        log::info!("Saved layout.toml (async body)");
                    }
                },
                Err(e) => log::error!("Failed to serialize layout config: {}", e),
            }
        });
    }

    pub fn get_tile_at(&self, col: usize, row: usize) -> Option<&TileConfig> {
        for tile in &self.config.tiles {
            let t_col = tile.col;
            let t_row = tile.row;
            let t_cols = tile.colspan.unwrap_or(1);
            let t_rows = tile.rowspan.unwrap_or(1);
            
            if col >= t_col && col < t_col + t_cols && row >= t_row && row < t_row + t_rows {
                return Some(tile);
            }
        }
        None
    }

    /// Calculate the screen rect for a tile
    pub fn calculate_rect(&self, tile: &TileConfig) -> Option<Rect> {
        let (col_tracks, row_tracks) = self.config.generate_tracks();
        let cols = self.resolve_tracks(&col_tracks, self.window_rect.w());
        let rows = self.resolve_tracks(&row_tracks, self.window_rect.h());

        let start_x = cols.iter().take(tile.col).sum::<f32>();
        let width = cols.iter().skip(tile.col).take(tile.colspan.unwrap_or(1)).sum::<f32>();
        
        let start_y_from_top = rows.iter().take(tile.row).sum::<f32>();
        let height = rows.iter().skip(tile.row).take(tile.rowspan.unwrap_or(1)).sum::<f32>();
        
        // Nannou Coordinate Conversion (center-based, Y up)
        let cx = self.window_rect.left() + start_x + width / 2.0;
        let cy = self.window_rect.top() - start_y_from_top - height / 2.0;
        
        Some(Rect::from_x_y_w_h(cx, cy, width, height))
    }
    
    /// Resolve track definitions (px, %, fr) to pixel values
    pub fn resolve_tracks(&self, tracks: &[String], total_size: f32) -> Vec<f32> {
        let mut resolved = vec![0.0; tracks.len()];
        let mut used_px = 0.0;
        let mut total_fr = 0.0;
        
        // First pass: PX, %, and FR sum
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
                // Fallback parsing
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
                let is_fr = track.contains("fr");
                if is_fr {
                    let val = track.trim_end_matches("fr").parse::<f32>().unwrap_or(1.0);
                    resolved[i] = (val / total_fr) * remaining;
                }
            }
        }
        
        resolved
    }
}
