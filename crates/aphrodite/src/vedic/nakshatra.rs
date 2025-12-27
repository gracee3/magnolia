//! Nakshatra utilities for Vedic astrology.
//! 
//! Nakshatras are 27 lunar mansions, each spanning 13°20' (360/27 degrees).
//! Each nakshatra is divided into 4 padas (quarters).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::ephemeris::types::LayerPositions;

pub const NAKSHATRA_SEGMENT_SIZE: f64 = 360.0 / 27.0;
pub const PADA_SIZE: f64 = NAKSHATRA_SEGMENT_SIZE / 4.0;

// (slug, display_name, planetary lord)
pub const NAKSHATRA_ORDER: &[(&str, &str, &str)] = &[
    ("ashwini", "Ashwini", "ketu"),
    ("bharani", "Bharani", "venus"),
    ("krittika", "Krittika", "sun"),
    ("rohini", "Rohini", "moon"),
    ("mrigashira", "Mrigashira", "mars"),
    ("ardra", "Ardra", "rahu"),
    ("punarvasu", "Punarvasu", "jupiter"),
    ("pushya", "Pushya", "saturn"),
    ("ashlesha", "Ashlesha", "mercury"),
    ("magha", "Magha", "ketu"),
    ("purva_phalguni", "Purva Phalguni", "venus"),
    ("uttara_phalguni", "Uttara Phalguni", "sun"),
    ("hasta", "Hasta", "moon"),
    ("chitra", "Chitra", "mars"),
    ("swati", "Swati", "rahu"),
    ("vishakha", "Vishakha", "jupiter"),
    ("anuradha", "Anuradha", "saturn"),
    ("jyeshtha", "Jyeshtha", "mercury"),
    ("mula", "Mula", "ketu"),
    ("purva_ashadha", "Purva Ashadha", "venus"),
    ("uttara_ashadha", "Uttara Ashadha", "sun"),
    ("shravana", "Shravana", "moon"),
    ("dhanishta", "Dhanishta", "mars"),
    ("shatabhisha", "Shatabhisha", "rahu"),
    ("purva_bhadrapada", "Purva Bhadrapada", "jupiter"),
    ("uttara_bhadrapada", "Uttara Bhadrapada", "saturn"),
    ("revati", "Revati", "mercury"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseNakshatraRecord {
    pub id: String,
    pub name: String,
    pub lord: String,
    pub start: f64,
    pub end: f64,
    pub index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NakshatraMetadata {
    #[serde(flatten)]
    pub base: BaseNakshatraRecord,
    pub offset: f64,
    pub progress: f64,
    pub pada: i32,
    pub pada_fraction: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NakshatraPlacement {
    #[serde(rename = "objectId")]
    pub object_id: String,
    pub longitude: f64,
    #[serde(rename = "nakshatraId")]
    pub nakshatra_id: String,
    #[serde(rename = "nakshatraName")]
    pub nakshatra_name: String,
    #[serde(rename = "startDegree")]
    pub start_degree: f64,
    #[serde(rename = "endDegree")]
    pub end_degree: f64,
    pub lord: String,
    pub pada: i32,
    #[serde(rename = "padaFraction")]
    pub pada_fraction: f64,
}

fn build_nakshatra_table() -> Vec<BaseNakshatraRecord> {
    let mut table = Vec::new();
    for (idx, (slug, display_name, lord)) in NAKSHATRA_ORDER.iter().enumerate() {
        let start = idx as f64 * NAKSHATRA_SEGMENT_SIZE;
        let end = start + NAKSHATRA_SEGMENT_SIZE;
        table.push(BaseNakshatraRecord {
            id: slug.to_string(),
            name: display_name.to_string(),
            lord: lord.to_string(),
            start,
            end,
            index: idx,
        });
    }
    table
}

lazy_static::lazy_static! {
    static ref NAKSHATRA_TABLE: Vec<BaseNakshatraRecord> = build_nakshatra_table();
}

/// Normalize degrees to [0, 360).
pub fn normalize_degrees(value: f64) -> f64 {
    let mut normalized = value % 360.0;
    if normalized < 0.0 {
        normalized += 360.0;
    }
    normalized
}

/// Return metadata for the nakshatra containing the given longitude.
/// 
/// Returns a struct containing id, name, lord, index, start/end degrees,
/// within-nakshatra offset, pada number, and pada fraction.
pub fn get_nakshatra_for_longitude(longitude: f64) -> NakshatraMetadata {
    let lon = normalize_degrees(longitude);
    let index = (lon / NAKSHATRA_SEGMENT_SIZE) as usize % NAKSHATRA_TABLE.len();
    let entry = &NAKSHATRA_TABLE[index];
    
    let offset = lon - entry.start;
    let pada = (offset / PADA_SIZE) as i32 + 1;
    let pada_offset = offset - ((pada - 1) as f64 * PADA_SIZE);
    let pada_fraction = pada_offset / PADA_SIZE;
    
    NakshatraMetadata {
        base: entry.clone(),
        offset,
        progress: offset / NAKSHATRA_SEGMENT_SIZE,
        pada,
        pada_fraction,
    }
}

fn build_placement(object_id: String, longitude: f64) -> NakshatraPlacement {
    let metadata = get_nakshatra_for_longitude(longitude);
    NakshatraPlacement {
        object_id,
        longitude: normalize_degrees(longitude),
        nakshatra_id: metadata.base.id.clone(),
        nakshatra_name: metadata.base.name.clone(),
        start_degree: metadata.base.start,
        end_degree: metadata.base.end,
        lord: metadata.base.lord.clone(),
        pada: metadata.pada,
        pada_fraction: metadata.pada_fraction,
    }
}

/// Annotate layer planets (and optionally angles) with nakshatra placements.
pub fn annotate_layer_nakshatras(
    layer_positions: &LayerPositions,
    include_angles: bool,
    object_filter: Option<&Vec<String>>,
) -> HashMap<String, NakshatraPlacement> {
    let mut placements: HashMap<String, NakshatraPlacement> = HashMap::new();
    
    let planets = &layer_positions.planets;
    let target_ids: Vec<&String> = if let Some(filter) = object_filter {
        planets.keys().filter(|id| filter.contains(id)).collect()
    } else {
        planets.keys().collect()
    };
    
    for obj_id in target_ids {
        if let Some(planet) = planets.get(obj_id) {
            placements.insert(obj_id.clone(), build_placement(obj_id.clone(), planet.lon));
        }
    }
    
    if include_angles {
        if let Some(houses) = &layer_positions.houses {
            for (angle_id, lon) in &houses.angles {
                placements.insert(angle_id.clone(), build_placement(angle_id.clone(), *lon));
            }
        }
    }
    
    placements
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_normalize_degrees() {
        assert_eq!(normalize_degrees(0.0), 0.0);
        assert_eq!(normalize_degrees(360.0), 0.0);
        assert_eq!(normalize_degrees(720.0), 0.0);
        assert_eq!(normalize_degrees(-10.0), 350.0);
        assert_eq!(normalize_degrees(370.0), 10.0);
    }
    
    #[test]
    fn test_get_nakshatra_for_longitude() {
        let meta = get_nakshatra_for_longitude(0.0);
        assert_eq!(meta.base.id, "ashwini");
        assert_eq!(meta.base.lord, "ketu");
        assert_eq!(meta.pada, 1);
        
        let meta2 = get_nakshatra_for_longitude(13.33);
        assert_eq!(meta2.base.id, "ashwini");
        assert!(meta2.pada >= 1 && meta2.pada <= 4);
    }
}

