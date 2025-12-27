//! Vedic astrology types and integration structures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::vedic::nakshatra::NakshatraPlacement;
use crate::vedic::vargas::VargaLayer;
use crate::vedic::yogas::Yoga;
use crate::vedic::dashas::VimshottariResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NakshatraLayer {
    #[serde(rename = "layerId")]
    pub layer_id: String,
    pub placements: HashMap<String, NakshatraPlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VedicLayerData {
    #[serde(rename = "layerId")]
    pub layer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nakshatras: Option<NakshatraLayer>,
    pub vargas: HashMap<String, VargaLayer>,
    pub yogas: Vec<Yoga>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VedicPayload {
    pub layers: HashMap<String, VedicLayerData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashas: Option<VimshottariResponse>,
}

