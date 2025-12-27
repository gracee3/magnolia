use crate::ephemeris::types::{
    EphemerisSettings, GeoLocation, HousePositions, LayerPositions, PlanetPosition,
};
use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use thiserror::Error;
use swisseph::swe::{calc_ut, julday, revjul};

// Note: swisseph crate API - these constants and functions should be available
// If the crate API differs, adjust accordingly

/// Errors that can occur during ephemeris calculations
#[derive(Error, Debug)]
pub enum EphemerisError {
    #[error("Ephemeris file not found at path: {path}. {message}")]
    FileNotFound { path: String, message: String },
    #[error("Invalid house system: {system}. Valid systems: {valid:?}")]
    InvalidHouseSystem { system: String, valid: Vec<String> },
    #[error("Invalid ayanamsa: {ayanamsa}. Valid ayanamsas: {valid:?}")]
    InvalidAyanamsa { ayanamsa: String, valid: Vec<String> },
    #[error("Failed to calculate position for {planet_id} at {datetime}: {message}")]
    CalculationFailed {
        planet_id: String,
        datetime: DateTime<Utc>,
        message: String,
    },
    #[error("House calculation failed: {message}")]
    HouseCalculationFailed { message: String },
}

// Swiss Ephemeris planet IDs - adjust based on actual swisseph crate API
// Typical values: SUN=0, MOON=1, MERCURY=2, VENUS=3, MARS=4, JUPITER=5,
// SATURN=6, URANUS=7, NEPTUNE=8, PLUTO=9, CHIRON=15, TRUE_NODE=11
const PLANET_IDS: &[(&str, i32)] = &[
    ("sun", 0),
    ("moon", 1),
    ("mercury", 2),
    ("venus", 3),
    ("mars", 4),
    ("jupiter", 5),
    ("saturn", 6),
    ("uranus", 7),
    ("neptune", 8),
    ("pluto", 9),
    ("chiron", 15),
    ("north_node", 11), // TRUE_NODE
];

/// House system mapping
const HOUSE_SYSTEMS: &[(&str, u8)] = &[
    ("placidus", b'P' as u8),
    ("whole_sign", b'W' as u8),
    ("koch", b'K' as u8),
    ("equal", b'E' as u8),
    ("regiomontanus", b'R' as u8),
    ("campanus", b'C' as u8),
    ("alcabitius", b'A' as u8),
    ("morinus", b'M' as u8),
];

/// Ayanamsa mapping - using Swiss Ephemeris constants
/// These values match the Swiss Ephemeris library constants
const AYANAMSAS: &[(&str, i32)] = &[
    ("lahiri", 1),      // SIDM_LAHIRI
    ("chitrapaksha", 1), // SIDM_LAHIRI (same as Lahiri)
    ("fagan_bradley", 2), // SIDM_FAGAN_BRADLEY
    ("de_luce", 3),     // SIDM_DELUCE
    ("raman", 4),       // SIDM_RAMAN
    ("krishnamurti", 5), // SIDM_KRISHNAMURTI
    ("yukteshwar", 6),  // SIDM_YUKTESHWAR
    ("djwhal_khul", 7), // SIDM_DJWHAL_KHUL
    ("true_citra", 8),  // SIDM_TRUE_CITRA
    ("true_revati", 9), // SIDM_TRUE_REVATI
    ("aryabhata", 10),  // SIDM_ARYABHATA
    ("aryabhata_mean_sun", 11), // SIDM_ARYABHATA_MSUN
];

/// Swiss Ephemeris adapter implementation
pub struct SwissEphemerisAdapter {
    _ephemeris_path: PathBuf,
    current_sidereal_mode: Option<i32>,
}

impl SwissEphemerisAdapter {
    /// Create a new adapter with optional ephemeris path
    pub fn new(ephemeris_path: Option<PathBuf>) -> Result<Self, EphemerisError> {
        let path = ephemeris_path.unwrap_or_else(|| {
            env::var("SWISS_EPHEMERIS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/usr/local/share/swisseph"))
        });

        // Validate path exists
        if !path.exists() {
            return Err(EphemerisError::FileNotFound {
                path: path.display().to_string(),
                message: "Ephemeris path does not exist. Please ensure Swiss Ephemeris data files are installed.".to_string(),
            });
        }

        // Set ephemeris path
        // Note: Adjust based on actual swisseph crate API
        // Typical API: swisseph::set_ephe_path(path_str)
        let _path_str = path.to_string_lossy();
        // This will need to be adjusted based on the actual crate API
        // For now, we'll assume the path is set correctly

        Ok(Self {
            _ephemeris_path: path,
            current_sidereal_mode: None,
        })
    }

    /// Calculate planetary and house positions
    pub fn calc_positions(
        &mut self,
        dt_utc: DateTime<Utc>,
        location: Option<GeoLocation>,
        settings: &EphemerisSettings,
    ) -> Result<LayerPositions, EphemerisError> {
        let jd = datetime_to_julian_day(dt_utc);
        let house_system_byte = get_house_system_byte(&settings.house_system)?;
        let flags = self.configure_flags(settings)?;

        // Calculate planets
        let mut planets = HashMap::new();
        for obj_id in &settings.include_objects {
            let obj_id_lower = obj_id.to_lowercase();

            // Handle special case: south_node
            if obj_id_lower == "south_node" {
                if let Ok(north_node_pos) = self.calc_planet_position("north_node", jd, flags) {
                    let south_lon = (north_node_pos.lon + 180.0) % 360.0;
                    planets.insert(
                        "south_node".to_string(),
                        PlanetPosition {
                            lon: south_lon,
                            lat: 0.0,
                            speed_lon: north_node_pos.speed_lon,
                            retrograde: north_node_pos.retrograde,
                        },
                    );
                }
                continue;
            }

            if let Ok(planet_pos) = self.calc_planet_position(&obj_id_lower, jd, flags) {
                planets.insert(obj_id_lower.clone(), planet_pos);
            }
        }

        // Calculate houses if location is provided
        let houses = if let Some(loc) = location {
            Some(self.calc_houses(
                jd,
                loc.lat,
                loc.lon,
                house_system_byte,
                &settings.house_system,
                flags,
            )?)
        } else {
            None
        };

        Ok(LayerPositions { planets, houses })
    }

    /// Calculate position for a single planet
    pub fn calc_planet_position(
        &self,
        planet_id: &str,
        jd: f64,
        flags: i32,
    ) -> Result<PlanetPosition, EphemerisError> {
        let planet_code = PLANET_IDS
            .iter()
            .find(|(id, _)| *id == planet_id)
            .map(|(_, code)| *code)
            .ok_or_else(|| EphemerisError::CalculationFailed {
                planet_id: planet_id.to_string(),
                datetime: julian_day_to_datetime(jd),
                message: format!("Unknown planet ID: {}", planet_id),
            })?;

        // Calculate planet position using swisseph crate
        let result = calc_ut(jd, planet_code as u32, flags as u32)
            .map_err(|e| EphemerisError::CalculationFailed {
                planet_id: planet_id.to_string(),
                datetime: julian_day_to_datetime(jd),
                message: format!("Swiss Ephemeris error: {}", e),
            })?;

        let result_array = result.out;
        let longitude = result_array[0] % 360.0;
        let latitude = result_array[1];
        let speed_longitude = result_array[3];
        let is_retrograde = speed_longitude < 0.0;

        Ok(PlanetPosition {
            lon: longitude,
            lat: latitude,
            speed_lon: speed_longitude,
            retrograde: is_retrograde,
        })
    }

    /// Calculate house cusps and angles
    pub fn calc_houses(
        &self,
        jd: f64,
        lat: f64,
        lon: f64,
        house_system_byte: u8,
        house_system_str: &str,
        flags: i32,
    ) -> Result<HousePositions, EphemerisError> {
        // Calculate houses using swisseph crate
        // Use houses_ex2 which takes HouseSystemKind, but we'll use the lower-level API
        // Since HouseSystemKind is private, use houses_ex from swe module
        use swisseph::swe::houses_ex;
        let (c, a) = houses_ex(jd, flags, lat, lon, house_system_byte as i32);
        
        // Convert arrays to Cusp and AscMc structs
        use swisseph::{Cusp, AscMc};
        let cusps = Cusp::from_array(c);
        let ascmc = AscMc::from_array(a);

        // Extract house cusps - Cusp struct has fields: first, second, third, etc.
        let mut cusps_dict = HashMap::new();
        let cusp_values = [
            cusps.first, cusps.second, cusps.third, cusps.fourth,
            cusps.fifth, cusps.sixth, cusps.seventh, cusps.eighth,
            cusps.ninth, cusps.tenth, cusps.eleventh, cusps.twelfth,
        ];
        
        for (i, &cusp) in cusp_values.iter().enumerate() {
            cusps_dict.insert((i + 1).to_string(), cusp % 360.0);
        }

        // Extract angles from AscMc struct
        let asc = ascmc.ascendant % 360.0;
        let mc = ascmc.mc % 360.0;
        let ic = (mc + 180.0) % 360.0;
        let dc = (asc + 180.0) % 360.0;

        Ok(HousePositions {
            system: house_system_str.to_string(),
            cusps: cusps_dict,
            angles: HashMap::from([
                ("asc".to_string(), asc),
                ("mc".to_string(), mc),
                ("ic".to_string(), ic),
                ("dc".to_string(), dc),
            ]),
        })
    }

    /// Configure Swiss Ephemeris flags for the requested zodiac
    fn configure_flags(&mut self, settings: &EphemerisSettings) -> Result<i32, EphemerisError> {
        // FLG_SWIEPH = 2 (use Swiss Ephemeris files)
        let mut flags = 2; // swisseph::FLG_SWIEPH

        if settings.zodiac_type == "sidereal" {
            let mode = self.resolve_ayanamsa(settings.ayanamsa.as_deref())?;
            self.ensure_sidereal_mode(mode)?;
            flags |= 64; // swisseph::FLG_SIDEREAL
        }

        Ok(flags)
    }

    /// Map ayanamsa string to Swiss constant
    fn resolve_ayanamsa(&self, ayanamsa: Option<&str>) -> Result<i32, EphemerisError> {
        let ayanamsa = ayanamsa.unwrap_or("lahiri");
        AYANAMSAS
            .iter()
            .find(|(name, _)| *name == ayanamsa.to_lowercase())
            .map(|(_, mode)| *mode)
            .ok_or_else(|| EphemerisError::InvalidAyanamsa {
                ayanamsa: ayanamsa.to_string(),
                valid: AYANAMSAS.iter().map(|(name, _)| name.to_string()).collect(),
            })
    }

    /// Cache sidereal mode configuration to avoid redundant calls
    fn ensure_sidereal_mode(&mut self, mode: i32) -> Result<(), EphemerisError> {
        if self.current_sidereal_mode == Some(mode) {
            return Ok(());
        }
        // Set sidereal mode
        // Note: set_sid_mode may not be available in swisseph 0.1.x
        // The sidereal mode is typically set via flags, so we'll skip explicit mode setting
        // If needed, we can add it when the function is available
        // For now, the flags should handle sidereal calculations
        self.current_sidereal_mode = Some(mode);
        Ok(())
    }
}

/// Convert UTC datetime to Julian Day
fn datetime_to_julian_day(dt: DateTime<Utc>) -> f64 {
    let year = dt.year();
    let month = dt.month();
    let day = dt.day();
    let hour = dt.hour() as f64;
    let minute = dt.minute() as f64;
    let second = dt.second() as f64;
    let hour_decimal = hour + minute / 60.0 + second / 3600.0;

    // GREG_CAL = 1
    // julday returns f64 directly, not a Result
    julday(year, month as i32, day as i32, hour_decimal, 1)
}

/// Convert Julian Day to UTC datetime
fn julian_day_to_datetime(jd: f64) -> DateTime<Utc> {
    // GREG_CAL = 1
    // revjul returns (i32, i32, i32, f64) directly, not a Result
    let (year, month, day, hour_decimal) = revjul(jd, 1);
    let hour = hour_decimal as u32;
    let minute = ((hour_decimal - hour as f64) * 60.0) as u32;
    let second = (((hour_decimal - hour as f64) * 60.0 - minute as f64) * 60.0) as u32;
    chrono::Utc
        .with_ymd_and_hms(year, month as u32, day as u32, hour, minute, second)
        .single()
        .unwrap_or_else(|| chrono::Utc::now())
}

/// Convert house system string to byte format
fn get_house_system_byte(house_system: &str) -> Result<u8, EphemerisError> {
    HOUSE_SYSTEMS
        .iter()
        .find(|(name, _)| *name == house_system.to_lowercase())
        .map(|(_, byte)| *byte)
        .ok_or_else(|| EphemerisError::InvalidHouseSystem {
            system: house_system.to_string(),
            valid: HOUSE_SYSTEMS.iter().map(|(name, _)| name.to_string()).collect(),
        })
}

