use aphrodite_core::ephemeris::{EphemerisSettings, GeoLocation, SwissEphemerisAdapter};
use chrono::Utc;

#[tokio::test]
#[ignore] // Requires Swiss Ephemeris files
async fn test_calc_positions_basic() {
    let mut adapter = SwissEphemerisAdapter::new(None).unwrap();
    
    let settings = EphemerisSettings {
        zodiac_type: "tropical".to_string(),
        ayanamsa: None,
        house_system: "placidus".to_string(),
        include_objects: vec!["sun".to_string(), "moon".to_string()],
    };
    
    let location = Some(GeoLocation {
        lat: 40.7128,
        lon: -74.0060,
    });
    
    let dt = Utc::now();
    let result = adapter.calc_positions(dt, location, &settings);
    
    assert!(result.is_ok());
    let positions = result.unwrap();
    assert!(!positions.planets.is_empty());
    assert!(positions.houses.is_some());
}

#[test]
fn test_ephemeris_settings_default() {
    let settings = EphemerisSettings {
        zodiac_type: "tropical".to_string(),
        ayanamsa: None,
        house_system: "placidus".to_string(),
        include_objects: vec![],
    };
    
    assert_eq!(settings.zodiac_type, "tropical");
    assert_eq!(settings.house_system, "placidus");
}

