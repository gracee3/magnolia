//! Dignities calculation for Western astrology.
//! 
//! Calculates rulership, detriment, exaltation, fall, and exact exaltation for planets.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DignityType {
    Rulership,
    Detriment,
    Exaltation,
    Fall,
    ExactExaltation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DignityResult {
    #[serde(rename = "type")]
    pub dignity_type: DignityType,
    pub sign: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degree: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExactExaltation {
    #[serde(rename = "planetId")]
    pub planet_id: String,
    pub position: f64, // Longitude in degrees
    pub orbit: f64, // Orb in degrees (default 2)
}

/// Get sign index (0-11) from longitude
fn get_sign_index(longitude: f64) -> usize {
    let normalized = longitude % 360.0;
    (normalized / 30.0) as usize
}

/// Get sign name from index
fn get_sign_name(sign_index: usize) -> String {
    const SIGN_NAMES: &[&str] = &[
        "aries", "taurus", "gemini", "cancer",
        "leo", "virgo", "libra", "scorpio",
        "sagittarius", "capricorn", "aquarius", "pisces",
    ];
    SIGN_NAMES[sign_index % 12].to_string()
}

/// Check if planet has exact exaltation
fn has_exact_exaltation(
    planet_position: f64,
    exact_position: f64,
    orbit: f64,
) -> bool {
    let diff1 = (planet_position - exact_position).abs();
    let diff2 = (planet_position - exact_position + 360.0).abs();
    let diff3 = (planet_position - exact_position - 360.0).abs();
    
    diff1 <= orbit || diff2 <= orbit || diff3 <= orbit
}

pub struct DignitiesService;

impl DignitiesService {
    /// Get dignities for a planet based on its longitude
    pub fn get_dignities(
        &self,
        planet_id: &str,
        longitude: f64,
        exact_exaltations: Option<&[ExactExaltation]>,
    ) -> Vec<DignityResult> {
        let planet_id_lower = planet_id.to_lowercase();
        
        if planet_id_lower.is_empty() {
            return Vec::new();
        }
        
        let mut result: Vec<DignityResult> = Vec::new();
        let sign_index = get_sign_index(longitude);
        let sign_name = get_sign_name(sign_index);
        let normalized_position = longitude % 360.0;
        
        match planet_id_lower.as_str() {
            "sun" => {
                if sign_index == 4 { // Leo
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 10 { // Aquarius
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
                if sign_index == 0 { // Aries
                    result.push(DignityResult {
                        dignity_type: DignityType::Exaltation,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 5 { // Virgo
                    result.push(DignityResult {
                        dignity_type: DignityType::Fall,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "moon" => {
                if sign_index == 3 { // Cancer
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 9 { // Capricorn
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
                if sign_index == 1 { // Taurus
                    result.push(DignityResult {
                        dignity_type: DignityType::Exaltation,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 7 { // Scorpio
                    result.push(DignityResult {
                        dignity_type: DignityType::Fall,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "mercury" => {
                if sign_index == 2 || sign_index == 5 { // Gemini or Virgo
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 8 || sign_index == 11 { // Sagittarius or Pisces
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
                if sign_index == 5 { // Virgo
                    result.push(DignityResult {
                        dignity_type: DignityType::Exaltation,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 11 { // Pisces
                    result.push(DignityResult {
                        dignity_type: DignityType::Fall,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "venus" => {
                if sign_index == 1 || sign_index == 6 { // Taurus or Libra
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 0 || sign_index == 7 { // Aries or Scorpio
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
                if sign_index == 11 { // Pisces
                    result.push(DignityResult {
                        dignity_type: DignityType::Exaltation,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 5 { // Virgo
                    result.push(DignityResult {
                        dignity_type: DignityType::Fall,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "mars" => {
                if sign_index == 0 || sign_index == 7 { // Aries or Scorpio
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 6 || sign_index == 1 { // Libra or Taurus
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
                if sign_index == 9 { // Capricorn
                    result.push(DignityResult {
                        dignity_type: DignityType::Exaltation,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 3 { // Cancer
                    result.push(DignityResult {
                        dignity_type: DignityType::Fall,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "jupiter" => {
                if sign_index == 8 || sign_index == 11 { // Sagittarius or Pisces
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 2 || sign_index == 5 { // Gemini or Virgo
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
                if sign_index == 3 { // Cancer
                    result.push(DignityResult {
                        dignity_type: DignityType::Exaltation,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 9 { // Capricorn
                    result.push(DignityResult {
                        dignity_type: DignityType::Fall,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "saturn" => {
                if sign_index == 9 || sign_index == 10 { // Capricorn or Aquarius
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 3 || sign_index == 4 { // Cancer or Leo
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
                if sign_index == 6 { // Libra
                    result.push(DignityResult {
                        dignity_type: DignityType::Exaltation,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 0 { // Aries
                    result.push(DignityResult {
                        dignity_type: DignityType::Fall,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "uranus" => {
                if sign_index == 10 { // Aquarius
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 4 { // Leo
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "neptune" => {
                if sign_index == 11 { // Pisces
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 5 { // Virgo
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            "pluto" => {
                if sign_index == 7 { // Scorpio
                    result.push(DignityResult {
                        dignity_type: DignityType::Rulership,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 1 { // Taurus
                    result.push(DignityResult {
                        dignity_type: DignityType::Detriment,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
                if sign_index == 0 { // Aries
                    result.push(DignityResult {
                        dignity_type: DignityType::Exaltation,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                } else if sign_index == 6 { // Libra
                    result.push(DignityResult {
                        dignity_type: DignityType::Fall,
                        sign: sign_name.clone(),
                        degree: None,
                    });
                }
            }
            _ => {}
        }
        
        // Check for exact exaltation if provided
        if let Some(exact_exaltations) = exact_exaltations {
            for exact_exalt in exact_exaltations {
                if exact_exalt.planet_id.to_lowercase() == planet_id_lower {
                    let orbit = exact_exalt.orbit;
                    if has_exact_exaltation(normalized_position, exact_exalt.position, orbit) {
                        result.push(DignityResult {
                            dignity_type: DignityType::ExactExaltation,
                            sign: sign_name.clone(),
                            degree: Some(exact_exalt.position),
                        });
                    }
                }
            }
        }
        
        result
    }
    
    /// Get default exact exaltation positions (based on Aleister Crowley)
    pub fn get_default_exact_exaltations() -> Vec<ExactExaltation> {
        vec![
            ExactExaltation { planet_id: "sun".to_string(), position: 19.0, orbit: 2.0 },
            ExactExaltation { planet_id: "moon".to_string(), position: 33.0, orbit: 2.0 },
            ExactExaltation { planet_id: "mercury".to_string(), position: 165.0, orbit: 2.0 },
            ExactExaltation { planet_id: "venus".to_string(), position: 357.0, orbit: 2.0 },
            ExactExaltation { planet_id: "mars".to_string(), position: 298.0, orbit: 2.0 },
            ExactExaltation { planet_id: "jupiter".to_string(), position: 95.0, orbit: 2.0 },
            ExactExaltation { planet_id: "saturn".to_string(), position: 201.0, orbit: 2.0 },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_get_dignities_sun() {
        let service = DignitiesService;
        // Sun in Leo (120-150 degrees)
        let dignities = service.get_dignities("sun", 135.0, None);
        assert!(dignities.iter().any(|d| d.dignity_type == DignityType::Rulership));
    }
    
    #[test]
    fn test_get_dignities_moon() {
        let service = DignitiesService;
        // Moon in Cancer (90-120 degrees)
        let dignities = service.get_dignities("moon", 105.0, None);
        assert!(dignities.iter().any(|d| d.dignity_type == DignityType::Rulership));
    }
}

