#[cfg(test)]
mod tests {
    use aphrodite_core::vedic::vargas::*;
    use aphrodite_core::ephemeris::types::{LayerPositions, PlanetPosition};
    use std::collections::HashMap;

    #[test]
    fn test_build_varga_layers() {
        let mut planets = HashMap::new();
        planets.insert("sun".to_string(), PlanetPosition {
            lon: 45.0, // 15° Taurus
            lat: 0.0,
            speed_lon: 0.0,
            retrograde: false,
        });
        
        let layer_positions = LayerPositions {
            planets,
            houses: None,
        };
        
        let vargas = vec!["d9".to_string()]; // Navamsa
        let layers = build_varga_layers("natal", &layer_positions, &vargas);
        assert!(!layers.is_empty());
        assert!(layers.contains_key("d9"));
    }
}

