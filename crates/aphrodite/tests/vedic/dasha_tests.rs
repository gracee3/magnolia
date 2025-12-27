#[cfg(test)]
mod tests {
    use aphrodite_core::vedic::dashas::*;
    use aphrodite_core::ephemeris::types::{LayerPositions, PlanetPosition};
    use chrono::Utc;
    use std::collections::HashMap;

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

