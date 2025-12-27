#[cfg(test)]
mod tests {
    use aphrodite_core::vedic::yogas::*;
    use aphrodite_core::ephemeris::types::{LayerPositions, PlanetPosition, HousePositions};
    use std::collections::HashMap;

    #[test]
    fn test_identify_yogas() {
        let mut planets = HashMap::new();
        planets.insert("jupiter".to_string(), PlanetPosition {
            lon: 0.0, // 1st house (kendra)
            lat: 0.0,
            speed_lon: 0.0,
            retrograde: false,
        });
        planets.insert("moon".to_string(), PlanetPosition {
            lon: 90.0, // 4th house (kendra)
            lat: 0.0,
            speed_lon: 0.0,
            retrograde: false,
        });
        
        let mut angles = HashMap::new();
        angles.insert("asc".to_string(), 0.0);
        
        let houses = Some(HousePositions {
            system: "placidus".to_string(),
            cusps: HashMap::new(),
            angles,
        });
        
        let layer_positions = LayerPositions {
            planets,
            houses,
        };
        
        let yogas = identify_yogas(&layer_positions);
        // Should detect Gajakesari Yoga if both are in kendras/trikonas
        assert!(!yogas.is_empty());
    }
}

