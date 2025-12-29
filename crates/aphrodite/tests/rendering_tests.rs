use aphrodite::rendering::ChartSpec;
use aphrodite::rendering::primitives::{Color, Point, Shape, Stroke, TextAnchor};

#[test]
fn test_chartspec_new() {
    let spec = ChartSpec::new(800.0, 600.0);
    
    assert_eq!(spec.width, 800.0);
    assert_eq!(spec.height, 600.0);
    assert_eq!(spec.center.x, 400.0);
    assert_eq!(spec.center.y, 300.0);
    assert_eq!(spec.rotation_offset, 0.0);
    assert_eq!(spec.background_color.r, Color::BLACK.r);
    assert_eq!(spec.background_color.g, Color::BLACK.g);
    assert_eq!(spec.background_color.b, Color::BLACK.b);
    assert_eq!(spec.background_color.a, Color::BLACK.a);
    assert!(spec.shapes.is_empty());
}

#[test]
fn test_color_constants() {
    assert_eq!(Color::WHITE.r, 255);
    assert_eq!(Color::WHITE.g, 255);
    assert_eq!(Color::WHITE.b, 255);
    assert_eq!(Color::WHITE.a, 255);
    
    assert_eq!(Color::BLACK.r, 0);
    assert_eq!(Color::BLACK.g, 0);
    assert_eq!(Color::BLACK.b, 0);
    assert_eq!(Color::BLACK.a, 255);
}

#[test]
fn test_color_from_hex_rgb() {
    let color = Color::from_hex("#FF0000");
    assert!(color.is_some());
    let color = color.unwrap();
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
    assert_eq!(color.b, 0);
    assert_eq!(color.a, 255); // Default alpha
}

#[test]
fn test_color_from_hex_rgba() {
    let color = Color::from_hex("#FF000080");
    assert!(color.is_some());
    let color = color.unwrap();
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
    assert_eq!(color.b, 0);
    assert_eq!(color.a, 128);
}

#[test]
fn test_color_from_hex_invalid() {
    assert!(Color::from_hex("invalid").is_none());
    assert!(Color::from_hex("#FF").is_none());
    assert!(Color::from_hex("#FF00000").is_none()); // Wrong length
}

#[test]
fn test_color_to_css_string_opaque() {
    let color = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    
    let css = color.to_css_string();
    assert!(css.contains("rgb(255, 0, 0)"));
    assert!(!css.contains("rgba")); // Should use rgb for opaque
}

#[test]
fn test_color_to_css_string_transparent() {
    let color = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 128,
    };
    
    let css = color.to_css_string();
    assert!(css.contains("rgba"));
    assert!(css.contains("0.5")); // 128/255 ≈ 0.5
}

#[test]
fn test_shape_circle_serialization() {
    let shape = Shape::Circle {
        center: Point { x: 100.0, y: 200.0 },
        radius: 50.0,
        fill: Some(Color::WHITE),
        stroke: None,
    };
    
    let json = serde_json::to_string(&shape);
    assert!(json.is_ok());
    
    // Test deserialization
    let json_str = json.unwrap();
    let deserialized: Result<Shape, _> = serde_json::from_str(&json_str);
    assert!(deserialized.is_ok());
}

#[test]
fn test_shape_line_serialization() {
    let shape = Shape::Line {
        from: Point { x: 0.0, y: 0.0 },
        to: Point { x: 100.0, y: 100.0 },
        stroke: Stroke {
            color: Color::WHITE,
            width: 2.0,
            dash_array: None,
        },
    };
    
    let json = serde_json::to_string(&shape);
    assert!(json.is_ok());
}

#[test]
fn test_shape_text_serialization() {
    let shape = Shape::Text {
        position: Point { x: 50.0, y: 50.0 },
        content: "Test".to_string(),
        size: 12.0,
        color: Color::WHITE,
        anchor: TextAnchor::Middle,
        rotation: Some(45.0),
    };
    
    let json = serde_json::to_string(&shape);
    assert!(json.is_ok());
}

#[test]
fn test_shape_planet_glyph_serialization() {
    let shape = Shape::PlanetGlyph {
        center: Point { x: 100.0, y: 100.0 },
        planet_id: "sun".to_string(),
        size: 16.0,
        color: Color::WHITE,
        retrograde: false,
    };
    
    let json = serde_json::to_string(&shape);
    assert!(json.is_ok());
}

#[test]
fn test_chartspec_serialization() {
    let mut spec = ChartSpec::new(800.0, 600.0);
    spec.shapes.push(Shape::Circle {
        center: Point { x: 400.0, y: 300.0 },
        radius: 50.0,
        fill: Some(Color::WHITE),
        stroke: None,
    });
    
    let json = serde_json::to_string(&spec);
    assert!(json.is_ok());
    
    // Test round-trip
    let json_str = json.unwrap();
    let deserialized: Result<ChartSpec, _> = serde_json::from_str(&json_str);
    assert!(deserialized.is_ok());
    let deserialized = deserialized.unwrap();
    assert_eq!(deserialized.width, 800.0);
    assert_eq!(deserialized.height, 600.0);
    assert_eq!(deserialized.shapes.len(), 1);
}

