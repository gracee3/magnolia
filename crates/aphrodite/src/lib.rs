
pub mod ephemeris;
pub mod aspects;
pub mod source;

#[cfg(feature = "tile-rendering")]
pub mod tile;

pub use source::AphroditeSource;

#[cfg(feature = "tile-rendering")]
pub use tile::AstroTile;

pub mod vedic;
pub mod western;

use ephemeris::{SwissEphemerisAdapter, EphemerisSettings, GeoLocation};

pub fn get_astro_salt() -> String {
    // 1. Init Adapter
    let mut adapter = SwissEphemerisAdapter::new(None).unwrap_or_else(|_| {
        // Fallback if SwissEph is not available
        return SwissEphemerisAdapter::new(None).expect("Failed to init SwissEph adapter");
    });

    // 2. Settings: Tropical Zodiac, Placidus Houses
    let settings = EphemerisSettings {
        zodiac_type: "tropical".to_string(),
        ayanamsa: None,
        house_system: "placidus".to_string(),
        include_objects: vec!["moon".to_string(), "asc".to_string(), "sun".to_string()],
    };

    // 3. User Location (Washington DC)
    let loc = Some(GeoLocation { lat: 38.9072, lon: -77.0369 });
    let now = chrono::Utc::now();

    // 4. Calculate
    match adapter.calc_positions(now, loc, &settings) {
        Ok(pos) => {
            let moon_lon = pos.planets.get("moon").map(|p| p.lon).unwrap_or(0.0);
            let sun_lon = pos.planets.get("sun").map(|p| p.lon).unwrap_or(0.0);
            let asc = pos.houses.as_ref().and_then(|h| h.angles.get("asc")).unwrap_or(&0.0);
            
            // Format: "SUN:120.5|MOON:45.2|ASC:12.1"
            format!("SUN:{:.2}|MOON:{:.2}|ASC:{:.2}", sun_lon, moon_lon, asc)
        },
        Err(_) => "VOID_OF_COURSE".to_string()
    }
}

