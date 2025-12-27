use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Geographic location coordinates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub lat: f64,
    pub lon: f64,
}

/// Planetary position data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanetPosition {
    /// Longitude in degrees (0-360)
    pub lon: f64,
    /// Latitude in degrees
    pub lat: f64,
    /// Speed in longitude (degrees per day)
    pub speed_lon: f64,
    /// Whether the planet is retrograde
    pub retrograde: bool,
}

/// House system positions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HousePositions {
    /// House system name
    pub system: String,
    /// House cusps: "1".."12" -> degrees
    pub cusps: HashMap<String, f64>,
    /// Angles: "asc", "mc", "ic", "dc" -> degrees
    pub angles: HashMap<String, f64>,
}

/// Complete position data for a chart layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerPositions {
    /// Planet ID -> position
    pub planets: HashMap<String, PlanetPosition>,
    /// House positions (None if no location provided)
    pub houses: Option<HousePositions>,
}

/// Settings for ephemeris calculations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EphemerisSettings {
    /// Zodiac type: "tropical" or "sidereal"
    pub zodiac_type: String,
    /// Ayanamsa name (for sidereal zodiac)
    pub ayanamsa: Option<String>,
    /// House system name
    pub house_system: String,
    /// List of planet IDs to include
    pub include_objects: Vec<String>,
}

/// Context for calculating positions for a chart layer
#[derive(Debug, Clone)]
pub struct LayerContext {
    pub layer_id: String,
    pub kind: String,
    pub datetime: chrono::DateTime<chrono::Utc>,
    pub location: Option<GeoLocation>,
    pub settings: EphemerisSettings,
}

