//! Yoga detection helpers for Vedic astrology.
//! 
//! Yogas are planetary combinations that indicate specific life outcomes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::ephemeris::types::LayerPositions;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Yoga {
    pub name: String,
    #[serde(rename = "type")]
    pub yoga_type: String, // "benefic", "malefic", "mixed"
    pub description: String,
}

const BENEFIC_PLANETS: &[&str] = &["jupiter", "venus", "mercury", "moon"];
const MALEFIC_PLANETS: &[&str] = &["saturn", "mars", "rahu", "ketu", "sun"];

/// Normalize degrees to [0, 360).
fn normalize_degrees(value: f64) -> f64 {
    let mut normalized = value % 360.0;
    if normalized < 0.0 {
        normalized += 360.0;
    }
    normalized
}

/// Calculate angular difference between two longitudes.
fn angular_difference(lon1: f64, lon2: f64) -> f64 {
    let diff = (normalize_degrees(lon1) - normalize_degrees(lon2)).abs();
    diff.min(360.0 - diff)
}

/// Check if two planets are in conjunction.
fn is_conjunction(lon1: f64, lon2: f64, orb: f64) -> bool {
    angular_difference(lon1, lon2) <= orb
}

/// Check if two planets are in opposition.
#[allow(dead_code)]
fn is_opposition(lon1: f64, lon2: f64, orb: f64) -> bool {
    let diff = angular_difference(lon1, lon2);
    (diff - 180.0).abs() <= orb
}

/// Get house number (1-12) for a given longitude.
fn get_house_number(longitude: f64, ascendant: f64) -> i32 {
    let diff = normalize_degrees(longitude - ascendant);
    let house = (diff / 30.0) as i32 + 1;
    if house <= 12 { house } else { house - 12 }
}

/// Check if planet is in a kendra (1, 4, 7, 10 houses).
fn is_in_kendra(longitude: f64, ascendant: f64) -> bool {
    let house = get_house_number(longitude, ascendant);
    matches!(house, 1 | 4 | 7 | 10)
}

/// Check if planet is in a trikona (1, 5, 9 houses).
fn is_in_trikona(longitude: f64, ascendant: f64) -> bool {
    let house = get_house_number(longitude, ascendant);
    matches!(house, 1 | 5 | 9)
}

/// Identify classic Vedic yogas from planetary positions.
pub fn identify_yogas(layer_positions: &LayerPositions) -> Vec<Yoga> {
    let mut yogas: Vec<Yoga> = Vec::new();
    
    let planets = &layer_positions.planets;
    let houses = layer_positions.houses.as_ref();
    
    if planets.is_empty() || houses.is_none() {
        return yogas;
    }
    
    let houses = houses.unwrap();
    let angles = &houses.angles;
    let ascendant = angles.get("asc").copied().unwrap_or(0.0);
    
    // Get planet longitudes
    let planet_lons: HashMap<String, f64> = planets.iter()
        .map(|(id, pos)| (id.clone(), pos.lon))
        .collect();
    
    if planet_lons.is_empty() {
        return yogas;
    }
    
    // Helper to get planet longitude safely
    let get_lon = |planet_id: &str| -> Option<f64> {
        planet_lons.get(planet_id).copied()
    };
    
    // 1. Gajakesari Yoga - Jupiter and Moon in kendras or trikonas
    if let (Some(jupiter_lon), Some(moon_lon)) = (get_lon("jupiter"), get_lon("moon")) {
        if (is_in_kendra(jupiter_lon, ascendant) || is_in_trikona(jupiter_lon, ascendant)) &&
           (is_in_kendra(moon_lon, ascendant) || is_in_trikona(moon_lon, ascendant)) {
            yogas.push(Yoga {
                name: "Gajakesari Yoga".to_string(),
                yoga_type: "benefic".to_string(),
                description: "Jupiter and Moon in kendras or trikonas - brings wisdom and prosperity".to_string(),
            });
        }
    }
    
    // 2. Budh Aditya Yoga - Mercury and Sun conjunction
    if let (Some(mercury_lon), Some(sun_lon)) = (get_lon("mercury"), get_lon("sun")) {
        if is_conjunction(mercury_lon, sun_lon, 15.0) {
            yogas.push(Yoga {
                name: "Budh Aditya Yoga".to_string(),
                yoga_type: "benefic".to_string(),
                description: "Mercury and Sun in conjunction - brings intelligence and communication skills".to_string(),
            });
        }
    }
    
    // 3. Raj Yoga - Benefic planets in kendras and trikonas
    let mut benefic_in_kendra = false;
    let mut benefic_in_trikona = false;
    
    for benefic in BENEFIC_PLANETS {
        if let Some(lon) = get_lon(benefic) {
            if is_in_kendra(lon, ascendant) {
                benefic_in_kendra = true;
            }
            if is_in_trikona(lon, ascendant) {
                benefic_in_trikona = true;
            }
        }
    }
    
    if benefic_in_kendra && benefic_in_trikona {
        yogas.push(Yoga {
            name: "Raj Yoga".to_string(),
            yoga_type: "benefic".to_string(),
            description: "Benefic planets in both kendras and trikonas - brings power and authority".to_string(),
        });
    }
    
    // 4. Dhan Yoga - 2nd and 11th house lords in good positions
    // This is simplified - full implementation would need house lords
    if let (Some(venus_lon), Some(jupiter_lon)) = (get_lon("venus"), get_lon("jupiter")) {
        let venus_house = get_house_number(venus_lon, ascendant);
        let jupiter_house = get_house_number(jupiter_lon, ascendant);
        if venus_house == 2 || venus_house == 11 || jupiter_house == 2 || jupiter_house == 11 {
            yogas.push(Yoga {
                name: "Dhan Yoga".to_string(),
                yoga_type: "benefic".to_string(),
                description: "Wealth-giving planets in 2nd or 11th house - brings financial prosperity".to_string(),
            });
        }
    }
    
    // 5. Chandra-Mangal Yoga - Moon and Mars conjunction
    if let (Some(moon_lon), Some(mars_lon)) = (get_lon("moon"), get_lon("mars")) {
        if is_conjunction(moon_lon, mars_lon, 10.0) {
            yogas.push(Yoga {
                name: "Chandra-Mangal Yoga".to_string(),
                yoga_type: "mixed".to_string(),
                description: "Moon and Mars in conjunction - brings courage but may cause emotional volatility".to_string(),
            });
        }
    }
    
    // 6. Shubh Kartari Yoga - Benefic planets on both sides of Moon
    if let Some(moon_lon) = get_lon("moon") {
        let mut benefics_around_moon = 0;
        for benefic in BENEFIC_PLANETS {
            if *benefic == "moon" {
                continue;
            }
            if let Some(lon) = get_lon(benefic) {
                let diff = angular_difference(moon_lon, lon);
                if diff <= 30.0 {  // Within 30 degrees
                    benefics_around_moon += 1;
                }
            }
        }
        
        if benefics_around_moon >= 2 {
            yogas.push(Yoga {
                name: "Shubh Kartari Yoga".to_string(),
                yoga_type: "benefic".to_string(),
                description: "Two or more benefic planets around Moon - brings happiness and prosperity".to_string(),
            });
        }
    }
    
    // 7. Pap Kartari Yoga - Malefic planets on both sides of Moon
    if let Some(moon_lon) = get_lon("moon") {
        let mut malefics_around_moon = 0;
        for malefic in MALEFIC_PLANETS {
            if *malefic == "moon" {
                continue;
            }
            if let Some(lon) = get_lon(malefic) {
                let diff = angular_difference(moon_lon, lon);
                if diff <= 30.0 {  // Within 30 degrees
                    malefics_around_moon += 1;
                }
            }
        }
        
        if malefics_around_moon >= 2 {
            yogas.push(Yoga {
                name: "Pap Kartari Yoga".to_string(),
                yoga_type: "malefic".to_string(),
                description: "Two or more malefic planets around Moon - may cause difficulties".to_string(),
            });
        }
    }
    
    // 8. Neecha Bhanga Raj Yoga - Debilitated planet with benefic
    // Simplified version - full implementation needs exaltation/debilitation tables
    if let (Some(sun_lon), Some(jupiter_lon)) = (get_lon("sun"), get_lon("jupiter")) {
        // Sun is debilitated in Libra (180-210 degrees)
        let sun_normalized = normalize_degrees(sun_lon);
        if sun_normalized >= 180.0 && sun_normalized <= 210.0 {
            if is_conjunction(sun_lon, jupiter_lon, 10.0) ||
               is_in_kendra(jupiter_lon, ascendant) {
                yogas.push(Yoga {
                    name: "Neecha Bhanga Raj Yoga".to_string(),
                    yoga_type: "benefic".to_string(),
                    description: "Debilitated planet with benefic - cancels debilitation and brings success".to_string(),
                });
            }
        }
    }
    
    // 9. Vipreet Raj Yoga - Malefic in 6th, 8th, or 12th house
    for malefic in MALEFIC_PLANETS {
        if *malefic == "sun" {
            continue; // Sun is not typically considered malefic for this yoga
        }
        if let Some(lon) = get_lon(malefic) {
            let house = get_house_number(lon, ascendant);
            if matches!(house, 6 | 8 | 12) {
                yogas.push(Yoga {
                    name: "Vipreet Raj Yoga".to_string(),
                    yoga_type: "benefic".to_string(),
                    description: format!("{} in {}th house - turns adversity into success", 
                        malefic.chars().next().unwrap().to_uppercase().collect::<String>() + &malefic[1..], 
                        house),
                });
                break;
            }
        }
    }
    
    // 10. Pancha Mahapurusha Yoga - Strong planets in own signs or exaltation
    // Simplified - checks for planets in angular houses
    let mut strong_planets = Vec::new();
    for planet in &["sun", "moon", "mars", "mercury", "jupiter", "venus", "saturn"] {
        if let Some(lon) = get_lon(planet) {
            if is_in_kendra(lon, ascendant) {
                strong_planets.push(*planet);
            }
        }
    }
    
    if strong_planets.len() >= 3 {
        yogas.push(Yoga {
            name: "Pancha Mahapurusha Yoga".to_string(),
            yoga_type: "benefic".to_string(),
            description: "Multiple planets in angular houses - brings great achievements".to_string(),
        });
    }
    
    yogas
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_angular_difference() {
        assert!((angular_difference(0.0, 10.0) - 10.0).abs() < 0.01);
        assert!((angular_difference(350.0, 10.0) - 20.0).abs() < 0.01);
    }
    
    #[test]
    fn test_get_house_number() {
        let asc = 0.0; // Aries rising
        assert_eq!(get_house_number(0.0, asc), 1);
        assert_eq!(get_house_number(30.0, asc), 2);
        assert_eq!(get_house_number(90.0, asc), 4);
    }
    
    #[test]
    fn test_is_in_kendra() {
        let asc = 0.0;
        assert!(is_in_kendra(0.0, asc));   // 1st house
        assert!(is_in_kendra(90.0, asc));  // 4th house
        assert!(is_in_kendra(180.0, asc)); // 7th house
        assert!(is_in_kendra(270.0, asc)); // 10th house
        assert!(!is_in_kendra(60.0, asc)); // 3rd house
    }
}

