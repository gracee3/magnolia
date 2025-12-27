use serde::{Deserialize, Serialize};

/// Core aspect information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspectCore {
    /// Aspect type: "conjunction", "trine", etc.
    pub aspect_type: String,
    /// Exact angle for this aspect (0, 60, 90, 120, 180)
    pub exact_angle: f64,
    /// Orb value (deviation from exact angle)
    pub orb: f64,
    /// Precision (same as orb)
    pub precision: f64,
    /// Whether the aspect is applying (approaching exact)
    pub is_applying: bool,
    /// Whether the aspect is exact (within 0.1 degrees)
    pub is_exact: bool,
    /// Whether either planet is retrograde
    pub is_retrograde: bool,
}

/// Reference to an object in an aspect
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspectObjectRef {
    pub layer_id: String,
    pub object_type: String, // "planet", "house", "angle"
    pub object_id: String,   // "sun", "moon", "asc", "1", etc.
}

/// An aspect pair between two objects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspectPair {
    pub from: AspectObjectRef,
    pub to: AspectObjectRef,
    pub aspect: AspectCore,
}

/// A set of aspects (intra-layer or inter-layer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspectSet {
    pub id: String,
    pub label: String,
    pub kind: String, // "intra_layer" or "inter_layer"
    pub layer_ids: Vec<String>,
    pub pairs: Vec<AspectPair>,
}

/// Settings for aspect calculations
#[derive(Debug, Clone)]
pub struct AspectSettings {
    /// Orb settings per aspect type
    pub orb_settings: std::collections::HashMap<String, f64>,
    /// List of planet IDs to include
    pub include_objects: Vec<String>,
    /// Whether to only include major aspects
    pub only_major: Option<bool>,
}

