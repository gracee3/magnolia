use crate::ephemeris::types::LayerPositions;
use std::collections::HashMap;

/// Sign names and glyphs
const SIGNS: &[(&str, &str)] = &[
    ("aries", "♈"),
    ("taurus", "♉"),
    ("gemini", "♊"),
    ("cancer", "♋"),
    ("leo", "♌"),
    ("virgo", "♍"),
    ("libra", "♎"),
    ("scorpio", "♏"),
    ("sagittarius", "♐"),
    ("capricorn", "♑"),
    ("aquarius", "♒"),
    ("pisces", "♓"),
];

/// Get sign index (0-11) from longitude
pub fn get_sign_index(longitude: f64) -> u8 {
    let normalized = longitude % 360.0;
    let normalized = if normalized == 360.0 { 0.0 } else { normalized };
    (normalized / 30.0).floor() as u8
}

/// Get position within sign (0-30)
pub fn get_sign_degree(longitude: f64) -> f64 {
    longitude % 30.0
}

/// Get house index from planet longitude
pub fn get_house_index(planet_lon: f64, houses: &HashMap<String, f64>) -> Option<u8> {
    if houses.is_empty() {
        return None;
    }

    // Convert cusps to sorted array
    let mut cusps: Vec<(u8, f64)> = houses
        .iter()
        .filter_map(|(k, v)| k.parse::<u8>().ok().map(|num| (num, *v)))
        .collect();
    cusps.sort_by(|a, b| a.0.cmp(&b.0));

    // Find which house contains the planet
    for i in 0..cusps.len() {
        let current_house_num = cusps[i].0;
        let current_cusp = cusps[i].1;
        let next_index = (i + 1) % cusps.len();
        let next_cusp = cusps[next_index].1;

        // Handle wrap-around (next cusp < current cusp)
        if next_cusp < current_cusp {
            // Planet is in this house if it's >= current cusp or < next cusp
            if planet_lon >= current_cusp || planet_lon < next_cusp {
                return Some((current_house_num - 1) as u8);
            }
        } else {
            // Normal case: next cusp > current cusp
            if planet_lon >= current_cusp && planet_lon < next_cusp {
                return Some((current_house_num - 1) as u8);
            }
        }
    }

    // Default to house 1 if not found
    Some(0)
}

/// Sign ring item
#[derive(Debug, Clone)]
pub struct SignRingItem {
    pub id: String,
    pub kind: String,
    pub index: u8,
    pub label: String,
    pub glyph: Option<String>,
    pub start_lon: f64,
    pub end_lon: f64,
}

/// House ring item
#[derive(Debug, Clone)]
pub struct HouseRingItem {
    pub id: String,
    pub kind: String,
    pub house_index: u8,
    pub lon: f64,
}

/// Planet ring item
#[derive(Debug, Clone)]
pub struct PlanetRingItem {
    pub id: String,
    pub kind: String,
    pub planet_id: String,
    pub layer_id: String,
    pub lon: f64,
    pub lat: Option<f64>,
    pub speed_lon: Option<f64>,
    pub retrograde: Option<bool>,
    pub sign_index: u8,
    pub sign_degree: f64,
    pub house_index: Option<u8>,
}

/// Aspect ring item
#[derive(Debug, Clone)]
pub struct AspectRingItem {
    pub id: String,
    pub kind: String,
    pub aspect_id: String,
    pub from_lon: f64,
    pub to_lon: f64,
    pub aspect_type: String,
}

/// Ring item (enum of all types)
#[derive(Debug, Clone)]
pub enum RingItem {
    Sign(SignRingItem),
    House(HouseRingItem),
    Planet(PlanetRingItem),
    Aspect(AspectRingItem),
}

/// Build static zodiac items (12 signs)
pub fn build_static_zodiac_items(slug: &str) -> Vec<SignRingItem> {
    let mut items = Vec::new();

    for (i, (sign_name, glyph)) in SIGNS.iter().enumerate() {
        let start_lon = i as f64 * 30.0;
        let end_lon = (i + 1) as f64 * 30.0;

        let label = format!(
            "{}{}",
            sign_name.chars().next().unwrap().to_uppercase(),
            &sign_name[1..]
        );

        items.push(SignRingItem {
            id: format!("{}_sign_{}", slug, sign_name),
            kind: "sign".to_string(),
            index: i as u8,
            label,
            glyph: Some(glyph.to_string()),
            start_lon,
            end_lon,
        });
    }

    items
}

/// Build house items from layer positions
pub fn build_house_items(
    slug: &str,
    _layer_id: &str,
    positions: &LayerPositions,
) -> Vec<HouseRingItem> {
    let mut items = Vec::new();

    if let Some(houses) = &positions.houses {
        for (house_num_str, cusp_lon) in &houses.cusps {
            if let Ok(house_num) = house_num_str.parse::<u8>() {
                let house_index = house_num - 1;

                items.push(HouseRingItem {
                    id: format!("{}_house_{}", slug, house_num_str),
                    kind: "houseCusp".to_string(),
                    house_index,
                    lon: *cusp_lon,
                });
            }
        }
    }

    items
}

/// Build planet items from layer positions
pub fn build_planet_items(
    slug: &str,
    layer_id: &str,
    positions: &LayerPositions,
    include_objects: Option<&[String]>,
) -> Vec<PlanetRingItem> {
    let mut items = Vec::new();

    let planets = &positions.planets;
    let houses = &positions.houses;

    // Add planets
    for (planet_id, planet_pos) in planets {
        let lon = planet_pos.lon;
        let sign_index = get_sign_index(lon);
        let sign_degree = get_sign_degree(lon);
        let house_index = houses
            .as_ref()
            .and_then(|h| get_house_index(lon, &h.cusps));

        items.push(PlanetRingItem {
            id: format!("{}_{}", slug, planet_id),
            kind: "planet".to_string(),
            planet_id: planet_id.clone(),
            layer_id: layer_id.to_string(),
            lon,
            lat: Some(planet_pos.lat),
            speed_lon: Some(planet_pos.speed_lon),
            retrograde: Some(planet_pos.retrograde),
            sign_index,
            sign_degree,
            house_index,
        });
    }

    // Add angles (asc, mc, ic, dc) from houses if requested
    if let Some(houses) = houses {
        let angle_ids = ["asc", "mc", "ic", "dc"];
        for angle_id in &angle_ids {
            if let Some(lon) = houses.angles.get(*angle_id) {
                // Check if angle is in include_objects
                if let Some(include) = include_objects {
                    if !include.iter().any(|obj| obj == angle_id) {
                        continue;
                    }
                }

                let sign_index = get_sign_index(*lon);
                let sign_degree = get_sign_degree(*lon);
                let house_index = get_house_index(*lon, &houses.cusps);

                items.push(PlanetRingItem {
                    id: format!("{}_{}", slug, angle_id),
                    kind: "planet".to_string(),
                    planet_id: angle_id.to_string(),
                    layer_id: layer_id.to_string(),
                    lon: *lon,
                    lat: None,
                    speed_lon: None,
                    retrograde: None,
                    sign_index,
                    sign_degree,
                    house_index,
                });
            }
        }
    }

    items
}

