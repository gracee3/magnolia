//! Chart Animation Module
//!
//! Provides smooth transitions for astrological chart updates,
//! handling the 360°/0° wrap-around correctly for seamless animations.

use super::data::{ChartData, PlanetData};
use crate::rendering::glyphs::Glyph;
use std::collections::HashMap;

/// Animation speed per frame (0.0 to 1.0 progress per update)
pub const ANIM_SPEED: f32 = 0.08;

/// Chart animation state for smooth position transitions
#[derive(Debug, Clone)]
pub struct ChartAnimation {
    /// Current animated positions (degrees 0-360)
    current_positions: HashMap<Glyph, f32>,
    /// Target positions to animate towards
    target_positions: HashMap<Glyph, f32>,
    /// Animation progress 0.0-1.0
    progress: f32,
    /// Whether animation is currently active
    animating: bool,
}

impl Default for ChartAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartAnimation {
    pub fn new() -> Self {
        Self {
            current_positions: HashMap::new(),
            target_positions: HashMap::new(),
            progress: 0.0,
            animating: false,
        }
    }

    /// Initialize animation from existing chart data
    pub fn init_from(&mut self, data: &ChartData) {
        self.current_positions.clear();
        for (glyph, planet) in &data.planets {
            self.current_positions.insert(*glyph, planet.position);
        }
        self.target_positions = self.current_positions.clone();
        self.animating = false;
    }

    /// Start smooth transition to new positions
    pub fn animate_to(&mut self, new_data: &ChartData) {
        // If no current positions, just initialize directly
        if self.current_positions.is_empty() {
            self.init_from(new_data);
            return;
        }

        // Set new targets
        self.target_positions.clear();
        for (glyph, planet) in &new_data.planets {
            self.target_positions.insert(*glyph, planet.position);
        }

        // Start animation
        self.progress = 0.0;
        self.animating = true;
    }

    /// Update animation state (call each frame)
    /// Returns true if animation just completed
    pub fn update(&mut self) -> bool {
        if !self.animating {
            return false;
        }

        self.progress = (self.progress + ANIM_SPEED).min(1.0);
        let t = self.eased();

        // Interpolate each planet towards target
        for (glyph, current) in self.current_positions.iter_mut() {
            if let Some(&target) = self.target_positions.get(glyph) {
                *current = lerp_degrees(*current, target, t);
            }
        }

        // Handle new planets that weren't in current
        for (glyph, &target) in &self.target_positions {
            if !self.current_positions.contains_key(glyph) {
                self.current_positions.insert(*glyph, target);
            }
        }

        if self.progress >= 1.0 {
            self.animating = false;
            // Snap to exact targets
            for (glyph, &target) in &self.target_positions {
                self.current_positions.insert(*glyph, target);
            }
            return true;
        }

        false
    }

    /// Get cubic ease in-out factor (same as ModalAnim)
    fn eased(&self) -> f32 {
        let t = self.progress;
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            (t - 1.0) * (2.0 * t - 2.0) * (2.0 * t - 2.0) + 1.0
        }
    }

    /// Check if animation is currently active
    pub fn is_animating(&self) -> bool {
        self.animating
    }

    /// Get current animated position for a glyph
    pub fn get_position(&self, glyph: &Glyph) -> Option<f32> {
        self.current_positions.get(glyph).copied()
    }

    /// Build animated ChartData from current animation state
    pub fn build_animated_data(&self, original: &ChartData) -> ChartData {
        let mut planets = Vec::new();
        for (glyph, planet_data) in &original.planets {
            let position = self
                .current_positions
                .get(glyph)
                .copied()
                .unwrap_or(planet_data.position);
            planets.push((
                *glyph,
                PlanetData {
                    position,
                    speed: planet_data.speed,
                },
            ));
        }
        ChartData {
            planets,
            cusps: original.cusps.clone(),
        }
    }
}

/// Interpolate degrees handling 360°/0° wrap-around correctly
/// Always takes the shortest path around the circle
fn lerp_degrees(from: f32, to: f32, t: f32) -> f32 {
    // Calculate difference, handling wrap-around
    let diff = ((to - from + 540.0) % 360.0) - 180.0;
    // Interpolate and normalize
    (from + diff * t + 360.0) % 360.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lerp_degrees_normal() {
        // Simple case: 10° to 20°
        assert!((lerp_degrees(10.0, 20.0, 0.5) - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_lerp_degrees_wrap_forward() {
        // 350° to 10° should go forward through 0°
        let result = lerp_degrees(350.0, 10.0, 0.5);
        assert!((result - 0.0).abs() < 0.01 || (result - 360.0).abs() < 0.01);
    }

    #[test]
    fn test_lerp_degrees_wrap_backward() {
        // 10° to 350° should go backward through 0°
        let result = lerp_degrees(10.0, 350.0, 0.5);
        assert!((result - 0.0).abs() < 0.01 || (result - 360.0).abs() < 0.01);
    }

    #[test]
    fn test_lerp_degrees_start_end() {
        assert!((lerp_degrees(45.0, 90.0, 0.0) - 45.0).abs() < 0.01);
        assert!((lerp_degrees(45.0, 90.0, 1.0) - 90.0).abs() < 0.01);
    }
}
