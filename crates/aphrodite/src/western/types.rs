//! Western astrology types and integration structures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::western::dignities::DignityResult;
use crate::western::decans::DecanInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WesternLayerData {
    #[serde(rename = "layerId")]
    pub layer_id: String,
    pub dignities: HashMap<String, Vec<DignityResult>>,
    pub decans: HashMap<String, DecanInfo>,
}

