use crate::aspects::types::{AspectCore, AspectPair, AspectObjectRef, AspectSet, AspectSettings};
use crate::ephemeris::types::LayerPositions;
use std::collections::HashMap;

/// Aspect angles in order of frequency (most common first)
const ASPECT_ANGLES: &[(&str, f64)] = &[
    ("conjunction", 0.0),
    ("opposition", 180.0),
    ("trine", 120.0),
    ("square", 90.0),
    ("sextile", 60.0),
];

/// Aspect calculator
pub struct AspectCalculator;

impl AspectCalculator {
    /// Create a new aspect calculator
    pub fn new() -> Self {
        Self
    }

    /// Compute aspects within a single layer
    pub fn compute_intra_layer_aspects(
        &self,
        layer_id: &str,
        positions: &LayerPositions,
        settings: &AspectSettings,
    ) -> AspectSet {
        let planets = &positions.planets;
        let mut planet_ids: Vec<String> = planets.keys().cloned().collect();

        // Filter to included objects
        if !settings.include_objects.is_empty() {
            let include_set: std::collections::HashSet<&str> =
                settings.include_objects.iter().map(|s| s.as_str()).collect();
            planet_ids.retain(|pid| include_set.contains(pid.as_str()));
        }

        // Early exit if not enough planets
        if planet_ids.len() < 2 {
            return AspectSet {
                id: layer_id.to_string(),
                label: format!("{} Aspects", capitalize_first(layer_id)),
                kind: "intra_layer".to_string(),
                layer_ids: vec![layer_id.to_string()],
                pairs: vec![],
            };
        }

        // Calculate aspects between all planet pairs
        let mut pairs = Vec::new();
        for i in 0..planet_ids.len() {
            for j in (i + 1)..planet_ids.len() {
                let p1_id = &planet_ids[i];
                let p2_id = &planet_ids[j];

                let p1_pos = &planets[p1_id];
                let p2_pos = &planets[p2_id];

                if let Some(aspect) = self.calculate_aspect(
                    p1_pos.lon,
                    p2_pos.lon,
                    p1_pos.speed_lon,
                    p2_pos.speed_lon,
                    &settings.orb_settings,
                ) {
                    pairs.push(AspectPair {
                        from: AspectObjectRef {
                            layer_id: layer_id.to_string(),
                            object_type: "planet".to_string(),
                            object_id: p1_id.clone(),
                        },
                        to: AspectObjectRef {
                            layer_id: layer_id.to_string(),
                            object_type: "planet".to_string(),
                            object_id: p2_id.clone(),
                        },
                        aspect,
                    });
                }
            }
        }

        AspectSet {
            id: layer_id.to_string(),
            label: format!("{} Aspects", capitalize_first(layer_id)),
            kind: "intra_layer".to_string(),
            layer_ids: vec![layer_id.to_string()],
            pairs,
        }
    }

    /// Compute aspects between two layers
    pub fn compute_inter_layer_aspects(
        &self,
        layer_id_a: &str,
        layer_id_b: &str,
        positions_a: &LayerPositions,
        positions_b: &LayerPositions,
        settings: &AspectSettings,
    ) -> AspectSet {
        let planets_a = &positions_a.planets;
        let planets_b = &positions_b.planets;

        let mut planet_ids_a: Vec<String> = planets_a.keys().cloned().collect();
        let mut planet_ids_b: Vec<String> = planets_b.keys().cloned().collect();

        // Filter to included objects
        if !settings.include_objects.is_empty() {
            let include_set: std::collections::HashSet<&str> =
                settings.include_objects.iter().map(|s| s.as_str()).collect();
            planet_ids_a.retain(|pid| include_set.contains(pid.as_str()));
            planet_ids_b.retain(|pid| include_set.contains(pid.as_str()));
        }

        // Calculate aspects between all planet pairs
        let mut pairs = Vec::new();
        for p1_id in &planet_ids_a {
            for p2_id in &planet_ids_b {
                // Skip if same planet
                if p1_id == p2_id {
                    continue;
                }

                let p1_pos = &planets_a[p1_id];
                let p2_pos = &planets_b[p2_id];

                if let Some(aspect) = self.calculate_aspect(
                    p1_pos.lon,
                    p2_pos.lon,
                    p1_pos.speed_lon,
                    p2_pos.speed_lon,
                    &settings.orb_settings,
                ) {
                    pairs.push(AspectPair {
                        from: AspectObjectRef {
                            layer_id: layer_id_a.to_string(),
                            object_type: "planet".to_string(),
                            object_id: p1_id.clone(),
                        },
                        to: AspectObjectRef {
                            layer_id: layer_id_b.to_string(),
                            object_type: "planet".to_string(),
                            object_id: p2_id.clone(),
                        },
                        aspect,
                    });
                }
            }
        }

        AspectSet {
            id: format!("{}:{}", layer_id_a, layer_id_b),
            label: format!("{} / {} Aspects", capitalize_first(layer_id_a), capitalize_first(layer_id_b)),
            kind: "inter_layer".to_string(),
            layer_ids: vec![layer_id_a.to_string(), layer_id_b.to_string()],
            pairs,
        }
    }

    /// Compute all aspect sets for multiple layers
    pub fn compute_all_aspect_sets(
        &self,
        layers: &HashMap<String, LayerPositions>,
        settings: &AspectSettings,
    ) -> HashMap<String, AspectSet> {
        let mut aspect_sets = HashMap::new();
        let layer_ids: Vec<String> = layers.keys().cloned().collect();

        // Intra-layer aspects
        for layer_id in &layer_ids {
            if let Some(positions) = layers.get(layer_id) {
                let aspect_set = self.compute_intra_layer_aspects(layer_id, positions, settings);
                aspect_sets.insert(aspect_set.id.clone(), aspect_set);
            }
        }

        // Inter-layer aspects
        for i in 0..layer_ids.len() {
            for j in (i + 1)..layer_ids.len() {
                let layer_id_a = &layer_ids[i];
                let layer_id_b = &layer_ids[j];

                if let (Some(positions_a), Some(positions_b)) =
                    (layers.get(layer_id_a), layers.get(layer_id_b))
                {
                    let aspect_set = self.compute_inter_layer_aspects(
                        layer_id_a,
                        layer_id_b,
                        positions_a,
                        positions_b,
                        settings,
                    );
                    aspect_sets.insert(aspect_set.id.clone(), aspect_set);
                }
            }
        }

        aspect_sets
    }

    /// Calculate aspect between two longitudes using planet speeds
    pub fn calculate_aspect(
        &self,
        lon1: f64,
        lon2: f64,
        speed1: f64,
        speed2: f64,
        orb_settings: &HashMap<String, f64>,
    ) -> Option<AspectCore> {
        // Calculate angle difference (normalized to 0-180)
        let raw_diff = (lon1 - lon2).abs();
        let angle_diff = if raw_diff > 180.0 {
            360.0 - raw_diff
        } else {
            raw_diff
        };

        // Early exit if angle is too large to be any aspect (with max orb)
        let max_orb = orb_settings
            .values()
            .copied()
            .fold(8.0, f64::max);
        if angle_diff > 180.0 + max_orb {
            return None;
        }

        // Check each aspect type in order of frequency (most common first)
        for (aspect_name, aspect_angle) in ASPECT_ANGLES {
            let orb = orb_settings.get(*aspect_name).copied().unwrap_or(8.0);
            let orb_value = (angle_diff - aspect_angle).abs();

            if orb_value <= orb {
                // Determine if applying or separating
                let is_applying = self.is_aspect_applying(
                    lon1,
                    lon2,
                    speed1,
                    speed2,
                    *aspect_angle,
                    angle_diff,
                );
                let is_exact = orb_value < 0.1; // Within 0.1 degrees is "exact"
                let is_retrograde = speed1 < 0.0 || speed2 < 0.0;

                return Some(AspectCore {
                    aspect_type: aspect_name.to_string(),
                    exact_angle: *aspect_angle,
                    orb: orb_value,
                    precision: orb_value,
                    is_applying,
                    is_exact,
                    is_retrograde,
                });
            }
        }

        None
    }

    /// Determine if an aspect is applying (approaching exact) or separating
    fn is_aspect_applying(
        &self,
        lon1: f64,
        lon2: f64,
        speed1: f64,
        speed2: f64,
        aspect_angle: f64,
        current_angle: f64,
    ) -> bool {
        // Calculate relative speed (degrees per day)
        let relative_speed = speed1 - speed2;

        // If speeds are equal or very close, we can't determine direction reliably
        if relative_speed.abs() < 0.01 {
            // Default to applying if very close to exact aspect
            return current_angle < aspect_angle + 0.5;
        }

        // Calculate the signed angular difference (considering direction)
        let mut signed_diff = lon1 - lon2;
        if signed_diff > 180.0 {
            signed_diff -= 360.0;
        } else if signed_diff < -180.0 {
            signed_diff += 360.0;
        }

        // Calculate the current distance from exact aspect
        let current_distance = (current_angle - aspect_angle).abs();

        // Project forward a small amount to see if we're getting closer to exact
        let time_step = 0.1; // Small time step (days)
        let mut future_signed_diff = signed_diff + relative_speed * time_step;

        // Normalize future difference
        if future_signed_diff > 180.0 {
            future_signed_diff -= 360.0;
        } else if future_signed_diff < -180.0 {
            future_signed_diff += 360.0;
        }

        // Calculate future angular separation
        let future_angle = future_signed_diff.abs();

        // Calculate distances from exact aspect
        let future_distance = (future_angle - aspect_angle).abs();

        // Applying if we're moving closer to the exact aspect
        future_distance < current_distance
    }
}

impl Default for AspectCalculator {
    fn default() -> Self {
        Self::new()
    }
}

/// Capitalize first letter of a string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

