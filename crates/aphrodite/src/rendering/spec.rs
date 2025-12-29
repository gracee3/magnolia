use crate::rendering::primitives::{Color, Point, Shape};
use serde::{Deserialize, Serialize};

/// Chart metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartMetadata {
    pub layers: Vec<LayerMetadata>,
    pub aspect_sets: Vec<AspectSetMetadata>,
}

/// Layer metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerMetadata {
    pub id: String,
    pub kind: String,
}

/// Aspect set metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspectSetMetadata {
    pub id: String,
    pub layer_ids: Vec<String>,
}

/// Chart specification - declarative description of chart to render
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartSpec {
    pub width: f32,
    pub height: f32,
    pub center: Point,
    pub rotation_offset: f32, // For chart rotation
    pub background_color: Color,
    pub shapes: Vec<Shape>,
    pub metadata: ChartMetadata,
}

impl ChartSpec {
    /// Create a new empty chart spec
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            center: Point {
                x: width / 2.0,
                y: height / 2.0,
            },
            rotation_offset: 0.0,
            background_color: Color::BLACK,
            shapes: Vec::new(),
            metadata: ChartMetadata {
                layers: Vec::new(),
                aspect_sets: Vec::new(),
            },
        }
    }
}

