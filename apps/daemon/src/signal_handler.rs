//! Signal Handler - Processes signals from the orchestrator
//!
//! This module centralizes signal processing logic that was previously
//! embedded in the update() function.

use nannou::prelude::*;
use talisman_core::Signal;
use kamea::SigilConfig;
use std::collections::VecDeque;
use talisman_core::LayoutConfig;

/// Process incoming signals from the receiver
/// 
/// This function drains the signal channel and updates the relevant state.
pub fn process_signals(
    receiver: &std::sync::mpsc::Receiver<Signal>,
    current_intent: &mut String,
    word_count: &mut String,
    devowel_text: &mut String,
    astro_data: &mut String,
    path_points: &mut Vec<Point2>,
    config: &mut SigilConfig,
    layout_config: &LayoutConfig,
    audio_buffer: &mut VecDeque<f32>,
    calculate_rect: impl Fn(&talisman_core::TileConfig) -> Option<Rect>,
) {
    while let Ok(signal) = receiver.try_recv() {
        match signal {
            Signal::Text(text) => {
                *current_intent = text.clone();
                
                let mut hasher = sha2::Sha256::new();
                use sha2::Digest;
                hasher.update(text.as_bytes());
                let result = hasher.finalize();
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&result);

                let len_factor = text.len().min(100);
                let size = if len_factor > 10 { 5 } else { 4 };
                config.grid_rows = size;
                config.grid_cols = size;
                
                // Find tile for kamea to calculate spacing
                let sigil_tile = layout_config.tiles.iter().find(|t| t.module == "kamea_sigil");
                if let Some(tile) = sigil_tile {
                    if let Some(rect) = calculate_rect(tile) {
                        config.spacing = rect.w() / (size as f32 * 2.0);
                    } else {
                        config.spacing = 30.0;
                    }
                } else {
                    config.spacing = 30.0;
                }

                *path_points = kamea::generate_path(seed, *config)
                    .into_iter()
                    .map(|(x, y)| pt2(x, y))
                    .collect();
            }
            Signal::Computed { source, content } => {
                if source == "word_count" {
                    *word_count = content;
                } else if source == "devowelizer" {
                    *devowel_text = content;
                }
            }
            Signal::Audio { data, .. } => {
                // Push audio samples to buffer
                for sample in data {
                    audio_buffer.push_back(sample);
                }
                // Maintain buffer size (e.g. 2048)
                while audio_buffer.len() > 2048 {
                    audio_buffer.pop_front();
                }
            }
            Signal::Astrology {
                sun_sign,
                moon_sign,
                rising_sign,
                planetary_positions,
            } => {
                // Format planetary positions for display
                let planets: Vec<String> = planetary_positions
                    .iter()
                    .take(5)
                    .map(|(name, lon)| format!("{}: {:.0}°", name, lon % 360.0))
                    .collect();

                *astro_data = format!(
                    "{}|{}|{}|{}",
                    sun_sign,
                    moon_sign,
                    rising_sign,
                    planets.join("|")
                );
            }
            _ => {}
        }
    }
}
