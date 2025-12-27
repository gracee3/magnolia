//! Divisional chart (varga) helpers for Vedic astrology.
//! 
//! Vargas are derived charts that divide each sign into multiple parts.
//! Each varga has specific calculation rules based on sign qualities and planetary rulers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::ephemeris::types::{LayerPositions, PlanetPosition};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VargaPlanetPosition {
    pub lon: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrograde: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VargaLayer {
    #[serde(rename = "baseLayerId")]
    pub base_layer_id: String,
    #[serde(rename = "vargaId")]
    pub varga_id: String,
    pub label: String,
    pub planets: HashMap<String, VargaPlanetPosition>,
}

pub struct VargaSpec {
    pub label: &'static str,
    pub division: i32,
}

pub const SUPPORTED_VARGAS: &[(&str, VargaSpec)] = &[
    ("d2", VargaSpec { label: "Hora", division: 2 }),
    ("d3", VargaSpec { label: "Drekkana", division: 3 }),
    ("d4", VargaSpec { label: "Chaturthamsa", division: 4 }),
    ("d5", VargaSpec { label: "Panchamsa", division: 5 }),
    ("d6", VargaSpec { label: "Shashthamsa", division: 6 }),
    ("d7", VargaSpec { label: "Saptamsa", division: 7 }),
    ("d8", VargaSpec { label: "Ashtamsa", division: 8 }),
    ("d9", VargaSpec { label: "Navamsa", division: 9 }),
    ("d10", VargaSpec { label: "Dasamsa", division: 10 }),
    ("d12", VargaSpec { label: "Dvadasamsa", division: 12 }),
    ("d16", VargaSpec { label: "Shodasamsa", division: 16 }),
    ("d20", VargaSpec { label: "Vimsamsa", division: 20 }),
    ("d24", VargaSpec { label: "ChaturVimsamsa", division: 24 }),
    ("d27", VargaSpec { label: "Bhamsa", division: 27 }),
    ("d30", VargaSpec { label: "Trimsamsa", division: 30 }),
    ("d60", VargaSpec { label: "Shashtiamsa", division: 60 }),
];

const SIGN_QUALITIES: &[&str] = &[
    "movable", "fixed", "dual",
    "movable", "fixed", "dual",
    "movable", "fixed", "dual",
    "movable", "fixed", "dual",
];

const QUALITY_OFFSETS: &[i32] = &[
    0,  // movable -> Aries (0)
    8,  // fixed -> Sagittarius (8)
    4,  // dual -> Leo (4)
];

/// Generate derived varga layers for the requested divisional charts.
pub fn build_varga_layers(
    layer_id: &str,
    layer_positions: &LayerPositions,
    requested_vargas: &[String],
) -> HashMap<String, VargaLayer> {
    let planets = &layer_positions.planets;
    let mut results: HashMap<String, VargaLayer> = HashMap::new();
    
    for varga in requested_vargas {
        let varga_key = varga.to_lowercase();
        let spec = SUPPORTED_VARGAS.iter().find(|(id, _)| *id == varga_key);
        
        if let Some((_, spec)) = spec {
            if !planets.is_empty() {
                let positions = build_varga_positions(planets, &varga_key);
                results.insert(varga_key.clone(), VargaLayer {
                    base_layer_id: layer_id.to_string(),
                    varga_id: varga_key,
                    label: spec.label.to_string(),
                    planets: positions,
                });
            }
        }
    }
    
    results
}

fn build_varga_positions(
    planets: &HashMap<String, PlanetPosition>,
    varga_id: &str,
) -> HashMap<String, VargaPlanetPosition> {
    let mut varga_positions: HashMap<String, VargaPlanetPosition> = HashMap::new();
    
    // Map varga IDs to their calculation functions
    let calculator: Option<fn(f64) -> f64> = match varga_id {
        "d2" => Some(calculate_hora_d2),
        "d3" => Some(calculate_drekkana_d3),
        "d4" => Some(calculate_chaturthamsa_d4),
        "d7" => Some(calculate_saptamsa_d7),
        "d16" => Some(calculate_shodasamsa_d16),
        "d20" => Some(calculate_vimsamsa_d20),
        "d24" => Some(calculate_chaturvimsamsa_d24),
        "d27" => Some(calculate_bhamsa_d27),
        "d30" => Some(calculate_trimsamsa_d30),
        "d60" => Some(calculate_shashtiamsa_d60),
        _ => None,
    };
    
    let spec = SUPPORTED_VARGAS.iter().find(|(id, _)| *id == varga_id);
    
    if let Some(calc_fn) = calculator {
        // Use special calculation method
        for (obj_id, pos) in planets {
            let new_lon = calc_fn(pos.lon);
            varga_positions.insert(obj_id.clone(), VargaPlanetPosition {
                lon: new_lon,
                lat: Some(pos.lat),
                retrograde: Some(pos.retrograde),
            });
        }
    } else if let Some((_, spec)) = spec {
        // Use standard calculation method
        for (obj_id, pos) in planets {
            let new_lon = calculate_varga_longitude(pos.lon, spec.division);
            varga_positions.insert(obj_id.clone(), VargaPlanetPosition {
                lon: new_lon,
                lat: Some(pos.lat),
                retrograde: Some(pos.retrograde),
            });
        }
    }
    
    varga_positions
}

fn calculate_varga_longitude(longitude: f64, division: i32) -> f64 {
    if division <= 0 {
        panic!("division must be > 0 for varga calculations");
    }
    
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    let segment_size = 30.0 / division as f64;
    let part_index = (within_sign / segment_size) as i32;
    let remainder = within_sign - (part_index as f64 * segment_size);
    
    let quality = SIGN_QUALITIES[sign_index as usize % 12];
    let quality_idx = match quality {
        "movable" => 0,
        "fixed" => 1,
        "dual" => 2,
        _ => 0,
    };
    let start_offset = QUALITY_OFFSETS[quality_idx];
    let start_sign = (sign_index + start_offset) % 12;
    let varga_sign = (start_sign + part_index) % 12;
    let scaled_remainder = remainder * division as f64;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_hora_d2(longitude: f64) -> f64 {
    // D2 (Hora): Odd signs: 0-15° = Sun's hora (Leo), 15-30° = Moon's hora (Cancer)
    //            Even signs: 0-15° = Moon's hora (Cancer), 15-30° = Sun's hora (Leo)
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let sun_sign = 4; // Leo
    let moon_sign = 3; // Cancer
    
    let is_odd_sign = sign_index % 2 == 0; // 0-indexed: 0,2,4,6,8,10 are odd
    
    let varga_sign = if within_sign < 15.0 {
        // First half
        if is_odd_sign { sun_sign } else { moon_sign }
    } else {
        // Second half
        if is_odd_sign { moon_sign } else { sun_sign }
    };
    
    // Scale remainder: 0-15° maps to 0-30° in varga sign
    let remainder = within_sign % 15.0;
    let scaled_remainder = remainder * 2.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_drekkana_d3(longitude: f64) -> f64 {
    // D3 (Drekkana): 0-10°: 1st sign from current sign
    //                10-20°: 5th sign from current sign
    //                20-30°: 9th sign from current sign
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let offset = if within_sign < 10.0 {
        0  // 1st sign (self)
    } else if within_sign < 20.0 {
        4  // 5th sign
    } else {
        8  // 9th sign
    };
    
    let varga_sign = (sign_index + offset) % 12;
    let remainder = within_sign % 10.0;
    let scaled_remainder = remainder * 3.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_chaturthamsa_d4(longitude: f64) -> f64 {
    // D4 (Chaturthamsa): Uses 4 angles: 1st, 4th, 7th, 10th from the sign.
    // Each segment is 7°30' (7.5°).
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let segment_size = 7.5;
    let part_index = (within_sign / segment_size) as i32;
    
    // Angles: 1st (0), 4th (3), 7th (6), 10th (9)
    let angle_offsets = [0, 3, 6, 9];
    let offset = angle_offsets[part_index as usize % 4];
    
    let varga_sign = (sign_index + offset) % 12;
    let remainder = within_sign - (part_index as f64 * segment_size);
    let scaled_remainder = remainder * 4.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_saptamsa_d7(longitude: f64) -> f64 {
    // D7 (Saptamsa): Odd signs: start from sign itself
    //                Even signs: start from 7th sign from current sign
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let is_odd_sign = sign_index % 2 == 0; // 0-indexed: 0,2,4,6,8,10 are odd
    let segment_size = 30.0 / 7.0;
    let part_index = (within_sign / segment_size) as i32;
    
    let start_sign = if is_odd_sign {
        sign_index
    } else {
        (sign_index + 6) % 12  // 7th sign
    };
    
    let varga_sign = (start_sign + part_index) % 12;
    let remainder = within_sign - (part_index as f64 * segment_size);
    let scaled_remainder = remainder * 7.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_shodasamsa_d16(longitude: f64) -> f64 {
    // D16 (Shodasamsa): Movable: start from Aries (0)
    //                    Fixed: start from Leo (4)
    //                    Dual: start from Sagittarius (8)
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let quality = SIGN_QUALITIES[sign_index as usize % 12];
    let start_sign = match quality {
        "movable" => 0,  // Aries
        "fixed" => 4,    // Leo
        "dual" => 8,     // Sagittarius
        _ => 0,
    };
    
    let segment_size = 30.0 / 16.0;
    let part_index = (within_sign / segment_size) as i32;
    
    let varga_sign = (start_sign + part_index) % 12;
    let remainder = within_sign - (part_index as f64 * segment_size);
    let scaled_remainder = remainder * 16.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_vimsamsa_d20(longitude: f64) -> f64 {
    // D20 (Vimsamsa): Movable: start from Aries (0)
    //                 Fixed: start from Sagittarius (8)
    //                 Dual: start from Leo (4)
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let quality = SIGN_QUALITIES[sign_index as usize % 12];
    let start_sign = match quality {
        "movable" => 0,  // Aries
        "fixed" => 8,   // Sagittarius
        "dual" => 4,    // Leo
        _ => 0,
    };
    
    let segment_size = 30.0 / 20.0;
    let part_index = (within_sign / segment_size) as i32;
    
    let varga_sign = (start_sign + part_index) % 12;
    let remainder = within_sign - (part_index as f64 * segment_size);
    let scaled_remainder = remainder * 20.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_chaturvimsamsa_d24(longitude: f64) -> f64 {
    // D24 (ChaturVimsamsa): Odd signs: start from Leo (4)
    //                       Even signs: start from Cancer (3)
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let is_odd_sign = sign_index % 2 == 0; // 0-indexed: 0,2,4,6,8,10 are odd
    let start_sign = if is_odd_sign { 4 } else { 3 }; // Leo or Cancer
    
    let segment_size = 30.0 / 24.0;
    let part_index = (within_sign / segment_size) as i32;
    
    let varga_sign = (start_sign + part_index) % 12;
    let remainder = within_sign - (part_index as f64 * segment_size);
    let scaled_remainder = remainder * 24.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_bhamsa_d27(longitude: f64) -> f64 {
    // D27 (Bhamsa): Starts from Aries (0) for all signs.
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let start_sign = 0; // Aries
    let segment_size = 30.0 / 27.0;
    let part_index = (within_sign / segment_size) as i32;
    
    let varga_sign = (start_sign + part_index) % 12;
    let remainder = within_sign - (part_index as f64 * segment_size);
    let scaled_remainder = remainder * 27.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_trimsamsa_d30(longitude: f64) -> f64 {
    // D30 (Trimsamsa): Unequal divisions with planet rulers.
    // Odd signs: Mars(0-5°), Saturn(5-10°), Jupiter(10-18°), Mercury(18-25°), Venus(25-30°)
    // Even signs: Venus(0-5°), Mercury(5-10°), Jupiter(10-18°), Saturn(18-25°), Mars(25-30°)
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let is_odd_sign = sign_index % 2 == 0; // 0-indexed: 0,2,4,6,8,10 are odd
    
    // Planet rulers: Mars=Aries(0), Venus=Taurus(1), Mercury=Gemini(2), Moon=Cancer(3),
    // Sun=Leo(4), Mercury=Virgo(5), Venus=Libra(6), Mars=Scorpio(7), Jupiter=Sagittarius(8),
    // Saturn=Capricorn(9), Saturn=Aquarius(10), Jupiter=Pisces(11)
    // For Trimsamsa: Mars=0, Saturn=9, Jupiter=8, Mercury=2, Venus=1
    
    let (planet_sign, segment_start, segment_end) = if is_odd_sign {
        // Odd signs: Mars(0-5°), Saturn(5-10°), Jupiter(10-18°), Mercury(18-25°), Venus(25-30°)
        if within_sign < 5.0 {
            (0, 0.0, 5.0)  // Mars -> Aries
        } else if within_sign < 10.0 {
            (9, 5.0, 10.0)  // Saturn -> Capricorn
        } else if within_sign < 18.0 {
            (8, 10.0, 18.0)  // Jupiter -> Sagittarius
        } else if within_sign < 25.0 {
            (2, 18.0, 25.0)  // Mercury -> Gemini
        } else {
            (1, 25.0, 30.0)  // Venus -> Taurus
        }
    } else {
        // Even signs: Venus(0-5°), Mercury(5-10°), Jupiter(10-18°), Saturn(18-25°), Mars(25-30°)
        if within_sign < 5.0 {
            (1, 0.0, 5.0)  // Venus -> Taurus
        } else if within_sign < 10.0 {
            (2, 5.0, 10.0)  // Mercury -> Gemini
        } else if within_sign < 18.0 {
            (8, 10.0, 18.0)  // Jupiter -> Sagittarius
        } else if within_sign < 25.0 {
            (9, 18.0, 25.0)  // Saturn -> Capricorn
        } else {
            (0, 25.0, 30.0)  // Mars -> Aries
        }
    };
    
    // Scale remainder to 0-30° in the target sign
    let segment_size = segment_end - segment_start;
    let remainder = within_sign - segment_start;
    let scaled_remainder = (remainder / segment_size) * 30.0;
    
    (planet_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

fn calculate_shashtiamsa_d60(longitude: f64) -> f64 {
    // D60 (Shashtiamsa): Starts from Aries (0) for all signs.
    let lon = longitude % 360.0;
    let sign_index = (lon / 30.0) as i32;
    let within_sign = lon - (sign_index as f64 * 30.0);
    
    let start_sign = 0; // Aries
    let segment_size = 30.0 / 60.0;
    let part_index = (within_sign / segment_size) as i32;
    
    let varga_sign = (start_sign + part_index) % 12;
    let remainder = within_sign - (part_index as f64 * segment_size);
    let scaled_remainder = remainder * 60.0;
    
    (varga_sign as f64 * 30.0 + scaled_remainder) % 360.0
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_calculate_varga_longitude() {
        // Test standard varga calculation (D9 - Navamsa)
        let lon = 45.0; // 15° Taurus
        let result = calculate_varga_longitude(lon, 9);
        // Should be in a specific sign based on quality offset
        assert!(result >= 0.0 && result < 360.0);
    }
    
    #[test]
    fn test_calculate_hora_d2() {
        let lon = 0.0; // 0° Aries (odd sign, first half)
        let result = calculate_hora_d2(lon);
        // Should be in Leo (Sun's hora)
        assert!(result >= 120.0 && result < 150.0); // Leo range
    }
}

