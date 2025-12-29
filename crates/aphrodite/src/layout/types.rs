use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Ring type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RingType {
    Signs,
    Houses,
    Planets,
    Aspects,
}

/// Data source for a ring
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RingDataSource {
    StaticZodiac,
    StaticNakshatras,
    LayerHouses {
        layer_id: String,
    },
    LayerPlanets {
        layer_id: String,
    },
    LayerVargaPlanets {
        layer_id: String,
        varga_id: String,
    },
    AspectSet {
        aspect_set_id: String,
        filter: Option<AspectSetFilter>,
    },
}

/// Filter for aspect sets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspectSetFilter {
    pub include_types: Option<Vec<String>>,
    pub min_strength: Option<f64>,
    pub only_major: Option<bool>,
}

/// Definition for a single ring in a wheel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RingDefinition {
    pub slug: String,
    #[serde(rename = "type")]
    pub ring_type: RingType,
    pub label: String,
    #[serde(rename = "orderIndex")]
    pub order_index: u32,
    #[serde(rename = "radiusInner")]
    pub radius_inner: f32,
    #[serde(rename = "radiusOuter")]
    pub radius_outer: f32,
    #[serde(rename = "dataSource")]
    pub data_source: RingDataSource,
    #[serde(rename = "displayOptions", default)]
    pub display_options: HashMap<String, serde_json::Value>,
}

/// Complete wheel definition with all rings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WheelDefinition {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub rings: Vec<RingDefinition>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

/// Wheel definition with preset overrides and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WheelDefinitionWithPresets {
    #[serde(flatten)]
    pub wheel: WheelDefinition,
    #[serde(rename = "defaultVisualConfig", default)]
    pub default_visual_config: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "defaultGlyphConfig", default)]
    pub default_glyph_config: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

