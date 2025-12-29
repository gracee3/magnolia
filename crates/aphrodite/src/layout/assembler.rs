use crate::aspects::types::AspectSet;
use crate::ephemeris::types::LayerPositions;
use crate::layout::rings::{
    build_house_items, build_planet_items, build_static_zodiac_items, RingItem,
};
use crate::layout::types::{RingDefinition, WheelDefinition};
use std::collections::HashMap;

/// Assembled wheel with resolved ring items
#[derive(Debug, Clone)]
pub struct AssembledWheel {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub radius_inner: f32,
    pub radius_outer: f32,
    pub rings: Vec<AssembledRing>,
}

/// Assembled ring with resolved items
#[derive(Debug, Clone)]
pub struct AssembledRing {
    pub id: String,
    pub ring_type: String,
    pub label: String,
    pub order: u32,
    pub radius_inner: f32,
    pub radius_outer: f32,
    pub data_source: crate::layout::types::RingDataSource,
    pub items: Vec<RingItem>,
}

/// Wheel assembler
pub struct WheelAssembler;

impl WheelAssembler {
    /// Build a complete wheel with resolved ring items
    pub fn build_wheel(
        wheel_config: &WheelDefinition,
        positions_by_layer: &HashMap<String, LayerPositions>,
        aspect_sets: &HashMap<String, AspectSet>,
        include_objects: Option<&[String]>,
    ) -> AssembledWheel {
        let mut ring_dtos = Vec::new();

        for ring_config in &wheel_config.rings {
            let ring_dto = Self::build_ring(
                ring_config,
                positions_by_layer,
                aspect_sets,
                &ring_dtos,
                include_objects,
            );
            ring_dtos.push(ring_dto);
        }

        // Determine wheel radius
        let (inner_radius, outer_radius) = if !wheel_config.rings.is_empty() {
            let inner = wheel_config
                .rings
                .iter()
                .map(|r| r.radius_inner)
                .fold(f32::INFINITY, f32::min);
            let outer = wheel_config
                .rings
                .iter()
                .map(|r| r.radius_outer)
                .fold(0.0, f32::max);
            (inner, outer)
        } else {
            (0.0, 1.0)
        };

        AssembledWheel {
            id: uuid::Uuid::new_v4().to_string(),
            name: wheel_config.name.clone(),
            description: wheel_config.description.clone(),
            radius_inner: inner_radius,
            radius_outer: outer_radius,
            rings: ring_dtos,
        }
    }

    /// Build a single ring with resolved items
    fn build_ring(
        ring_config: &RingDefinition,
        positions_by_layer: &HashMap<String, LayerPositions>,
        aspect_sets: &HashMap<String, AspectSet>,
        _existing_rings: &[AssembledRing],
        include_objects: Option<&[String]>,
    ) -> AssembledRing {
        let slug = &ring_config.slug;
        let mut items: Vec<RingItem> = Vec::new();

        match &ring_config.data_source {
            crate::layout::types::RingDataSource::StaticZodiac => {
                let sign_items = build_static_zodiac_items(slug);
                items.extend(sign_items.into_iter().map(RingItem::Sign));
            }
            crate::layout::types::RingDataSource::LayerHouses { layer_id } => {
                if let Some(positions) = positions_by_layer.get(layer_id) {
                    let house_items = build_house_items(slug, layer_id, positions);
                    items.extend(house_items.into_iter().map(RingItem::House));
                }
            }
            crate::layout::types::RingDataSource::LayerPlanets { layer_id } => {
                if let Some(positions) = positions_by_layer.get(layer_id) {
                    let planet_items = build_planet_items(slug, layer_id, positions, include_objects);
                    items.extend(planet_items.into_iter().map(RingItem::Planet));
                }
            }
            crate::layout::types::RingDataSource::LayerVargaPlanets { .. } => {
                // Vedic varga planets - deferred to Phase 6
                // For now, leave items empty
            }
            crate::layout::types::RingDataSource::AspectSet { aspect_set_id, .. } => {
                if let Some(_aspect_set) = aspect_sets.get(aspect_set_id) {
                    // Build aspect items from aspect set
                    // This is a simplified version - full implementation would
                    // need to resolve planet positions and create aspect lines
                    // For now, we'll leave this as a placeholder
                }
            }
            crate::layout::types::RingDataSource::StaticNakshatras => {
                // Nakshatras - deferred to Phase 6
                // For now, leave items empty
            }
        }

        AssembledRing {
            id: uuid::Uuid::new_v4().to_string(),
            ring_type: format!("{:?}", ring_config.ring_type).to_lowercase(),
            label: ring_config.label.clone(),
            order: ring_config.order_index,
            radius_inner: ring_config.radius_inner,
            radius_outer: ring_config.radius_outer,
            data_source: ring_config.data_source.clone(),
            items,
        }
    }
}

