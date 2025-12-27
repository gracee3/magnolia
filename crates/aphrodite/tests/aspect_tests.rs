use aphrodite_core::aspects::{AspectCalculator, AspectSettings};
use aphrodite_core::ephemeris::{LayerPositions, PlanetPosition};
use std::collections::HashMap;

#[test]
fn test_calculate_aspect_conjunction() {
    let calculator = AspectCalculator::new();
    let mut orb_settings = HashMap::new();
    orb_settings.insert("conjunction".to_string(), 8.0);
    
    // Two planets at same longitude (conjunction)
    let lon1 = 100.0;
    let lon2 = 102.0; // Within 8 degree orb
    let speed1 = 1.0;
    let speed2 = 1.0;
    
    let aspect = calculator.calculate_aspect(lon1, lon2, speed1, speed2, &orb_settings);
    
    assert!(aspect.is_some());
    let aspect = aspect.unwrap();
    assert_eq!(aspect.aspect_type, "conjunction");
    assert!(aspect.orb <= 8.0);
}

#[test]
fn test_calculate_aspect_opposition() {
    let calculator = AspectCalculator::new();
    let mut orb_settings = HashMap::new();
    orb_settings.insert("opposition".to_string(), 8.0);
    
    // Two planets 180 degrees apart (opposition)
    let lon1 = 100.0;
    let lon2 = 278.0; // 178 degrees apart (within 8 degree orb)
    let speed1 = 1.0;
    let speed2 = 1.0;
    
    let aspect = calculator.calculate_aspect(lon1, lon2, speed1, speed2, &orb_settings);
    
    assert!(aspect.is_some());
    let aspect = aspect.unwrap();
    assert_eq!(aspect.aspect_type, "opposition");
}

#[test]
fn test_compute_intra_layer_aspects() {
    let calculator = AspectCalculator::new();
    
    let mut planets = HashMap::new();
    planets.insert("sun".to_string(), PlanetPosition {
        lon: 100.0,
        lat: 0.0,
        speed_lon: 1.0,
        retrograde: false,
    });
    planets.insert("moon".to_string(), PlanetPosition {
        lon: 102.0,
        lat: 0.0,
        speed_lon: 13.0,
        retrograde: false,
    });
    
    let positions = LayerPositions {
        planets,
        houses: None,
    };
    
    let mut orb_settings = HashMap::new();
    orb_settings.insert("conjunction".to_string(), 8.0);
    
    let settings = AspectSettings {
        orb_settings,
        include_objects: vec![],
        only_major: None,
    };
    
    let aspect_set = calculator.compute_intra_layer_aspects("natal", &positions, &settings);
    
    assert_eq!(aspect_set.layer_ids, vec!["natal"]);
    assert!(!aspect_set.pairs.is_empty());
}

