//! Western astrology decans calculations.
//! 
//! Each sign is divided into 3 decans (10 degrees each), with decan rulers based on element groups.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Element {
    Fire,
    Earth,
    Air,
    Water,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignMeta {
    pub name: String,
    pub ruler: String,
    pub element: Element,
}

// Use lazy_static for SIGNS since we can't call to_string() in const context
lazy_static::lazy_static! {
    static ref SIGNS: Vec<SignMeta> = vec![
        SignMeta { name: "aries".to_string(), ruler: "mars".to_string(), element: Element::Fire },
        SignMeta { name: "taurus".to_string(), ruler: "venus".to_string(), element: Element::Earth },
        SignMeta { name: "gemini".to_string(), ruler: "mercury".to_string(), element: Element::Air },
        SignMeta { name: "cancer".to_string(), ruler: "moon".to_string(), element: Element::Water },
        SignMeta { name: "leo".to_string(), ruler: "sun".to_string(), element: Element::Fire },
        SignMeta { name: "virgo".to_string(), ruler: "mercury".to_string(), element: Element::Earth },
        SignMeta { name: "libra".to_string(), ruler: "venus".to_string(), element: Element::Air },
        SignMeta { name: "scorpio".to_string(), ruler: "mars".to_string(), element: Element::Water },
        SignMeta { name: "sagittarius".to_string(), ruler: "jupiter".to_string(), element: Element::Fire },
        SignMeta { name: "capricorn".to_string(), ruler: "saturn".to_string(), element: Element::Earth },
        SignMeta { name: "aquarius".to_string(), ruler: "saturn".to_string(), element: Element::Air },
        SignMeta { name: "pisces".to_string(), ruler: "jupiter".to_string(), element: Element::Water },
    ];
}

const SIGN_ORDER: &[&str] = &[
    "aries", "taurus", "gemini", "cancer",
    "leo", "virgo", "libra", "scorpio",
    "sagittarius", "capricorn", "aquarius", "pisces",
];

// Build element groups
lazy_static::lazy_static! {
    static ref ELEMENT_GROUPS: std::collections::HashMap<Element, Vec<SignMeta>> = {
        let mut groups: std::collections::HashMap<Element, Vec<SignMeta>> = std::collections::HashMap::new();
        for sign in SIGNS.iter() {
            groups.entry(sign.element).or_insert_with(Vec::new).push(sign.clone());
        }
        // Sort by zodiac order
        for group in groups.values_mut() {
            group.sort_by_key(|s| SIGN_ORDER.iter().position(|&name| name == s.name).unwrap_or(0));
        }
        groups
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecanInfo {
    pub sign: String,
    pub element: Element,
    #[serde(rename = "signRuler")]
    pub sign_ruler: String,
    #[serde(rename = "decanIndex")]
    pub decan_index: u8, // 1, 2, or 3
    #[serde(rename = "degreeInSign")]
    pub degree_in_sign: f64, // 0 <= x < 30
    #[serde(rename = "decanRuler")]
    pub decan_ruler: String,
}

/// Given degree in sign (0–29.999...), returns decan index 1, 2, or 3.
pub fn get_decan_index(degree_in_sign: f64) -> u8 {
    if degree_in_sign < 0.0 || degree_in_sign >= 30.0 {
        panic!("degree_in_sign must be in [0, 30), got {}", degree_in_sign);
    }
    
    if degree_in_sign < 10.0 {
        1
    } else if degree_in_sign < 20.0 {
        2
    } else {
        3
    }
}

/// Compute decan info given a sign and degree in that sign.
pub fn get_decan_info_for_sign_and_degree(
    sign: &str,
    degree_in_sign: f64,
) -> Result<DecanInfo, String> {
    let sign_meta = SIGNS.iter()
        .find(|s| s.name == sign)
        .ok_or_else(|| format!("Unknown sign: {}", sign))?;
    
    let decan_index = get_decan_index(degree_in_sign);
    
    let group = ELEMENT_GROUPS.get(&sign_meta.element)
        .ok_or_else(|| format!("Element group not found for {}", sign))?;
    
    let group_index = group.iter()
        .position(|g| g.name == sign)
        .ok_or_else(|| format!("Sign {} not found in element group", sign))?;
    
    // Rotate through the 3 rulers in the element group
    let ruler_index = (group_index + (decan_index as usize - 1)) % group.len();
    let decan_ruler = group[ruler_index].ruler.clone();
    
    Ok(DecanInfo {
        sign: sign.to_string(),
        element: sign_meta.element,
        sign_ruler: sign_meta.ruler.clone(),
        decan_index,
        degree_in_sign,
        decan_ruler,
    })
}

/// Optional helper: from absolute longitude 0–360.
pub fn get_decan_info_from_longitude(longitude: f64) -> DecanInfo {
    let lon = ((longitude % 360.0) + 360.0) % 360.0; // normalize
    let sign_index = (lon / 30.0) as usize;
    let degree_in_sign = lon - (sign_index as f64 * 30.0);
    let sign = SIGN_ORDER[sign_index % 12];
    
    get_decan_info_for_sign_and_degree(sign, degree_in_sign)
        .expect("Failed to get decan info")
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_get_decan_index() {
        assert_eq!(get_decan_index(0.0), 1);
        assert_eq!(get_decan_index(5.0), 1);
        assert_eq!(get_decan_index(9.999), 1);
        assert_eq!(get_decan_index(10.0), 2);
        assert_eq!(get_decan_index(15.0), 2);
        assert_eq!(get_decan_index(19.999), 2);
        assert_eq!(get_decan_index(20.0), 3);
        assert_eq!(get_decan_index(25.0), 3);
        assert_eq!(get_decan_index(29.999), 3);
    }
    
    #[test]
    fn test_get_decan_info_for_sign_and_degree() {
        let info = get_decan_info_for_sign_and_degree("aries", 5.0).unwrap();
        assert_eq!(info.sign, "aries");
        assert_eq!(info.decan_index, 1);
        assert_eq!(info.sign_ruler, "mars");
        // First decan of Aries (fire) should be ruled by Mars (first in fire group)
        assert_eq!(info.decan_ruler, "mars");
    }
}

