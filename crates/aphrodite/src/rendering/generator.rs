use crate::aspects::types::AspectSet;
use crate::layout::{AssembledRing, AssembledWheel};
use crate::rendering::primitives::{
    Color, Point, Shape, Stroke,
};
use crate::rendering::spec::{AspectSetMetadata, ChartMetadata, ChartSpec};
use crate::rendering::visual_config::{GlyphConfig, VisualConfig};
use crate::layout::rings::RingItem;

/// ChartSpec generator - converts assembled wheel to ChartSpec
pub struct ChartSpecGenerator {
    visual_config: VisualConfig,
    glyph_config: GlyphConfig,
}

impl ChartSpecGenerator {
    /// Create a new generator with default configs
    pub fn new() -> Self {
        Self {
            visual_config: VisualConfig::default(),
            glyph_config: GlyphConfig::default(),
        }
    }

    /// Create a generator with custom configs
    pub fn with_configs(visual_config: VisualConfig, glyph_config: GlyphConfig) -> Self {
        Self {
            visual_config,
            glyph_config,
        }
    }

    /// Generate ChartSpec from assembled wheel
    pub fn generate(
        &self,
        wheel: &AssembledWheel,
        aspect_sets: &std::collections::HashMap<String, AspectSet>,
        width: f32,
        height: f32,
    ) -> ChartSpec {
        let center = Point {
            x: width / 2.0,
            y: height / 2.0,
        };
        let max_radius = width.min(height) / 2.0 - 20.0; // padding

        let mut shapes = Vec::new();

        // Generate shapes for each ring (in order)
        for ring in &wheel.rings {
            let ring_shapes = self.generate_ring_shapes(ring, center, max_radius);
            shapes.extend(ring_shapes);
        }

        // Generate aspect lines
        for aspect_set in aspect_sets.values() {
            let aspect_shapes = self.generate_aspect_shapes(aspect_set, center, max_radius);
            shapes.extend(aspect_shapes);
        }

        // Build metadata
        let metadata = ChartMetadata {
            layers: vec![], // TODO: Extract from wheel if available
            aspect_sets: aspect_sets
                .values()
                .map(|a| AspectSetMetadata {
                    id: a.id.clone(),
                    layer_ids: a.layer_ids.clone(),
                })
                .collect(),
        };

        ChartSpec {
            width,
            height,
            center,
            rotation_offset: 0.0,
            background_color: self.visual_config.background_color,
            shapes,
            metadata,
        }
    }

    /// Generate shapes for a single ring
    fn generate_ring_shapes(
        &self,
        ring: &AssembledRing,
        center: Point,
        max_radius: f32,
    ) -> Vec<Shape> {
        let mut shapes = Vec::new();

        for item in &ring.items {
            match item {
                RingItem::Sign(sign_item) => {
                    let radius_inner = max_radius * ring.radius_inner;
                    let radius_outer = max_radius * ring.radius_outer;
                    let start_angle = self.astro_to_svg_angle(sign_item.start_lon, 0.0);
                    let end_angle = self.astro_to_svg_angle(sign_item.end_lon, 0.0);

                    let sign_color = self
                        .visual_config
                        .sign_colors
                        .get(sign_item.index as usize)
                        .copied()
                        .unwrap_or(Color::WHITE);

                    shapes.push(Shape::SignSegment {
                        center,
                        sign_index: sign_item.index,
                        start_angle,
                        end_angle,
                        radius_inner,
                        radius_outer,
                        fill: sign_color,
                        stroke: Some(Stroke {
                            color: self.visual_config.stroke_color,
                            width: self.visual_config.stroke_width.unwrap_or(1.0),
                            dash_array: None,
                        }),
                    });
                }
                RingItem::House(_house_item) => {
                    // House cusps are typically drawn as lines, not segments
                    // For now, we'll skip house cusp rendering in the generator
                    // This can be enhanced later
                }
                RingItem::Planet(planet_item) => {
                    let radius = max_radius
                        * (ring.radius_inner + ring.radius_outer) / 2.0;
                    let angle = self.astro_to_svg_angle(planet_item.lon, 0.0);
                    let pos = self.polar_to_cartesian(angle, radius, center);

                    let planet_color = self
                        .visual_config
                        .planet_colors
                        .get(&planet_item.planet_id)
                        .copied()
                        .unwrap_or(Color::WHITE);

                    shapes.push(Shape::PlanetGlyph {
                        center: pos,
                        planet_id: planet_item.planet_id.clone(),
                        size: self.glyph_config.glyph_size.unwrap_or(12.0),
                        color: planet_color,
                        retrograde: planet_item.retrograde.unwrap_or(false),
                    });
                }
                RingItem::Aspect(_) => {
                    // Aspects are handled separately
                }
            }
        }

        shapes
    }

    /// Generate aspect line shapes
    fn generate_aspect_shapes(
        &self,
        aspect_set: &AspectSet,
        _center: Point,
        _max_radius: f32,
    ) -> Vec<Shape> {
        let shapes = Vec::new();

        // For aspect lines, we need to find the planet positions
        // This is a simplified version - full implementation would need
        // to resolve planet positions from the wheel rings
        // For now, we'll create a placeholder that can be enhanced

        for pair in &aspect_set.pairs {
            // Get aspect color
            let _aspect_color = self
                .visual_config
                .aspect_colors
                .get(&pair.aspect.aspect_type)
                .copied()
                .unwrap_or(Color::WHITE);

            // Calculate positions (simplified - would need actual planet positions)
            // For now, we'll skip rendering aspect lines without planet positions
            // This can be enhanced when we have full planet position resolution
        }

        shapes
    }

    /// Convert astronomical angle to SVG angle
    fn astro_to_svg_angle(&self, astro_angle: f64, rotation_offset: f64) -> f32 {
        let mut angle = 90.0 - (astro_angle + rotation_offset);
        while angle < 0.0 {
            angle += 360.0;
        }
        while angle >= 360.0 {
            angle -= 360.0;
        }
        angle as f32
    }

    /// Convert polar coordinates to cartesian
    fn polar_to_cartesian(&self, angle_deg: f32, radius: f32, center: Point) -> Point {
        let math_angle = (90.0 - angle_deg).to_radians();
        Point {
            x: center.x + radius * math_angle.cos(),
            y: center.y + radius * math_angle.sin(),
        }
    }
}

impl Default for ChartSpecGenerator {
    fn default() -> Self {
        Self::new()
    }
}

