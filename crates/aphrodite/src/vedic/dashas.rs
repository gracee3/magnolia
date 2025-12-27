//! Vimshottari and other dasha calculations for Vedic astrology.
//! 
//! Dashas are time periods ruled by planets, calculated based on the Moon's nakshatra.

use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use crate::ephemeris::types::LayerPositions;
use crate::vedic::nakshatra::get_nakshatra_for_longitude;

pub const VIMSHOTTARI_TOTAL_YEARS: f64 = 120.0;
pub const VIMSHOTTARI_YEAR_DAYS: f64 = 365.25; // Placeholder synodic year

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DashaLevel {
    Mahadasha,
    Antardasha,
    Pratyantardasha,
}

const DEPTH_LEVELS: &[DashaLevel] = &[
    DashaLevel::Mahadasha,
    DashaLevel::Antardasha,
    DashaLevel::Pratyantardasha,
];

type PlanetYears = (&'static str, f64);

const VIMSHOTTARI_SEQUENCE: &[PlanetYears] = &[
    ("ketu", 7.0),
    ("venus", 20.0),
    ("sun", 6.0),
    ("moon", 10.0),
    ("mars", 7.0),
    ("rahu", 18.0),
    ("jupiter", 16.0),
    ("saturn", 19.0),
    ("mercury", 17.0),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashaPeriod {
    pub planet: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    #[serde(rename = "durationDays")]
    pub duration_days: f64,
    pub level: DashaLevel,
    pub children: Vec<DashaPeriod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VimshottariResponse {
    pub system: String,
    pub depth: DashaLevel,
    #[serde(rename = "birthDateTime")]
    pub birth_date_time: DateTime<Utc>,
    pub periods: Vec<DashaPeriod>,
}

/// Compute Vimshottari dasha periods based on the Moon's sidereal longitude.
pub fn compute_vimshottari_dasha(
    birth_datetime: DateTime<Utc>,
    layer_positions: &LayerPositions,
    depth: DashaLevel,
) -> Result<Vec<DashaPeriod>, String> {
    let moon = layer_positions.planets.get("moon")
        .ok_or_else(|| "Moon position required for Vimshottari dasha calculation".to_string())?;
    
    let moon_meta = get_nakshatra_for_longitude(moon.lon);
    let moon_lord = moon_meta.base.lord.clone();
    let progress = moon_meta.progress;
    let start_index = find_sequence_index(&moon_lord, VIMSHOTTARI_SEQUENCE)?;
    
    let target_depth_index = DEPTH_LEVELS.iter().position(|&d| d == depth)
        .unwrap_or(0);
    let mut current_start = birth_datetime;
    let mut periods: Vec<DashaPeriod> = Vec::new();
    
    for offset in 0..VIMSHOTTARI_SEQUENCE.len() {
        let seq_index = (start_index + offset) % VIMSHOTTARI_SEQUENCE.len();
        let (planet, years) = VIMSHOTTARI_SEQUENCE[seq_index];
        
        // First period is partial depending on Moon's position within the nakshatra
        let effective_years = if offset == 0 {
            years * (1.0 - progress)
        } else {
            years
        };
        
        let period = build_period(
            planet,
            current_start,
            effective_years,
            0,
            target_depth_index,
            seq_index,
            VIMSHOTTARI_SEQUENCE,
            VIMSHOTTARI_TOTAL_YEARS,
        )?;
        periods.push(period.clone());
        current_start = period.end;
    }
    
    Ok(periods)
}

fn build_period(
    planet: &str,
    start: DateTime<Utc>,
    duration_years: f64,
    level_index: usize,
    target_depth_index: usize,
    sequence_start_index: usize,
    sequence: &[PlanetYears],
    total_years: f64,
) -> Result<DashaPeriod, String> {
    let duration_days = duration_years * VIMSHOTTARI_YEAR_DAYS;
    let end = start + Duration::days(duration_days as i64);
    let level = DEPTH_LEVELS[level_index.min(DEPTH_LEVELS.len() - 1)];
    
    let mut period = DashaPeriod {
        planet: planet.to_string(),
        start,
        end,
        duration_days,
        level,
        children: Vec::new(),
    };
    
    if level_index >= target_depth_index {
        return Ok(period);
    }
    
    let mut child_start = start;
    for offset in 0..sequence.len() {
        let child_index = (sequence_start_index + offset) % sequence.len();
        let (child_planet, child_years) = sequence[child_index];
        let child_duration_years = duration_years * (child_years / total_years);
        let child_period = build_period(
            child_planet,
            child_start,
            child_duration_years,
            level_index + 1,
            target_depth_index,
            child_index,
            sequence,
            total_years,
        )?;
        period.children.push(child_period.clone());
        child_start = child_period.end;
    }
    
    Ok(period)
}

fn find_sequence_index(planet: &str, sequence: &[PlanetYears]) -> Result<usize, String> {
    sequence.iter()
        .position(|(p, _)| *p == planet)
        .ok_or_else(|| format!("Planet '{}' not found in sequence", planet))
}

// Yogini Dasha (8 years cycle)
pub const YOGINI_TOTAL_YEARS: f64 = 8.0;

const YOGINI_SEQUENCE: &[(&str, f64)] = &[
    ("mangala", 1.0),   // Mars
    ("pingala", 2.0),   // Sun
    ("dhanya", 3.0),    // Jupiter
    ("bhramari", 4.0),  // Mercury
    ("bhadrika", 5.0),  // Saturn
    ("ulkika", 6.0),    // Venus
    ("siddha", 7.0),    // Moon
    ("sankata", 8.0),   // Rahu
];

const YOGINI_TO_PLANET: &[(&str, &str)] = &[
    ("mangala", "mars"),
    ("pingala", "sun"),
    ("dhanya", "jupiter"),
    ("bhramari", "mercury"),
    ("bhadrika", "saturn"),
    ("ulkika", "venus"),
    ("siddha", "moon"),
    ("sankata", "rahu"),
];

fn yogini_name_to_planet(yogini_name: &str) -> &str {
    YOGINI_TO_PLANET.iter()
        .find(|(name, _)| *name == yogini_name)
        .map(|(_, planet)| *planet)
        .unwrap_or(yogini_name)
}

/// Compute Yogini dasha periods (8-year cycle) based on the Moon's nakshatra.
pub fn compute_yogini_dasha(
    birth_datetime: DateTime<Utc>,
    layer_positions: &LayerPositions,
    depth: DashaLevel,
) -> Result<Vec<DashaPeriod>, String> {
    let moon = layer_positions.planets.get("moon")
        .ok_or_else(|| "Moon position required for Yogini dasha calculation".to_string())?;
    
    let moon_meta = get_nakshatra_for_longitude(moon.lon);
    let nakshatra_index = moon_meta.base.index;
    let progress = moon_meta.progress;
    
    // Yogini dasha starts from nakshatra index mod 8
    let start_index = nakshatra_index % YOGINI_SEQUENCE.len();
    
    let target_depth_index = DEPTH_LEVELS.iter().position(|&d| d == depth)
        .unwrap_or(0);
    let mut current_start = birth_datetime;
    let mut periods: Vec<DashaPeriod> = Vec::new();
    
    for offset in 0..YOGINI_SEQUENCE.len() {
        let seq_index = (start_index + offset) % YOGINI_SEQUENCE.len();
        let (yogini_name, years) = YOGINI_SEQUENCE[seq_index];
        let planet = yogini_name_to_planet(yogini_name);
        
        // First period is partial
        let effective_years = if offset == 0 {
            years * (1.0 - progress)
        } else {
            years
        };
        
        let period = build_period_yogini(
            planet,
            yogini_name,
            current_start,
            effective_years,
            0,
            target_depth_index,
            seq_index,
        )?;
        periods.push(period.clone());
        current_start = period.end;
    }
    
    Ok(periods)
}

fn build_period_yogini(
    planet: &str,
    _yogini_name: &str,
    start: DateTime<Utc>,
    duration_years: f64,
    level_index: usize,
    target_depth_index: usize,
    sequence_start_index: usize,
) -> Result<DashaPeriod, String> {
    let duration_days = duration_years * VIMSHOTTARI_YEAR_DAYS;
    let end = start + Duration::days(duration_days as i64);
    let level = DEPTH_LEVELS[level_index.min(DEPTH_LEVELS.len() - 1)];
    
    let mut period = DashaPeriod {
        planet: planet.to_string(),
        start,
        end,
        duration_days,
        level,
        children: Vec::new(),
    };
    
    if level_index >= target_depth_index {
        return Ok(period);
    }
    
    let mut child_start = start;
    for offset in 0..YOGINI_SEQUENCE.len() {
        let child_index = (sequence_start_index + offset) % YOGINI_SEQUENCE.len();
        let (child_yogini, child_years) = YOGINI_SEQUENCE[child_index];
        let child_planet = yogini_name_to_planet(child_yogini);
        let child_duration_years = duration_years * (child_years / YOGINI_TOTAL_YEARS);
        let child_period = build_period_yogini(
            child_planet,
            child_yogini,
            child_start,
            child_duration_years,
            level_index + 1,
            target_depth_index,
            child_index,
        )?;
        period.children.push(child_period.clone());
        child_start = child_period.end;
    }
    
    Ok(period)
}

// Ashtottari Dasha (108 years cycle)
pub const ASHTOTTARI_TOTAL_YEARS: f64 = 108.0;

const ASHTOTTARI_SEQUENCE: &[PlanetYears] = &[
    ("sun", 6.0),
    ("moon", 15.0),
    ("mars", 8.0),
    ("rahu", 17.0),
    ("jupiter", 19.0),
    ("saturn", 21.0),
    ("mercury", 17.0),
    ("ketu", 7.0),
    ("venus", 20.0),
];

/// Compute Ashtottari dasha periods (108-year cycle) based on the Moon's nakshatra.
pub fn compute_ashtottari_dasha(
    birth_datetime: DateTime<Utc>,
    layer_positions: &LayerPositions,
    depth: DashaLevel,
) -> Result<Vec<DashaPeriod>, String> {
    let moon = layer_positions.planets.get("moon")
        .ok_or_else(|| "Moon position required for Ashtottari dasha calculation".to_string())?;
    
    let moon_meta = get_nakshatra_for_longitude(moon.lon);
    let nakshatra_index = moon_meta.base.index;
    let progress = moon_meta.progress;
    
    // Ashtottari starts from specific nakshatra groups
    // Simplified: use nakshatra index mod 9
    let start_index = nakshatra_index % ASHTOTTARI_SEQUENCE.len();
    
    let target_depth_index = DEPTH_LEVELS.iter().position(|&d| d == depth)
        .unwrap_or(0);
    let mut current_start = birth_datetime;
    let mut periods: Vec<DashaPeriod> = Vec::new();
    
    for offset in 0..ASHTOTTARI_SEQUENCE.len() {
        let seq_index = (start_index + offset) % ASHTOTTARI_SEQUENCE.len();
        let (planet, years) = ASHTOTTARI_SEQUENCE[seq_index];
        
        // First period is partial
        let effective_years = if offset == 0 {
            years * (1.0 - progress)
        } else {
            years
        };
        
        let period = build_period(
            planet,
            current_start,
            effective_years,
            0,
            target_depth_index,
            seq_index,
            ASHTOTTARI_SEQUENCE,
            ASHTOTTARI_TOTAL_YEARS,
        )?;
        periods.push(period.clone());
        current_start = period.end;
    }
    
    Ok(periods)
}

// Kalachakra Dasha (Time Wheel Dasha)
pub const KALACHAKRA_TOTAL_YEARS: f64 = 120.0;
// Kalachakra uses the same sequence as Vimshottari but different calculation method
const KALACHAKRA_SEQUENCE: &[PlanetYears] = VIMSHOTTARI_SEQUENCE;

/// Compute Kalachakra dasha periods based on the Moon's nakshatra.
/// Kalachakra is similar to Vimshottari but uses a different calculation method.
pub fn compute_kalachakra_dasha(
    birth_datetime: DateTime<Utc>,
    layer_positions: &LayerPositions,
    depth: DashaLevel,
) -> Result<Vec<DashaPeriod>, String> {
    let moon = layer_positions.planets.get("moon")
        .ok_or_else(|| "Moon position required for Kalachakra dasha calculation".to_string())?;
    
    let moon_meta = get_nakshatra_for_longitude(moon.lon);
    let moon_lord = moon_meta.base.lord.clone();
    let progress = moon_meta.progress;
    
    // Find starting planet based on nakshatra lord
    let start_index = find_sequence_index(&moon_lord, KALACHAKRA_SEQUENCE)?;
    
    let target_depth_index = DEPTH_LEVELS.iter().position(|&d| d == depth)
        .unwrap_or(0);
    let mut current_start = birth_datetime;
    let mut periods: Vec<DashaPeriod> = Vec::new();
    
    // Kalachakra uses reverse order for some calculations
    // Simplified version - using standard sequence
    for offset in 0..KALACHAKRA_SEQUENCE.len() {
        let seq_index = (start_index + offset) % KALACHAKRA_SEQUENCE.len();
        let (planet, years) = KALACHAKRA_SEQUENCE[seq_index];
        
        // First period is partial
        let effective_years = if offset == 0 {
            years * (1.0 - progress)
        } else {
            years
        };
        
        let period = build_period(
            planet,
            current_start,
            effective_years,
            0,
            target_depth_index,
            seq_index,
            KALACHAKRA_SEQUENCE,
            KALACHAKRA_TOTAL_YEARS,
        )?;
        periods.push(period.clone());
        current_start = period.end;
    }
    
    Ok(periods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ephemeris::types::{PlanetPosition, LayerPositions};
    use std::collections::HashMap;
    
    #[test]
    fn test_find_sequence_index() {
        let idx = find_sequence_index("venus", VIMSHOTTARI_SEQUENCE).unwrap();
        assert_eq!(idx, 1);
    }
    
    #[test]
    fn test_compute_vimshottari_dasha() {
        let mut planets = HashMap::new();
        planets.insert("moon".to_string(), PlanetPosition {
            lon: 13.33, // In Ashwini nakshatra (Ketu lord)
            lat: 0.0,
            speed_lon: 0.0,
            retrograde: false,
        });
        
        let layer_positions = LayerPositions {
            planets,
            houses: None,
        };
        
        let birth = Utc::now();
        let result = compute_vimshottari_dasha(birth, &layer_positions, DashaLevel::Mahadasha);
        assert!(result.is_ok());
        let periods = result.unwrap();
        assert_eq!(periods.len(), 9);
        assert_eq!(periods[0].planet, "ketu");
    }
}

