use aphrodite::ephemeris::{LayerPositions, PlanetPosition};
use aphrodite::layout::{load_wheel_definition_from_json, WheelAssembler};
use std::collections::HashMap;

#[test]
fn test_load_wheel_definition_valid() {
    let json = r#"
    {
      "name": "Test Wheel",
      "rings": [
        {
          "slug": "ring_signs",
          "type": "signs",
          "label": "Zodiac Signs",
          "orderIndex": 0,
          "radiusInner": 0.85,
          "radiusOuter": 1.0,
          "dataSource": {
            "kind": "static_zodiac"
          }
        }
      ]
    }
    "#;

    let result = load_wheel_definition_from_json(json);
    assert!(result.is_ok());
    let wheel = result.unwrap();
    assert_eq!(wheel.wheel.name, "Test Wheel");
    assert_eq!(wheel.wheel.rings.len(), 1);
    assert_eq!(wheel.wheel.rings[0].slug, "ring_signs");
    assert_eq!(
        wheel.wheel.rings[0].ring_type,
        aphrodite::layout::types::RingType::Signs
    );
}

#[test]
fn test_load_wheel_definition_with_multiple_rings() {
    let json = r#"
    {
      "name": "Full Wheel",
      "rings": [
        {
          "slug": "ring_signs",
          "type": "signs",
          "label": "Zodiac Signs",
          "orderIndex": 0,
          "radiusInner": 0.85,
          "radiusOuter": 1.0,
          "dataSource": { "kind": "static_zodiac" }
        },
        {
          "slug": "ring_planets",
          "type": "planets",
          "label": "Planets",
          "orderIndex": 1,
          "radiusInner": 0.70,
          "radiusOuter": 0.85,
          "dataSource": { "kind": "layer_planets", "layerId": "natal" }
        }
      ]
    }
    "#;

    let result = load_wheel_definition_from_json(json);
    assert!(result.is_ok());
    let wheel = result.unwrap();
    assert_eq!(wheel.wheel.rings.len(), 2);
    match &wheel.wheel.rings[1].data_source {
        aphrodite::layout::types::RingDataSource::LayerPlanets { layer_id } => {
            assert_eq!(layer_id, "natal");
        }
        _ => panic!("Expected LayerPlanets data source"),
    }
}

#[test]
fn test_load_wheel_definition_invalid_json() {
    let json = r#"
    {
      "name": "Test Wheel"
    }
    "#;

    let result = load_wheel_definition_from_json(json);
    assert!(result.is_err());
}

#[test]
fn test_load_wheel_definition_missing_rings() {
    let json = r#"
    {
      "name": "Test Wheel",
      "rings": []
    }
    "#;

    let result = load_wheel_definition_from_json(json);
    assert!(result.is_err());
}

#[test]
fn test_load_wheel_definition_invalid_ring_type() {
    let json = r#"
    {
      "name": "Test Wheel",
      "rings": [
        {
          "slug": "ring_invalid",
          "type": "invalid_type",
          "label": "Invalid",
          "orderIndex": 0,
          "radiusInner": 0.5,
          "radiusOuter": 1.0,
          "dataSource": { "kind": "static_zodiac" }
        }
      ]
    }
    "#;

    let result = load_wheel_definition_from_json(json);
    assert!(result.is_err());
}

#[test]
fn test_wheel_assembler_build_static_zodiac() {
    let json = r#"
    {
      "name": "Zodiac Wheel",
      "rings": [
        {
          "slug": "ring_signs",
          "type": "signs",
          "label": "Zodiac Signs",
          "orderIndex": 0,
          "radiusInner": 0.85,
          "radiusOuter": 1.0,
          "dataSource": { "kind": "static_zodiac" }
        }
      ]
    }
    "#;

    let wheel_def = load_wheel_definition_from_json(json).unwrap();
    let positions_by_layer: HashMap<String, LayerPositions> = HashMap::new();
    let aspect_sets: HashMap<String, aphrodite::aspects::types::AspectSet> = HashMap::new();

    let wheel =
        WheelAssembler::build_wheel(&wheel_def.wheel, &positions_by_layer, &aspect_sets, None);

    assert_eq!(wheel.name, "Zodiac Wheel");
    assert_eq!(wheel.rings.len(), 1);
    // Static zodiac should have 12 sign items
    assert_eq!(wheel.rings[0].items.len(), 12);
}

#[test]
fn test_wheel_assembler_build_with_planets() {
    let json = r#"
    {
      "name": "Planet Wheel",
      "rings": [
        {
          "slug": "ring_planets",
          "type": "planets",
          "label": "Planets",
          "orderIndex": 0,
          "radiusInner": 0.70,
          "radiusOuter": 0.85,
          "dataSource": { "kind": "layer_planets", "layerId": "natal" }
        }
      ]
    }
    "#;

    let wheel_def = load_wheel_definition_from_json(json).unwrap();

    let mut planets = HashMap::new();
    planets.insert(
        "sun".to_string(),
        PlanetPosition {
            lon: 100.0,
            lat: 0.0,
            speed_lon: 1.0,
            retrograde: false,
        },
    );
    planets.insert(
        "moon".to_string(),
        PlanetPosition {
            lon: 200.0,
            lat: 0.0,
            speed_lon: 13.0,
            retrograde: false,
        },
    );

    let mut positions_by_layer = HashMap::new();
    positions_by_layer.insert(
        "natal".to_string(),
        LayerPositions {
            planets,
            houses: None,
        },
    );

    let aspect_sets: HashMap<String, aphrodite::aspects::types::AspectSet> = HashMap::new();

    let wheel =
        WheelAssembler::build_wheel(&wheel_def.wheel, &positions_by_layer, &aspect_sets, None);

    assert_eq!(wheel.rings.len(), 1);
    // Should have 2 planet items
    assert_eq!(wheel.rings[0].items.len(), 2);
}
